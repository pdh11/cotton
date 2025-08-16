#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use panic_probe as _;
use rp235x_hal as hal;

#[link_section = ".start_block"]
#[used]
pub static IMAGE_DEF: hal::block::ImageDef =
    hal::block::ImageDef::secure_exe();

#[rtic::app(device = rp235x_hal::pac, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use core::future::Future;
    use core::pin::pin;
    use cotton_scsi::{
        AsyncBlockDevice, PeripheralType, ScsiBlockDevice, ScsiDevice,
    };
    use cotton_usb_host::device::identify::IdentifyFromDescriptors;
    use cotton_usb_host::host::rp2040::{UsbShared, UsbStatics};
    use cotton_usb_host::usb_bus::{DeviceEvent, HubState, UsbBus};
    use cotton_usb_host::wire::ShowDescriptors;
    use cotton_usb_host_msc::{IdentifyMassStorage, MassStorage};
    use futures_util::StreamExt;
    use rp235x_pac as pac;
    use rtic_monotonics::rp235x::prelude::*;
    use static_cell::ConstStaticCell;

    #[shared]
    struct Shared {
        shared: &'static UsbShared,
    }

    #[local]
    struct Local {
        resets: pac::RESETS,
        regs: Option<pac::USB>,
        dpram: Option<pac::USB_DPRAM>,
    }

    rp235x_timer_monotonic!(Mono); // 1MHz!

    #[init()]
    fn init(c: init::Context) -> (Shared, Local) {
        defmt::println!(
            "{} from {} {}-g{}",
            env!("CARGO_BIN_NAME"),
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            git_version::git_version!()
        );

        let device = c.device;
        let mut resets = device.RESETS;
        let mut watchdog =
            rp235x_hal::watchdog::Watchdog::new(device.WATCHDOG);

        let _clocks = rp235x_hal::clocks::init_clocks_and_plls(
            12_000_000,
            device.XOSC,
            device.CLOCKS,
            device.PLL_SYS,
            device.PLL_USB,
            &mut resets,
            &mut watchdog,
        )
        .ok()
        .unwrap();

        Mono::start(device.TIMER0, &resets);

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
        /*
        unsafe {
            rp2040_hal::pac::TIMER::steal()
                .dbgpause()
                .write(|w| w.bits(0));
        }
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
                regs: Some(device.USB),
                dpram: Some(device.USB_DPRAM),
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

                let mut ims = IdentifyMassStorage::default();
                let Ok(()) = stack.get_configuration(&device, &mut ims).await
                else {
                    continue;
                };
                if let Some(cfg) = ims.identify() {
                    defmt::println!("Could be MSC");
                    let Ok(device) = stack.configure(device, cfg).await else {
                        continue;
                    };
                    let Ok(ms) = MassStorage::new(&stack, device) else {
                        continue;
                    };
                    let mut device = ScsiDevice::new(ms);
                    defmt::println!("Is MSC!");
                    rtic_delay(1500).await;

                    let Ok(info) = device.inquiry().await else {
                        continue;
                    };
                    if info.peripheral_type != PeripheralType::Disk {
                        continue;
                    }

                    rtic_delay(1500).await;
                    defmt::println!("Is MSC DASD");

                    let Ok(()) = device.test_unit_ready().await else {
                        defmt::println!("Unit NOT ready");
                        continue;
                    };

                    //defmt::println!("{:?}", device.supported_vpd_pages().await);
                    //defmt::println!("{:?}", device.block_limits_page().await);

                    let mut abd = ScsiBlockDevice::new(device);

                    //defmt::println!("{:?}", abd.query_commands().await);

                    let device_info = match abd.device_info().await {
                        Ok(info) => info,
                        Err(e) => {
                            defmt::println!("device_info: {:?}", e);
                            continue;
                        }
                    };
                    let capacity =
                        device_info.blocks * (device_info.block_size as u64);
                    defmt::println!(
                        "{} blocks x {} bytes = {} B / {} KB / {} MB / {} GB",
                        device_info.blocks,
                        device_info.block_size,
                        capacity,
                        (capacity + (1 << 9)) >> 10,
                        (capacity + (1 << 19)) >> 20,
                        (capacity + (1 << 29)) >> 30
                    );

                    let mut buf = [0u8; 512];
                    buf[42] = 43;

                    let rc = abd.write_blocks(2, 1, &buf).await;
                    defmt::println!("write16: {:?}", rc);

                    buf[42] = 0;

                    let rc = abd.read_blocks(2, 1, &mut buf).await;
                    defmt::println!("read10: {:?}", rc);

                    assert!(buf[42] == 43);

                    rtic_delay(1500).await;
                    defmt::println!("MSC OK");
                } else if let Err(e) = stack
                    .get_configuration(&device, &mut ShowDescriptors)
                    .await
                {
                    defmt::println!("error {}", e);
                }
            }
        }
    }

    #[task(binds = USBCTRL_IRQ, shared = [&shared], priority = 2)]
    fn usb_interrupt(cx: usb_interrupt::Context) {
        cx.shared.shared.on_irq();
    }
}
