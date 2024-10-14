use crate::async_pool::Pool;
use crate::core::driver::{self, Driver, InterruptPacket};
use crate::debug;
use crate::types::{EndpointType, SetupPacket, UsbDevice, UsbError, UsbSpeed};
use crate::types::{
    DEVICE_DESCRIPTOR, DEVICE_TO_HOST, GET_DESCRIPTOR, HOST_TO_DEVICE,
    SET_ADDRESS,
};
use core::cell::Cell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures::future::FutureExt;
use futures::Stream;
use futures::StreamExt;
use rp2040_pac as pac;
use rtic_common::waker_registration::CriticalSectionWakerRegistration;

pub struct UsbDeviceSet(pub u32);

impl UsbDeviceSet {
    pub fn contains(&self, n: u8) -> bool {
        (self.0 & (1 << n)) != 0
    }
}

pub enum DeviceEvent {
    Connect(UsbDevice),
    Disconnect(UsbDeviceSet),
}

pub struct UsbStatics {
    device_waker: CriticalSectionWakerRegistration,
    pipe_wakers: [CriticalSectionWakerRegistration; 16],
}

impl UsbStatics {
    pub fn on_irq(&self) {
        let regs = unsafe { pac::USBCTRL_REGS::steal() };
        let ints = regs.ints().read();
        defmt::println!(
            "IRQ ints={:x} inte={:x}",
            ints.bits(),
            regs.inte().read().bits()
        );
        if ints.buff_status().bit() {
            let bs = regs.buff_status().read().bits();
            for i in 0..15 {
                if (bs & (3 << (i * 2))) != 0 {
                    defmt::println!("IRQ wakes {}", i);
                    self.pipe_wakers[i].wake();
                }
            }
            regs.buff_status().write(|w| unsafe { w.bits(0xFFFF_FFFC) });
        }
        if (ints.bits() & 1) != 0 {
            // This clears the interrupt but does NOT clear sie_status.speed!
            unsafe { regs.sie_status().modify(|_, w| w.speed().bits(3)) };
            self.device_waker.wake();
        }
        if (ints.bits() & 0x448) != 0 {
            self.pipe_wakers[0].wake();
        }

        // Disable any remaining interrupts so we don't have an IRQ storm
        let bits = regs.ints().read().bits();
        unsafe {
            regs.inte().modify(|r, w| w.bits(r.bits() & !bits));
        }
        defmt::println!(
            "IRQ2 ints={:x} inte={:x}",
            bits,
            regs.inte().read().bits()
        );
    }
}

impl UsbStatics {
    // Only exists so that we can initialise the array in a const way
    #[allow(clippy::declare_interior_mutable_const)]
    const W: CriticalSectionWakerRegistration =
        CriticalSectionWakerRegistration::new();

    pub const fn new() -> Self {
        Self {
            device_waker: CriticalSectionWakerRegistration::new(),
            pipe_wakers: [Self::W; 16],
        }
    }
}

impl Default for UsbStatics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    Present(UsbSpeed),
    Absent,
}

#[derive(Copy, Clone)]
pub struct DeviceDetect<'a> {
    waker: &'a CriticalSectionWakerRegistration,
    status: DeviceStatus,
}

impl<'a> DeviceDetect<'a> {
    fn new(waker: &'a CriticalSectionWakerRegistration) -> Self {
        Self {
            waker,
            status: DeviceStatus::Absent,
        }
    }
}

impl Stream for DeviceDetect<'_> {
    type Item = DeviceStatus;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        defmt::trace!("DE register");
        self.waker.register(cx.waker());

        let regs = unsafe { pac::USBCTRL_REGS::steal() };
        let status = regs.sie_status().read();
        let device_status = match status.speed().bits() {
            0 => DeviceStatus::Absent,
            1 => DeviceStatus::Present(UsbSpeed::Low1_5),
            _ => DeviceStatus::Present(UsbSpeed::Full12),
        };

        if device_status != self.status {
            defmt::info!("DE ready {:x}", status.bits());
            regs.inte().modify(|_, w| w.host_conn_dis().set_bit());
            self.status = device_status;
            Poll::Ready(Some(device_status))
        } else {
            defmt::trace!(
                "DE pending intr={:x} st={:x}",
                regs.intr().read().bits(),
                status.bits()
            );
            regs.inte().modify(|_, w| w.host_conn_dis().set_bit());
            Poll::Pending
        }
    }
}

