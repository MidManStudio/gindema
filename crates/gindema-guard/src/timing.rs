// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-guard.md, section "timing.rs"
// ============================================================================

use std::time::{Duration, Instant};

/// Measures elapsed wall-clock time between two points, as a cheap signal
/// that something paused execution for longer than the code in between
/// should plausibly take — a debugger breakpoint or single-stepping, most
/// obviously. See `docs/gindema-guard.md` for what this does and does not
/// actually catch.
#[derive(Debug, Clone, Copy)]
pub struct TimingWindow {
    start: Instant,
}

impl TimingWindow {
    /// Starts a new timing window, measured from now.
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Time elapsed since [`TimingWindow::start`] was called.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Whether the elapsed time is at or beyond `threshold` — the check a
    /// caller runs at the end of a window it expects to complete quickly.
    pub fn elapsed_at_least(&self, threshold: Duration) -> bool {
        self.elapsed() >= threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn a_window_well_under_its_threshold_does_not_trip() {
        let window = TimingWindow::start();
        // No sleep at all: elapsed is whatever it takes to call `start` and
        // immediately check it, which is reliably far under a 2-second
        // threshold even on a loaded CI machine.
        assert!(!window.elapsed_at_least(Duration::from_secs(2)));
    }

    #[test]
    fn a_window_that_actually_waits_trips_a_short_threshold() {
        let window = TimingWindow::start();
        sleep(Duration::from_millis(50));
        // 10ms is a wide enough margin below the 50ms sleep that this
        // shouldn't flake even on a slow or busy sandbox.
        assert!(window.elapsed_at_least(Duration::from_millis(10)));
    }

    #[test]
    fn elapsed_only_grows() {
        let window = TimingWindow::start();
        let first = window.elapsed();
        sleep(Duration::from_millis(5));
        let second = window.elapsed();
        assert!(second >= first);
    }
}
