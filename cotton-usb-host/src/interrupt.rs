use crate::host_controller::{InterruptPacket, InterruptPipe};
use core::pin::Pin;
use core::task::{Context, Poll};
use futures::Stream;

/// A [`futures::Stream`] based on an [`InterruptPipe`]
pub struct InterruptStream<PIPE: InterruptPipe> {
    pipe: PIPE,
}

impl<PIPE: InterruptPipe> InterruptStream<PIPE> {
    /// Create a new InterruptStream wrapping an InterruptPipe
    pub fn new(pipe: PIPE) -> Self {
        Self {
            pipe
        }
    }
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
