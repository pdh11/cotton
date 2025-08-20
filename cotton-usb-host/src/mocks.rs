use crate::host_controller::{
    DataPhase, DeviceStatus, HostController, InterruptPacket, TransferExtras,
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

    impl Stream for InterruptPipe {
        type Item = InterruptPacket;

        fn poll_next<'a>(
            self: Pin<&mut Self>,
            cx: &mut Context<'a>
        ) -> Poll<Option<<Self as Stream>::Item>>;
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
        #[allow(missing_docs)]
        pub fn device_detect(&self) -> MockDeviceDetect;

        #[allow(missing_docs)]
        pub fn reset_root_port(&self, rst: bool);

        #[allow(missing_docs)]
        pub fn control_transfer<'a>(
            &self,
            address: u8,
            transfer_extras: TransferExtras,
            packet_size: u8,
            setup: SetupPacket,
            data_phase: DataPhase<'a>,
        ) -> impl core::future::Future<Output = Result<usize, UsbError>>;

        #[allow(missing_docs)]
        pub fn bulk_in_transfer(
            &self,
            address: u8,
            endpoint: u8,
            packet_size: u16,
            data: &mut [u8],
            transfer_type: TransferType,
            data_toggle: &Cell<bool>,
        ) -> impl core::future::Future<Output = Result<usize, UsbError>>;

        #[allow(missing_docs)]
        pub fn bulk_out_transfer(
            &self,
            address: u8,
            endpoint: u8,
            packet_size: u16,
            data: &[u8],
            transfer_type: TransferType,
            data_toggle: &Cell<bool>,
        ) -> impl core::future::Future<Output = Result<usize, UsbError>>;

        #[allow(missing_docs)]
        pub fn alloc_interrupt_pipe(
            &self,
            address: u8,
            transfer_extras: TransferExtras,
            endpoint: u8,
            max_packet_size: u16,
            interval_ms: u8,
        ) -> impl core::future::Future<Output = MockInterruptPipe>;

        #[allow(missing_docs)]
        pub fn try_alloc_interrupt_pipe(
            &self,
            address: u8,
            transfer_extras: TransferExtras,
            endpoint: u8,
            max_packet_size: u16,
            interval_ms: u8,
        ) -> Result<MockInterruptPipe, UsbError>;
    }
}

/// A mock HostController, for testing purposes
///
/// Because the lifetimes got icky, the actual Mockall mock is kept as an
/// inner struct inside this one. So expectations should typically be set
/// on `mock_controller.inner`, not `mock_controller` itself. All methods
/// on MockHostController itself just forward straight to the inner struct.
pub struct MockHostController {
    /// Mock HostController, for testing purposes
    ///
    /// See src/tests/usb_bus.rs for widespread use of this facility.
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
        transfer_extras: TransferExtras,
        packet_size: u8,
        setup: SetupPacket,
        data_phase: DataPhase<'_>,
    ) -> impl core::future::Future<Output = Result<usize, UsbError>> {
        self.inner.control_transfer(
            address,
            transfer_extras,
            packet_size,
            setup,
            data_phase,
        )
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
        transfer_extras: TransferExtras,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> impl Future<Output = Self::InterruptPipe> {
        self.inner.alloc_interrupt_pipe(
            address,
            transfer_extras,
            endpoint,
            max_packet_size,
            interval_ms,
        )
    }

    fn try_alloc_interrupt_pipe(
        &self,
        address: u8,
        transfer_extras: TransferExtras,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::InterruptPipe, UsbError> {
        self.inner.try_alloc_interrupt_pipe(
            address,
            transfer_extras,
            endpoint,
            max_packet_size,
            interval_ms,
        )
    }
}
