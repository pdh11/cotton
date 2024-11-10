use crate::wire::SetupPacket;
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
pub mod tests {
    use super::*;
    use mockall::mock;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    extern crate alloc;

    mock! {
        pub InterruptPipe {}

        impl InterruptPipe for InterruptPipe {
            fn set_waker(&self, waker: &core::task::Waker);
            fn poll(&self) -> Option<InterruptPacket>;
        }
    }

    mock! {
        pub DeviceDetect {}

        impl Stream for DeviceDetect {
            type Item = DeviceStatus;

            fn poll_next<'a>(
                self: Pin<&mut Self>,
                cx: &mut Context<'a>
            ) -> Poll<Option<<Self as Stream>::Item>>;
        }
    }

    mock! {
        pub HostControllerInner {
            pub fn device_detect(&self) -> MockDeviceDetect;

            pub fn reset_root_port(&self, rst: bool);

            pub fn control_transfer<'a>(
                &self,
                address: u8,
                packet_size: u8,
                setup: SetupPacket,
                data_phase: DataPhase<'a>,
            ) -> impl core::future::Future<Output = Result<usize, UsbError>>;

            pub fn alloc_interrupt_pipe(
                &self,
                address: u8,
                endpoint: u8,
                max_packet_size: u16,
                interval_ms: u8,
            ) -> impl core::future::Future<Output = MockInterruptPipe>;

            pub fn try_alloc_interrupt_pipe(
                &self,
                address: u8,
                endpoint: u8,
                max_packet_size: u16,
                interval_ms: u8,
            ) -> Result<MockInterruptPipe, UsbError>;
        }
    }

    pub struct MockHostController {
        pub inner: MockHostControllerInner,
    }

    impl Default for MockHostController {
        fn default() -> Self {
            Self {
                inner: MockHostControllerInner::new(),
            }
        }
    }

    impl HostController for MockHostController {
        type InterruptPipe = MockInterruptPipe;
        type DeviceDetect = MockDeviceDetect;

        fn device_detect(&self) -> Self::DeviceDetect {
            self.inner.device_detect()
        }

        fn reset_root_port(&self, rst: bool) {
            self.inner.reset_root_port(rst);
        }

        fn control_transfer<'a>(
            &self,
            address: u8,
            packet_size: u8,
            setup: SetupPacket,
            data_phase: DataPhase<'a>,
        ) -> impl core::future::Future<Output = Result<usize, UsbError>>
        {
            self.inner.control_transfer(
                address,
                packet_size,
                setup,
                data_phase,
            )
        }

        fn alloc_interrupt_pipe(
            &self,
            address: u8,
            endpoint: u8,
            max_packet_size: u16,
            interval_ms: u8,
        ) -> impl Future<Output = Self::InterruptPipe> {
            self.inner.alloc_interrupt_pipe(
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
            self.inner.try_alloc_interrupt_pipe(
                address,
                endpoint,
                max_packet_size,
                interval_ms,
            )
        }
    }

    #[test]
    fn packet_default() {
        let p = InterruptPacket::default();
        assert_eq!(p.size, 0);
    }

    #[test]
    fn packet_new() {
        let p = InterruptPacket::new();
        assert_eq!(p.size, 0);
    }

    #[test]
    fn packet_deref() {
        let mut p = InterruptPacket::new();
        p.size = 10;
        p.data[9] = 1;
        assert_eq!(p.len(), 10);
        assert_eq!((&p)[9], 1);
    }

    fn add_one(b: &mut [u8]) {
        b[0] += 1;
    }

    #[test]
    fn dataphase_accessors() {
        let mut b = [1u8; 1];
        let mut d1 = DataPhase::In(&mut b);
        assert!(d1.is_in());
        assert!(!d1.is_out());
        assert!(!d1.is_none());
        d1.in_with(add_one);
        assert_eq!(b[0], 2);
        let mut d1 = DataPhase::Out(&b);
        assert!(!d1.is_in());
        assert!(d1.is_out());
        assert!(!d1.is_none());
        d1.in_with(add_one);
        assert_eq!(b[0], 2); // not IN, nothing added
        let mut d1 = DataPhase::None;
        assert!(!d1.is_in());
        assert!(!d1.is_out());
        assert!(d1.is_none());
        d1.in_with(add_one);
        assert_eq!(b[0], 2); // not IN, nothing added
    }
}
