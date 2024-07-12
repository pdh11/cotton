use core::ops::AddAssign;

/// A timebase is a pair of types Duration/Instant
///
/// The standard types (`std::time::{Duration, Instant}`) and the smoltcp
/// types (`smoltcp::time::{Duration, Instant}`) have almost but not quite the
/// same API. This trait papers over the difference and lets RefreshTimer be
/// generic, i.e. able to use *either* the `std` types (where available) or
/// the smoltcp types (in no_std).
pub trait Timebase {
    /// Representing a span of elapsed time, see `core::time::Duration`
    type Duration: From<core::time::Duration>;

    /// Representing a moment in time, see `std::time::Instant`
    type Instant: AddAssign<Self::Duration> + Ord + Copy;
}

/// Implementing the `Timebase` abstraction in terms of smoltcp types
#[cfg(feature = "smoltcp")]
pub struct SmoltcpTimebase();

#[cfg(feature = "smoltcp")]
impl Timebase for SmoltcpTimebase {
    type Duration = smoltcp::time::Duration;
    type Instant = smoltcp::time::Instant;
}

/// Implementing the `Timebase` abstraction in terms of standard types
#[cfg(feature = "std")]
pub struct StdTimebase();

#[cfg(feature = "std")]
impl Timebase for StdTimebase {
    type Duration = std::time::Duration;
    type Instant = std::time::Instant;
}

/// Encapsulating the SSDP retransmit process
///
/// The idea is, every 15 minutes or so, send a few repeated salvos of
/// notification messages. The interval between salvos is randomised to
/// help avoid network congestion.
///
pub struct RefreshTimer<T: Timebase> {
    random_seed: u32,
    next_salvo: T::Instant,
    phase: u8,
}

impl<T: Timebase> RefreshTimer<T> {
    /// Create a new [`RefreshTimer`]
    ///
    #[must_use]
    pub fn new(random_seed: u32, now: T::Instant) -> Self {
        Self {
            random_seed,
            next_salvo: now,
            phase: 0u8,
        }
    }

    /// Reset the refresh timer (e.g. if network has gone away and come back)
    pub fn reset(&mut self, now: T::Instant) {
        self.next_salvo = now;
        self.phase = 0;
    }

    /// Obtain the desired delay before the next refresh is needed
    #[must_use]
    pub fn next_refresh(&self) -> T::Instant {
        self.next_salvo
    }

    /// Update the refresh timer
    ///
    /// The desired timeout duration can be obtained from
    /// [`RefreshTimer::next_refresh`].
    ///
    pub fn update_refresh(&mut self, now: T::Instant) {
        if now < self.next_salvo {
            return;
        }
        // random offset 0-2550ms
        let random_offset =
            ((self.random_seed >> (self.phase * 8)) & 255) * 10;
        let period_msec = if self.phase == 0 { 800_000 } else { 1_000 }
            + (random_offset as u64);
        self.next_salvo +=
            core::time::Duration::from_millis(period_msec).into();
        self.phase = (self.phase + 1) % 4;
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn retransmit_due_immediately() {
        let now = Instant::now();
        let f = RefreshTimer::<StdTimebase>::new(0, now);

        assert_eq!(f.next_refresh(), now);
    }

    #[test]
    fn retransmit_sets_timeouts() {
        let mut now = Instant::now();
        let mut f = RefreshTimer::<StdTimebase>::new(0, now);

        f.update_refresh(now);
        let t = f.next_refresh() - now;
        assert!(t > Duration::from_secs(780) && t < Duration::from_secs(820));
        now += t;

        f.update_refresh(now);
        let t = f.next_refresh() - now;
        assert!(t < Duration::from_secs(20));
        now += t;

        f.update_refresh(now);
        let t = f.next_refresh() - now;
        assert!(t < Duration::from_secs(20));
        now += t;

        f.update_refresh(now);
        let t = f.next_refresh() - now;
        assert!(t < Duration::from_secs(20));
        now += t;

        f.update_refresh(now);
        let t = f.next_refresh() - now;
        assert!(t > Duration::from_secs(780) && t < Duration::from_secs(820));

        // note no "now += t"
        f.update_refresh(now);
        let t2 = f.next_refresh() - now;
        assert!(t == t2);
    }

    #[test]
    fn reset() {
        let now = Instant::now();
        let mut f = RefreshTimer::<StdTimebase>::new(0, now);
        f.update_refresh(now);
        assert_ne!(f.next_refresh(), now);
        f.reset(now);
        assert_eq!(f.next_refresh(), now);
    }
}
