
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
