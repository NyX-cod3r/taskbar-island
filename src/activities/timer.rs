//! timer.rs — the app-internal countdown. Time is derived from an `Instant`
//! base; the UI only asks for a redraw once per second while running.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct Countdown {
    pub name: String,
    pub total: Duration,
    started_at: Option<Instant>,
    /// Seconds elapsed before the current run segment (pause support).
    accumulated: Duration,
    pub finished: bool,
}

impl Default for Countdown {
    fn default() -> Self {
        Self {
            name: "Timer".into(),
            total: Duration::ZERO,
            started_at: None,
            accumulated: Duration::ZERO,
            finished: false,
        }
    }
}

impl Countdown {
    pub fn start(name: impl Into<String>, total: Duration, now: Instant) -> Self {
        Self {
            name: name.into(),
            total,
            started_at: Some(now),
            accumulated: Duration::ZERO,
            finished: false,
        }
    }

    /// A duration exists, whether it is armed, running, paused or done.
    pub fn active(&self) -> bool {
        self.total > Duration::ZERO
    }

    /// A duration is set but the countdown has not been started.
    pub fn armed(&self) -> bool {
        self.active() && self.started_at.is_none() && self.accumulated.is_zero() && !self.finished
    }

    pub fn running(&self) -> bool {
        self.started_at.is_some() && !self.finished
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        let live = self
            .started_at
            .map(|start| now.saturating_duration_since(start))
            .unwrap_or_default();
        (self.accumulated + live).min(self.total)
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.total.saturating_sub(self.elapsed(now))
    }

    pub fn fraction(&self, now: Instant) -> f32 {
        if self.total.is_zero() {
            return 0.0;
        }
        (self.elapsed(now).as_secs_f64() / self.total.as_secs_f64()).clamp(0.0, 1.0) as f32
    }

    pub fn remaining_fraction(&self, now: Instant) -> f32 {
        1.0 - self.fraction(now)
    }

    /// Update the finished flag; returns true on the transition to finished.
    pub fn tick(&mut self, now: Instant) -> bool {
        if !self.finished && self.total > Duration::ZERO && self.remaining(now).is_zero() {
            self.accumulated = self.total;
            self.started_at = None;
            self.finished = true;
            return true;
        }
        false
    }

    pub fn toggle_pause(&mut self, now: Instant) {
        if self.finished {
            return;
        }
        if self.started_at.is_some() {
            self.accumulated = self.elapsed(now);
            self.started_at = None;
        } else if self.accumulated < self.total {
            self.started_at = Some(now);
        }
    }

    /// Start a paused or armed countdown; a finished timer restarts its total.
    pub fn resume(&mut self, now: Instant) {
        if !self.active() {
            return;
        }
        if self.finished {
            self.accumulated = Duration::ZERO;
            self.finished = false;
        }
        if self.started_at.is_none() && self.accumulated < self.total {
            self.started_at = Some(now);
        }
    }

    /// Preset chip: idle arms the chosen duration, paused/running extends the
    /// remaining time by that many minutes.
    pub fn preset(&mut self, minutes: u32) {
        let add = Duration::from_secs(minutes as u64 * 60);
        if self.running() || (!self.finished && self.accumulated > Duration::ZERO) {
            self.total += add;
        } else {
            self.total = add;
            self.started_at = None;
            self.accumulated = Duration::ZERO;
            self.finished = false;
        }
    }

    #[allow(dead_code)] // Timer +1 button (A-scope); tests also use it.
    pub fn add_minute(&mut self) {
        self.add_minutes(1);
    }

    #[allow(dead_code)] // N-minute presets (A-scope).
    pub fn add_minutes(&mut self, minutes: u32) {
        if self.finished || minutes == 0 {
            return;
        }
        self.total += Duration::from_secs(minutes as u64 * 60);
    }

    pub fn stop(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_ticks_and_finishes() {
        let now = Instant::now();
        let mut t = Countdown::start("Study session", Duration::from_secs(300), now);
        assert_eq!(
            t.remaining(now + Duration::from_secs(20)),
            Duration::from_secs(280)
        );
        assert!(!t.tick(now + Duration::from_secs(299)));
        assert!(t.tick(now + Duration::from_secs(300)));
        assert!(t.finished);
        assert!(!t.running());
        assert!(t.remaining(now + Duration::from_secs(999)).is_zero());
    }

    #[test]
    fn pause_freezes_elapsed() {
        let now = Instant::now();
        let mut t = Countdown::start("Timer", Duration::from_secs(600), now);
        t.toggle_pause(now + Duration::from_secs(10));
        assert!(!t.running());
        let paused = t.elapsed(now + Duration::from_secs(50));
        assert_eq!(paused, Duration::from_secs(10));
        t.toggle_pause(now + Duration::from_secs(60));
        assert!(t.running());
        assert_eq!(
            t.elapsed(now + Duration::from_secs(65)),
            Duration::from_secs(15)
        );
    }

    #[test]
    fn add_minute_extends_total() {
        let now = Instant::now();
        let mut t = Countdown::start("Timer", Duration::from_secs(60), now);
        t.add_minute();
        assert_eq!(t.total, Duration::from_secs(120));
        assert_eq!(t.remaining(now), Duration::from_secs(120));
    }

    #[test]
    fn empty_timer_is_inactive() {
        let t = Countdown::default();
        assert!(!t.active());
        assert!(!t.armed());
    }

    #[test]
    fn preset_arms_then_start_runs() {
        let now = Instant::now();
        let mut t = Countdown::default();
        t.preset(5);
        assert!(t.active());
        assert!(t.armed());
        assert_eq!(t.total, Duration::from_secs(300));
        t.resume(now);
        assert!(t.running());
        assert_eq!(
            t.remaining(now + Duration::from_secs(60)),
            Duration::from_secs(240)
        );
    }

    #[test]
    fn presets_extend_a_running_timer() {
        let now = Instant::now();
        let mut t = Countdown::start("Timer", Duration::from_secs(60), now);
        t.preset(5);
        assert_eq!(t.total, Duration::from_secs(360));
        t.preset(10);
        assert_eq!(t.total, Duration::from_secs(960));
    }

    #[test]
    fn finished_timer_restarts_from_its_total() {
        let now = Instant::now();
        let mut t = Countdown::start("Timer", Duration::from_secs(30), now);
        assert!(t.tick(now + Duration::from_secs(30)));
        t.resume(now + Duration::from_secs(40));
        assert!(t.running());
        assert!(!t.finished);
        assert_eq!(
            t.remaining(now + Duration::from_secs(40)),
            Duration::from_secs(30)
        );
    }
}
