use crate::async_pool::Pool;
use crate::debug;
use crate::host_controller::{
    DataPhase, DeviceStatus, HostController, InterruptPacket, TransferType,
    UsbError, UsbSpeed,
};
use crate::wire::{Direction, EndpointType, SetupPacket};
use core::cell::Cell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures::Stream;
use rp235x_pac as pac;
use rtic_common::waker_registration::CriticalSectionWakerRegistration;

/// Data shared between interrupt handler and thread-mode code
pub struct UsbShared {
    device_waker: CriticalSectionWakerRegistration,
    pipe_wakers: [CriticalSectionWakerRegistration; 16],
}

impl UsbShared {
    /// IRQ handler
    pub fn on_irq(&self) {
        let regs = unsafe { pac::USB::steal() };
        let ints = regs.ints().read();
        /*defmt::info!(
                    "IRQ ints={:x} inte={:x}",
                    ints.bits(),
            regs.inte().read().bits()
        );*/

        if ints.buff_status().bit() {
            let bs = regs.buff_status().read().bits();
            for i in 0..15 {
                if (bs & (3 << (i * 2))) != 0 {
                    defmt::trace!("IRQ wakes {}", i);
                    self.pipe_wakers[i].wake();
                }
            }
            regs.buff_status().write(|w| unsafe { w.bits(0xFFFF_FFFC) });
        }
        if (ints.bits() & 1) != 0 {
            self.device_waker.wake();
        }
        if (ints.bits() & 0x458) != 0 {
            //defmt::info!("IRQ wakes 0 {:x}", ints.bits());
            self.pipe_wakers[0].wake();
        }

        // Disable any remaining interrupts so we don't have an IRQ storm
        let bits = regs.ints().read().bits();
        unsafe {
            regs.inte().modify(|r, w| w.bits(r.bits() & !bits));
        }
        /*        defmt::info!(
            "IRQ2 ints={:x} inte={:x}",
            bits,
            regs.inte().read().bits()
        ); */
    }
}

impl UsbShared {
    // Only exists so that we can initialise the array in a const way
    #[allow(clippy::declare_interior_mutable_const)]
    const W: CriticalSectionWakerRegistration =
        CriticalSectionWakerRegistration::new();

    /// Create a new `UsbShared` (nb, is const, unlike `default()`)
    pub const fn new() -> Self {
        Self {
            device_waker: CriticalSectionWakerRegistration::new(),
            pipe_wakers: [Self::W; 16],
        }
    }
}

impl Default for UsbShared {
    fn default() -> Self {
        Self::new()
    }
}

/// Data that isn't shared with the IRQ handler, but must be 'static anyway
pub struct UsbStatics {
    bulk_pipes: Pool,
    control_pipes: Pool,
}

impl UsbStatics {
    /// Crate a new `UsbStatics` (nb, is const, unlike `default()`)
    pub const fn new() -> Self {
        Self {
            bulk_pipes: Pool::new(15),
            control_pipes: Pool::new(1),
        }
    }
}

impl Default for UsbStatics {
    fn default() -> Self {
        Self::new()
    }
}

/// Implementation of `HostController::DeviceDetect` for RP235x
#[derive(Copy, Clone)]
pub struct Rp235xDeviceDetect {
    waker: &'static CriticalSectionWakerRegistration,
    status: DeviceStatus,
}

impl Rp235xDeviceDetect {
    fn new(waker: &'static CriticalSectionWakerRegistration) -> Self {
        Self {
            waker,
            status: DeviceStatus::Absent,
        }
    }
}

impl Stream for Rp235xDeviceDetect {
    type Item = DeviceStatus;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        //defmt::trace!("DE register");
        self.waker.register(cx.waker());

        let regs = unsafe { pac::USB::steal() };
        let status = regs.sie_status().read();
        let device_status = match status.speed().bits() {
            0 => DeviceStatus::Absent,
            1 => DeviceStatus::Present(UsbSpeed::Low1_5),
            _ => DeviceStatus::Present(UsbSpeed::Full12),
        };

        if device_status != self.status {
            defmt::info!(
                "DE ready {:x} {}->{}",
                status.bits(),
                self.status,
                device_status,
            );
            regs.inte().modify(|_, w| w.host_conn_dis().set_bit());
            self.status = device_status;
            Poll::Ready(Some(device_status))
        } else {
            /*defmt::trace!(
                            "DE pending intr={:x} st={:x}",
                            regs.intr().read().bits(),
                            status.bits()
                        );
            */
            regs.inte().modify(|_, w| w.host_conn_dis().set_bit());
            Poll::Pending
        }
    }
}

struct Rp235xControlEndpoint<'a> {
    waker: &'a CriticalSectionWakerRegistration,
}

