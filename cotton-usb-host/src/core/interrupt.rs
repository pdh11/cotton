use crate::core::driver::{
    Driver, InterruptPacket, InterruptPipe, MultiInterruptPipe,
};
use crate::types::UsbError;
use core::cell::RefCell;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures::Stream;

pub struct InterruptStream<'driver, D: Driver + 'driver> {
    pub pipe: D::InterruptPipe<'driver>,
}

impl<D: Driver> Stream for InterruptStream<'_, D> {
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

pub struct MultiInterruptStream<'stack, D: Driver + 'stack> {
    pub pipe: &'stack RefCell<D::MultiInterruptPipe>,
}

impl<D: Driver> MultiInterruptStream<'_, D> {
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

impl<D: Driver> Stream for MultiInterruptStream<'_, D> {
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
