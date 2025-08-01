#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use panic_probe as _;
use rp_pico as _; // includes boot2

#[rtic::app(device = rp_pico::hal::pac, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use core::future::Future;
    use core::pin::pin;
    use cotton_usb_host::device::identify::IdentifyFromDescriptors;
    use cotton_usb_host::host::rp2040::{UsbShared, UsbStatics};
    use cotton_usb_host::usb_bus::{DeviceEvent, HubState, UsbBus};
    use cotton_usb_host_hid::{hid, Hid, IdentifyHid};
    use futures_util::StreamExt;
    use rp_pico::pac;
    use rtic_monotonics::rp2040::prelude::*;
    use static_cell::ConstStaticCell;
    use cotton_usb_host::wire::ShowDescriptors;

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

    fn rtic_delay(ms: usize) -> impl Future<Output = ()> {
        Mono::delay(<Mono as rtic_monotonics::Monotonic>::Duration::millis(
            ms as u64,
        ))
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
        let hub_state = HubState::default();
        let stack = UsbBus::new(driver);

        let mut p = pin!(stack.device_events(&hub_state, rtic_delay));

        loop {
            defmt::println!("loop");
            let device = p.next().await;

            if let Some(DeviceEvent::EnumerationError(h, p, e)) = device {
                defmt::println!(
                    "Enumeration error {} on hub {} port {}",
                    e,
                    h,
                    p
                );
            }

            defmt::println!("{:?}", hub_state.topology());

            if let Some(DeviceEvent::Connect(device, info)) = device {
                defmt::println!("Got device {:x} {:x}", device, info);

                let _ = stack.get_configuration(&device, &mut ShowDescriptors).await;

                let mut hid = IdentifyHid::default();
                let Ok(()) = stack.get_configuration(&device, &mut hid).await
                else {
                    continue;
                };
                if let Some(cfg) = hid.identify() {
                    defmt::println!("Could be HID");
                    let Ok(device) = stack.configure(device, cfg).await else {
                        continue;
                    };
                    let address = device.address();
                    let Ok(mut ms) = Hid::new(&stack, device) else {
                        continue;
                    };
                    defmt::println!("Is HID!");

                    let hid_stream = pin!(ms.handle());

                    enum Event {
                        Report(hid::HidReport),
                        Device(DeviceEvent),
                    }

                    let mut stream = futures::stream::select(
                        hid_stream.map(Event::Report),
                        p.as_mut().map(Event::Device),
                    );

                    loop {
                        if let Some(ev) = stream.next().await {
                            match ev {
                                Event::Device(ev) => {
                                    if let DeviceEvent::Disconnect(bs) = ev {
                                        if bs.contains(address) {
                                            defmt::println!("HID disconnect");
                                            break;
                                        }
                                    }
                                }
                                Event::Report(hr) => {
                                    defmt::println!("HID report {:?}", hr);
                                }
                            }
                        }
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