impl<'a> Rp235xControlEndpoint<'a> {
    fn new(waker: &'a CriticalSectionWakerRegistration) -> Self {
        Self { waker }
    }
}

impl Future for Rp235xControlEndpoint<'_> {
    type Output = pac::usb::sie_status::R;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //defmt::trace!("CE register");
        self.waker.register(cx.waker());

        let regs = unsafe { pac::USB::steal() };
        let status = regs.sie_status().read();
        let intr = regs.intr().read();
        let bcsh = regs.buff_cpu_should_handle().read();
        if (intr.bits() & 0x458) != 0 {
            defmt::trace!(
                "CE ready {:x} {:x} {:x}",
                status.bits(),
                intr.bits(),
                bcsh.bits()
            );
            regs.sie_status().write(|w| unsafe { w.bits(0xFF08_0000) });
            Poll::Ready(status)
        } else {
            regs.sie_status().write(|w| unsafe { w.bits(0xFF08_0000) });
            defmt::trace!(
                "CE pending intr={:x} st={:x}->{:x}",
                intr.bits(),
                status.bits(),
                regs.sie_status().read().bits(),
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

/*
struct Rp235xBulkEndpoint<'a> {
    n: u8,
    waker: &'a CriticalSectionWakerRegistration,
}

impl<'a> Rp235xBulkEndpoint<'a> {
    pub fn new(n: u8, waker: &'a CriticalSectionWakerRegistration) -> Self {
        Self { n, waker }
    }
}

impl Future for Rp235xBulkEndpoint<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        defmt::trace!("BE register");
        self.waker.register(cx.waker());

        let regs = unsafe { pac::USB::steal() };
        let dpram = unsafe { pac::USB_DPRAM::steal() };
        let intr = regs.intr().read();
        let epc = dpram.ep_control(((self.n - 1) * 2) as usize);
        let epbc = dpram.ep_buffer_control((self.n * 2) as usize);
        let epbc_value = epbc.read();
        let buf0_done = (epbc_value.bits() & 0xFFFF) != 0
            && !epbc_value.available_0().bit();
        let buf1_done = (epbc_value.bits() & 0xFFFF_0000) != 0
            && !epbc_value.available_1().bit();
        // TODO EP_STATUS_STALL_NAK
        if buf0_done || buf1_done {
            defmt::info!("BE ready {:x}", intr.bits());
            regs.buff_status()
                .write(|w| unsafe { w.bits(0x3 << self.n) });

            Poll::Ready(())
        } else {
            regs.inte().modify(|_, w| w.buff_status().set_bit());
            defmt::trace!(
                "BE pending intr={:x} inte={:x} intec={:x} epc={:x} epbc={:x}",
                intr.bits(),
                regs.inte().read().bits(),
                regs.int_ep_ctrl().read().bits(),
                epc.read().bits(),
                epbc.read().bits()
            );
            Poll::Pending
        }
    }
}
*/

struct Pipe {
    /// "pooled" is never read, it's just here for its drop glue
    _pooled: crate::async_pool::Pooled<'static>,
    which: u8,
}

impl Pipe {
    fn new(pooled: crate::async_pool::Pooled<'static>, offset: u8) -> Self {
        let which = pooled.which() + offset;
        Self {
            _pooled: pooled,
            which,
        }
    }

    fn which(&self) -> u8 {
        self.which
    }
}

/// Implementation of `HostController::InterruptPipe` for RP235x
pub struct Rp235xInterruptPipe {
    shared: &'static UsbShared,
    pipe: Pipe,
    max_packet_size: u16,
    data_toggle: Cell<bool>,
}

impl Rp235xInterruptPipe {
    fn set_waker(&self, waker: &core::task::Waker) {
        self.shared.pipe_wakers[self.pipe.which() as usize].register(waker);
    }

    fn poll(&self) -> Option<InterruptPacket> {
        let dpram = unsafe { pac::USB_DPRAM::steal() };
        let regs = unsafe { pac::USB::steal() };
        let which = self.pipe.which();
        let bc = dpram.ep_buffer_control((which * 2) as usize).read();
        if bc.full_0().bit() {
            let addr_endp = regs.host_addr_endp((which - 1) as usize).read();
            let mut result = InterruptPacket {
                address: addr_endp.address().bits() as u8,
                endpoint: addr_endp.endpoint().bits() as u8,
                size: core::cmp::min(bc.length_0().bits(), 64) as u8,
                ..Default::default()
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (0x5010_0200 + (which as u32) * 128) as *const u8,
                    &mut result.data[0] as *mut u8,
                    result.size as usize,
                )
            };
            self.data_toggle.set(!self.data_toggle.get());
            dpram
                .ep_buffer_control((which * 2) as usize)
                .write(|w| unsafe {
                    w.full_0()
                        .clear_bit()
                        .pid_0()
                        .bit(self.data_toggle.get())
                        .length_0()
                        .bits(self.max_packet_size)
                        .last_0()
                        .set_bit()
                });

            cortex_m::asm::delay(12);

            dpram
                .ep_buffer_control((which * 2) as usize)
                .modify(|_, w| w.available_0().set_bit());
            defmt::trace!(
                "IE ready inte {:x} iec {:x} ecr {:x} epbc {:x}",
                regs.inte().read().bits(),
                regs.int_ep_ctrl().read().bits(),
                dpram.ep_control((which * 2) as usize - 2).read().bits(),
                dpram.ep_buffer_control((which * 2) as usize).read().bits(),
            );

            Some(result)
        } else {
            regs.inte().modify(|_, w| w.buff_status().set_bit());
            regs.int_ep_ctrl()
                .modify(|r, w| unsafe { w.bits(r.bits() | (1 << which)) });
            defmt::trace!(
                "IE pending inte {:x} iec {:x} ecr {:x} epbc {:x}",
                regs.inte().read().bits(),
                regs.int_ep_ctrl().read().bits(),
                dpram.ep_control((which * 2) as usize - 2).read().bits(),
                dpram.ep_buffer_control((which * 2) as usize).read().bits(),
            );
            regs.ep_status_stall_nak()
                .write(|w| unsafe { w.bits(3 << (which * 2)) });

            None
        }
    }
}

impl Stream for Rp235xInterruptPipe {
    type Item = InterruptPacket;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.set_waker(cx.waker());

        if let Some(packet) = self.poll() {
            Poll::Ready(Some(packet))
        } else {
            Poll::Pending
        }
    }
}

