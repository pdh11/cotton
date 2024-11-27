use crate::bitset::BitSet;
use core::cell::{Cell, RefCell};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
#[cfg(feature = "std")]
use std::fmt::{self, Display};

/// Managing access to N equivalent resources
///
/// Callers who wish to access a resource (but don't care which one of the N)
/// can call the async function [`Pool::alloc`] which will return (awakening
/// the task) as soon as a resource is available.
///
/// Once the returned resource (represented as a [`Pooled`]) is
/// finished with, the caller can then drop it (i.e., let it go out of
/// scope) and it will be returned to the pool for another user.
///
/// Not quite the same as async_semaphore, because callers need to know
/// *which* of the N devices they were allotted.
///
/// # Example
/// ```rust
/// use cotton_usb_host::async_pool::Pool;
/// let mut pool = Pool::new(2); // this pool has two resources
/// let res = pool.try_alloc().unwrap(); // obtain a resource
/// println!("I got resource {}", res.which());
/// {
///     let res2 = pool.try_alloc().unwrap(); // obtain a resource
///     println!("I got resource {}", res.which());
///     let res3 = pool.try_alloc();
///     assert!(res3.is_none());   // oh dear, no resources available
///     // But now res2 goes out of scope (i.e., back into the pool)
/// }
/// let res4 = pool.try_alloc().unwrap();
/// println!("I got resource {}", res.which()); // success!
/// ```
///
/// For a larger example, see how the RP2040 USB host-controller driver
/// shares out its USB endpoints.
pub struct Pool {
    total: u8,
    allocated: Cell<BitSet>,
    waker: RefCell<Option<Waker>>,
}

/// Representing ownership of one of the resources in a [`Pool`]
pub struct Pooled<'a> {
    n: u8,
    pool: &'a Pool,
}

impl Pooled<'_> {
    /// Returns which one of the [`Pool`]'s N resources is owned by this `Pooled`
    pub fn which(&self) -> u8 {
        self.n
    }
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

struct PoolFuture<'a> {
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
    /// Create a new Pool, sharing out a number of equivalent resources
    ///
    /// # Parameters
    /// - `total`: The number of resources (must be 0-32)
    ///
    /// # Panics
    /// Will panic if `total`>32.
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

    /// Obtain one of the resources
    ///
    /// This asynchronous function will return immediately if any of
    /// the resources is currently idle (unused). Otherwise, it will
    /// wait until a resource is available. Once it has returned, the
    /// caller has ownership of the resource (represented by ownership
    /// of the [`Pooled`] object) until the `Pooled` is dropped -- for instance,
    /// at the end of a scope.
    ///
    /// It is not unsafe or unsound (in the Rust sense) to keep hold
    /// of a `Pooled` indefinitely, nor to `mem::forget` it -- but it is
    /// inadvisable, as this constitutes a denial-of-service against
    /// other potential resource users.
    ///
    /// # See also
    /// [`Pool::try_alloc()`] for a synchronous version
    pub async fn alloc(&self) -> Pooled {
        let fut = PoolFuture { pool: self };
        fut.await
    }

    /// Obtain a resource if one is immediately available
    ///
    /// Returns `Some` if any of the resources is currently idle (unused).
    /// Otherwise, returns `None`.
    ///
    /// # See also
    /// [`Pool::alloc()`] for an asynchronous version
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
