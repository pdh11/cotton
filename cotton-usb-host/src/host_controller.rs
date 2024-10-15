use crate::types::{UsbError, UsbSpeed};
use core::ops::Deref;

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    Present(UsbSpeed),
    Absent,
}

pub struct InterruptPacket {
    pub address: u8,
    pub endpoint: u8,
    pub size: u8,
    pub data: [u8; 64],
}

impl Default for InterruptPacket {
    fn default() -> Self {
        Self::new()
    }
}

impl InterruptPacket {
    pub const fn new() -> Self {
        Self {
            address: 0,
            endpoint: 0,
            size: 0,
            data: [0u8; 64],
        }
    }
}

impl Deref for InterruptPacket {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.data[0..(self.size as usize)]
    }
}

pub trait InterruptPipe {
    fn set_waker(&self, waker: &core::task::Waker);
    fn poll(&self) -> Option<InterruptPacket>;
}

pub trait MultiInterruptPipe: InterruptPipe {
    fn try_add(
        &mut self,
        address: u8,
        endpoint: u8,
        max_packet_size: u8,
        interval_ms: u8,
    ) -> Result<(), UsbError>;
    fn remove(&mut self, address: u8);
}

pub trait HostController {
    type InterruptPipe<'driver>: InterruptPipe
    where
        Self: 'driver;
    type MultiInterruptPipe: MultiInterruptPipe;

    fn alloc_interrupt_pipe(
        &self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> impl core::future::Future<Output = Self::InterruptPipe<'_>>;

    fn multi_interrupt_pipe(&self) -> Self::MultiInterruptPipe;
}
