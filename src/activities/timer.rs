//! Stopwatch and countdown state. Scheduling is owned by the UI's WM_TIMER;
//! this module never spins or polls on its own.

#![allow(dead_code)] // enabled when the timer UI lands (later phase).

use std::time::{Duration, Instant};

use crate::state::{Activity, ActivityId, Priority};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerMode {
    Stopwatch,
    Countdown,
}

pub struct TimerActivity {
    mode: TimerMode,
    started: Option<Instant>,
    accumulated: Duration,
    countdown: Duration,
    ended: bool,
}

impl Default for TimerActivity {
    fn default() -> Self {
        Self {
            mode: TimerMode::Stopwatch,
            started: None,
            accumulated: Duration::ZERO,
            countdown: Duration::ZERO,
            ended: false,
        }
    }
}

impl TimerActivity {
    pub fn start_stopwatch(&mut self, now: Instant) {
        self.mode = TimerMode::Stopwatch;
        self.started = Some(now);
        self.accumulated = Duration::ZERO;
        self.ended = false;
    }

    pub fn start_countdown(&mut self, duration: Duration, now: Instant) {
        self.mode = TimerMode::Countdown;
        self.started = Some(now);
        self.countdown = duration;
        self.accumulated = Duration::ZERO;
        self.ended = false;
    }

    pub fn stop(&mut self, now: Instant) {
        self.accumulated = self.elapsed(now);
        self.started = None;
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        self.accumulated
            + self
                .started
                .map(|start| now.saturating_duration_since(start))
                .unwrap_or_default()
    }

    pub fn remaining(&mut self, now: Instant) -> Duration {
        if self.mode != TimerMode::Countdown {
            return Duration::ZERO;
        }
        let remaining = self.countdown.saturating_sub(self.elapsed(now));
        self.ended = self.started.is_some() && remaining.is_zero();
        remaining
    }

    pub fn has_ended(&self) -> bool {
        self.ended
    }
}

impl Activity for TimerActivity {
    fn id(&self) -> ActivityId {
        ActivityId::Timer
    }
    fn priority(&self) -> Priority {
        if self.ended {
            Priority::TimerEnding
        } else {
            Priority::Media
        }
    }
    fn active(&self) -> bool {
        self.started.is_some() || self.ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_ends_and_is_promoted() {
        let now = Instant::now();
        let mut timer = TimerActivity::default();
        timer.start_countdown(Duration::from_secs(5), now);
        assert_eq!(
            timer.remaining(now + Duration::from_secs(4)),
            Duration::from_secs(1)
        );
        assert_eq!(
            timer.remaining(now + Duration::from_secs(5)),
            Duration::ZERO
        );
        assert!(timer.has_ended());
        assert_eq!(timer.priority(), Priority::TimerEnding);
    }
}
