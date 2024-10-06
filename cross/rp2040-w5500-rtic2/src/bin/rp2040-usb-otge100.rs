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

    #[derive(Copy, Clone)]
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
            defmt::trace!("register");
            self.waker.register(cx.waker());

            let regs = unsafe { pac::USBCTRL_REGS::steal() };
            let status = regs.sie_status().read();
            let ints = regs.ints().read();
            if ints.bits() != 0 {
                defmt::info!("ready {:x}", status.bits());
                regs.sie_status().write(|w| unsafe { w.bits(0xFF04_0000) });
                Poll::Ready(status)
            } else {
                defmt::trace!(
                    "pending ints={:x} st={:x}",
                    ints.bits(),
                    status.bits()
                );
                Poll::Pending
            }
        }
    }

    trait Packetiser {
        fn prepare(&mut self, reg: &pac::usbctrl_dpram::EP_BUFFER_CONTROL);
    }

    struct InPacketiser {
        next_prep: u8,
        remain: u16,
        packet_size: u16,
        need_zero_size_packet: bool,
    }

    impl InPacketiser {
        fn new(remain: u16, packet_size: u16) -> Self {
            Self {
                next_prep: 0,
                remain,
                packet_size,
                need_zero_size_packet: (remain % packet_size) == 0,
            }
        }

        fn next_packet(&mut self) -> Option<(u16, bool)> {
            if self.remain == 0 {
                if self.need_zero_size_packet {
                    self.need_zero_size_packet = false;
                    return Some((0, true));
                } else {
                    return None;
                }
            }
            if self.remain < self.packet_size {
                return Some((self.remain, true));
            }
            if self.remain > self.packet_size {
                return Some((self.packet_size, false));
            }
            Some((self.remain, false))
        }
    }

    impl Packetiser for InPacketiser {
        fn prepare(&mut self, reg: &pac::usbctrl_dpram::EP_BUFFER_CONTROL) {
            let val = reg.read();
            match self.next_prep {
                0 => {
                    if !val.available_0().bit() {
                        if let Some((this_packet, is_last)) =
                            self.next_packet()
                        {
                            self.remain -= this_packet;
                            reg.modify(|_, w| {
                                w.full_0().clear_bit();
                                w.pid_0().set_bit();
                                w.last_0().bit(is_last);
                                unsafe { w.length_0().bits(this_packet) };
                                w
                            });

                            cortex_m::asm::delay(12);

                            reg.modify(|_, w| w.available_0().set_bit());

                            self.next_prep = 1;
                        }
                    }
                }

                _ => {
                    if !val.available_1().bit() {
                        if let Some((this_packet, is_last)) =
                            self.next_packet()
                        {
                            self.remain -= this_packet;
                            reg.modify(|_, w| {
                                w.full_1().clear_bit();
                                w.pid_1().clear_bit();
                                w.last_1().bit(is_last);
                                unsafe { w.length_1().bits(this_packet) };
                                w
                            });

                            cortex_m::asm::delay(12);

                            reg.modify(|_, w| w.available_1().set_bit());

                            self.next_prep = 0;
                        }
                    }
                }
            }
        }
    }

    struct OutPacketiser<'a> {
        next_prep: u8,
        remain: usize,
        offset: usize,
        packet_size: usize,
        need_zero_size_packet: bool,
        buf: &'a [u8],
    }

    impl<'a> OutPacketiser<'a> {
        fn new(size: u16, packet_size: u16, buf: &'a [u8]) -> Self {
            Self {
                next_prep: 0,
                remain: size as usize,
                offset: 0,
                packet_size: packet_size as usize,
                need_zero_size_packet: (size % packet_size) == 0,
                buf,
            }
        }

        fn next_packet(&mut self) -> Option<(usize, bool)> {
            if self.remain == 0 {
                if self.need_zero_size_packet {
                    self.need_zero_size_packet = false;
                    return Some((0, true));
                } else {
                    return None;
                }
            }
            if self.remain < self.packet_size {
                return Some((self.remain, true));
            }
            if self.remain > self.packet_size {
                return Some((self.packet_size, false));
            }
            Some((self.remain, false))
        }
    }

    impl<'a> Packetiser for OutPacketiser<'a> {
        fn prepare(&mut self, reg: &pac::usbctrl_dpram::EP_BUFFER_CONTROL) {
            let val = reg.read();
            match self.next_prep {
                0 => {
                    if !val.available_0().bit() {
                        if let Some((this_packet, is_last)) =
                            self.next_packet()
                        {
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    &self.buf[self.offset] as *const u8,
                                    (0x5010_0000 + 0x180) as *mut u8,
                                    this_packet,
                                );
                            }
                            reg.modify(|_, w| {
                                // @todo Why is this "if" necessary?
                                if this_packet > 0 {
                                    w.full_0().set_bit();
                                }
                                w.pid_0().set_bit();
                                w.last_0().bit(is_last);
                                unsafe {
                                    w.length_0().bits(this_packet as u16)
                                };
                                w
                            });

                            cortex_m::asm::delay(12);

                            reg.modify(|_, w| w.available_0().set_bit());

                            self.remain -= this_packet;
                            self.offset += this_packet;
                            self.next_prep = 1;
                        }
                    }
                }

                _ => {
                    if !val.available_1().bit() {
                        if let Some((this_packet, is_last)) =
                            self.next_packet()
                        {
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    &self.buf[self.offset] as *const u8,
                                    (0x5010_0000 + 0x1C0) as *mut u8,
                                    this_packet,
                                );
                            }
                            reg.modify(|_, w| {
                                w.full_1().set_bit();
                                w.pid_1().clear_bit();
                                w.last_1().bit(is_last);
                                unsafe {
                                    w.length_1().bits(this_packet as u16)
                                };
                                w
                            });

                            cortex_m::asm::delay(12);

                            reg.modify(|_, w| w.available_1().set_bit());

                            self.remain -= this_packet;
                            self.offset += this_packet;
                            self.next_prep = 0;
                        }
                    }
                }
            }
        }
    }

    trait Depacketiser {
        fn retire(&mut self, reg: &pac::usbctrl_dpram::EP_BUFFER_CONTROL);
    }

    struct InDepacketiser<'a> {
        next_retire: u8,
        remain: usize,
        offset: usize,
        buf: &'a mut [u8],
    }

    impl<'a> InDepacketiser<'a> {
        fn new(size: u16, buf: &'a mut [u8]) -> Self {
            Self {
                next_retire: 0,
                remain: size as usize,
                offset: 0,
                buf,
            }
        }

        fn total(&self) -> usize {
            self.offset
        }
    }

    impl<'a> Depacketiser for InDepacketiser<'a> {
        fn retire(&mut self, reg: &pac::usbctrl_dpram::EP_BUFFER_CONTROL) {
            let val = reg.read();
            match self.next_retire {
                0 => {
                    if val.full_0().bit() {
                        let this_packet = core::cmp::min(
                            self.remain,
                            val.length_0().bits() as usize,
                        );
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                (0x5010_0000 + 0x180) as *const u8,
                                &mut self.buf[self.offset] as *mut u8,
                                this_packet,
                            );
                        }

                        self.remain -= this_packet;
                        self.offset += this_packet;
                        self.next_retire = 1;
                    }
                }
                _ => {
                    if val.full_1().bit() {
                        let this_packet = core::cmp::min(
                            self.remain,
                            val.length_1().bits() as usize,
                        );
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                (0x5010_0000 + 0x1C0) as *const u8,
                                &mut self.buf[self.offset] as *mut u8,
                                this_packet,
                            );
                        }

                        self.remain -= this_packet;
                        self.offset += this_packet;
                        self.next_retire = 0;
                    }
                }
            }
        }
    }

    struct OutDepacketiser {
        next_retire: u8,
    }

    impl OutDepacketiser {
        fn new() -> Self {
            Self { next_retire: 0 }
        }
    }

    impl Depacketiser for OutDepacketiser {
        fn retire(&mut self, reg: &pac::usbctrl_dpram::EP_BUFFER_CONTROL) {
            let val = reg.read();
            match self.next_retire {
                0 => {
                    if val.full_0().bit() {
                        self.next_retire = 1;
                    }
                }
                _ => {
                    if val.full_1().bit() {
                        self.next_retire = 0;
                    }
                }
            }
        }
    }

    pub struct UsbStack {
        regs: pac::USBCTRL_REGS,
        dpram: pac::USBCTRL_DPRAM,
        addresses_in_use: [u32; 4],
    }

    impl UsbStack {
        pub fn new(
            regs: pac::USBCTRL_REGS,
            dpram: pac::USBCTRL_DPRAM,
        ) -> Self {
            Self {
                regs,
                dpram,
                addresses_in_use: [0u32; 4],
            }
        }

        pub fn allocate_address(&mut self) -> Option<u8> {
            for i in 1..127 {
                if (self.addresses_in_use[i / 32] >> (i % 32)) == 0 {
                    self.addresses_in_use[i / 32] |= 1 << (i % 32);
                    return Some(i as u8);
                }
            }
            None
        }

        async fn control_transfer_inner(
            &self,
            address: u8,
            packet_size: u8,
            setup: SetupPacket,
            packetiser: &mut impl Packetiser,
            depacketiser: &mut impl Depacketiser,
            f: impl Future<Output = pac::usbctrl_regs::sie_status::R> + Copy,
        ) -> Result<(), UsbError> {
            let packets = setup.wLength / (packet_size as u16) + 1;
            defmt::info!("we'll need {} packets", packets);

            self.dpram.epx_control().write(|w| {
                unsafe {
                    w.buffer_address().bits(0x180);
                }
                if packets > 1 {
                    w.double_buffered().set_bit();
                    w.interrupt_per_buff().set_bit();
                }
                w.enable().set_bit()
            });

            self.dpram
                .ep_buffer_control(0)
                .write(|w| unsafe { w.bits(0) });
            packetiser.prepare(self.dpram.ep_buffer_control(0));

            // USB 2.0 s9.4.3
            self.dpram.setup_packet_low().write(|w| unsafe {
                w.bmrequesttype().bits(setup.bmRequestType);
                w.brequest().bits(setup.bRequest);
                w.wvalue().bits(setup.wValue)
            });
            self.dpram.setup_packet_high().write(|w| unsafe {
                w.wlength().bits(setup.wLength);
                w.windex().bits(setup.wIndex)
            });

            self.regs
                .sie_status()
                .write(|w| unsafe { w.bits(0xFFFF_FFFF) });

            self.regs.addr_endp().write(|w| unsafe {
                w.endpoint().bits(0);
                w.address().bits(address)
            });

            let mut started = false;

            loop {
                packetiser.prepare(self.dpram.ep_buffer_control(0));
                packetiser.prepare(self.dpram.ep_buffer_control(0));

                self.regs
                    .sie_status()
                    .write(|w| unsafe { w.bits(0xFF00_0000) });
                self.regs
                    .buff_status()
                    .write(|w| unsafe { w.bits(0xFFFF_FFFF) });
                self.regs.inte().write(|w| {
                    if packets > 2 {
                        w.buff_status().set_bit();
                    }
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

                unsafe {
                    pac::NVIC::unpend(pac::Interrupt::USBCTRL_IRQ);
                    pac::NVIC::unmask(pac::Interrupt::USBCTRL_IRQ);
                }

                defmt::info!(
                    "Initial bcr {:x}",
                    self.dpram.ep_buffer_control(0).read().bits()
                );

                if !started {
                    started = true;

                    self.regs.sie_ctrl().modify(|_, w| {
                        if setup.wLength > 0 {
                            if (setup.bmRequestType & DEVICE_TO_HOST) != 0 {
                                w.receive_data().set_bit();
                            } else {
                                w.send_data().set_bit();
                            }
                        }
                        // @todo Non-control transactions?
                        w.send_setup().set_bit()
                    });

                    cortex_m::asm::delay(12);

                    self.regs
                        .sie_ctrl()
                        .modify(|_, w| w.start_trans().set_bit());
                }

                let status = f.await;

                defmt::trace!("awaited");

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
                        .buff_status()
                        .clear_bit()
                });

                if status.trans_complete().bit() {
                    depacketiser.retire(self.dpram.ep_buffer_control(0));
                    break;
                }

                let bcr = self.dpram.ep_buffer_control(0).read();
                let ctrl = self.regs.sie_ctrl().read();
                let bstat = self.regs.buff_status().read();
                defmt::trace!(
                    "bcr=0x{:x} sie_status=0x{:x} sie_ctrl=0x{:x} bstat={:x}",
                    bcr.bits(),
                    status.bits(),
                    ctrl.bits(),
                    bstat.bits(),
                );
                if status.data_seq_error().bit() {
                    return Err(UsbError::DataSeqError);
                }
                if status.stall_rec().bit() {
                    return Err(UsbError::Stall);
                }
                // if status.nak_rec().bit() {
                //     return Err(UsbError::Nak);
                // }
                if status.rx_overflow().bit() {
                    return Err(UsbError::Overflow);
                }
                if status.rx_timeout().bit() {
                    return Err(UsbError::Timeout);
                }
                if status.bit_stuff_error().bit() {
                    return Err(UsbError::BitStuffError);
                }
                if status.crc_error().bit() {
                    return Err(UsbError::CrcError);
                }

                depacketiser.retire(self.dpram.ep_buffer_control(0));
                depacketiser.retire(self.dpram.ep_buffer_control(0));
            }

            let bcr = self.dpram.ep_buffer_control(0).read();
            let ctrl = self.regs.sie_ctrl().read();
            defmt::trace!(
                "COMPLETE bcr=0x{:x} sie_ctrl=0x{:x}",
                bcr.bits(),
                ctrl.bits()
            );
            depacketiser.retire(self.dpram.ep_buffer_control(0));
            Ok(())
        }

        pub async fn control_transfer_in<
            F: Future<Output = pac::usbctrl_regs::sie_status::R> + Copy,
        >(
            &self,
            address: u8,
            packet_size: u8,
            setup: SetupPacket,
            buf: &mut [u8],
            f: F,
        ) -> Result<usize, UsbError> {
            if buf.len() < setup.wLength as usize {
                return Err(UsbError::BufferTooSmall);
            }

            let mut packetiser =
                InPacketiser::new(setup.wLength, packet_size as u16);
            let mut depacketiser = InDepacketiser::new(setup.wLength, buf);

            self.control_transfer_inner(
                address,
                packet_size,
                setup,
                &mut packetiser,
                &mut depacketiser,
                f,
            )
            .await?;

            Ok(depacketiser.total())
        }

        pub async fn control_transfer_out<
            F: Future<Output = pac::usbctrl_regs::sie_status::R> + Copy,
        >(
            &self,
            address: u8,
            packet_size: u8,
            setup: SetupPacket,
            buf: &[u8],
            f: F,
        ) -> Result<(), UsbError> {
            if buf.len() < setup.wLength as usize {
                return Err(UsbError::BufferTooSmall);
            }

            let mut packetiser =
                OutPacketiser::new(setup.wLength, packet_size as u16, buf);
            let mut depacketiser = OutDepacketiser::new();

            self.control_transfer_inner(
                address,
                packet_size,
                setup,
                &mut packetiser,
                &mut depacketiser,
                f,
            )
            .await?;

            Ok(())
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
            defmt::trace!("sie_status=0x{:x}", status.bits());
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
        defmt::trace!("fetching1");
        let rc = stack
            .control_transfer_in(
                0,
                8,
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
        let mps0 = if rc.is_ok() {
            defmt::println!(
                "Device: len {}, class {}, subclass {}, mps0 {}",
                descriptors[0],
                descriptors[4],
                descriptors[5],
                descriptors[7]
            );
            descriptors[7]
        } else {
            defmt::println!("fetched {:?}", rc);
            8
        };

        defmt::trace!("setting");
        let future = UsbFuture::new(cx.shared.waker);
        let rc = stack
            .control_transfer_out(
                0,
                mps0,
                SetupPacket {
                    bmRequestType: HOST_TO_DEVICE,
                    bRequest: SET_ADDRESS,
                    wValue: 1,
                    wIndex: 0,
                    wLength: 0,
                },
                &descriptors,
                future,
            )
            .await;
        defmt::println!("fetched {:?}", rc);

        defmt::trace!("fetching3");
        let future = UsbFuture::new(cx.shared.waker);
        let mut vid = 0;
        let mut pid = 0;
        let rc = stack
            .control_transfer_in(
                1,
                mps0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((DEVICE_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 18,
                },
                &mut descriptors,
                future,
            )
            .await;
        if let Ok(_sz) = rc {
            vid = u16::from_le_bytes([descriptors[8], descriptors[9]]);
            pid = u16::from_le_bytes([descriptors[10], descriptors[11]]);
            defmt::println!("VID:PID = {:04x}:{:04x}", vid, pid);
        } else {
            defmt::println!("fetched {:?}", rc);
        }

        defmt::trace!("fetching2");
        let future = UsbFuture::new(cx.shared.waker);
        let rc = stack
            .control_transfer_in(
                1,
                mps0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((CONFIGURATION_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 39,
                },
                &mut descriptors,
                future,
            )
            .await;
        if let Ok(sz) = rc {
            show_descriptors(&descriptors[0..sz]);
        } else {
            defmt::println!("fetched {:?}", rc);
        }

        if vid == 0x0B95 && pid == 0x7720 {
            // ASIX AX88772
            defmt::trace!("fetching4");
            let future = UsbFuture::new(cx.shared.waker);
            let rc = stack
                .control_transfer_in(
                    1,
                    mps0,
                    SetupPacket {
                        bmRequestType: DEVICE_TO_HOST | VENDOR_REQUEST,
                        bRequest: 0x13,
                        wValue: 0,
                        wIndex: 0,
                        wLength: 6,
                    },
                &mut descriptors,
                    future,
                )
                .await;
            if let Ok(_sz) = rc {
                defmt::println!("AX88772 MAC {:x}", &descriptors[0..6]);
            } else {
                defmt::println!("fetched {:?}", rc);
            }
        }
    }

    #[task(binds = USBCTRL_IRQ, shared = [&waker], priority = 2)]
    fn usb_interrupt(cx: usb_interrupt::Context) {
        pac::NVIC::mask(pac::Interrupt::USBCTRL_IRQ);
        cx.shared.waker.wake();
    }
}