pub struct ControlEndpoint<'a> {
    waker: &'a CriticalSectionWakerRegistration,
}

impl<'a> ControlEndpoint<'a> {
    fn new(waker: &'a CriticalSectionWakerRegistration) -> Self {
        Self { waker }
    }
}

impl Future for ControlEndpoint<'_> {
    type Output = pac::usbctrl_regs::sie_status::R;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        defmt::trace!("CE register");
        self.waker.register(cx.waker());

        let regs = unsafe { pac::USBCTRL_REGS::steal() };
        let status = regs.sie_status().read();
        let intr = regs.intr().read();
        if (intr.bits() & 0x448) != 0 {
            defmt::info!("CE ready {:x}", status.bits());
            regs.sie_status().write(|w| unsafe { w.bits(0xFF0C_0000) });
            Poll::Ready(status)
        } else {
            defmt::trace!(
                "CE pending intr={:x} st={:x}",
                intr.bits(),
                status.bits()
            );
            regs.inte().modify(|_, w| {
                w.stall()
                    .set_bit()
                    .error_rx_timeout()
                    .set_bit()
                    .trans_complete()
                    .set_bit()
            });
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
                    if let Some((this_packet, is_last)) = self.next_packet() {
                        self.remain -= this_packet;
                        defmt::println!("Prepared {}-byte space", this_packet);
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
                    if let Some((this_packet, is_last)) = self.next_packet() {
                        self.remain -= this_packet;
                        defmt::println!("Prepared {}-byte space", this_packet);
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

impl Packetiser for OutPacketiser<'_> {
    fn prepare(&mut self, reg: &pac::usbctrl_dpram::EP_BUFFER_CONTROL) {
        let val = reg.read();
        match self.next_prep {
            0 => {
                if !val.available_0().bit() {
                    if let Some((this_packet, is_last)) = self.next_packet() {
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
                            unsafe { w.length_0().bits(this_packet as u16) };
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
                    if let Some((this_packet, is_last)) = self.next_packet() {
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
                            unsafe { w.length_1().bits(this_packet as u16) };
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

impl Depacketiser for InDepacketiser<'_> {
    fn retire(&mut self, reg: &pac::usbctrl_dpram::EP_BUFFER_CONTROL) {
        let val = reg.read();
        match self.next_retire {
            0 => {
                if val.full_0().bit() {
                    defmt::println!("Got {} bytes", val.length_0().bits());
                    let this_packet = core::cmp::min(
                        self.remain,
                        val.length_0().bits() as usize,
                    );
                    if this_packet > 0 {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                (0x5010_0000 + 0x180) as *const u8,
                                &mut self.buf[self.offset] as *mut u8,
                                this_packet,
                            );
                        }
                    }

                    self.remain -= this_packet;
                    self.offset += this_packet;
                    self.next_retire = 1;
                }
            }
            _ => {
                if val.full_1().bit() {
                    defmt::println!("Got {} bytes", val.length_1().bits());
                    let this_packet = core::cmp::min(
                        self.remain,
                        val.length_1().bits() as usize,
                    );
                    if this_packet > 0 {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                (0x5010_0000 + 0x1C0) as *const u8,
                                &mut self.buf[self.offset] as *mut u8,
                                this_packet,
                            );
                        }
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

type Pipe<'a> = crate::async_pool::Pooled<'a>;

pub struct InterruptPipe<'driver> {
    driver: &'driver HostController,
    pipe: Pipe<'driver>,
    max_packet_size: u16,
    data_toggle: Cell<bool>,
}

impl driver::InterruptPipe for InterruptPipe<'_> {
    fn set_waker(&self, waker: &core::task::Waker) {
        self.driver.statics.pipe_wakers[self.pipe.n as usize].register(waker);
    }

    fn poll(&self) -> Option<InterruptPacket> {
        let dpram = unsafe { pac::USBCTRL_DPRAM::steal() };
        let bc = dpram.ep_buffer_control((self.pipe.n * 2) as usize).read();
        if bc.full_0().bit() {
            let mut result = InterruptPacket {
                size: core::cmp::min(bc.length_0().bits(), 64) as u8,
                ..Default::default()
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (0x5010_0200 + (self.pipe.n as u32) * 64) as *const u8,
                    &mut result.data[0] as *mut u8,
                    result.size as usize,
                )
            };
            self.data_toggle.set(!self.data_toggle.get());
            dpram.ep_buffer_control((self.pipe.n * 2) as usize).write(
                |w| unsafe {
                    w.full_0()
                        .clear_bit()
                        .pid_0()
                        .bit(self.data_toggle.get())
                        .length_0()
                        .bits(self.max_packet_size)
                        .last_0()
                        .set_bit()
                },
            );

            cortex_m::asm::delay(12);

            dpram
                .ep_buffer_control((self.pipe.n * 2) as usize)
                .modify(|_, w| w.available_0().set_bit());
            let regs = unsafe { pac::USBCTRL_REGS::steal() };
            defmt::println!(
                "IE ready inte {:x} iec {:x} ecr {:x} epbc {:x}",
                regs.inte().read().bits(),
                regs.int_ep_ctrl().read().bits(),
                dpram
                    .ep_control((self.pipe.n * 2) as usize - 2)
                    .read()
                    .bits(),
                dpram
                    .ep_buffer_control((self.pipe.n * 2) as usize)
                    .read()
                    .bits(),
            );

            Some(result)
        } else {
            let regs = unsafe { pac::USBCTRL_REGS::steal() };
            regs.inte().modify(|_, w| w.buff_status().set_bit());
            regs.int_ep_ctrl().modify(|r, w| unsafe {
                w.bits(r.bits() | (1 << self.pipe.n))
            });
            defmt::println!(
                "IE pending inte {:x} iec {:x} ecr {:x} epbc {:x}",
                regs.inte().read().bits(),
                regs.int_ep_ctrl().read().bits(),
                dpram
                    .ep_control((self.pipe.n * 2) as usize - 2)
                    .read()
                    .bits(),
                dpram
                    .ep_buffer_control((self.pipe.n * 2) as usize)
                    .read()
                    .bits(),
            );
            regs.ep_status_stall_nak()
                .write(|w| unsafe { w.bits(3 << (self.pipe.n * 2)) });

            None
        }
    }
}

pub struct HostController {
    statics: &'static UsbStatics,
    //control_pipes: Pool,
    bulk_pipes: Pool,
}

impl HostController {
    pub fn new(statics: &'static UsbStatics) -> Self {
        Self {
            statics,
            //control_pipes: Pool::new(1),
            bulk_pipes: Pool::new(15),
        }
    }
}

impl Driver for HostController {
    type InterruptPipe<'driver> = InterruptPipe<'driver> where Self: 'driver;

    // The trait defines this with "-> impl Future"-style syntax, but the one
    // is just sugar for the other according to Clippy.
    async fn alloc_interrupt_pipe(
        &self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> InterruptPipe {
        let mut pipe = self.bulk_pipes.alloc().await;
        pipe.n += 1;
        debug::println!("interrupt_endpoint on pipe {}", pipe.n);

        let n = pipe.n;
        let regs = unsafe { pac::USBCTRL_REGS::steal() };
        let dpram = unsafe { pac::USBCTRL_DPRAM::steal() };
        regs.host_addr_endp((n - 1) as usize).write(|w| unsafe {
            w.address()
                .bits(address)
                .endpoint()
                .bits(endpoint)
                .intep_dir()
                .clear_bit() // IN
        });

        dpram.ep_control((n * 2 - 2) as usize).write(|w| unsafe {
            w.enable()
                .set_bit()
                .interrupt_per_buff()
                .set_bit()
                .endpoint_type()
                .interrupt()
                .buffer_address()
                .bits(0x200 + (n as u16) * 64)
                .host_poll_interval()
                .bits(core::cmp::min(interval_ms as u16, 9))
        });

        dpram.ep_buffer_control((n * 2) as usize).write(|w| unsafe {
            w.full_0()
                .clear_bit()
                .length_0()
                .bits(max_packet_size)
                .pid_0()
                .clear_bit()
                .last_0()
                .set_bit()
        });

        cortex_m::asm::delay(12);

        dpram
            .ep_buffer_control((n * 2) as usize)
            .modify(|_, w| w.available_0().set_bit());

        InterruptPipe {
            driver: self,
            pipe,
            max_packet_size,
            data_toggle: Cell::new(false),
        }
    }
}

pub struct UsbStack<'a, D: Driver> {
    driver: D,
    regs: pac::USBCTRL_REGS,
    dpram: pac::USBCTRL_DPRAM,
    statics: &'a UsbStatics,
    control_pipes: Pool,
    bulk_pipes: Pool,
}

impl<'a, D: Driver> UsbStack<'a, D> {
    pub fn new(
        driver: D,
        regs: pac::USBCTRL_REGS,
        dpram: pac::USBCTRL_DPRAM,
        resets: &mut pac::RESETS,
        statics: &'a UsbStatics,
    ) -> Self {
        resets.reset().modify(|_, w| w.usbctrl().set_bit());
        resets.reset().modify(|_, w| w.usbctrl().clear_bit());

        Self {
            driver,
            regs,
            dpram,
            statics,
            control_pipes: Pool::new(1),
            bulk_pipes: Pool::new(15),
        }
    }

    async fn alloc_pipe(&self, endpoint_type: EndpointType) -> Pipe {
        if endpoint_type == EndpointType::Control {
            self.control_pipes.alloc().await
        } else {
            let mut p = self.bulk_pipes.alloc().await;
            p.n += 1;
            p
        }
    }

    async fn new_device(&self, speed: UsbSpeed, address: u8) -> UsbDevice {
        // Read prefix of device descriptor
        let mut descriptors = [0u8; 18];
        let rc = self
            .control_transfer_in(
                0,
                64,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((DEVICE_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 8,
                },
                &mut descriptors,
            )
            .await;

        let packet_size_ep0 = if rc.is_ok() { descriptors[7] } else { 8 };

        // Set address
        let _ = self
            .control_transfer_out(
                0,
                packet_size_ep0,
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

        // Fetch rest of device descriptor
        let mut vid = 0;
        let mut pid = 0;
        let rc = self
            .control_transfer_in(
                1,
                packet_size_ep0,
                SetupPacket {
                    bmRequestType: DEVICE_TO_HOST,
                    bRequest: GET_DESCRIPTOR,
                    wValue: ((DEVICE_DESCRIPTOR as u16) << 8),
                    wIndex: 0,
                    wLength: 18,
                },
                &mut descriptors,
            )
            .await;
        if let Ok(_sz) = rc {
            vid = u16::from_le_bytes([descriptors[8], descriptors[9]]);
            pid = u16::from_le_bytes([descriptors[10], descriptors[11]]);
        } else {
            defmt::println!("Dtor fetch 2 {:?}", rc);
        }

        UsbDevice {
            address,
            packet_size_ep0,
            vid,
            pid,
            speed,
        }
    }

    pub async fn enumerate_root_device(&self) -> UsbDevice {
        self.regs.usb_muxing().modify(|_, w| {
            w.to_phy().set_bit();
            w.softcon().set_bit()
        });
        self.regs.usb_pwr().modify(|_, w| {
            w.vbus_detect().set_bit();
            w.vbus_detect_override_en().set_bit()
        });
        self.regs.main_ctrl().modify(|_, w| {
            w.sim_timing().clear_bit();
            w.host_ndevice().set_bit();
            w.controller_en().set_bit()
        });
        self.regs.sie_ctrl().write(|w| {
            w.pulldown_en().set_bit();
            w.vbus_en().set_bit();
            w.keep_alive_en().set_bit();
            w.sof_en().set_bit()
        });

        unsafe {
            pac::NVIC::unpend(pac::Interrupt::USBCTRL_IRQ);
            pac::NVIC::unmask(pac::Interrupt::USBCTRL_IRQ);
        }

        self.regs.inte().write(|w| w.host_conn_dis().set_bit());

        let mut f = DeviceDetect::new(&self.statics.device_waker);

        f.next().await;

        let status = self.regs.sie_status().read();
        defmt::trace!("conn_dis awaited, sie_status=0x{:x}", status.bits());
        let speed = match status.speed().bits() {
            1 => {
                defmt::println!("LS detected");
                UsbSpeed::Low1_5
            }
            2 => {
                defmt::println!("FS detected");
                UsbSpeed::Full12
            }
            _ => UsbSpeed::Low1_5,
        };

        // Clear interrupt
        unsafe { self.regs.sie_status().modify(|_, w| w.speed().bits(3)) };

        defmt::trace!(
            "conn_dis cleared, sie_status=0x{:x}",
            self.regs.sie_status().read().bits()
        );

        /* RP2040 seems not to need the bus_reset() bit set at this point --
         * perhaps it does it automatically? TinyUSB doesn't set it either.
         *
        self.regs
            .inte()
            .modify(|_, w| w.bus_reset().set_bit());
         */
        self.new_device(speed, 1).await
    }

    async fn control_transfer_inner(
        &self,
        address: u8,
        packet_size: u8,
        setup: SetupPacket,
        packetiser: &mut impl Packetiser,
        depacketiser: &mut impl Depacketiser,
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
            self.regs.buff_status().write(|w| unsafe { w.bits(0x3) });
            self.regs.inte().modify(|_, w| {
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

            let f = ControlEndpoint::new(&self.statics.pipe_wakers[0]);

            let status = f.await;

            defmt::trace!("awaited");

            self.regs.buff_status().write(|w| unsafe { w.bits(0x3) });

            self.regs.inte().modify(|_, w| {
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

    pub async fn control_transfer_in(
        &self,
        address: u8,
        packet_size: u8,
        setup: SetupPacket,
        buf: &mut [u8],
    ) -> Result<usize, UsbError> {
        if buf.len() < setup.wLength as usize {
            return Err(UsbError::BufferTooSmall);
        }

        let _pipe = self.alloc_pipe(EndpointType::Control).await;

        let mut packetiser =
            InPacketiser::new(setup.wLength, packet_size as u16);
        let mut depacketiser = InDepacketiser::new(setup.wLength, buf);

        self.control_transfer_inner(
            address,
            packet_size,
            setup,
            &mut packetiser,
            &mut depacketiser,
        )
        .await?;

        Ok(depacketiser.total())
    }

    pub async fn control_transfer_out(
        &self,
        address: u8,
        packet_size: u8,
        setup: SetupPacket,
        buf: &[u8],
    ) -> Result<(), UsbError> {
        if buf.len() < setup.wLength as usize {
            return Err(UsbError::BufferTooSmall);
        }

        let _pipe = self.alloc_pipe(EndpointType::Control).await;

        let mut packetiser =
            OutPacketiser::new(setup.wLength, packet_size as u16, buf);
        let mut depacketiser = OutDepacketiser::new();

        self.control_transfer_inner(
            address,
            packet_size,
            setup,
            &mut packetiser,
            &mut depacketiser,
        )
        .await?;

        Ok(())
    }

    pub fn interrupt_endpoint_in<'b>(
        &'b self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval: u8,
    ) -> impl Stream<Item = InterruptPacket> + 'b
    where
        'a: 'b,
    {
        let pipe = self.driver.alloc_interrupt_pipe(
            address,
            endpoint,
            max_packet_size,
            interval,
        );
        async move {
            let pipe = pipe.await;
            crate::core::interrupt::InterruptStream::<D> { pipe }
        }
        .flatten_stream()
    }

    pub fn device_events_no_hubs(
        &self,
    ) -> impl Stream<Item = DeviceEvent> + '_ {
        let root_device = DeviceDetect::new(&self.statics.device_waker);
        root_device.then(|status| {
            if let DeviceStatus::Present(speed) = status {
                self.new_device(speed, 1)
                    .map(DeviceEvent::Connect)
                    .left_future()
            } else {
                core::future::ready(DeviceEvent::Disconnect(UsbDeviceSet(
                    0xFFFF_FFFF,
                )))
                .right_future()
            }
        })
    }

    /*
    pub fn device_events(&self) -> impl Stream<Item = DeviceEvent> {
        const MAX_HUBS: usize = 15;

        let mut topology = crate::core::bus::Bus::new();
        let mut endpoints: [MaybeInterruptEndpoint; MAX_HUBS] = Default::default();
        let mut hub_count = 0usize;
        let mut root_device = DeviceDetect::new(&self.statics.device_waker);
        let mut data_toggle = [false; MAX_HUBS];

        let multistream = select_slice(
            pin!(&mut endpoints[0..hub_count])
        );

        futures::stream::select(
            root_device.map(|_| DeviceEvent::Disconnect(UsbDeviceSet(0))),
            multistream.map(|_| DeviceEvent::Disconnect(UsbDeviceSet(1))),
        )
    }
     */
}
