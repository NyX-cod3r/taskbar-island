//! Countdown-timer state. Scheduling is owned by the UI's WM_TIMER (a
//! 1-second tick while a countdown runs — never faster, and killed on end or
//! cancel); this module never spins or polls on its own.

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
    label: String,
}

impl Default for TimerActivity {
    fn default() -> Self {
        Self {
            mode: TimerMode::Stopwatch,
            started: None,
            accumulated: Duration::ZERO,
            countdown: Duration::ZERO,
            ended: false,
            label: "Study session".into(),
        }
    }
}

impl TimerActivity {
    #[allow(dead_code)]
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

    pub fn add_seconds(&mut self, secs: u64) {
        self.countdown += Duration::from_secs(secs);
    }

    pub fn toggle_pause(&mut self, now: Instant) {
        if self.started.is_some() {
            self.stop(now);
        } else if self.mode == TimerMode::Countdown && self.countdown > self.accumulated {
            self.started = Some(now);
        }
    }

    pub fn stop(&mut self, now: Instant) {
        self.accumulated = self.elapsed(now);
        self.started = None;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
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
        self.ended = (self.started.is_some() || self.accumulated > Duration::ZERO) && remaining.is_zero();
        remaining
    }

    pub fn has_ended(&self) -> bool {
        self.ended
    }

    pub fn is_running(&self) -> bool {
        self.started.is_some()
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
            Priority::Idle
        }
    }
    fn active(&self) -> bool {
        self.started.is_some() || self.ended
    }
}

/// Display strings for the timer, rebuilt per tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerView {
    pub title: String,
    pub artist: String,
    pub ended: bool,
    pub label: String,
    pub running: bool,
    pub remaining_secs: u64,
    pub total_secs: u64,
}

impl TimerView {
    pub fn demo() -> Self {
        Self {
            title: "12:40".into(),
            artist: "Running".into(),
            ended: false,
            label: "Study session".into(),
            running: true,
            remaining_secs: 760,
            total_secs: 1500,
        }
    }
}

pub fn view(timer: &mut TimerActivity, now: Instant) -> Option<TimerView> {
    if !timer.active() {
        return None;
    }
    timer.remaining(now);
    if timer.has_ended() {
        return Some(TimerView {
            title: "Timer done".into(),
            artist: "click to dismiss".into(),
            ended: true,
            label: timer.label.clone(),
            running: false,
            remaining_secs: 0,
            total_secs: timer.countdown.as_secs(),
        });
    }
    let remaining = timer.remaining(now);
    let whole = remaining.as_secs() + u64::from(remaining.subsec_millis() > 0);
    let minutes = whole / 60;
    let seconds = whole % 60;
    Some(TimerView {
        title: format!("{minutes}:{seconds:02}"),
        artist: if timer.is_running() { "Running".into() } else { "Paused".into() },
        ended: false,
        label: timer.label.clone(),
        running: timer.is_running(),
        remaining_secs: whole,
        total_secs: timer.countdown.as_secs(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn countdown() -> (TimerActivity, Instant) {
        let now = Instant::now();
        let mut timer = TimerActivity::default();
        timer.start_countdown(Duration::from_secs(300), now);
        (timer, now)
    }

    #[test]
    fn countdown_ends_and_is_promoted() {
        let (mut timer, now) = countdown();
        assert_eq!(
            timer.remaining(now + Duration::from_secs(4)),
            Duration::from_secs(296)
        );
        assert_eq!(
            timer.remaining(now + Duration::from_secs(300)),
            Duration::ZERO
        );
        assert!(timer.has_ended());
        assert_eq!(timer.priority(), Priority::TimerEnding);
    }

    #[test]
    fn running_countdown_yields_to_media_priority() {
        let (timer, _now) = countdown();
        assert!(timer.is_running());
        assert!(timer.active());
        assert_eq!(timer.priority(), Priority::Idle);
    }

    #[test]
    fn view_formats_m_ss_and_rounds_up() {
        let (mut timer, now) = countdown();
        let view = view(&mut timer, now + Duration::from_millis(1000)).unwrap();
        assert_eq!(view.title, "4:59");
        assert_eq!(view.artist, "Running");
        assert!(!view.ended);
    }

    #[test]
    fn view_at_zero_shows_done_banner() {
        let (mut timer, now) = countdown();
        let view = view(&mut timer, now + Duration::from_secs(300)).unwrap();
        assert_eq!(view.title, "Timer done");
        assert_eq!(view.artist, "click to dismiss");
        assert!(view.ended);
    }

    #[test]
    fn view_is_none_when_reset() {
        let (mut timer, now) = countdown();
        timer.reset();
        assert!(view(&mut timer, now).is_none());
        assert!(!timer.active());
        assert_eq!(timer.priority(), Priority::Idle);
    }

    #[test]
    fn stopwatch_mode_has_no_countdown() {
        let now = Instant::now();
        let mut timer = TimerActivity::default();
        timer.start_stopwatch(now);
        assert_eq!(timer.remaining(now + Duration::from_secs(5)), Duration::ZERO);
        assert!(!timer.has_ended());
    }
}
