use crate::host_controller::{InterruptPacket, InterruptPipe};
use core::pin::Pin;
use core::task::{Context, Poll};
use futures::Stream;

pub struct InterruptStream<PIPE: InterruptPipe> {
    pub pipe: PIPE,
}

impl<PIPE: InterruptPipe> Stream for InterruptStream<PIPE> {
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

#[cfg(all(test, feature = "std"))]
#[path = "tests/interrupt.rs"]
mod tests;
