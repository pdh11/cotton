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

        pub fn bulk_in_transfer(
            &self,
            address: u8,
            endpoint: u8,
            packet_size: u16,
            data: &mut [u8],
            transfer_type: TransferType,
            data_toggle: &Cell<bool>,
        ) -> impl core::future::Future<Output = Result<usize, UsbError>>;

        pub fn bulk_out_transfer(
            &self,
            address: u8,
            endpoint: u8,
            packet_size: u16,
            data: &[u8],
            transfer_type: TransferType,
            data_toggle: &Cell<bool>,
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
    ) -> impl core::future::Future<Output = Result<usize, UsbError>> {
        self.inner
            .control_transfer(address, packet_size, setup, data_phase)
    }

    fn bulk_in_transfer(
        &self,
        address: u8,
        endpoint: u8,
        packet_size: u16,
        data: &mut [u8],
        transfer_type: TransferType,
        data_toggle: &Cell<bool>,
    ) -> impl core::future::Future<Output = Result<usize, UsbError>> {
        self.inner.bulk_in_transfer(
            address,
            endpoint,
            packet_size,
            data,
            transfer_type,
            data_toggle,
        )
    }

    fn bulk_out_transfer(
        &self,
        address: u8,
        endpoint: u8,
        packet_size: u16,
        data: &[u8],
        transfer_type: TransferType,
        data_toggle: &Cell<bool>,
    ) -> impl core::future::Future<Output = Result<usize, UsbError>> {
        self.inner.bulk_out_transfer(
            address,
            endpoint,
            packet_size,
            data,
            transfer_type,
            data_toggle,
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
