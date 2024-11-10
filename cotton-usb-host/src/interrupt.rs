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
mod tests {
    use super::*;
    use crate::host_controller::tests::MockInterruptPipe;
    use futures::Stream;
    use std::pin::pin;
    use std::sync::Arc;
    use std::task::{Wake, Waker};
    extern crate alloc;

    struct NoOpWaker;

    impl Wake for NoOpWaker {
        fn wake(self: Arc<Self>) {}
    }

    #[test]
    fn interrupt_stream_pending() {
        let mut ip = MockInterruptPipe::new();

        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        ip.expect_set_waker().return_const(());
        ip.expect_poll().returning(|| None);

        let stm = InterruptStream { pipe: ip };

        let stm = pin!(stm);
        let r = stm.poll_next(&mut c);
        assert!(r.is_pending());
    }

    #[test]
    fn interrupt_stream_ready() {
        let mut ip = MockInterruptPipe::new();

        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        ip.expect_set_waker().return_const(());
        ip.expect_poll()
            .returning(|| Some(InterruptPacket::default()));

        let stm = InterruptStream { pipe: ip };

        let stm = pin!(stm);
        c.waker().wake_by_ref();
        let r = stm.poll_next(&mut c);
        assert!(r.is_ready());
    }
}
