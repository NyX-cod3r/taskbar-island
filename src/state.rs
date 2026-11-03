//! Event-driven activity arbitration for the taskbar island.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Idle,
    Shelf,
    Media,
    VolumeHud,
    TimerEnding,
    Call,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivityId {
    Idle,
    Call,
    Timer,
    Volume,
    Media,
    Shelf,
}

/// A visible activity is completely event driven: `active` changes only when
/// its platform subscription or explicit user action posts an update.
pub trait Activity {
    fn id(&self) -> ActivityId;
    fn priority(&self) -> Priority;
    fn active(&self) -> bool;
}

#[derive(Default)]
pub struct ActivityQueue {
    entries: Vec<(ActivityId, Priority, bool)>,
}

impl ActivityQueue {
    pub fn update(&mut self, id: ActivityId, priority: Priority, active: bool) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.0 == id) {
            *entry = (id, priority, active);
        } else {
            self.entries.push((id, priority, active));
        }
    }

    pub fn current(&self) -> Option<ActivityId> {
        self.entries
            .iter()
            .filter(|(_, _, active)| *active)
            .max_by_key(|(_, priority, _)| *priority)
            .map(|(id, _, _)| *id)
    }
}

/// Resolves the manual switcher for one refresh.
///
/// A pin survives while the automatic winner is unchanged. It clears when a
/// new event changes the winner to a different activity (an incoming call, a
/// timer finishing, …) or when the pinned activity itself ends. Returns
/// `(pin, shown)`.
pub fn resolve_pin(
    pin: Option<ActivityId>,
    auto: ActivityId,
    last_auto: ActivityId,
) -> (Option<ActivityId>, ActivityId) {
    let pin = match pin {
        Some(pinned) if auto != last_auto && auto != pinned => None,
        other => other,
    };
    (pin, pin.unwrap_or(auto))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_priority_order_wins() {
        // Spec rule 4: call, then timer finishing, then volume HUD, then music, then shelf.
        let mut queue = ActivityQueue::default();
        queue.update(ActivityId::Shelf, Priority::Shelf, true);
        assert_eq!(queue.current(), Some(ActivityId::Shelf));

        queue.update(ActivityId::Media, Priority::Media, true);
        assert_eq!(queue.current(), Some(ActivityId::Media));

        queue.update(ActivityId::Volume, Priority::VolumeHud, true);
        assert_eq!(queue.current(), Some(ActivityId::Volume));

        queue.update(ActivityId::Timer, Priority::TimerEnding, true);
        assert_eq!(queue.current(), Some(ActivityId::Timer));

        queue.update(ActivityId::Call, Priority::Call, true);
        assert_eq!(queue.current(), Some(ActivityId::Call));

        // When Call deactivates, TimerEnding takes over
        queue.update(ActivityId::Call, Priority::Call, false);
        assert_eq!(queue.current(), Some(ActivityId::Timer));

        // When TimerEnding deactivates, VolumeHud takes over
        queue.update(ActivityId::Timer, Priority::TimerEnding, false);
        assert_eq!(queue.current(), Some(ActivityId::Volume));

        // When VolumeHud deactivates, Media takes over
        queue.update(ActivityId::Volume, Priority::VolumeHud, false);
        assert_eq!(queue.current(), Some(ActivityId::Media));

        // When Media deactivates, Shelf takes over
        queue.update(ActivityId::Media, Priority::Media, false);
        assert_eq!(queue.current(), Some(ActivityId::Shelf));
    }

    #[test]
    fn inactive_activity_is_not_selected() {
        let mut queue = ActivityQueue::default();
        queue.update(ActivityId::Call, Priority::Call, false);
        queue.update(ActivityId::Media, Priority::Media, true);
        assert_eq!(queue.current(), Some(ActivityId::Media));
    }

    #[test]
    fn pin_holds_while_auto_is_unchanged() {
        // Pinned to the shelf while nothing is active: stays pinned.
        let (pin, shown) = resolve_pin(Some(ActivityId::Shelf), ActivityId::Idle, ActivityId::Idle);
        assert_eq!(pin, Some(ActivityId::Shelf));
        assert_eq!(shown, ActivityId::Shelf);
    }

    #[test]
    fn pin_survives_its_own_activity_starting() {
        // Browsing the shelf, then a file is dropped: keep the shelf.
        let (pin, shown) = resolve_pin(Some(ActivityId::Shelf), ActivityId::Shelf, ActivityId::Idle);
        assert_eq!(pin, Some(ActivityId::Shelf));
        assert_eq!(shown, ActivityId::Shelf);
    }

    #[test]
    fn new_event_takes_over_from_pin() {
        // Browsing the shelf, then a call arrives: the call wins.
        let (pin, shown) = resolve_pin(Some(ActivityId::Shelf), ActivityId::Call, ActivityId::Idle);
        assert_eq!(pin, None);
        assert_eq!(shown, ActivityId::Call);
    }

    #[test]
    fn pin_clears_when_pinned_activity_ends() {
        // Pinned to music, then music stops: fall back to automatic idle.
        let (pin, shown) = resolve_pin(Some(ActivityId::Media), ActivityId::Idle, ActivityId::Media);
        assert_eq!(pin, None);
        assert_eq!(shown, ActivityId::Idle);
    }

    #[test]
    fn auto_mode_ignores_last_auto() {
        let (pin, shown) = resolve_pin(None, ActivityId::Media, ActivityId::Call);
        assert_eq!(pin, None);
        assert_eq!(shown, ActivityId::Media);
    }
}
