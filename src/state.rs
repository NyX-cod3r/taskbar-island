//! state.rs — activity ranking, main/second/pinned and the flyout state machine.
//!
//! Pure state. No drawing and no Win32 calls live here, so the ranking rules
//! can be tested without a window.

use std::time::Instant;

use crate::design::{self, ActivityKind};

/// Priority numbers: smaller wins. Only live (non-empty) activities compete.
pub fn rank(kind: ActivityKind, timer_finished: bool) -> u8 {
    match kind {
        ActivityKind::Call => 1,
        ActivityKind::Timer if timer_finished => 2,
        ActivityKind::Volume => 3,
        ActivityKind::Music => 4,
        ActivityKind::Timer => 5,
        ActivityKind::Shelf => 6,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Live {
    pub kind: ActivityKind,
    /// Only meaningful for Timer: in its last 60 s or finished.
    pub timer_finished: bool,
}

impl Live {
    pub fn new(kind: ActivityKind) -> Self {
        Self {
            kind,
            timer_finished: false,
        }
    }

    pub fn timer(finished: bool) -> Self {
        Self {
            kind: ActivityKind::Timer,
            timer_finished: finished,
        }
    }

    pub fn rank(&self) -> u8 {
        rank(self.kind, self.timer_finished)
    }
}

/// Ranking with the spec's pin rules.
///
/// Without a pin the first two ranked activities are main/second. Swapping
/// pins the new main until it or the second activity ends, or a call starts.
#[derive(Debug, Default, Clone)]
pub struct Ranker {
    pub main: Option<ActivityKind>,
    pub second: Option<ActivityKind>,
    pub pinned: Option<ActivityKind>,
}

impl Ranker {
    /// Recompute main/second from the currently live activities.
    pub fn update(&mut self, live: &[Live]) {
        let has = |kind| live.iter().any(|l| l.kind == kind);
        let call_live = has(ActivityKind::Call);

        if let Some(pinned) = self.pinned {
            if !has(pinned) || (call_live && pinned != ActivityKind::Call) {
                self.pinned = None;
            }
        }

        let mut ordered: Vec<Live> = live.to_vec();
        ordered.sort_by_key(|l| l.rank());

        if let Some(pinned) = self.pinned {
            if let Some(pos) = ordered.iter().position(|l| l.kind == pinned) {
                let main = ordered.remove(pos).kind;
                self.main = Some(main);
                self.second = ordered.first().map(|l| l.kind);
                return;
            }
            self.pinned = None;
        }

        self.main = ordered.first().map(|l| l.kind);
        self.second = ordered.get(1).map(|l| l.kind);
    }

    /// Bubble/chip tap: swap the two and pin the new main.
    pub fn swap(&mut self) {
        if let Some(second) = self.second {
            std::mem::swap(&mut self.main, &mut self.second);
            self.pinned = self.main;
            debug_assert_eq!(self.main, Some(second));
        }
    }

    pub fn main_is(&self, kind: ActivityKind) -> bool {
        self.main == Some(kind)
    }

    pub fn second_is(&self, kind: ActivityKind) -> bool {
        self.second == Some(kind)
    }
}

// --------------------------------------------------------------- flyout

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Closed,
    Opening { start: Instant },
    Open,
    Closing { start: Instant },
}

#[derive(Debug, Clone)]
pub struct FlyoutState {
    pub phase: Phase,
    /// The pointer has been inside the island since this flyout opened.
    pub entered: bool,
    /// Start of an activity swap while open (top/body crossfade).
    pub content_swap: Option<Instant>,
}

impl Default for FlyoutState {
    fn default() -> Self {
        Self {
            phase: Phase::Closed,
            entered: false,
            content_swap: None,
        }
    }
}

impl FlyoutState {
    pub fn is_open(&self) -> bool {
        matches!(self.phase, Phase::Opening { .. } | Phase::Open)
    }

    pub fn is_visible(&self, now: Instant, reduced_motion: bool) -> bool {
        match self.phase {
            Phase::Closed => false,
            Phase::Open => true,
            Phase::Opening { start } => {
                reduced_motion
                    || now.saturating_duration_since(start).as_millis() < design::OPEN_MS as u128
            }
            Phase::Closing { start } => {
                !reduced_motion
                    && now.saturating_duration_since(start).as_millis() < design::CLOSE_MS as u128
            }
        }
    }

    pub fn open(&mut self, now: Instant) {
        self.phase = Phase::Opening { start: now };
        self.entered = false;
        self.content_swap = None;
    }

    /// Opens without motion (reduced motion / snapshots).
    pub fn open_instant(&mut self) {
        self.phase = Phase::Open;
        self.entered = false;
        self.content_swap = None;
    }

    pub fn close(&mut self, now: Instant) {
        if matches!(self.phase, Phase::Closed | Phase::Closing { .. }) {
            return;
        }
        self.phase = Phase::Closing { start: now };
        self.entered = false;
    }

    pub fn close_instant(&mut self) {
        self.phase = Phase::Closed;
        self.entered = false;
        self.content_swap = None;
    }

    pub fn content_swap(&mut self, now: Instant) {
        self.content_swap = Some(now);
    }

    /// Called from the frame tick. Returns true when the state changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.phase {
            Phase::Opening { start } => {
                if now.saturating_duration_since(start).as_millis() >= design::OPEN_MS as u128 {
                    self.phase = Phase::Open;
                    true
                } else {
                    false
                }
            }
            Phase::Closing { start } => {
                if now.saturating_duration_since(start).as_millis() >= design::CLOSE_MS as u128 {
                    self.phase = Phase::Closed;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(kinds: &[ActivityKind]) -> Vec<Live> {
        kinds.iter().copied().map(Live::new).collect()
    }

    #[test]
    fn call_beats_everything() {
        let mut r = Ranker::default();
        r.update(&live(&[
            ActivityKind::Music,
            ActivityKind::Call,
            ActivityKind::Shelf,
        ]));
        assert_eq!(r.main, Some(ActivityKind::Call));
        assert_eq!(r.second, Some(ActivityKind::Music));
    }

    #[test]
    fn timer_finishing_beats_music_but_not_call() {
        let mut r = Ranker::default();
        r.update(&[
            Live::new(ActivityKind::Music),
            Live::timer(true),
            Live::new(ActivityKind::Shelf),
        ]);
        assert_eq!(r.main, Some(ActivityKind::Timer));
        assert_eq!(r.second, Some(ActivityKind::Music));

        r.update(&[
            Live::new(ActivityKind::Music),
            Live::new(ActivityKind::Call),
        ]);
        assert_eq!(r.main, Some(ActivityKind::Call));
    }

    #[test]
    fn volume_hud_beats_music_and_running_timer() {
        let mut r = Ranker::default();
        r.update(&live(&[
            ActivityKind::Music,
            ActivityKind::Timer,
            ActivityKind::Volume,
        ]));
        assert_eq!(r.main, Some(ActivityKind::Volume));
        assert_eq!(r.second, Some(ActivityKind::Music));
    }

    #[test]
    fn running_timer_sits_below_music_and_above_shelf() {
        let mut r = Ranker::default();
        r.update(&live(&[
            ActivityKind::Shelf,
            ActivityKind::Timer,
            ActivityKind::Music,
        ]));
        assert_eq!(r.main, Some(ActivityKind::Music));
        assert_eq!(r.second, Some(ActivityKind::Timer));
    }

    #[test]
    fn third_activity_waits_silently() {
        let mut r = Ranker::default();
        r.update(&live(&[
            ActivityKind::Call,
            ActivityKind::Music,
            ActivityKind::Shelf,
        ]));
        assert_eq!(r.main, Some(ActivityKind::Call));
        assert_eq!(r.second, Some(ActivityKind::Music));
        assert!(!r.second_is(ActivityKind::Shelf));
    }

    #[test]
    fn swap_pins_until_one_ends() {
        let mut r = Ranker::default();
        r.update(&live(&[ActivityKind::Call, ActivityKind::Music]));
        r.swap();
        assert_eq!(r.main, Some(ActivityKind::Music));
        assert_eq!(r.second, Some(ActivityKind::Call));
        assert_eq!(r.pinned, Some(ActivityKind::Music));

        // A live call always takes priority over a pinned non-call activity.
        r.update(&live(&[
            ActivityKind::Call,
            ActivityKind::Music,
            ActivityKind::Shelf,
        ]));
        assert_eq!(r.main, Some(ActivityKind::Call));
        assert_eq!(r.pinned, None);

        // A call always wins and clears the pin.
        r.update(&[Live::new(ActivityKind::Call)]);
        assert_eq!(r.main, Some(ActivityKind::Call));
        assert_eq!(r.pinned, None);
    }

    #[test]
    fn pinned_main_ending_clears_the_pin() {
        let mut r = Ranker::default();
        r.update(&live(&[ActivityKind::Music, ActivityKind::Shelf]));
        r.swap();
        assert_eq!(r.main, Some(ActivityKind::Shelf));
        r.update(&live(&[ActivityKind::Music]));
        assert_eq!(r.main, Some(ActivityKind::Music));
        assert_eq!(r.pinned, None);
    }

    #[test]
    fn flyout_morph_finishes_at_the_spec_duration() {
        let now = Instant::now();
        let mut f = FlyoutState::default();
        f.open(now);
        assert!(f.is_open());
        assert!(!f.tick(now + std::time::Duration::from_millis(100)));
        assert!(f.tick(now + std::time::Duration::from_millis(460)));
        assert_eq!(f.phase, Phase::Open);
        f.close(now + std::time::Duration::from_millis(500));
        assert!(f.tick(now + std::time::Duration::from_millis(760)));
        assert_eq!(f.phase, Phase::Closed);
    }
}
