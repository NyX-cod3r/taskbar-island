//! state.rs — activity ranking, main/second/pinned and the flyout state machine.
//!
//! Pure state. No drawing and no Win32 calls live here, so the ranking rules
//! can be tested without a window.

// The live surface resolves its content directly from events; these ranking
// rules remain the tested reference for the spec's priority order.
#![allow(dead_code)]

use crate::design::ActivityKind;

/// Pill content is selected independently of the expanded card so a call can
/// be surfaced without changing which card the user has open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillState {
    Idle,
    Music,
    ClipboardPop { added: u32 },
    TimerAlert,
    ActiveCall,
    IncomingCall,
}

pub fn pill_priority(states: &[PillState]) -> PillState {
    let rank = |state: PillState| match state {
        PillState::IncomingCall => 0,
        PillState::ActiveCall => 1,
        PillState::TimerAlert => 2,
        PillState::ClipboardPop { .. } => 3,
        PillState::Music => 4,
        PillState::Idle => 5,
    };
    states
        .iter()
        .copied()
        .min_by_key(|state| rank(*state))
        .unwrap_or(PillState::Idle)
}

/// Priority numbers: smaller wins. Only live (non-empty) activities compete.
pub fn rank(kind: ActivityKind, timer_finished: bool) -> u8 {
    match kind {
        ActivityKind::Call => 1,
        ActivityKind::Timer if timer_finished => 2,
        ActivityKind::Music => 4,
        ActivityKind::Timer => 5,
        ActivityKind::Shelf => 6,
        ActivityKind::Clipboard => 7,
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
    fn pill_priority_restores_the_next_live_state() {
        assert_eq!(
            pill_priority(&[PillState::Music, PillState::ClipboardPop { added: 3 }]),
            PillState::ClipboardPop { added: 3 }
        );
        assert_eq!(
            pill_priority(&[
                PillState::Music,
                PillState::ClipboardPop { added: 3 },
                PillState::IncomingCall,
            ]),
            PillState::IncomingCall
        );
        assert_eq!(pill_priority(&[PillState::Music]), PillState::Music);
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
}
