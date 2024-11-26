use crate::host_controller::{
    DataPhase, DeviceStatus, HostController, InterruptPacket, InterruptPipe,
    TransferType, UsbError,
};
use crate::wire::SetupPacket;
use futures::Future;
use futures::Stream;
use mockall::mock;
use std::cell::Cell;
use std::pin::Pin;
use std::task::{Context, Poll};

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

    fn control_transfer(
        &self,
        address: u8,
        packet_size: u8,
        setup: SetupPacket,
        data_phase: DataPhase<'_>,
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
