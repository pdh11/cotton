use crate::async_pool::Pool;
use crate::types::{SetupPacket, UsbDevice, UsbError, UsbSpeed};
use crate::types::{
    DEVICE_DESCRIPTOR, DEVICE_TO_HOST, GET_DESCRIPTOR, HOST_TO_DEVICE,
    SET_ADDRESS,
};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use rp2040_pac as pac;
use rtic_common::waker_registration::CriticalSectionWakerRegistration;

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

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
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
                    if let Some((this_packet, is_last)) = self.next_packet() {
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
                    if let Some((this_packet, is_last)) = self.next_packet() {
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

type Pipe<'a> = crate::async_pool::Pooled<'a, 1>;

pub struct UsbStack<'a> {
    regs: pac::USBCTRL_REGS,
    dpram: pac::USBCTRL_DPRAM,
    waker: &'a CriticalSectionWakerRegistration,
    control_pipes: Pool<1>,
}

impl<'a> UsbStack<'a> {
    pub fn new(
        regs: pac::USBCTRL_REGS,
        dpram: pac::USBCTRL_DPRAM,
        resets: &mut pac::RESETS,
        waker: &'a CriticalSectionWakerRegistration,
    ) -> Self {
        resets.reset().modify(|_, w| w.usbctrl().set_bit());
        resets.reset().modify(|_, w| w.usbctrl().clear_bit());

        Self {
            regs,
            dpram,
            waker,
            control_pipes: Pool::new(),
        }
    }

    async fn alloc_pipe(&self) -> Pipe {
        self.control_pipes.alloc().await
    }

    pub async fn enumerate_root_device(
        &self,
        mut delay: impl embedded_hal_async::delay::DelayNs,
    ) -> UsbDevice {
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

        let f = UsbFuture::new(self.waker);

        f.await;

        let status = self.regs.sie_status().read();
        defmt::trace!("sie_status=0x{:x}", status.bits());
        let speed = match status.speed().bits() {
            1 => {
                defmt::println!("LS detected");
                UsbSpeed::Low1_1
            }
            2 => {
                defmt::println!("FS detected");
                UsbSpeed::Full12
            }
            _ => UsbSpeed::Low1_1,
        };

        self.regs.sie_ctrl().modify(|_, w| w.reset_bus().set_bit());

        delay.delay_ms(50).await;

        self.regs
            .sie_ctrl()
            .modify(|_, w| w.reset_bus().clear_bit());

        // Read prefix of device descriptor
        let mut descriptors = [0u8; 18];
        let rc = self
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
            )
            .await;

        let packet_size_ep0 = if rc.is_ok() { descriptors[7] } else { 8 };

        // Set address (root device always gets "1")
        let _ = self
            .control_transfer_out(
                0,
                packet_size_ep0,
                SetupPacket {
                    bmRequestType: HOST_TO_DEVICE,
                    bRequest: SET_ADDRESS,
                    wValue: 1,
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
            defmt::println!("fetched {:?}", rc);
        }

        UsbDevice {
            address: 1,
            packet_size_ep0,
            vid,
            pid,
            speed,
        }
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

            let f = UsbFuture::new(self.waker);

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

        let _pipe = self.alloc_pipe().await;

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

        let _pipe = self.alloc_pipe().await;

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
}
