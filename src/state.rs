//! Event-driven activity arbitration for the taskbar island.
#![allow(dead_code)] // Activity variants are enabled as their platform modules land.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Idle,
    Media,
    /// Files dropped on the pill take it over until opened: a drop is a direct
    /// user action aimed at the pill, so it outranks the in-progress media.
    Shelf,
    VolumeHud,
    TimerEnding,
    Call,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityId {
    Media,
    Shelf,
    Call,
    Timer,
    Volume,
    Battery,
    Bluetooth,
    Privacy,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_priority_order_wins() {
        let mut queue = ActivityQueue::default();
        queue.update(ActivityId::Media, Priority::Media, true);
        queue.update(ActivityId::Shelf, Priority::Shelf, true);
        // A drop is a user action on the pill itself → the shelf takes over
        // from the in-progress media run.
        assert_eq!(queue.current(), Some(ActivityId::Shelf));

        queue.update(ActivityId::Shelf, Priority::Shelf, false);
        assert_eq!(queue.current(), Some(ActivityId::Media));

        queue.update(ActivityId::Volume, Priority::VolumeHud, true);
        assert_eq!(queue.current(), Some(ActivityId::Volume));

        queue.update(ActivityId::Timer, Priority::TimerEnding, true);
        assert_eq!(queue.current(), Some(ActivityId::Timer));

        queue.update(ActivityId::Call, Priority::Call, true);
        assert_eq!(queue.current(), Some(ActivityId::Call));
    }

    #[test]
    fn inactive_activity_is_not_selected() {
        let mut queue = ActivityQueue::default();
        queue.update(ActivityId::Call, Priority::Call, false);
        queue.update(ActivityId::Media, Priority::Media, true);
        assert_eq!(queue.current(), Some(ActivityId::Media));
    }
}
