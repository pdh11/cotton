use crate::async_pool::Pool;
use crate::debug;
use crate::host_controller::{
    DeviceStatus, HostController, InterruptPacket, InterruptPipe,
    MultiInterruptPipe,
};
use crate::types::{UsbError, UsbSpeed};
use core::cell::Cell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures::Stream;
use rp2040_pac as pac;
use rtic_common::waker_registration::CriticalSectionWakerRegistration;

pub struct UsbShared {
    // @TODO shouldn't be pub
    pub device_waker: CriticalSectionWakerRegistration,
    pub pipe_wakers: [CriticalSectionWakerRegistration; 16],
}

impl UsbShared {
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

impl UsbShared {
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

impl Default for UsbShared {
    fn default() -> Self {
        Self::new()
    }
}

pub struct UsbStatics {
    // @TODO shouldn't be pub
    pub bulk_pipes: Pool,
}

impl UsbStatics {
    pub const fn new() -> Self {
        Self {
            bulk_pipes: Pool::new(15),
        }
    }
}

impl Default for UsbStatics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Copy, Clone)]
pub struct DeviceDetect<'a> {
    waker: &'a CriticalSectionWakerRegistration,
    status: DeviceStatus,
}

impl<'a> DeviceDetect<'a> {
    pub fn new(waker: &'a CriticalSectionWakerRegistration) -> Self {
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

pub struct Rp2040ControlEndpoint<'a> {
    waker: &'a CriticalSectionWakerRegistration,
}

impl<'a> Rp2040ControlEndpoint<'a> {
    pub fn new(waker: &'a CriticalSectionWakerRegistration) -> Self {
        Self { waker }
    }
}

impl Future for Rp2040ControlEndpoint<'_> {
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

pub type Pipe<'a> = crate::async_pool::Pooled<'a>;

pub struct Rp2040InterruptPipe<'driver> {
    driver: &'driver Rp2040HostController,
    pipe: Pipe<'driver>,
    max_packet_size: u16,
    data_toggle: Cell<bool>,
}

impl InterruptPipe for Rp2040InterruptPipe<'_> {
    fn set_waker(&self, waker: &core::task::Waker) {
        self.driver.shared.pipe_wakers[self.pipe.n as usize].register(waker);
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

type MultiPooled<'a> = crate::async_pool::MultiPooled<'a>;

struct PipeInfo {
    address: u8,
    endpoint: u8,
    max_packet_size: u8,
    data_toggle: Cell<bool>,
}

pub struct Rp2040MultiInterruptPipe {
    shared: &'static UsbShared,
    //statics: &'static UsbStatics,
    pipes: MultiPooled<'static>,
    pipe_info: [Option<PipeInfo>; 16],
}

impl InterruptPipe for Rp2040MultiInterruptPipe {
    fn set_waker(&self, waker: &core::task::Waker) {
        for i in self.pipes.iter() {
            self.shared.pipe_wakers[(i + 1) as usize].register(waker);
        }
    }

    fn poll(&self) -> Option<InterruptPacket> {
        let dpram = unsafe { pac::USBCTRL_DPRAM::steal() };
        for i in self.pipes.iter() {
            let pipe = i + 1;
            let bc = dpram.ep_buffer_control((pipe * 2) as usize).read();
            if bc.full_0().bit() {
                let info = self.pipe_info[pipe as usize].as_ref().unwrap();
                let mut result = InterruptPacket {
                    address: info.address,
                    endpoint: info.endpoint,
                    size: core::cmp::min(bc.length_0().bits(), 64) as u8,
                    ..Default::default()
                };
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        (0x5010_0200 + (pipe as u32) * 64) as *const u8,
                        &mut result.data[0] as *mut u8,
                        result.size as usize,
                    )
                };
                info.data_toggle.set(!info.data_toggle.get());
                dpram.ep_buffer_control((pipe * 2) as usize).write(
                    |w| unsafe {
                        w.full_0()
                            .clear_bit()
                            .pid_0()
                            .bit(info.data_toggle.get())
                            .length_0()
                            .bits(info.max_packet_size as u16)
                            .last_0()
                            .set_bit()
                    },
                );

                cortex_m::asm::delay(12);

                dpram
                    .ep_buffer_control((pipe * 2) as usize)
                    .modify(|_, w| w.available_0().set_bit());
                let regs = unsafe { pac::USBCTRL_REGS::steal() };
                defmt::println!(
                    "ME ready inte {:x} iec {:x} ecr {:x} epbc {:x}",
                    regs.inte().read().bits(),
                    regs.int_ep_ctrl().read().bits(),
                    dpram.ep_control((pipe * 2) as usize - 2).read().bits(),
                    dpram.ep_buffer_control((pipe * 2) as usize).read().bits(),
                );

                return Some(result);
            }
        }

        let regs = unsafe { pac::USBCTRL_REGS::steal() };
        regs.inte().modify(|_, w| w.buff_status().set_bit());
        // shift pipes.bits left because we don't use pipe 0
        regs.int_ep_ctrl().modify(|r, w| unsafe {
            w.bits(r.bits() | (self.pipes.bits() * 2))
        });
        defmt::println!(
            "ME pending bits {:x} inte {:x} iec {:x}",
            self.pipes.bits() * 2,
            regs.inte().read().bits(),
            regs.int_ep_ctrl().read().bits(),
        );
        let mut mask = 0;
        for i in self.pipes.iter() {
            mask |= 3 << (i * 2);
        }
        regs.ep_status_stall_nak()
            .write(|w| unsafe { w.bits(mask) });

        None
    }
}

