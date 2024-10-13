use crate::core::driver::{Driver, InterruptPacket, InterruptPipe};
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
