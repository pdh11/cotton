#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use defmt_rtt as _;
use panic_probe as _;
use rp_pico as _; // includes boot2

#[rtic::app(device = rp_pico::hal::pac, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use core::pin::pin;
    use cotton_usb_host::host::rp2040::DeviceEvent;
    use cotton_usb_host::host::rp2040::{UsbShared, UsbStack, UsbStatics};
    use cotton_usb_host::types::{
        parse_descriptors, HubDescriptor, ShowDescriptors, CLASS_REQUEST,
        CLEAR_FEATURE, CONFIGURATION_DESCRIPTOR, DEVICE_TO_HOST,
        GET_DESCRIPTOR, GET_STATUS, HOST_TO_DEVICE, HUB_DESCRIPTOR,
        PORT_POWER, PORT_RESET, RECIPIENT_OTHER, SET_ADDRESS,
        SET_CONFIGURATION, SET_FEATURE, VENDOR_REQUEST,
    };
    use cotton_usb_host::types::{SetupPacket, UsbDevice};
    use futures_util::StreamExt;
    use rp_pico::pac;
    use rtic_monotonics::rp2040::prelude::*;
    use static_cell::ConstStaticCell;

    #[inline(never)]
    unsafe fn unique_flash_id() -> cotton_unique::UniqueId {
        let mut unique_bytes = [0u8; 16];
        cortex_m::interrupt::free(|_| {
            rp2040_flash::flash::flash_unique_id(&mut unique_bytes, true);
        });
        cotton_unique::UniqueId::new(&unique_bytes)
    }

    #[shared]
    struct Shared {
        shared: &'static UsbShared,
    }

    #[local]
    struct Local {
        resets: pac::RESETS,
        regs: Option<pac::USBCTRL_REGS>,
        dpram: Option<pac::USBCTRL_DPRAM>,
    }

    rp2040_timer_monotonic!(Mono); // 1MHz!

    #[init()]
    fn init(c: init::Context) -> (Shared, Local) {
        defmt::println!(
            "{} from {} {}-g{}",
            env!("CARGO_BIN_NAME"),
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            git_version::git_version!()
        );

        let _unique_id = unsafe { unique_flash_id() };

        let device = c.device;
        let mut resets = device.RESETS;
        let mut watchdog =
            rp2040_hal::watchdog::Watchdog::new(device.WATCHDOG);

        let _clocks = rp2040_hal::clocks::init_clocks_and_plls(
            rp_pico::XOSC_CRYSTAL_FREQ,
            device.XOSC,
            device.CLOCKS,
            device.PLL_SYS,
            device.PLL_USB,
            &mut resets,
            &mut watchdog,
        )
        .ok()
        .unwrap();

        Mono::start(device.TIMER, &resets);

        // The timer doesn't increment if either RP2040 core is under
        // debug, unless the DBGPAUSE bits are cleared, which they
        // aren't by default.
        //
        // There is no neat and tidy method on hal::Timer to clear
        // these bits, and they can't be cleared before
        // hal::Timer::new because it resets the peripheral. So we
        // have to steal the peripheral, but that's OK because we only
        // access the DBGPAUSE register, which nobody else is
        // accessing.
        unsafe {
            rp2040_hal::pac::TIMER::steal()
                .dbgpause()
                .write(|w| w.bits(0));
        }
        /*
        let sio = rp2040_hal::Sio::new(device.SIO);
        let pins = rp_pico::Pins::new(
            device.IO_BANK0,
            device.PADS_BANK0,
            sio.gpio_bank0,
            &mut resets,
        );
        */

        usb_task::spawn().unwrap();

        static USB_SHARED: UsbShared = UsbShared::new();

        (
            Shared {
                shared: &USB_SHARED,
            },
            Local {
                regs: Some(device.USBCTRL_REGS),
                dpram: Some(device.USBCTRL_DPRAM),
                resets,
            },
        )
    }

    #[allow(dead_code)]
    #[inline(never)]
    async fn hub_class<HC: cotton_usb_host::core::driver::Driver>(
        stack: &UsbStack<HC>,
        device: UsbDevice,
    ) {
        let mut descriptors = [0u8; 64];

        let rc = stack
            .control_transfer_out(
                1,
                device.packet_size_ep0,
                SetupPacket {
                    bmRequestType: HOST_TO_DEVICE,
                    bRequest: SET_CONFIGURATION,
                    wValue: 1,
                    wIndex: 0,
                    wLength: 0,
                },
                &descriptors,
            )
            .await;
        defmt::println!("Set configuration: {}", rc);

        let rc = stack
            .control_transfer_in(
                1,
                device.packet_size_ep0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST | CLASS_REQUEST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: (HUB_DESCRIPTOR as u16) << 8,
                    wIndex: 0,
                    wLength: 64,
                },
                &mut descriptors,
            )
            .await;
        defmt::println!("Get hub dtor: {}", rc);

        let ports = if let Ok(sz) = rc {
            defmt::println!("hd {:?}", &descriptors[0..sz]);
            if sz >= core::mem::size_of::<HubDescriptor>() {
                defmt::println!(
                    "{}",
                    &HubDescriptor::try_from_bytes(&descriptors[0..sz])
                        .unwrap()
                );
                descriptors[2]
            } else {
                4
            }
        } else {
            4
        };
        defmt::println!("{}-port hub", ports);

        // Hub state machine: each hub must have each port powered,
        // then reset. But only one hub port on the whole *bus* can be
        // in reset at any one time, because it becomes sensitive to
        // address zero. So there needs to be a bus-wide hub state
        // machine.
        //
        // Idea: hubs get given addresses 1-15, so a bitmap of hubs
        // fits in u16. Newly found hubs get their interrupt EP added
        // to the waker, and all their ports powered. Ports that see a
        // C_PORT_CONNECTION get the parent hub added to a bitmap of
        // "hubs needing resetting". When no port is currently in
        // reset, a hub is removed from the bitmap, and a port counter
        // (1..=N) is set up; each port in turn has GetStatus called,
        // and on returning CONNECTION but not ENABLE, the port is
        // reset. (Not connected or already enabled means progress to
        // the next port, then the next hub.)
        //
        // On a reset complete, give the new device an address (1-15
        // if it, too, is a hub) and then again proceed to the next
        // port, then the next hub.

        // Ports are numbered from 1..=N (not 0..N)
        for port in 1..=ports {
            let rc = stack
                .control_transfer_out(
                    1,
                    device.packet_size_ep0,
                    SetupPacket {
                        bmRequestType: HOST_TO_DEVICE
                            | CLASS_REQUEST
                            | RECIPIENT_OTHER,
                        bRequest: SET_FEATURE,
                        wValue: PORT_POWER,
                        wIndex: port as u16,
                        wLength: 0,
                    },
                    &descriptors,
                )
                .await;
            defmt::println!("Set port power {}", rc);
        }

        let mut ep = pin!(stack.interrupt_endpoint_in(1, 1, 1, 0xFF));

        while let Some(data) = ep.next().await {
            defmt::println!(
                "got {} on ep",
                data.data[0..(data.size as usize)]
            );

            for port in 1..4 {
                if (data.data[0] & (1 << port)) != 0 {
                    let rc = stack
                        .control_transfer_in(
                            1,
                            device.packet_size_ep0,
                            SetupPacket {
                                bmRequestType: DEVICE_TO_HOST
                                    | CLASS_REQUEST
                                    | RECIPIENT_OTHER,
                                bRequest: GET_STATUS,
                                wValue: 0,
                                wIndex: port as u16,
                                wLength: 4,
                            },
                            &mut descriptors,
                        )
                        .await;

                    defmt::println!("Get port status {}", rc);
                    if rc.is_ok() {
                        defmt::println!(
                            "  port {} status3 {:x}",
                            port,
                            &descriptors[0..4]
                        );
                        let state = descriptors[0];

                        let bit = descriptors[2].trailing_zeros() as u16;
                        // i.e. least set bit

                        if bit < 8 {
                            // Clear C_PORT_CONNECTION (or similar
                            // status-change bit); see USB 2.0
                            // s11.24.2.7.2
                            let rc = stack
                                .control_transfer_out(
                                    1,
                                    device.packet_size_ep0,
                                    SetupPacket {
                                        bmRequestType: HOST_TO_DEVICE
                                            | CLASS_REQUEST
                                            | RECIPIENT_OTHER,
                                        bRequest: CLEAR_FEATURE,
                                        wValue: bit + 16,
                                        wIndex: port as u16,
                                        wLength: 0,
                                    },
                                    &descriptors,
                                )
                                .await;
                            defmt::println!(
                                "Clear port {} {} {}",
                                port,
                                16 + bit,
                                rc
                            );
                        }

                        if bit == 0 {
                            // C_PORT_CONNECTION
                            if (state & 1) == 0 {
                                defmt::println!("port {} disconnect", port);
                            } else {
                                let rc = stack
                                    .control_transfer_out(
                                        1,
                                        device.packet_size_ep0,
                                        SetupPacket {
                                            bmRequestType: HOST_TO_DEVICE
                                                | CLASS_REQUEST
                                                | RECIPIENT_OTHER,
                                            bRequest: SET_FEATURE,
                                            wValue: PORT_RESET,
                                            wIndex: port as u16,
                                            wLength: 0,
                                        },
                                        &descriptors,
                                    )
                                    .await;
                                defmt::println!(
                                    "Set port {} reset {}",
                                    port,
                                    rc
                                );
                            }
                        }

                        if bit == 4 {
                            // C_PORT_RESET
                            let address = device.address * 4 + port;
                            let rc = stack
                                .control_transfer_out(
                                    0,
                                    device.packet_size_ep0,
                                    SetupPacket {
                                        bmRequestType: HOST_TO_DEVICE,
                                        bRequest: SET_ADDRESS,
                                        wValue: address as u16,
                                        wIndex: 0,
                                        wLength: 0,
                                    },
                                    &descriptors,
                                )
                                .await;
                            defmt::println!(
                                "Set address port {} {} {}",
                                port,
                                address,
                                rc
                            );
                        }
                    }
                }
            }
        }
    }

    #[task(local = [regs, dpram, resets], shared = [&shared], priority = 2)]
    async fn usb_task(cx: usb_task::Context) {
        static USB_STATICS: ConstStaticCell<UsbStatics> =
            ConstStaticCell::new(UsbStatics::new());
        let statics = USB_STATICS.take();

        let driver = cotton_usb_host::host::rp2040::HostController::new(
            cx.shared.shared,
            statics,
        );
        let stack = UsbStack::new(
            driver,
            cx.local.regs.take().unwrap(),
            cx.local.dpram.take().unwrap(),
            cx.local.resets,
            cx.shared.shared,
            statics,
        );

        let mut p = pin!(stack.device_events());

        loop {
            let device = p.next().await;

            if let Some(DeviceEvent::Connect(device)) = device {
                defmt::println!("Got device {:x}", device);

                defmt::trace!("fetching2");
                let mut descriptors = [0u8; 64];
                let rc = stack
                    .control_transfer_in(
                        1,
                        device.packet_size_ep0,
                        SetupPacket {
                            bmRequestType: DEVICE_TO_HOST,
                            bRequest: GET_DESCRIPTOR,
                            wValue: ((CONFIGURATION_DESCRIPTOR as u16) << 8),
                            wIndex: 0,
                            wLength: 64,
                        },
                        &mut descriptors,
                    )
                    .await;
                if let Ok(sz) = rc {
                    parse_descriptors(
                        &descriptors[0..sz],
                        &mut ShowDescriptors,
                    );
                } else {
                    defmt::println!("fetched {:?}", rc);
                }

                if device.vid == 0x0B95 && device.pid == 0x7720 {
                    // ASIX AX88772
                    defmt::trace!("fetching4");
                    let rc = stack
                        .control_transfer_in(
                            1,
                            device.packet_size_ep0,
                            SetupPacket {
                                bmRequestType: DEVICE_TO_HOST | VENDOR_REQUEST,
                                bRequest: 0x13,
                                wValue: 0,
                                wIndex: 0,
                                wLength: 6,
                            },
                            &mut descriptors,
                        )
                        .await;
                    if let Ok(_sz) = rc {
                        defmt::println!(
                            "AX88772 MAC {:x}",
                            &descriptors[0..6]
                        );
                    } else {
                        defmt::println!("fetched {:?}", rc);
                    }
                }
            }
        }
    }

    #[task(binds = USBCTRL_IRQ, shared = [&shared], priority = 2)]
    fn usb_interrupt(cx: usb_interrupt::Context) {
        cx.shared.shared.on_irq();
    }
}
