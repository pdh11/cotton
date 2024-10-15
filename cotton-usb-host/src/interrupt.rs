use crate::host_controller::{
    HostController, InterruptPacket, InterruptPipe, MultiInterruptPipe,
};
use crate::types::UsbError;
use core::cell::RefCell;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures::Stream;

pub struct InterruptStream<'driver, HC: HostController + 'driver> {
    pub pipe: HC::InterruptPipe<'driver>,
}

impl<HC: HostController> Stream for InterruptStream<'_, HC> {
    type Item = InterruptPacket;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.pipe.set_waker(cx.waker());

        if let Some(packet) = self.pipe.poll() {
            Poll::Ready(Some(packet))
        } else {
            Poll::Pending
        }
    }
}

pub struct MultiInterruptStream<'stack, HC: HostController + 'stack> {
    pub pipe: &'stack RefCell<HC::MultiInterruptPipe>,
}

impl<HC: HostController> MultiInterruptStream<'_, HC> {
    pub fn try_add(
        &mut self,
        address: u8,
        endpoint: u8,
        max_packet_size: u8,
        interval_ms: u8,
    ) -> Result<(), UsbError> {
        self.pipe.borrow_mut().try_add(
            address,
            endpoint,
            max_packet_size,
            interval_ms,
        )
    }
}

impl<HC: HostController> Stream for MultiInterruptStream<'_, HC> {
    type Item = InterruptPacket;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.pipe.borrow_mut().set_waker(cx.waker());

        if let Some(packet) = self.pipe.borrow_mut().poll() {
            Poll::Ready(Some(packet))
        } else {
            Poll::Pending
        }
    }
}