impl MultiInterruptPipe for Rp2040MultiInterruptPipe {
    fn try_add(
        &mut self,
        address: u8,
        endpoint: u8,
        max_packet_size: u8,
        interval_ms: u8,
    ) -> Result<(), UsbError> {
        let p = self.pipes.try_alloc().ok_or(UsbError::AllPipesInUse)? + 1;
        defmt::println!("ME got pipe {}", p);
        let pi = PipeInfo {
            address,
            endpoint,
            max_packet_size,
            data_toggle: Cell::new(false),
        };

        self.pipe_info[p as usize] = Some(pi);

        let regs = unsafe { pac::USBCTRL_REGS::steal() };
        let dpram = unsafe { pac::USBCTRL_DPRAM::steal() };
        regs.host_addr_endp((p - 1) as usize).write(|w| unsafe {
            w.address()
                .bits(address)
                .endpoint()
                .bits(endpoint)
                .intep_dir()
                .clear_bit() // IN
        });

        dpram.ep_control((p * 2 - 2) as usize).write(|w| unsafe {
            w.enable()
                .set_bit()
                .interrupt_per_buff()
                .set_bit()
                .endpoint_type()
                .interrupt()
                .buffer_address()
                .bits(0x200 + (p as u16) * 64)
                .host_poll_interval()
                .bits(core::cmp::min(interval_ms as u16, 9))
        });

        dpram.ep_buffer_control((p * 2) as usize).write(|w| unsafe {
            w.full_0()
                .clear_bit()
                .length_0()
                .bits(max_packet_size as u16)
                .pid_0()
                .clear_bit()
                .last_0()
                .set_bit()
        });

        cortex_m::asm::delay(12);

        dpram
            .ep_buffer_control((p * 2) as usize)
            .modify(|_, w| w.available_0().set_bit());

        Ok(())
    }

    fn remove(&mut self, _address: u8) {
        todo!()
    }
}

pub struct Rp2040HostController {
    shared: &'static UsbShared,
    statics: &'static UsbStatics,
    //control_pipes: Pool,
}

impl Rp2040HostController {
    pub fn new(
        shared: &'static UsbShared,
        statics: &'static UsbStatics,
    ) -> Self {
        Self {
            shared,
            statics,
            //control_pipes: Pool::new(1),
        }
    }
}

impl HostController for Rp2040HostController {
    type InterruptPipe<'driver> = Rp2040InterruptPipe<'driver> where Self: 'driver;
    type MultiInterruptPipe = Rp2040MultiInterruptPipe;

    // The trait defines this with "-> impl Future"-style syntax, but the one
    // is just sugar for the other according to Clippy.
    async fn alloc_interrupt_pipe(
        &self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Rp2040InterruptPipe<'_> {
        let mut pipe = self.statics.bulk_pipes.alloc().await;
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

        Self::InterruptPipe {
            driver: self,
            pipe,
            max_packet_size,
            data_toggle: Cell::new(false),
        }
    }

    fn multi_interrupt_pipe(&self) -> Rp2040MultiInterruptPipe {
        const N: Option<PipeInfo> = None;
        Self::MultiInterruptPipe {
            shared: self.shared,
            //statics: self.statics,
            pipes: MultiPooled::new(&self.statics.bulk_pipes),
            pipe_info: [N; 16],
        }
    }
}
