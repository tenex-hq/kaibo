//! Wall-clock time, injectable.
//!
//! Same shape as [`crate::config`]'s `Environment`: [`SystemClock`] is the
//! only implementation used outside tests, and the only place in the crate
//! that calls `SystemTime::now`. Anything that renders a relative age (e.g.
//! "3 days ago") needs a "now" to measure against, and a test that read the
//! real clock would be non-deterministic by definition - so tests inject a
//! fixed instant through this trait instead.

use std::time::SystemTime;

pub trait Clock {
    fn now(&self) -> SystemTime;
}

/// The only `Clock` used outside tests.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Test-only fixed clock: hermetic tests need a stable "now" rather than
    /// racing the wall clock.
    pub(crate) struct FixedClock(pub SystemTime);

    impl Clock for FixedClock {
        fn now(&self) -> SystemTime {
            self.0
        }
    }
}
