use crate::wire::SetupPacket;
use core::cell::Cell;
use core::ops::Deref;
use futures::Stream;

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UsbError {
    Nak,
    Stall,
    Timeout,
    Overflow,
    BitStuffError,
    CrcError,
    DataSeqError,
    BufferTooSmall,
    AllPipesInUse,
    ProtocolError,
    TooManyDevices,
    NoSuchEndpoint,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum UsbSpeed {
    Low1_5,
    Full12,
    High480,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    Present(UsbSpeed),
    Absent,
}

#[cfg_attr(feature = "std", derive(Debug))]
#[derive(PartialEq, Eq)]
pub enum DataPhase<'a> {
    In(&'a mut [u8]),
    Out(&'a [u8]),
    None,
}

impl DataPhase<'_> {
    pub fn is_in(&self) -> bool {
        matches!(self, DataPhase::In(_))
    }

    pub fn is_out(&self) -> bool {
        matches!(self, DataPhase::Out(_))
    }

    pub fn is_none(&self) -> bool {
        matches!(self, DataPhase::None)
    }

    pub fn in_with<F: FnOnce(&mut [u8])>(&mut self, f: F) {
        if let Self::In(x) = self {
            f(x)
        }
    }
}

/// Is this a fixed-size transfer or variable-size transfer?
///
/// According to USB 2.0 s5.3.2, the host must behave differently in
/// each case, so needs to know. (In particular, a fixed-size transfer
/// doesn't have a zero-length packet even if the data fills an exact
/// number of packets -- whereas a variable-size transfer does have a
/// zero-length packet in that case.)
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TransferType {
    /// Both ends know (via other means) how long the transfer should be
    FixedSize,
    /// Open-ended transfer (the size given is a maximum)
    VariableSize,
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

pub trait HostController {
    type InterruptPipe: InterruptPipe;
    type DeviceDetect: Stream<Item = DeviceStatus>;

    fn device_detect(&self) -> Self::DeviceDetect;

    fn reset_root_port(&self, rst: bool);

    fn control_transfer(
        &self,
        address: u8,
        packet_size: u8,
        setup: SetupPacket,
        data_phase: DataPhase<'_>,
    ) -> impl core::future::Future<Output = Result<usize, UsbError>>;

    fn bulk_in_transfer(
        &self,
        address: u8,
        endpoint: u8,
        packet_size: u16,
        data: &mut [u8],
        transfer_type: TransferType,
        data_toggle: &Cell<bool>,
    ) -> impl core::future::Future<Output = Result<usize, UsbError>>;

    fn bulk_out_transfer(
        &self,
        address: u8,
        endpoint: u8,
        packet_size: u16,
        data: &[u8],
        transfer_type: TransferType,
        data_toggle: &Cell<bool>,
    ) -> impl core::future::Future<Output = Result<usize, UsbError>>;

    fn alloc_interrupt_pipe(
        &self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> impl core::future::Future<Output = Self::InterruptPipe>;

    fn try_alloc_interrupt_pipe(
        &self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::InterruptPipe, UsbError>;
}

#[cfg(all(test, feature = "std"))]
#[path = "tests/host_controller.rs"]
pub(crate) mod tests;