enum ZeroLengthPacket {
    AsNeeded,
    Never,
}

trait Packetiser {
    fn prepare(&mut self, reg: &pac::usb_dpram::EP_BUFFER_CONTROL)
        -> bool;
}

struct InPacketiser {
    next_prep: u8,
    remain: u16,
    packet_size: u16,
    need_zero_size_packet: bool,
    initial_toggle: bool,
}

impl InPacketiser {
    fn new(
        remain: u16,
        packet_size: u16,
        initial_toggle: bool,
        zlp: ZeroLengthPacket,
    ) -> Self {
        Self {
            next_prep: 0,
            remain,
            packet_size,
            need_zero_size_packet: match zlp {
                ZeroLengthPacket::Never => remain == 0,
                ZeroLengthPacket::AsNeeded => (remain % packet_size) == 0,
            },
            initial_toggle,
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
        Some((self.remain, !self.need_zero_size_packet))
    }
}

impl Packetiser for InPacketiser {
    fn prepare(
        &mut self,
        reg: &pac::usb_dpram::EP_BUFFER_CONTROL,
    ) -> bool {
        let val = reg.read();
        match self.next_prep {
            0 => {
                if !val.available_0().bit() {
                    if let Some((this_packet, is_last)) = self.next_packet() {
                        //defmt::info!("Prepared {}/{}-byte space last {} @0", this_packet, self.remain, is_last);
                        self.remain -= this_packet;
                        reg.modify(|_, w| {
                            w.full_0().clear_bit();
                            w.pid_0().bit(self.initial_toggle);
                            w.last_0().bit(is_last);
                            unsafe { w.length_0().bits(self.packet_size) };
                            w
                        });

                        cortex_m::asm::delay(12);

                        reg.modify(|_, w| w.available_0().set_bit());

                        self.next_prep = 1;
                        return true;
                    }
                }
            }

            _ => {
                if !val.available_1().bit() {
                    if let Some((this_packet, is_last)) = self.next_packet() {
                        //defmt::info!("Prepared {}/{}-byte space last {} @1", this_packet, self.remain, is_last);
                        self.remain -= this_packet;
                        reg.modify(|_, w| {
                            w.full_1().clear_bit();
                            w.pid_1().bit(!self.initial_toggle);
                            w.last_1().bit(is_last);
                            unsafe { w.length_1().bits(self.packet_size) };
                            w
                        });

                        cortex_m::asm::delay(12);

                        reg.modify(|_, w| w.available_1().set_bit());

                        self.next_prep = 0;
                        return true;
                    }
                }
            }
        }
        false
    }
}

struct OutPacketiser<'a> {
    next_prep: u8,
    initial_pid: bool,
    remain: usize,
    offset: usize,
    packet_size: usize,
    need_zero_size_packet: bool,
    buf: &'a [u8],
}

