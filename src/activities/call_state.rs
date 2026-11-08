//! Pure call-domain state used by Phone Link and the debug simulator.
//!
//! Providers never persist caller identity. The state lives only in memory
//! until the call leaves the visible lifecycle.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPhase {
    Idle,
    Ringing,
    Active,
    Ended,
    Missed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallIdentity {
    pub name: Option<String>,
    pub number: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEvent {
    Incoming(CallIdentity),
    Answered,
    RemoteEnded,
    Missed,
    Cleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallAction {
    Answer,
    Decline,
    End,
    Dismiss,
}

/// Provider boundary: the UI state machine remains testable without Windows
/// or a linked phone. A provider must report its actual capability rather
/// than claiming that a control action succeeded.
pub trait CallProvider {
    fn poll_event(&mut self) -> Option<CallEvent>;
    fn perform(&mut self, action: CallAction) -> Result<(), CallProviderError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallProviderError {
    Unsupported,
    Unavailable,
    Failed,
}

/// The shipping Phone Link boundary until the spike has a validated detection
/// and control transport. It deliberately reports capability absence instead
/// of inventing a call or treating a button click as successful.
#[derive(Debug, Default)]
pub struct PhoneLinkProvider;

impl CallProvider for PhoneLinkProvider {
    fn poll_event(&mut self) -> Option<CallEvent> {
        None
    }

    fn perform(&mut self, _action: CallAction) -> Result<(), CallProviderError> {
        Err(CallProviderError::Unsupported)
    }
}

/// Deterministic provider used by state and UI tests without a linked phone.
#[derive(Debug)]
pub struct SimulatedProvider {
    events: VecDeque<CallEvent>,
    pub next_action_result: Result<(), CallProviderError>,
}

impl Default for SimulatedProvider {
    fn default() -> Self {
        Self {
            events: VecDeque::new(),
            next_action_result: Ok(()),
        }
    }
}

impl SimulatedProvider {
    pub fn with_events(events: impl IntoIterator<Item = CallEvent>) -> Self {
        Self {
            events: events.into_iter().collect(),
            next_action_result: Ok(()),
        }
    }
}

impl CallProvider for SimulatedProvider {
    fn poll_event(&mut self) -> Option<CallEvent> {
        self.events.pop_front()
    }

    fn perform(&mut self, _action: CallAction) -> Result<(), CallProviderError> {
        self.next_action_result.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallState {
    pub phase: CallPhase,
    pub identity: Option<CallIdentity>,
}

impl Default for CallState {
    fn default() -> Self {
        Self {
            phase: CallPhase::Idle,
            identity: None,
        }
    }
}

impl CallState {
    pub fn apply_event(&mut self, event: CallEvent) {
        match event {
            CallEvent::Incoming(identity) if self.phase == CallPhase::Idle => {
                self.phase = CallPhase::Ringing;
                self.identity = Some(identity);
            }
            CallEvent::Answered if self.phase == CallPhase::Ringing => {
                self.phase = CallPhase::Active
            }
            CallEvent::RemoteEnded
                if matches!(self.phase, CallPhase::Ringing | CallPhase::Active) =>
            {
                self.phase = CallPhase::Ended;
            }
            CallEvent::Missed if self.phase == CallPhase::Ringing => self.phase = CallPhase::Missed,
            CallEvent::Cleared if matches!(self.phase, CallPhase::Ended | CallPhase::Missed) => {
                self.phase = CallPhase::Idle;
                self.identity = None;
            }
            _ => {}
        }
    }

    /// Update UI state only after the provider reports that a requested action
    /// was delivered. This prevents a non-functional Phone Link action from
    /// looking successful.
    pub fn apply_action_result(
        &mut self,
        action: CallAction,
        result: Result<(), CallProviderError>,
    ) {
        if result.is_err() {
            return;
        }
        match (self.phase, action) {
            (CallPhase::Ringing, CallAction::Answer) => self.phase = CallPhase::Active,
            (CallPhase::Ringing, CallAction::Decline)
            | (CallPhase::Ringing, CallAction::Dismiss) => {
                self.phase = CallPhase::Ended;
            }
            (CallPhase::Active, CallAction::End) => self.phase = CallPhase::Ended,
            _ => {}
        }
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

    fn incoming() -> CallEvent {
        CallEvent::Incoming(CallIdentity {
            name: Some("Ada".into()),
            number: Some("+91 98765 43210".into()),
        })
    }

    #[test]
    fn lifecycle_and_duplicate_events_are_safe() {
        let mut state = CallState::default();
        state.apply_event(incoming());
        state.apply_event(incoming());
        assert_eq!(state.phase, CallPhase::Ringing);
        state.apply_event(CallEvent::Answered);
        state.apply_event(CallEvent::Answered);
        assert_eq!(state.phase, CallPhase::Active);
        state.apply_event(CallEvent::RemoteEnded);
        assert_eq!(state.phase, CallPhase::Ended);
        state.apply_event(CallEvent::Cleared);
        assert_eq!(state, CallState::default());
    }

    #[test]
    fn remote_hangup_wins_over_late_answer() {
        let mut state = CallState::default();
        state.apply_event(incoming());
        state.apply_event(CallEvent::RemoteEnded);
        state.apply_action_result(CallAction::Answer, Ok(()));
        assert_eq!(state.phase, CallPhase::Ended);
    }

    #[test]
    fn missed_and_failed_provider_action_do_not_fake_success() {
        let mut state = CallState::default();
        state.apply_event(incoming());
        state.apply_action_result(CallAction::Decline, Err(CallProviderError::Unsupported));
        assert_eq!(state.phase, CallPhase::Ringing);
        state.apply_event(CallEvent::Missed);
        assert_eq!(state.phase, CallPhase::Missed);
    }

    #[test]
    fn number_masking_keeps_only_the_last_three_digits() {
        assert_eq!(mask_number("+91 98765 43210"), "+•• ••••• ••210");
    }

    #[test]
    fn providers_are_explicit_about_real_and_simulated_capabilities() {
        let mut phone_link = PhoneLinkProvider;
        assert_eq!(
            phone_link.perform(CallAction::Answer),
            Err(CallProviderError::Unsupported)
        );
        let mut simulated = SimulatedProvider::with_events([incoming(), CallEvent::Answered]);
        assert!(matches!(
            simulated.poll_event(),
            Some(CallEvent::Incoming(_))
        ));
        assert_eq!(simulated.perform(CallAction::Answer), Ok(()));
    }
}
