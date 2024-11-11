use crate::bitset::BitSet;
use core::cell::{Cell, RefCell};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
#[cfg(feature = "std")]
use std::fmt::{self, Display};

pub struct Pool {
    total: u8,
    allocated: Cell<BitSet>,
    waker: RefCell<Option<Waker>>,
}

pub struct Pooled<'a> {
    pub n: u8,
    pool: &'a Pool,
}

#[cfg(feature = "std")]
impl Display for Pooled<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Pooled({})", self.n)
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for Pooled<'_> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "Pooled({})", self.n);
    }
}

impl Drop for Pooled<'_> {
    fn drop(&mut self) {
        self.pool.dealloc_internal(self.n);
    }
}

pub struct PoolFuture<'a> {
    pool: &'a Pool,
}

impl<'a> Future for PoolFuture<'a> {
    type Output = Pooled<'a>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.pool.waker.replace(Some(cx.waker().clone()));

        if let Some(n) = self.pool.alloc_internal() {
            Poll::Ready(Pooled { n, pool: self.pool })
        } else {
            Poll::Pending
        }
    }
}

impl Pool {
    pub const fn new(total: u8) -> Self {
        assert!(total <= 32);
        Self {
            total,
            allocated: Cell::new(BitSet::new()),
            waker: RefCell::new(None),
        }
    }

    fn alloc_internal(&self) -> Option<u8> {
        let mut bits = self.allocated.get();
        let n = bits.set_any()?;
        if n >= self.total {
            None
        } else {
            self.allocated.replace(bits);
            Some(n)
        }
    }

    fn dealloc_internal(&self, n: u8) {
        let mut bits = self.allocated.get();
        bits.clear(n);
        self.allocated.replace(bits);

        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }

    pub async fn alloc(&self) -> Pooled {
        let fut = PoolFuture { pool: self };
        fut.await
    }

    pub fn try_alloc(&self) -> Option<Pooled> {
        Some(Pooled {
            n: self.alloc_internal()?,
            pool: self,
        })
    }
}

#[cfg(all(test, feature = "std"))]
#[path = "tests/async_pool.rs"]
mod tests;