impl<'a> OutPacketiser<'a> {
    fn new(
        size: u16,
        packet_size: u16,
        buf: &'a [u8],
        initial_pid: bool,
        zlp: ZeroLengthPacket,
    ) -> Self {
        Self {
            next_prep: 0,
            initial_pid,
            remain: size as usize,
            offset: 0,
            packet_size: packet_size as usize,
            need_zero_size_packet: match zlp {
                ZeroLengthPacket::Never => size == 0,
                ZeroLengthPacket::AsNeeded => (size % packet_size) == 0,
            },
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
        Some((self.remain, !self.need_zero_size_packet))
    }
}

impl Packetiser for OutPacketiser<'_> {
    fn prepare(
        &mut self,
        reg: &pac::usb_dpram::EP_BUFFER_CONTROL,
    ) -> bool {
        let val = reg.read();
        match self.next_prep {
            0 => {
                if !val.available_0().bit() {
                    if let Some((this_packet, is_last)) = self.next_packet() {
                        defmt::trace!(
                            "Preparing {}/{} @0 last {}",
                            this_packet,
                            self.remain,
                            is_last
                        );
                        if this_packet > 0 {
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    &self.buf[self.offset] as *const u8,
                                    (0x5010_0000 + 0x180) as *mut u8,
                                    this_packet,
                                );
                            }
                        }
                        reg.modify(|_, w| {
                            w.full_0().set_bit();
                            w.pid_0().bit(self.initial_pid);
                            w.last_0().bit(is_last);
                            unsafe { w.length_0().bits(this_packet as u16) };
                            w
                        });

                        cortex_m::asm::delay(12);

                        reg.modify(|_, w| w.available_0().set_bit());

                        self.remain -= this_packet;
                        self.offset += this_packet;
                        self.next_prep = 1;
                        return true;
                    }
                }
            }

            _ => {
                if !val.available_1().bit() {
                    if let Some((this_packet, is_last)) = self.next_packet() {
                        defmt::trace!(
                            "Preparing {}/{} @1 last {}",
                            this_packet,
                            self.remain,
                            is_last
                        );
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                &self.buf[self.offset] as *const u8,
                                (0x5010_0000 + 0x1C0) as *mut u8,
                                this_packet,
                            );
                        }
                        reg.modify(|_, w| {
                            w.full_1().set_bit();
                            w.pid_1().bit(!self.initial_pid);
                            w.last_1().bit(is_last);
                            unsafe { w.length_1().bits(this_packet as u16) };
                            w
                        });

                        cortex_m::asm::delay(12);

                        reg.modify(|_, w| w.available_1().set_bit());

                        self.remain -= this_packet;
                        self.offset += this_packet;
                        self.next_prep = 0;
                        return true;
                    }
                }
            }
        }
        false
    }
}

trait Depacketiser {
    fn retire(&mut self, reg: &pac::usb_dpram::EP_BUFFER_CONTROL) -> bool;
}

struct InDepacketiser<'a> {
    next_retire: u8,
    packet_parity: bool,
    remain: usize,
    offset: usize,
    buf: &'a mut [u8],
}

impl<'a> InDepacketiser<'a> {
    fn new(size: u16, buf: &'a mut [u8]) -> Self {
        Self {
            next_retire: 0,
            packet_parity: false,
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
    fn retire(&mut self, reg: &pac::usb_dpram::EP_BUFFER_CONTROL) -> bool {
        let val = reg.read();
        match self.next_retire {
            0 => {
                if val.full_0().bit() {
                    self.packet_parity = !self.packet_parity;
                    defmt::trace!(
                        "Got {}/{} bytes @0",
                        val.length_0().bits(),
                        self.remain
                    );
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
                    return true;
                }
            }
            _ => {
                if val.full_1().bit() {
                    self.packet_parity = !self.packet_parity;
                    defmt::trace!(
                        "Got {}/{} bytes @1",
                        val.length_1().bits(),
                        self.remain
                    );
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
                    return true;
                }
            }
        }
        false
    }
}

struct OutDepacketiser {
    next_retire: u8,
    packet_parity: bool,
}

impl OutDepacketiser {
    fn new() -> Self {
        Self {
            next_retire: 0,
            packet_parity: false,
        }
    }
}

impl Depacketiser for OutDepacketiser {
    fn retire(&mut self, reg: &pac::usb_dpram::EP_BUFFER_CONTROL) -> bool {
        let val = reg.read();
        match self.next_retire {
            0 => {
                if !val.full_0().bit() {
                    defmt::trace!("Reaped @0");
                    self.packet_parity = !self.packet_parity;
                    self.next_retire = 1;
                    return true;
                }
            }
            _ => {
                if !val.full_1().bit() {
                    defmt::trace!("Reaped @1");
                    self.packet_parity = !self.packet_parity;
                    self.next_retire = 0;
                    return true;
                }
            }
        }
        false
    }
}

/// Implementation of HostController for RP235x
pub struct Rp235xHostController {
    shared: &'static UsbShared,
    statics: &'static UsbStatics,
    regs: pac::USB,
    dpram: pac::USB_DPRAM,
}

impl Rp235xHostController {
    /// Create a new RP235xHostController
    ///
    /// You'll need a rp235x::UsbShared, a rp235x::UsbStatics, and the
    /// register blocks from the PAC. (We only borrow the RESETS
    /// block, but we take ownership of the USB-specific ones.)
    ///
    /// See rp235x-usb-msc.rs for a complete working example.
    pub fn new(
        resets: &mut pac::RESETS,
        regs: pac::USB,
        dpram: pac::USB_DPRAM,
        shared: &'static UsbShared,
        statics: &'static UsbStatics,
    ) -> Self {
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
            w.phy_iso().clear_bit();
            w.controller_en().set_bit()
        });
        regs.sie_ctrl().write(|w| {
            w.pulldown_en().set_bit();
            w.vbus_en().set_bit();
            w.keep_alive_en().set_bit();
            w.sof_en().set_bit()
        });

        // Because rp235x_pac only declares pac::Interrupt to be a
        // cortex_m::InterruptNumber in target_arch="arm" builds, we can only
        // compile these lines in such builds.
        #[cfg(target_arch = "arm")]
        unsafe {
            cortex_m::peripheral::NVIC::unpend(pac::Interrupt::USBCTRL_IRQ);
            cortex_m::peripheral::NVIC::unmask(pac::Interrupt::USBCTRL_IRQ);
        }

        regs.inte().write(|w| w.host_conn_dis().set_bit());

        Self {
            regs,
            dpram,
            shared,
            statics,
        }
    }

