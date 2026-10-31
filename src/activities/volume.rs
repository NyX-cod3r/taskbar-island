//! Volume HUD activity: transient HUD when volume changes.
//! Never expands. Dismisses after 1.8 s.

use std::time::Instant;
use crate::state::{Activity, ActivityId, Priority};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeActivity {
    pub active: bool,
    pub level: u32, // 0..100
    pub last_active: Option<Instant>,
}

impl Default for VolumeActivity {
    fn default() -> Self {
        Self {
            active: false,
            level: 42,
            last_active: None,
        }
    }
}

impl VolumeActivity {
    pub fn trigger(&mut self, level: u32) {
        self.active = true;
        self.level = level.min(100);
        self.last_active = Some(Instant::now());
    }

    pub fn dismiss(&mut self) {
        self.active = false;
        self.last_active = None;
    }

    #[allow(dead_code)]
    pub fn check_dismiss(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_active {
            if now.duration_since(last).as_millis() >= 1800 {
                self.dismiss();
                return true;
            }
        }
        false
    }
}

impl Activity for VolumeActivity {
    fn id(&self) -> ActivityId {
        ActivityId::Volume
    }
    fn priority(&self) -> Priority {
        Priority::VolumeHud
    }
    fn active(&self) -> bool {
        self.active
    }
}
