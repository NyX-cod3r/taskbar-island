//! Pure call-domain state machine fed by the call sources (spec §3).
//!
//! Lifecycle: `None -> Incoming -> Active -> Ended` and
//! `None -> Dialing -> Active -> Ended`. `Ended` returns to `None` after
//! [`ENDED_CLEAR`]. The Active timer starts when Active begins.
//!
//! Sources never persist caller identity: the state lives only in memory
//! until the call leaves the visible lifecycle. The machine is pure — every
//! transition takes the caller's `now`, so tests drive time explicitly.

use std::time::{Duration, Instant};

/// How long an Ended call stays visible before the machine returns to None.
pub const ENDED_CLEAR: Duration = Duration::from_secs(3);

/// A call is the same call when the caller text and the source that reported
/// it match again within this window (spec §3, identity rule).
pub const SAME_CALL_WINDOW: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPhase {
    None,
    Incoming,
    Dialing,
    Active,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallIdentity {
    pub name: Option<String>,
    pub number: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEvent {
    /// A ringing call was observed (toast, notification).
    Incoming(CallIdentity),
    /// An outgoing call started (Phone Link dial UI).
    Dialing(CallIdentity),
    /// The call connected: Incoming/Dialing -> Active. Fired by a successful
    /// Answer action or by a source that sees the in-call UI.
    Answered,
    /// The call is over: any live phase -> Ended.
    Ended,
    /// Ended -> None, scheduled [`ENDED_CLEAR`] after the end.
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallState {
    pub phase: CallPhase,
    pub identity: Option<CallIdentity>,
    /// Set when Active begins; the pill and card read the elapsed clock from
    /// it, so no source is polled for the duration.
    pub active_since: Option<Instant>,
    /// Set when the call ends; [`CallState::clear_due`] reads it.
    pub ended_at: Option<Instant>,
}

impl Default for CallState {
    fn default() -> Self {
        Self {
            phase: CallPhase::None,
            identity: None,
            active_since: None,
            ended_at: None,
        }
    }
}

impl CallState {
    pub fn apply(&mut self, event: CallEvent, now: Instant) {
        match event {
            // A fresh call supersedes a finished one; a second call while a
            // call is already visible cannot be taken (call waiting is out
            // of scope and faking it would invent a caller).
            CallEvent::Incoming(identity)
                if matches!(self.phase, CallPhase::None | CallPhase::Ended) =>
            {
                self.phase = CallPhase::Incoming;
                self.identity = Some(identity);
                self.active_since = None;
                self.ended_at = None;
            }
            CallEvent::Dialing(identity)
                if matches!(self.phase, CallPhase::None | CallPhase::Ended) =>
            {
                self.phase = CallPhase::Dialing;
                self.identity = Some(identity);
                self.active_since = None;
                self.ended_at = None;
            }
            CallEvent::Answered
                if matches!(self.phase, CallPhase::Incoming | CallPhase::Dialing) =>
            {
                self.phase = CallPhase::Active;
                self.active_since = Some(now);
                self.ended_at = None;
            }
            CallEvent::Ended
                if matches!(
                    self.phase,
                    CallPhase::Incoming | CallPhase::Dialing | CallPhase::Active
                ) =>
            {
                self.phase = CallPhase::Ended;
                self.active_since = None;
                self.ended_at = Some(now);
            }
            CallEvent::Clear if self.phase == CallPhase::Ended => {
                self.phase = CallPhase::None;
                self.identity = None;
                self.active_since = None;
                self.ended_at = None;
            }
            _ => {}
        }
    }

    /// Whether the call occupies the pill: Ended is a3 s grace state, not a
    /// live call (spec §8: hide the pill when nothing is live).
    pub fn live(&self) -> bool {
        matches!(
            self.phase,
            CallPhase::Incoming | CallPhase::Dialing | CallPhase::Active
        )
    }

    /// Whether the machine is due for its one-shot Clear. The UI schedules a
    /// timer at Ended + [`ENDED_CLEAR`] and re-checks this on fire.
    pub fn clear_due(&self, now: Instant) -> bool {
        self.phase == CallPhase::Ended
            && self
                .ended_at
                .is_some_and(|at| now.duration_since(at) >= ENDED_CLEAR)
    }

    /// Seconds the call has been active; 0.0 unless Active.
    pub fn elapsed(&self, now: Instant) -> f64 {
        self.active_since
            .map(|since| now.duration_since(since).as_secs_f64())
            .unwrap_or(0.0)
    }

    /// The caller label for pills and cards: the name when the source read
    /// one, otherwise None so the UI can pick its own wording.
    pub fn caller(&self) -> Option<&str> {
        self.identity.as_ref()?.name.as_deref()
    }
}

/// Redact numbers before any diagnostic output. The display UI may use the
/// unmodified value while the call is active; logs never may.
pub fn mask_number(number: &str) -> String {
    let digits = number.chars().filter(|c| c.is_ascii_digit()).count();
    let keep = 3usize.min(digits);
    let mut seen = 0usize;
    number
        .chars()
        .map(|ch| {
            if ch.is_ascii_digit() {
                seen += 1;
                if seen + keep > digits {
                    ch
                } else {
                    '•'
                }
            } else {
                ch
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caller(name: &str) -> CallIdentity {
        CallIdentity {
            name: Some(name.into()),
            number: None,
        }
    }

    fn base() -> (CallState, Instant) {
        (CallState::default(), Instant::now())
    }

    #[test]
    fn incoming_lifecycle_reaches_active_and_clears_after_three_seconds() {
        let (mut state, now) = base();
        state.apply(CallEvent::Incoming(caller("Ada")), now);
        assert_eq!(state.phase, CallPhase::Incoming);
        assert!(state.live());
        state.apply(CallEvent::Answered, now + Duration::from_secs(2));
        assert_eq!(state.phase, CallPhase::Active);
        let connected = now + Duration::from_secs(2);
        assert!((state.elapsed(connected + Duration::from_secs(4)) - 4.0).abs() < 0.01);
        state.apply(CallEvent::Ended, connected + Duration::from_secs(4));
        assert_eq!(state.phase, CallPhase::Ended);
        assert!(!state.live(), "Ended must not hold the pill");
        assert!(!state.clear_due(connected + Duration::from_secs(6)));
        assert!(state.clear_due(connected + Duration::from_secs(7)));
        state.apply(CallEvent::Clear, connected + Duration::from_secs(7));
        assert_eq!(state, CallState::default());
    }

    #[test]
    fn dialing_goes_to_active_then_ended() {
        let (mut state, now) = base();
        state.apply(CallEvent::Dialing(caller("Ada")), now);
        assert_eq!(state.phase, CallPhase::Dialing);
        state.apply(CallEvent::Answered, now + Duration::from_secs(1));
        assert_eq!(state.phase, CallPhase::Active);
        state.apply(CallEvent::Ended, now + Duration::from_secs(9));
        assert_eq!(state.phase, CallPhase::Ended);
        assert_eq!(state.caller(), Some("Ada"));
    }

    #[test]
    fn remote_hangup_wins_over_late_answer() {
        let (mut state, now) = base();
        state.apply(CallEvent::Incoming(caller("Ada")), now);
        state.apply(CallEvent::Ended, now);
        // A late Answered must not resurrect an ended call.
        state.apply(CallEvent::Answered, now + Duration::from_millis(500));
        assert_eq!(state.phase, CallPhase::Ended);
    }

    #[test]
    fn second_call_cannot_replace_a_live_call() {
        let (mut state, now) = base();
        state.apply(CallEvent::Incoming(caller("Ada")), now);
        state.apply(
            CallEvent::Incoming(caller("Mallory")),
            now + Duration::from_secs(1),
        );
        assert_eq!(state.caller(), Some("Ada"), "no invented call waiting");
    }

    #[test]
    fn a_new_call_supersedes_a_finished_one_before_the_clear() {
        let (mut state, now) = base();
        state.apply(CallEvent::Incoming(caller("Ada")), now);
        state.apply(CallEvent::Ended, now);
        state.apply(
            CallEvent::Incoming(caller("Bo")),
            now + Duration::from_secs(1),
        );
        assert_eq!(state.phase, CallPhase::Incoming);
        assert_eq!(state.caller(), Some("Bo"));
        assert!(state.ended_at.is_none(), "the old clear timer is dropped");
    }

    #[test]
    fn stray_events_are_ignored() {
        let (mut state, now) = base();
        state.apply(CallEvent::Answered, now);
        state.apply(CallEvent::Ended, now);
        state.apply(CallEvent::Clear, now);
        assert_eq!(state, CallState::default());
        // Ended without a call, Clear without Ended: no-ops.
        state.apply(CallEvent::Clear, now);
        assert_eq!(state.phase, CallPhase::None);
    }

    #[test]
    fn number_masking_keeps_only_the_last_three_digits() {
        assert_eq!(mask_number("+91 98765 43210"), "+•• ••••• ••210");
    }
}