    async fn alloc_pipe(&self, endpoint_type: EndpointType) -> Pipe {
        if endpoint_type == EndpointType::Control {
            Pipe::new(self.statics.control_pipes.alloc().await, 0)
        } else {
            Pipe::new(self.statics.bulk_pipes.alloc().await, 1)
        }
    }

    fn try_alloc_pipe(&self, endpoint_type: EndpointType) -> Option<Pipe> {
        if endpoint_type == EndpointType::Control {
            Some(Pipe::new(self.statics.control_pipes.try_alloc()?, 0))
        } else {
            Some(Pipe::new(self.statics.bulk_pipes.try_alloc()?, 1))
        }
    }

    async fn send_setup(
        &self,
        address: u8,
        setup: &SetupPacket,
    ) -> Result<(), UsbError> {
        self.dpram.epx_control().write(|w| {
            unsafe {
                w.buffer_address().bits(0x180);
            }
            w.interrupt_per_buff().clear_bit();
            w.enable().clear_bit()
        });

        self.dpram
            .ep_buffer_control(0)
            .write(|w| unsafe { w.bits(0) });

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

        self.regs.sie_ctrl().modify(|_, w| {
            w.receive_data().clear_bit();
            w.send_data().clear_bit();
            w.send_setup().set_bit()
        });

        //defmt::trace!("S ctrl->{:x}", self.regs.sie_ctrl().read().bits());

        cortex_m::asm::delay(12);

        self.regs
            .sie_ctrl()
            .modify(|_, w| w.start_trans().set_bit());

        loop {
            let f = Rp235xControlEndpoint::new(&self.shared.pipe_wakers[0]);

            let status = f.await;

            // defmt::trace!("awaited");

            if status.trans_complete().bit() {
                break;
            }

            let bcr = self.dpram.ep_buffer_control(0).read();
            let ctrl = self.regs.sie_ctrl().read();
            let bstat = self.regs.buff_status().read();
            defmt::trace!(
                "S bcr=0x{:x} sie_status=0x{:x} sie_ctrl=0x{:x} bstat={:x}",
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
        }

        //defmt::trace!("S completed");

        Ok(())
    }

    async fn control_transfer_inner(
        &self,
        address: u8,
        endpoint: u8,
        packet_size: u8,
        direction: Direction,
        size: usize,
        packetiser: &mut impl Packetiser,
        depacketiser: &mut impl Depacketiser,
    ) -> Result<(), UsbError> {
        let packets = size / (packet_size as usize) + 1;
        //defmt::info!("we'll need {} packets", packets);

        self.dpram.epx_control().write(|w| {
            unsafe {
                w.buffer_address().bits(0x180);
            }
            if packets > 1 {
                w.double_buffered().set_bit();
                w.interrupt_per_buff().set_bit();
            }
            if endpoint == 0 {
                w.endpoint_type().control();
            } else {
                w.endpoint_type().bulk();
            }
            w.enable().set_bit()
        });

        self.dpram
            .ep_buffer_control(0)
            .write(|w| unsafe { w.bits(0) });

        self.regs
            .sie_status()
            .write(|w| unsafe { w.bits(0xFFFF_FFFF) });

        self.regs.addr_endp().write(|w| unsafe {
            w.endpoint().bits(endpoint);
            w.address().bits(address)
        });

        self.regs.nak_poll().write(|w| unsafe {
            w.delay_fs().bits(10);
            w.delay_ls().bits(16)
        });

        let mut started = false;

        let mut in_flight = 0;

        loop {
            if in_flight < 2
                && packetiser.prepare(self.dpram.ep_buffer_control(0))
            {
                in_flight += 1;
            }
            if in_flight < 2
                && packetiser.prepare(self.dpram.ep_buffer_control(0))
            {
                in_flight += 1;
            }

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

            /*            defmt::info!(
                "Initial bcr {:x}",
                self.dpram.ep_buffer_control(0).read().bits()
            );*/

            if !started {
                started = true;

                /*defmt::trace!(
                                    "len{} {} ctrl{:x}",
                                    size,
                                    direction,
                                    self.regs.sie_ctrl().read().bits()
                                );
                */
                self.regs.sie_ctrl().modify(|_, w| {
                    w.receive_data().bit(direction == Direction::In);
                    w.send_data().bit(direction == Direction::Out);
                    w.send_setup().clear_bit()
                });

                defmt::trace!(
                    "ctrl->{:x} st {:x} intr {:x} inte {:x}",
                    self.regs.sie_ctrl().read().bits(),
                    self.regs.sie_status().read().bits(),
                    self.regs.intr().read().bits(),
                    self.regs.inte().read().bits(),
                );

                cortex_m::asm::delay(12);

                self.regs
                    .sie_ctrl()
                    .modify(|_, w| w.start_trans().set_bit());
            }

            let f = Rp235xControlEndpoint::new(&self.shared.pipe_wakers[0]);

            let status = f.await;

            defmt::trace!("awaited {}", in_flight);

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
                if in_flight > 0 {
                    if depacketiser.retire(self.dpram.ep_buffer_control(0)) {
                        in_flight -= 1;
                    }
                }
                defmt::trace!("TC");
                break;
            }

            /*
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
            */
            if status.data_seq_error().bit() {
                defmt::trace!("DataSeqError");
                return Err(UsbError::DataSeqError);
            }
            if status.stall_rec().bit() {
                defmt::trace!("Stall");
                return Err(UsbError::Stall);
            }
            // if status.nak_rec().bit() {
            //     return Err(UsbError::Nak);
            // }
            if status.rx_overflow().bit() {
                defmt::trace!("Overflow");
                return Err(UsbError::Overflow);
            }
            if status.rx_timeout().bit() {
                defmt::trace!("Timeout");
                return Err(UsbError::Timeout);
            }
            if status.bit_stuff_error().bit() {
                defmt::trace!("BitStuff");
                return Err(UsbError::BitStuffError);
            }
            if status.crc_error().bit() {
                defmt::trace!("CRCError");
                return Err(UsbError::CrcError);
            }

            if in_flight > 0 {
                if depacketiser.retire(self.dpram.ep_buffer_control(0)) {
                    in_flight -= 1;
                }
                if in_flight > 0 {
                    if depacketiser.retire(self.dpram.ep_buffer_control(0)) {
                        in_flight -= 1;
                    }
                }
            }
        }

        /*
        let bcr = self.dpram.ep_buffer_control(0).read();
        let ctrl = self.regs.sie_ctrl().read();
        defmt::trace!(
            "COMPLETE bcr=0x{:x} sie_ctrl=0x{:x} in={}",
            bcr.bits(),
            ctrl.bits(),
            in_flight,
        );
        */
        self.regs
            .sie_status()
            .write(|w| unsafe { w.bits(0xFF00_0000) });
        if in_flight > 0 {
            depacketiser.retire(self.dpram.ep_buffer_control(0));
        }
        Ok(())
    }

