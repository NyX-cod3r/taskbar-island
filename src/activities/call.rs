//! Call activity: active phone/voice call (takes highest priority).

use crate::state::{Activity, ActivityId, Priority};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallState {
    pub name: String,
    pub duration_str: String,
    pub is_muted: bool,
    pub is_speaker: bool,
}

impl Default for CallState {
    fn default() -> Self {
        Self {
            name: "Priya".into(),
            duration_str: "02:14".into(),
            is_muted: false,
            is_speaker: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CallActivity {
    pub active: bool,
    pub state: CallState,
}

impl CallActivity {
    pub fn new_active() -> Self {
        Self {
            active: true,
            state: CallState::default(),
        }
    }

    pub fn end_call(&mut self) {
        self.active = false;
    }

    pub fn toggle_mute(&mut self) {
        self.state.is_muted = !self.state.is_muted;
    }

    pub fn toggle_speaker(&mut self) {
        self.state.is_speaker = !self.state.is_speaker;
    }
}

impl Activity for CallActivity {
    fn id(&self) -> ActivityId {
        ActivityId::Call
    }
    fn priority(&self) -> Priority {
        Priority::Call
    }
    fn active(&self) -> bool {
        self.active
    }
}
