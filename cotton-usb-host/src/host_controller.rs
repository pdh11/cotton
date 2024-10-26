use crate::types::{SetupPacket, UsbError, UsbSpeed};
use core::ops::Deref;
use futures::Stream;

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "std", derive(Debug))]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    Present(UsbSpeed),
    Absent,
}

pub enum DataPhase<'a> {
    In(&'a mut [u8]),
    Out(&'a [u8]),
    None,
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
    type DeviceDetect: Stream<Item = DeviceStatus>;

    fn device_detect(&self) -> Self::DeviceDetect;

    fn control_transfer<'a>(
        &self,
        address: u8,
        packet_size: u8,
        setup: SetupPacket,
        data_phase: DataPhase<'a>,
    ) -> impl core::future::Future<Output = Result<usize, UsbError>>;

    fn alloc_interrupt_pipe(
        &self,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> impl core::future::Future<Output = Self::InterruptPipe<'_>>;

    fn multi_interrupt_pipe(&self) -> Self::MultiInterruptPipe;
}

#[cfg(all(test, feature = "std"))]
pub mod tests {
    use super::*;
    use mockall::mock;
    //use std::future::Future;
    //use std::pin::Pin;
    //use std::task::{Context, Poll};

    extern crate alloc;

    mock! {
        pub InterruptPipe {}

        impl InterruptPipe for InterruptPipe {
            fn set_waker(&self, waker: &core::task::Waker);
            fn poll(&self) -> Option<InterruptPacket>;
        }
    }

    mock! {
        pub MultiInterruptPipe {}

        impl InterruptPipe for MultiInterruptPipe {
            fn set_waker(&self, waker: &core::task::Waker);
            fn poll(&self) -> Option<InterruptPacket>;
        }

        impl MultiInterruptPipe for MultiInterruptPipe {
            fn try_add(
                &mut self,
                address: u8,
                endpoint: u8,
                max_packet_size: u8,
                interval_ms: u8,
            ) -> Result<(), UsbError>;

            fn remove(&mut self, address: u8);
        }
    }

    /*
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
                fn device_detect(&self) -> MockDeviceDetect;

                fn control_transfer<'a>(
                    &self,
                    address: u8,
                    packet_size: u8,
                    setup: SetupPacket,
                    data_phase: DataPhase<'a>,
                ) -> impl core::future::Future<Output = Result<usize, UsbError>>;

                fn alloc_interrupt_pipe(
                    &self,
                    address: u8,
                    endpoint: u8,
                    max_packet_size: u16,
                    interval_ms: u8,
                ) -> impl core::future::Future<Output = MockInterruptPipe>;

                fn multi_interrupt_pipe(&self) -> MockMultiInterruptPipe;
            }
        }

        pub struct MockHostController {
            inner: MockHostControllerInner,
        }

        impl HostController for MockHostController {
            type InterruptPipe<'hc> = MockInterruptPipe
                where Self: 'hc;
            type MultiInterruptPipe = MockMultiInterruptPipe;
            type DeviceDetect = MockDeviceDetect;

            fn device_detect(&self) -> Self::DeviceDetect {
                self.inner.device_detect()
            }

            fn control_transfer<'a>(
                &self,
                address: u8,
                packet_size: u8,
                setup: SetupPacket,
                data_phase: DataPhase<'a>,
            ) -> impl core::future::Future<Output = Result<usize, UsbError>> {
                self.inner.control_transfer(address, packet_size, setup, data_phase)
            }

            fn alloc_interrupt_pipe(
                &self,
                address: u8,
                endpoint: u8,
                max_packet_size: u16,
                interval_ms: u8) -> impl Future<Output = Self::InterruptPipe<'_>> {
                self.inner.alloc_interrupt_pipe(address, endpoint, max_packet_size, interval_ms)
            }

            fn multi_interrupt_pipe(
                &self,
            ) -> MockMultiInterruptPipe {
                self.inner.multi_interrupt_pipe()
            }
        }
    */

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
        assert_eq!((&p).len(), 10);
        assert_eq!((&p)[9], 1);
    }
}