    async fn control_transfer_in(
        &self,
        address: u8,
        packet_size: u8,
        size: usize,
        buf: &mut [u8],
    ) -> Result<usize, UsbError> {
        if buf.len() < size {
            return Err(UsbError::BufferTooSmall);
        }
        let mut packetiser = InPacketiser::new(
            size as u16,
            packet_size as u16,
            true,
            ZeroLengthPacket::Never,
        ); // setup is PID0 so data starts with PID1
        let mut depacketiser = InDepacketiser::new(size as u16, buf);

        self.control_transfer_inner(
            address,
            0,
            packet_size,
            Direction::In,
            size,
            &mut packetiser,
            &mut depacketiser,
        )
        .await?;

        Ok(depacketiser.total())
    }

    async fn control_transfer_out(
        &self,
        address: u8,
        packet_size: u8,
        size: usize,
        buf: &[u8],
    ) -> Result<usize, UsbError> {
        if buf.len() < size {
            return Err(UsbError::BufferTooSmall);
        }
        let mut packetiser = OutPacketiser::new(
            size as u16,
            packet_size as u16,
            buf,
            true,
            ZeroLengthPacket::Never,
        ); // setup is PID0 so data starts with PID1
        let mut depacketiser = OutDepacketiser::new();

        self.control_transfer_inner(
            address,
            0,
            packet_size,
            Direction::Out,
            size,
            &mut packetiser,
            &mut depacketiser,
        )
        .await?;

        Ok(buf.len())
    }

