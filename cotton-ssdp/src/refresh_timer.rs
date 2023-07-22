use rand::Rng;
use std::time::Duration;

#[cfg(test)]
use mock_instant::Instant;

#[cfg(not(test))]
use std::time::Instant;

/// Encapsulating the SSDP retransmit process
///
/// The idea is, every 15 minutes or so, send a few repeated salvos of
/// notification messages. The interval between salvos is randomised to
/// help avoid network congestion.
///
pub struct RefreshTimer {
    next_salvo: Instant,
    phase: u8,
}

impl Default for RefreshTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl RefreshTimer {
    /// Create a new [`RefreshTimer`]
    ///
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_salvo: Instant::now(),
            phase: 0u8,
        }
    }

    /// Obtain the desired delay before the next refresh is needed
    #[must_use]
    pub fn next_refresh(&self) -> std::time::Duration {
        self.next_salvo.saturating_duration_since(Instant::now())
    }

    /// Update the refresh timer
    ///
    /// The desired timeout duration can be obtained from
    /// [`RefreshTimer::next_refresh`].
    ///
    pub fn update_refresh(&mut self) {
        if !self.next_refresh().is_zero() {
            return;
        }
        let random_offset = rand::thread_rng().gen_range(0..5);
        let period_sec = if self.phase == 0 { 800 } else { 1 } + random_offset;
        self.next_salvo += Duration::from_secs(period_sec);
        self.phase = (self.phase + 1) % 4;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retransmit_due_immediately() {
        let f = RefreshTimer::default();

        assert!(f.next_refresh().is_zero());
    }

    #[test]
    fn retransmit_sets_timeouts() {
        let mut f = RefreshTimer::default();

        f.update_refresh();
        let t = f.next_refresh();
        assert!(t > Duration::from_secs(780) && t < Duration::from_secs(820));
        mock_instant::MockClock::advance(t);

        f.update_refresh();
        let t = f.next_refresh();
        assert!(t < Duration::from_secs(20));
        mock_instant::MockClock::advance(t);

        f.update_refresh();
        let t = f.next_refresh();
        assert!(t < Duration::from_secs(20));
        mock_instant::MockClock::advance(t);

        f.update_refresh();
        let t = f.next_refresh();
        assert!(t < Duration::from_secs(20));
        mock_instant::MockClock::advance(t);

        f.update_refresh();
        let t = f.next_refresh();
        assert!(t > Duration::from_secs(780) && t < Duration::from_secs(820));

        // note no advance
        f.update_refresh();
        let t2 = f.next_refresh();
        assert!(t == t2);
    }
}
