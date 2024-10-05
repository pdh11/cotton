#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use defmt_rtt as _;
use panic_probe as _;
use rp_pico as _; // includes boot2

#[rtic::app(device = rp_pico::hal::pac, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll};
    use embedded_hal::delay::DelayNs;
    use rp_pico::pac;
    use rtic_common::waker_registration::CriticalSectionWakerRegistration;
    use rtic_monotonics::rp2040::prelude::*;

    #[inline(never)]
    unsafe fn unique_flash_id() -> cotton_unique::UniqueId {
        let mut unique_bytes = [0u8; 16];
        cortex_m::interrupt::free(|_| {
            rp2040_flash::flash::flash_unique_id(&mut unique_bytes, true);
        });
        cotton_unique::UniqueId::new(&unique_bytes)
    }

    #[repr(C)]
    #[derive(defmt::Format, Copy, Clone)]
    #[allow(non_snake_case)] // These names are from USB 2.0 table 9-2
    pub struct SetupPacket {
        bmRequestType: u8,
        bRequest: u8,
        wValue: u16,
        wIndex: u16,
        wLength: u16,
    }

    #[repr(C)]
    #[derive(defmt::Format, Copy, Clone)]
    #[allow(non_snake_case)] // These names are from USB 2.0 table 9-8
    pub struct DeviceDescriptor {
        bLength: u8,
        bDescriptorType: u8,
        bcdUSB: [u8; 2],
        bDeviceClass: u8,
        bDeviceSubClass: u8,
        bDeviceProtocol: u8,
        bMaxPacketSize0: u8,

        idVendor: [u8; 2],
        idProduct: [u8; 2],
        bcdDevice: [u8; 2],
        iManufacturer: u8,
        iProduct: u8,
        iSerialNumber: u8,
        bNumConfigurations: u8,
    }

    #[repr(C)]
    #[derive(defmt::Format, Copy, Clone)]
    #[allow(non_snake_case)] // These names are from USB 2.0 table 9-10
    pub struct ConfigurationDescriptor {
        bLength: u8,
        bDescriptorType: u8,
        wTotalLength: [u8; 2],
        bNumInterfaces: u8,
        bConfigurationValue: u8,
        iConfiguration: u8,
        bmAttributes: u8,
        bMaxPower: u8,
    }

    impl ConfigurationDescriptor {
        pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
            if bytes.len() >= core::mem::size_of::<Self>() {
                Some(unsafe { *(bytes as *const [u8] as *const Self) })
            } else {
                None
            }
        }
    }

    #[repr(C)]
    #[derive(defmt::Format, Copy, Clone)]
    #[allow(non_snake_case)] // These names are from USB 2.0 table 9-12
    pub struct InterfaceDescriptor {
        bLength: u8,
        bDescriptorType: u8,
        bInterfaceNumber: u8,
        bAlternateSetting: u8,
        bNumEndpoints: u8,
        bInterfaceClass: u8,
        bInterfaceSubClass: u8,
        bInterfaceProtocol: u8,
        iInterface: u8,
    }

    impl InterfaceDescriptor {
        pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
            if bytes.len() >= core::mem::size_of::<Self>() {
                Some(unsafe { *(bytes as *const [u8] as *const Self) })
            } else {
                None
            }
        }
    }

    #[repr(C)]
    #[derive(defmt::Format, Copy, Clone)]
    #[allow(non_snake_case)] // These names are from USB 2.0 table 9-13
    pub struct EndpointDescriptor {
        bLength: u8,
        bDescriptorType: u8,
        bEndpointAddress: u8,
        bmAttributes: u8,
        wMaxPacketSize: [u8; 2],
        bInterval: u8,
    }

    impl EndpointDescriptor {
        pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
            if bytes.len() >= core::mem::size_of::<Self>() {
                Some(unsafe { *(bytes as *const [u8] as *const Self) })
            } else {
                None
            }
        }
    }

    // For request_type (USB 2.0 table 9-2)
    pub const DEVICE_TO_HOST: u8 = 0x80;
    pub const HOST_TO_DEVICE: u8 = 0;
    pub const STANDARD_REQUEST: u8 = 0;
    pub const CLASS_REQUEST: u8 = 0x20;
    pub const VENDOR_REQUEST: u8 = 0x40;
    pub const RECIPIENT_DEVICE: u8 = 0;
    pub const RECIPIENT_INTERFACE: u8 = 1;
    pub const RECIPIENT_ENDPOINT: u8 = 2;
    pub const RECIPIENT_OTHER: u8 = 3;

    // For request (USB 2.0 table 9-4)
    pub const GET_STATUS: u8 = 0;
    pub const CLEAR_FEATURE: u8 = 1;
    pub const SET_FEATURE: u8 = 3;
    pub const SET_ADDRESS: u8 = 5;
    pub const GET_DESCRIPTOR: u8 = 6;
    pub const SET_DESCRIPTOR: u8 = 7;
    pub const SET_CONFIGURATION: u8 = 9;

    // Descriptor types (USB 2.0 table 9-5)
    pub const DEVICE_DESCRIPTOR: u8 = 1;
    pub const CONFIGURATION_DESCRIPTOR: u8 = 2;
    pub const STRING_DESCRIPTOR: u8 = 3;
    pub const INTERFACE_DESCRIPTOR: u8 = 4;
    pub const ENDPOINT_DESCRIPTOR: u8 = 5;

    #[shared]
    struct Shared {
        waker: CriticalSectionWakerRegistration,
    }

    #[local]
    struct Local {
        stack: UsbStack,
    }

    rp2040_timer_monotonic!(Mono); // 1MHz!

    #[derive(defmt::Format, Copy, Clone)]
    pub enum UsbError {
        Nak,
        Stall,
        Timeout,
        Overflow,
        BitStuffError,
        CrcError,
        DataSeqError,
        BufferTooSmall,
    }

    pub struct UsbFuture<'a> {
        waker: &'a CriticalSectionWakerRegistration,
        // pipe: u8
    }

    impl<'a> UsbFuture<'a> {
        fn new(waker: &'a CriticalSectionWakerRegistration) -> Self {
            Self { waker }
        }
    }

    impl<'a> Future for UsbFuture<'a> {
        type Output = pac::usbctrl_regs::sie_status::R;

        fn poll(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Self::Output> {
            defmt::println!("register");
            self.waker.register(cx.waker());

            let regs = unsafe { pac::USBCTRL_REGS::steal() };
            let status = regs.sie_status().read();
            if (status.bits() & 0xFF04_0000) != 0 {
                defmt::println!("ready {:x}", status.bits());
                regs.sie_status().write(|w| unsafe { w.bits(0xFF04_0000) });
                Poll::Ready(status)
            } else {
                defmt::println!("pending");
                Poll::Pending
            }
        }
    }

    pub struct UsbStack {
        regs: pac::USBCTRL_REGS,
        dpram: pac::USBCTRL_DPRAM,
    }

    impl UsbStack {
        pub fn new(
            regs: pac::USBCTRL_REGS,
            dpram: pac::USBCTRL_DPRAM,
        ) -> Self {
            Self { regs, dpram }
        }

        pub async fn control_transfer_in<
            F: Future<Output = pac::usbctrl_regs::sie_status::R>,
        >(
            &self,
            address: u8,
            setup: SetupPacket,
            buf: &mut [u8],
            f: F,
        ) -> Result<usize, UsbError> {
            assert!(setup.wLength <= 64);

            self.dpram.epx_control().write(|w| {
                unsafe {
                    w.buffer_address().bits(0x180);
                }
                w.enable().set_bit()
            });
            self.dpram.ep_buffer_control(0).write(|w| {
                w.last_0().set_bit();
                w.full_0().clear_bit();
                w.pid_0().set_bit();
                unsafe { w.length_0().bits(setup.wLength) }
            });

            cortex_m::asm::delay(12);

            self.dpram
                .ep_buffer_control(0)
                .modify(|_, w| w.available_0().set_bit());

            // USB 2.0 s9.4.3
            self.dpram.setup_packet_low().write(|w| unsafe {
                w.bmrequesttype().bits(setup.bmRequestType);
                w.brequest().bits(setup.bRequest);
                w.wvalue().bits(setup.wValue)
            });
            self.dpram
                .setup_packet_high()
                .write(|w| unsafe { w.wlength().bits(setup.wLength) });

            self.regs
                .sie_status()
                .write(|w| unsafe { w.bits(0xFFFF_FFFF) });

            self.regs.addr_endp().write(|w| unsafe {
                w.endpoint().bits(0);
                w.address().bits(address)
            });

            self.regs.inte().write(|w| {
                w.trans_complete()
                    .set_bit()
                    .error_data_seq()
                    .set_bit()
                    .stall()
                    .set_bit()
                    .error_rx_timeout()
                    .set_bit()
                    .error_rx_overflow()
                    .set_bit()
                    .error_bit_stuff()
                    .set_bit()
                    .error_crc()
                    .set_bit()
            });

            self.regs.sie_ctrl().modify(|_, w| {
                w.receive_data().set_bit();
                w.send_setup().set_bit()
            });

            cortex_m::asm::delay(12);

            unsafe {
                pac::NVIC::unpend(pac::Interrupt::USBCTRL_IRQ);
                pac::NVIC::unmask(pac::Interrupt::USBCTRL_IRQ);
            }

            self.regs
                .sie_ctrl()
                .modify(|_, w| w.start_trans().set_bit());

            let status = f.await;

            self.regs.inte().write(|w| {
                w.trans_complete()
                    .clear_bit()
                    .error_data_seq()
                    .clear_bit()
                    .stall()
                    .clear_bit()
                    .error_rx_timeout()
                    .clear_bit()
                    .error_rx_overflow()
                    .clear_bit()
                    .error_bit_stuff()
                    .clear_bit()
                    .error_crc()
                    .clear_bit()
            });

            //            let status = self.regs.sie_status().read();
            if !status.trans_complete().bit() {
                let bcr = self.dpram.ep_buffer_control(0).read();
                let ctrl = self.regs.sie_ctrl().read();
                defmt::println!(
                    "bcr=0x{:x} sie_status=0x{:x} sie_ctrl=0x{:x}",
                    bcr.bits(),
                    status.bits(),
                    ctrl.bits()
                );
                if status.data_seq_error().bit() {
                    return Err(UsbError::DataSeqError);
                }
                if status.stall_rec().bit() {
                    return Err(UsbError::Stall);
                }
                if status.nak_rec().bit() {
                    return Err(UsbError::Nak);
                }
                if status.rx_overflow().bit() {
                    return Err(UsbError::Overflow);
                }
                if status.bit_stuff_error().bit() {
                    return Err(UsbError::BitStuffError);
                }
                if status.crc_error().bit() {
                    return Err(UsbError::CrcError);
                }
                return Err(UsbError::Timeout);
            }

            let transferred = self
                .dpram
                .ep_buffer_control(0)
                .read()
                .length_0()
                .bits()
                .into();
            if buf.len() < transferred {
                return Err(UsbError::BufferTooSmall);
            }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (0x5010_0000 + 0x180) as *const u8,
                    &mut buf[0] as *mut u8,
                    transferred,
                );
            }
            Ok(transferred)
        }
    }

    pub fn show_descriptors(buf: &[u8]) {
        let mut index = 0;

        while buf.len() > index + 2 {
            let dlen = buf[index] as usize;
            let dtype = buf[index + 1];

            if buf.len() < index + dlen {
                defmt::println!("{}-byte dtor truncated", dlen);
                return;
            }

            match dtype {
                CONFIGURATION_DESCRIPTOR => {
                    let c = ConfigurationDescriptor::try_from_bytes(
                        &buf[index..index + dlen],
                    )
                    .unwrap();
                    defmt::println!("  {}", c);
                }
                INTERFACE_DESCRIPTOR => {
                    defmt::println!(
                        "  {}",
                        InterfaceDescriptor::try_from_bytes(
                            &buf[index..index + dlen]
                        )
                        .unwrap()
                    );
                }
                ENDPOINT_DESCRIPTOR => {
                    defmt::println!(
                        "  {}",
                        EndpointDescriptor::try_from_bytes(
                            &buf[index..index + dlen]
                        )
                        .unwrap()
                    );
                }
                _ => {
                    defmt::println!("  type {} len {} skipped", dtype, dlen);
                }
            }

            index += dlen;
        }
    }

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

        let clocks = rp2040_hal::clocks::init_clocks_and_plls(
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

        let mut timer =
            rp2040_hal::Timer::new(device.TIMER, &mut resets, &clocks);

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

        let regs = device.USBCTRL_REGS;
        let dpram = device.USBCTRL_DPRAM;

        resets.reset().modify(|_, w| w.usbctrl().set_bit());
        resets.reset().modify(|_, w| w.usbctrl().clear_bit());

        regs.usb_muxing().modify(|_, w| {
            w.to_phy().set_bit();
            w.softcon().set_bit()
        });
        regs.usb_pwr().modify(|_, w| {
            w.vbus_detect().set_bit();
            w.vbus_detect_override_en().set_bit()
        });
        regs.main_ctrl().modify(|_, w| {
            w.sim_timing().clear_bit();
            w.host_ndevice().set_bit();
            w.controller_en().set_bit()
        });
        regs.sie_ctrl().write(|w| {
            w.pulldown_en().set_bit();
            w.vbus_en().set_bit();
            w.keep_alive_en().set_bit();
            w.sof_en().set_bit()
        });

        loop {
            let status = regs.sie_status().read();
            defmt::println!("sie_status=0x{:x}", status.bits());
            match status.speed().bits() {
                1 => {
                    defmt::println!("LS detected");
                    break;
                }
                2 => {
                    defmt::println!("FS detected");
                    break;
                }
                _ => {}
            }
            timer.delay_ms(250);
        }

        regs.sie_ctrl().modify(|_, w| w.reset_bus().set_bit());

        timer.delay_ms(50);

        regs.sie_ctrl().modify(|_, w| w.reset_bus().clear_bit());
        /*
                // set up EPx and EPx buffer control
                // write setup packet
                // start transaction
                dpram.epx_control().write(|w| {
                    unsafe {
                        w.buffer_address().bits(0x180);
                    }
                    w.enable().set_bit()
                });
                dpram.ep_buffer_control(0).write(|w| {
                    w.last_0().set_bit();
                    w.full_0().clear_bit();
                    w.pid_0().set_bit();
                    unsafe { w.length_0().bits(18) }
                });

                cortex_m::asm::delay(12);

                dpram
                    .ep_buffer_control(0)
                    .modify(|_, w| w.available_0().set_bit());

                // USB 2.0 s9.4.3
                dpram.setup_packet_low().write(|w| unsafe {
                    w.bmrequesttype().bits(DEVICE_TO_HOST);
                    w.brequest().bits(GET_DESCRIPTOR);
                    w.wvalue().bits((DEVICE_DESCRIPTOR as u16) << 8)
                });
                dpram
                    .setup_packet_high()
                    .write(|w| unsafe { w.wlength().bits(18) });

                regs.addr_endp().write(|w| unsafe {
                    w.endpoint().bits(0);
                    w.address().bits(0)
                });
                defmt::println!(
                    "bcr=0x{:x}",
                    dpram.ep_buffer_control(0).read().bits()
                );

                regs.sie_ctrl().modify(|_, w| {
                    w.receive_data().set_bit();
                    w.send_setup().set_bit()
                });

                cortex_m::asm::delay(12);

                regs.sie_ctrl().modify(|_, w| w.start_trans().set_bit());

                loop {
                    let status = regs.sie_status().read();
                    let bcr = dpram.ep_buffer_control(0).read();
                    let ctrl = regs.sie_ctrl().read();
                    defmt::println!(
                        "bcr=0x{:x} sie_status=0x{:x} sie_ctrl=0x{:x}",
                        bcr.bits(),
                        status.bits(),
                        ctrl.bits()
                    );
                    if status.trans_complete().bit() {
                        break;
                    }
                    timer.delay_ms(250);
                }

                let s: DeviceDescriptor =
                    unsafe { *((0x5010_0000 + 0x180) as *const DeviceDescriptor) };

                defmt::println!("s={:?}", s);
        */
        let stack = UsbStack::new(regs, dpram);

        usb_task::spawn().unwrap();

        (
            Shared {
                waker: CriticalSectionWakerRegistration::new(),
            },
            Local { stack },
        )
    }

    #[task(local = [stack], shared = [&waker], priority = 2)]
    async fn usb_task(cx: usb_task::Context) {
        let stack = cx.local.stack;
        let future = UsbFuture::new(cx.shared.waker);
        let mut descriptors = [0u8; 64];
        defmt::println!("fetching1");
        let rc = stack
            .control_transfer_in(
                0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((DEVICE_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 8,
                },
                &mut descriptors,
                future,
            )
            .await;
        defmt::println!("fetched: {:?}", rc);
        if rc.is_ok() {
            defmt::println!(
                "Device: len {}, class {}, subclass {}, mps0 {}",
                descriptors[0],
                descriptors[4],
                descriptors[5],
                descriptors[7]
            );
        }

        defmt::println!("fetching2");
        let future = UsbFuture::new(cx.shared.waker);
        let rc = stack
            .control_transfer_in(
                0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((CONFIGURATION_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 8,
                },
                &mut descriptors,
                future,
            )
            .await;
        defmt::println!("fetched: {:?}", rc);
        if let Ok(sz) = rc {
            show_descriptors(&descriptors[0..sz]);
        }
    }

    #[task(binds = USBCTRL_IRQ, shared = [&waker], priority = 2)]
    fn usb_interrupt(cx: usb_interrupt::Context) {
        pac::NVIC::mask(pac::Interrupt::USBCTRL_IRQ);
        defmt::println!("IRQ");
        cx.shared.waker.wake();
        defmt::println!("woke");
    }
}