    fn interrupt_pipe(
        &self,
        pipe: Pipe,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Rp235xInterruptPipe {
        let n = pipe.which();
        let regs = unsafe { pac::USB::steal() };
        let dpram = unsafe { pac::USB_DPRAM::steal() };
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
                .bits(0x200 + (n as u16) * 128)
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

        Rp235xInterruptPipe {
            shared: self.shared,
            pipe,
            max_packet_size,
            data_toggle: Cell::new(false),
        }
    }

    /*
    async fn bulk_transfer_inner(
        &self,
        pipe: Pipe,
        address: u8,
        endpoint: u8,
        direction: Direction,
        size: usize,
        packet_size: u16,
        packetiser: &mut impl Packetiser,
        depacketiser: &mut impl Depacketiser,
    ) -> Result<usize, UsbError> {
        let n = pipe.n as usize;
        assert!(n > 0); // 0 is the control pipe, we want an "interrupt" pipe
        let epc = self.dpram.ep_control((n-1)*2);
        let epbc = self.dpram.ep_buffer_control(n*2);

        let packets = size / (packet_size as usize) + 1;
        defmt::info!("we'll need {} packets", packets);

        epc.write(|w| {
            if packets > 1 {
                w.double_buffered().set_bit();
            }
            // TODO: IRQ-on-stall
            w.interrupt_per_buff().set_bit();
            w.endpoint_type().bulk();
            unsafe { w.buffer_address().bits(0x200 + (n as u16) * 128) }
        });

        epbc
            .write(|w| unsafe { w.bits(0) });
        packetiser.prepare(self.dpram.ep_buffer_control(n*2));

        self.regs.host_addr_endp(n - 1).write(|w| unsafe {
            w.intep_dir().bit(direction == Direction::Out);
            w.endpoint().bits(endpoint);
            w.address().bits(address)
        });

        let mut started = false;

        loop {
            packetiser.prepare(epbc);
            packetiser.prepare(epbc);

            self.regs
                .sie_status()
                .write(|w| unsafe { w.bits(0xFF00_0000) });
            self.regs
                .buff_status()
                .write(|w| unsafe { w.bits(0x3 << n) });
            self.regs.inte().modify(|_, w| w.buff_status().set_bit());

            defmt::println!(
                "b bcr {:x} st {:x} hae {:x}",
                epbc.read().bits(),
                self.regs.sie_status().read().bits(),
                self.regs.host_addr_endp(n-1).read().bits(),
            );

            if !started {
                started = true;
                epc.modify(|_,w| w.enable().set_bit());
                self.regs
                    .int_ep_ctrl()
                    .modify(|r, w| unsafe { w.bits(r.bits() | (1 << n)) });
            }
            let f =
                Rp235xBulkEndpoint::new(pipe.n, &self.shared.pipe_wakers[n]);
            f.await;

            defmt::println!("b awaited");

            self.regs
                .buff_status()
                .write(|w| unsafe { w.bits(0x3 << n) });

            defmt::println!(
                "b bcr now {:x}",
                epbc.read().bits()
            );

            if epbc.read().stall().bit() {
                return Err(UsbError::Stall);
            }

            if let Some(packet) =
                depacketiser.retire(epbc)
            {
                if packet < packet_size.into() {
                    break;
                }
            }
            if let Some(packet) =
                depacketiser.retire(epbc)
            {
                if packet < packet_size.into() {
                    break;
                }
            }
        }


        Ok(size)
    }
     */
}

impl HostController for Rp235xHostController {
    type InterruptPipe = Rp235xInterruptPipe;
    type DeviceDetect = Rp235xDeviceDetect;

    fn device_detect(&self) -> Self::DeviceDetect {
        Rp235xDeviceDetect::new(&self.shared.device_waker)
    }

    fn reset_root_port(&self, rst: bool) {
        if rst {
            self.regs.sie_ctrl().modify(|_, w| w.reset_bus().set_bit());
        }
        // SIE_CTRL.RESET_BUS clears itself when done
    }

