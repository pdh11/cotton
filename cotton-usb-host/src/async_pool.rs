use core::cell::RefCell;
use core::cell::UnsafeCell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
#[cfg(feature = "std")]
use std::fmt::{self, Display};

pub struct Pool {
    total: u8,
    // @todo This can probably just be a RefCell<u32>
    allocated: UnsafeCell<u32>,
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
    pub fn new(total: u8) -> Self {
        assert!(total <= 32);
        Self {
            total,
            allocated: UnsafeCell::new(0),
            waker: None.into(),
        }
    }

    fn alloc_internal(&self) -> Option<u8> {
        // @todo We're always in thread context here, probably don't need CS
        critical_section::with(|_| unsafe {
            let bits: u32 = *self.allocated.get();
            for i in 0..self.total {
                if (bits & (1 << i)) == 0 {
                    *self.allocated.get() = bits | 1 << i;
                    return Some(i);
                }
            }
            None
        })
    }

    fn dealloc_internal(&self, n: u8) {
        critical_section::with(|_| unsafe {
            let mut bits = *self.allocated.get();
            bits &= !(1 << n);
            *self.allocated.get() = bits;
        });

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
