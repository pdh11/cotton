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
    use cotton_usb_host::device::mass_storage::{
        AsyncBlockDevice, IdentifyMassStorage, MassStorage, ScsiBlockDevice,
        ScsiDevice,
    };
    use cotton_usb_host::host::rp2040::{UsbShared, UsbStatics};
    use cotton_usb_host::host_controller::HostController;
    use cotton_usb_host::usb_bus::{
        DataPhase, DeviceEvent, DeviceInfo, HubState, UsbBus, UsbDevice,
        UsbError,
    };
    use cotton_usb_host::wire::{
        SetupPacket, ShowDescriptors, DEVICE_TO_HOST, HOST_TO_DEVICE,
        VENDOR_REQUEST,
    };
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

    struct AX88772<'a, T: HostController> {
        bus: &'a UsbBus<T>,
        device: UsbDevice,
    }

    const MII_ADVERTISE: u8 = 4;
    const MII_BMCR: u8 = 0;
    const ADVERTISE_ALL: u16 = 0x1E1; // /usr/include/linux/mii.h
    const BMCR_ANENABLE: u16 = 0x1000;
    const BMCR_ANRESTART: u16 = 0x200;

    impl<'a, T: HostController> AX88772<'a, T> {
        pub fn new(bus: &'a UsbBus<T>, device: UsbDevice) -> Self {
            Self { bus, device }
        }

        async fn vendor_command(
            &self,
            request: u8,
            value: u16,
            index: u16,
            data: DataPhase<'_>,
        ) -> Result<(), UsbError> {
            let (request_type, length) = match data {
                DataPhase::Out(bytes) => {
                    (HOST_TO_DEVICE | VENDOR_REQUEST, bytes.len() as u16)
                }
                DataPhase::In(ref bytes) => {
                    (DEVICE_TO_HOST | VENDOR_REQUEST, bytes.len() as u16)
                }
                DataPhase::None => (HOST_TO_DEVICE | VENDOR_REQUEST, 0),
            };
            self.bus
                .control_transfer(
                    &self.device,
                    SetupPacket {
                        bmRequestType: request_type,
                        bRequest: request,
                        wValue: value,
                        wIndex: index,
                        wLength: length,
                    },
                    data,
                )
                .await?;
            Ok(())
        }

        async fn write_phy(
            &self,
            phy_id: u8,
            reg: u8,
            value: u16,
        ) -> Result<(), UsbError> {
            let data = value.to_le_bytes();

            self.vendor_command(
                0x8,
                phy_id as u16,
                reg as u16,
                DataPhase::Out(&data),
            )
            .await?;
            Ok(())
        }

        pub async fn init(&self) -> Result<(), UsbError> {
            let mut data = [0u8; 6];
            self.vendor_command(0x13, 0, 0, DataPhase::In(&mut data[0..6]))
                .await?;

            defmt::println!("AX88772 MAC {:x}", &data[0..6]);

            self.vendor_command(0x19, 0, 0, DataPhase::In(&mut data[0..2]))
                .await?;

            defmt::println!("PHY id {:x} {:x}", data[0], data[1]);

            // Select PHY
            self.vendor_command(0x22, 1, 0, DataPhase::None).await?;

            // Enable PHY
            self.vendor_command(0x20, 0x28, 0, DataPhase::None).await?;

            Mono::delay(150.millis()).await;

            // Switch to SW PHY control
            self.vendor_command(0x6, 0, 0, DataPhase::None).await?;

            self.write_phy(0x10, MII_ADVERTISE, ADVERTISE_ALL).await?;
            self.write_phy(0x10, MII_BMCR, BMCR_ANENABLE | BMCR_ANRESTART)
                .await?;

            // Switch back to HW PHY control
            self.vendor_command(0xA, 0, 0, DataPhase::None).await?;

            defmt::println!("AX88772 OK");

            Ok(())
        }
    }

    fn identify_ax88772(info: &DeviceInfo) -> Option<u8> {
        if info.vid == 0x0B95 && info.pid == 0x7720 {
            Some(1)
        } else {
            None
        }
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

                if let Some(cfg) = identify_ax88772(&info) {
                    let Ok(device) = stack.configure(device, cfg).await else {
                        continue;
                    };
                    let otge = AX88772::new(&stack, device);
                    if let Err(e) = otge.init().await {
                        defmt::println!("error {}", e);
                    }
                } else {
                    let mut ims = IdentifyMassStorage::default();
                    let Ok(()) =
                        stack.get_configuration(&device, &mut ims).await
                    else {
                        continue;
                    };
                    if let Some(cfg) = ims.identify() {
                        defmt::println!("Could be MSC");
                        let Ok(device) = stack.configure(device, cfg).await
                        else {
                            continue;
                        };
                        let Ok(ms) = MassStorage::new(&stack, device) else {
                            continue;
                        };
                        let device = ScsiDevice::new(ms);
                        defmt::println!("Is MSC!");
                        rtic_delay(1500).await;
                        /*
                        let Ok(info) = device.inquiry().await else {
                            continue;
                        };
                        if info.peripheral_type != PeripheralType::Disk {
                            continue;
                        }

                        rtic_delay(1500).await;
                        defmt::println!("Is MSC DASD");
                        rtic_delay(1500).await;

                        let Ok(()) = device.test_unit_ready().await else {
                            defmt::println!("Unit NOT ready");
                            device.request_sense().await;
                            continue;
                        };
                        */

                        let mut abd = ScsiBlockDevice::new(device);
                        defmt::println!("{:?}", abd.capacity().await);
                    } else if let Err(e) = stack
                        .get_configuration(&device, &mut ShowDescriptors)
                        .await
                    {
                        defmt::println!("error {}", e);
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