    async fn control_transfer<'a>(
        &self,
        address: u8,
        packet_size: u8,
        setup: SetupPacket,
        data_phase: DataPhase<'a>,
    ) -> Result<usize, UsbError> {
        let _pipe = self.alloc_pipe(EndpointType::Control).await;

        self.send_setup(address, &setup).await?;
        match data_phase {
            DataPhase::In(buf) => {
                let sz = self
                    .control_transfer_in(
                        address,
                        packet_size,
                        setup.wLength as usize,
                        buf,
                    )
                    .await?;
                self.control_transfer_out(address, packet_size, 0, &[])
                    .await?;
                Ok(sz)
            }
            DataPhase::Out(buf) => {
                let sz = self
                    .control_transfer_out(
                        address,
                        packet_size,
                        setup.wLength as usize,
                        buf,
                    )
                    .await?;
                self.control_transfer_in(address, packet_size, 0, &mut [])
                    .await?;
                Ok(sz)
            }
            DataPhase::None => {
                self.control_transfer_in(address, packet_size, 0, &mut [])
                    .await
            }
        }
    }

    async fn bulk_in_transfer(
        &self,
        address: u8,
        endpoint: u8,
        packet_size: u16,
        data: &mut [u8],
        transfer_type: TransferType,
        data_toggle: &Cell<bool>,
    ) -> Result<usize, UsbError> {
        let _pipe = self.alloc_pipe(EndpointType::Control).await;
        /*
        debug::println!("bulk in {} on pipe {} parity {}",
                        data.len(),
                        _pipe.n,
                        data_toggle.get());
         */
        let mut packetiser = InPacketiser::new(
            data.len() as u16,
            packet_size as u16,
            data_toggle.get(),
            match transfer_type {
                TransferType::FixedSize => ZeroLengthPacket::Never,
                TransferType::VariableSize => ZeroLengthPacket::AsNeeded,
            },
        );
        let length = data.len() as u16;
        let mut depacketiser = InDepacketiser::new(length, data);

        self.control_transfer_inner(
            address,
            endpoint,
            packet_size as u8,
            Direction::In,
            length as usize,
            &mut packetiser,
            &mut depacketiser,
        )
        .await?;
        data_toggle.set(data_toggle.get() ^ depacketiser.packet_parity);
        /*
        let mut parity = (((depacketiser.total() / (packet_size as usize)) + 1) & 1) == 1;
        if length == 512 {
            parity = !parity;
        }
        debug::println!(
            "pp {} p {} toggle now {}",
            depacketiser.packet_parity,
            parity,
            data_toggle.get()
        );
         */
        Ok(depacketiser.total())
    }

    async fn bulk_out_transfer(
        &self,
        address: u8,
        endpoint: u8,
        packet_size: u16,
        data: &[u8],
        transfer_type: TransferType,
        data_toggle: &Cell<bool>,
    ) -> Result<usize, UsbError> {
        let _pipe = self.alloc_pipe(EndpointType::Control).await;
        /*
        debug::println!(
            "bulk out {} on pipe {} parity {}", data.len(),
            _pipe.n,
            data_toggle.get()
        );
        */
        let mut packetiser = OutPacketiser::new(
            data.len() as u16,
            packet_size as u16,
            data,
            data_toggle.get(),
            match transfer_type {
                TransferType::FixedSize => ZeroLengthPacket::Never,
                TransferType::VariableSize => ZeroLengthPacket::AsNeeded,
            },
        );
        let mut depacketiser = OutDepacketiser::new();

        self.control_transfer_inner(
            address,
            endpoint,
            packet_size as u8,
            Direction::Out,
            data.len(),
            &mut packetiser,
            &mut depacketiser,
        )
        .await?;
        data_toggle.set(data_toggle.get() ^ depacketiser.packet_parity);
        /*
        let parity = (((data.len() / (packet_size as usize)) + 1) & 1) == 1;
        debug::println!(
            "pp {} p {} toggle now {}",
            depacketiser.packet_parity,
            parity,
            data_toggle.get()
        );
        */
        Ok(data.len())
    }

    // The trait defines this with "-> impl Future"-style syntax, but the one
    // is just sugar for the other according to Clippy.
    async fn alloc_interrupt_pipe(
        &self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Rp235xInterruptPipe {
        let pipe = self.alloc_pipe(EndpointType::Interrupt).await;
        debug::println!("interrupt_endpoint on pipe {}", pipe.which());
        self.interrupt_pipe(
            pipe,
            address,
            endpoint,
            max_packet_size,
            interval_ms,
        )
    }

    fn try_alloc_interrupt_pipe(
        &self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::InterruptPipe, UsbError> {
        if let Some(pipe) = self.try_alloc_pipe(EndpointType::Interrupt) {
            debug::println!("interrupt_endpoint on pipe {}", pipe.which());
            Ok(self.interrupt_pipe(
                pipe,
                address,
                endpoint,
                max_packet_size,
                interval_ms,
            ))
        } else {
            Err(UsbError::TooManyDevices)
        }
    }
}
