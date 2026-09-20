//! Rolling restart-intensity window, matching supervisor.erl `add_restart`.
use std::collections::VecDeque;

/// Whole-second restart timestamps kept while `now <= then + period`.
///
/// Storage is bounded by `intensity + 1` entries: once the count exceeds the
/// intensity the supervisor retires, so older entries never matter.
#[derive(Clone, Debug)]
pub struct Window {
    intensity: usize,
    period: u64,
    stamps: VecDeque<u64>,
}

impl Window {
    pub fn new(intensity: u32, period_seconds: u32) -> Self {
        let intensity = intensity as usize;
        Self {
            intensity,
            period: u64::from(period_seconds),
            stamps: VecDeque::with_capacity(intensity.saturating_add(1)),
        }
    }

    /// Record one restart attempt at `now` and report whether the intensity is
    /// exceeded. The boundary is inclusive: a restart exactly `period` seconds
    /// after another still counts against it.
    pub fn charge(&mut self, now: u64) -> bool {
        self.stamps.push_front(now);
        let period = self.period;
        self.stamps
            .retain(|then| now <= then.saturating_add(period));
        self.stamps.truncate(self.intensity.saturating_add(1));
        self.stamps.len() > self.intensity
    }

    /// Restart attempts currently inside the window.
    pub fn count(&self) -> usize {
        self.stamps.len()
    }
}
