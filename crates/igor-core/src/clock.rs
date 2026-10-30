//! Injected wall-clock time.
//!
//! The runtime reads time only through [`Clock`], so tests can substitute a fixed clock and
//! produce byte-stable events and snapshots.

use std::time::SystemTime;

/// Source of the current wall-clock time.
pub trait Clock: Send + Sync {
    /// Returns the current time.
    fn now(&self) -> SystemTime;
}

/// The operating-system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}
