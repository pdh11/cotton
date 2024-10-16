#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use defmt_rtt as _;
use panic_probe as _;
use rp_pico as _; // includes boot2

#[rtic::app(device = rp_pico::hal::pac, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use core::pin::pin;
    use cotton_usb_host::host::rp2040::{UsbShared, UsbStatics};
    use cotton_usb_host::types::{
        parse_descriptors, SetupPacket, ShowDescriptors,
        CONFIGURATION_DESCRIPTOR, DEVICE_TO_HOST, GET_DESCRIPTOR,
        VENDOR_REQUEST,
    };
    use cotton_usb_host::usb_bus::{DataPhase, DeviceEvent, UsbBus};
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

    #[task(local = [regs, dpram, resets], shared = [&shared], priority = 2)]
    async fn usb_task(cx: usb_task::Context) {
        static USB_STATICS: ConstStaticCell<UsbStatics> =
            ConstStaticCell::new(UsbStatics::new());
        let statics = USB_STATICS.take();

        let driver = cotton_usb_host::host::rp2040::Rp2040HostController::new(
            cx.local.resets,
            cx.local.regs.take().unwrap(),
            cx.local.dpram.take().unwrap(),
            cx.shared.shared,
            statics,
        );
        let stack = UsbBus::new(driver);

        let mut p = pin!(stack.device_events());

        loop {
            let device = p.next().await;

            defmt::println!("{:?}", stack.topology());

            if let Some(DeviceEvent::Connect(device)) = device {
                defmt::println!("Got device {:x}", device);

                defmt::trace!("fetching2");
                let mut descriptors = [0u8; 64];
                let rc = stack
                    .control_transfer(
                        1,
                        device.packet_size_ep0,
                        SetupPacket {
                            bmRequestType: DEVICE_TO_HOST,
                            bRequest: GET_DESCRIPTOR,
                            wValue: ((CONFIGURATION_DESCRIPTOR as u16) << 8),
                            wIndex: 0,
                            wLength: 64,
                        },
                        DataPhase::In(&mut descriptors),
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
                        .control_transfer(
                            1,
                            device.packet_size_ep0,
                            SetupPacket {
                                bmRequestType: DEVICE_TO_HOST | VENDOR_REQUEST,
                                bRequest: 0x13,
                                wValue: 0,
                                wIndex: 0,
                                wLength: 6,
                            },
                            DataPhase::In(&mut descriptors),
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
