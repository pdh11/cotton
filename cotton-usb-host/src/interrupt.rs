use crate::host_controller::{
    InterruptPacket, InterruptPipe, MultiInterruptPipe, UsbError,
};
use core::cell::RefCell;
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

pub struct MultiInterruptStream<'stack, PIPE: MultiInterruptPipe + 'stack> {
    pub pipe: &'stack RefCell<PIPE>,
}

impl<PIPE: MultiInterruptPipe> MultiInterruptStream<'_, PIPE> {
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

impl<PIPE: MultiInterruptPipe> Stream for MultiInterruptStream<'_, PIPE> {
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

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::host_controller::tests::{
        MockInterruptPipe, MockMultiInterruptPipe,
    };
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
        c.waker().clone().wake();
        let r = stm.poll_next(&mut c);
        assert!(r.is_ready());
    }

    #[test]
    fn multi_interrupt_stream_pending() {
        let mut ip = MockMultiInterruptPipe::new();

        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        ip.expect_set_waker().return_const(());
        ip.expect_poll().returning(|| None);

        let rc = RefCell::new(ip);

        let stm = MultiInterruptStream { pipe: &rc };

        let stm = pin!(stm);
        let r = stm.poll_next(&mut c);
        assert!(r.is_pending());
    }

    #[test]
    fn multi_interrupt_stream_ready() {
        let mut ip = MockMultiInterruptPipe::new();

        let w = Waker::from(Arc::new(NoOpWaker));
        let mut c = core::task::Context::from_waker(&w);

        ip.expect_set_waker().return_const(());
        ip.expect_poll()
            .returning(|| Some(InterruptPacket::default()));

        let rc = RefCell::new(ip);

        let stm = MultiInterruptStream { pipe: &rc };

        let stm = pin!(stm);
        let r = stm.poll_next(&mut c);
        assert!(r.is_ready());
    }

    #[test]
    fn multi_interrupt_stream_passes_on_add() {
        let mut ip = MockMultiInterruptPipe::new();

        ip.expect_try_add().returning(|_, _, _, _| Ok(()));

        let rc = RefCell::new(ip);

        let mut stm = MultiInterruptStream { pipe: &rc };

        let r = stm.try_add(1, 2, 8, 10);
        assert!(r.is_ok());
    }
}
