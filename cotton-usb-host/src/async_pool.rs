use crate::debug;
use core::cell::RefCell;
use core::cell::UnsafeCell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

#[derive(Default)]
pub struct Pool<const N: usize> {
    allocated: UnsafeCell<u32>,
    waker: RefCell<Option<Waker>>,
}

pub struct Pooled<'a, const N: usize> {
    n: u8,
    pool: &'a Pool<N>,
}

impl<'a, const N: usize> Drop for Pooled<'a, N> {
    fn drop(&mut self) {
        self.pool.dealloc_internal(self.n);
    }
}

pub struct PoolFuture<'a, const N: usize> {
    pool: &'a Pool<N>,
}

impl<'a, const N: usize> Future for PoolFuture<'a, N> {
    type Output = Pooled<'a, N>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.pool.waker.replace(Some(cx.waker().clone()));

        if let Some(n) = self.pool.alloc_internal() {
            Poll::Ready(Pooled { n, pool: self.pool })
        } else {
            Poll::Pending
        }
    }
}

impl<const N: usize> Pool<N> {
    pub fn new() -> Self {
        assert!(N <= 32);
        Self {
            allocated: UnsafeCell::new(0),
            waker: None.into(),
        }
    }

    fn alloc_internal(&self) -> Option<u8> {
        let n = critical_section::with(|_| unsafe {
            let bits: u32 = *self.allocated.get();
            for i in 0..N {
                if (bits & (1 << i)) == 0 {
                    *self.allocated.get() = bits | 1 << i;
                    return Some(i as u8);
                }
            }
            None
        });

        debug::println!("allocated {:?}", n);
        n
    }

    fn dealloc_internal(&self, n: u8) {
        let bits = critical_section::with(|_| unsafe {
            let mut bits = *self.allocated.get();
            bits &= !(1 << n);
            *self.allocated.get() = bits;
            bits
        });

        debug::println!("deallocated, bits now {}", bits);

        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }

    pub async fn alloc(&self) -> Pooled<N> {
        let fut = PoolFuture { pool: self };
        fut.await
    }
}
