use super::*;
use mockall::mock;
use std::pin::pin;
use std::sync::Arc;
use std::task::Wake;
extern crate alloc;

mock! {
    TestWaker {}

    impl Wake for TestWaker {
        fn wake(self: Arc<Self>);
    }
}

#[test]
fn alloc_dealloc() {
    let p = Pool::new(2);
    assert_eq!(p.allocated.get().0, 0);
    {
        let pp = p.try_alloc().unwrap();
        assert_eq!(pp.n, 0);
        assert_eq!(p.allocated.get().0, 1);
    }
    assert_eq!(p.allocated.get().0, 0);
}

#[test]
fn alloc_fails() {
    let p = Pool::new(2);
    let _p1 = p.try_alloc().unwrap();
    let _p2 = p.try_alloc().unwrap();
    let r = p.try_alloc();
    assert!(r.is_none());
}

#[test]
fn display_pooled() {
    let p = Pool::new(2);
    let pp = p.try_alloc().unwrap();
    assert_eq!(format!("{}", pp), "Pooled(0)");
}

#[test]
fn alloc_setany_fails() {
    // setany only fails if we fill all 32 bits of the bitset
    let p = Pool::new(32);
    let _a: [Pooled; 32] = core::array::from_fn(|_| p.try_alloc().unwrap());
    assert!(p.try_alloc().is_none());
}

#[test]
fn dealloc_wakes_waker() {
    let p = Pool::new(2);
    let mut w = MockTestWaker::new();
    w.expect_wake().return_const(());

    let w = Waker::from(Arc::new(w));
    let mut c = core::task::Context::from_waker(&w);

    // We obtain the future but don't ".await" on it
    let mut pf = pin!(p.alloc());
    {
        let _p1 = p.try_alloc().unwrap();
        let _p2 = p.try_alloc().unwrap();
        let r = pf.as_mut().poll(&mut c);
        assert!(r.is_pending());
    }

    let r = pf.poll(&mut c);
    assert!(r.is_ready());
}
