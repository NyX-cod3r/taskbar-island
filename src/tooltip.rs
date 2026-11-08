//! Shared, non-activating tooltip policy.
//!
//! Rendering is platform-specific; this module owns the timing and placement
//! rules so every icon control behaves consistently, including controls in a
//! clipped scrolling list.

use std::time::{Duration, Instant};

use crate::design::RectD;

pub const INITIAL_DELAY: Duration = Duration::from_millis(450);
pub const REPEAT_WINDOW: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub rect: RectD,
    pub side: Side,
}

#[derive(Debug, Default)]
pub struct TooltipState {
    hovered: Option<(u64, Instant)>,
    last_hidden: Option<Instant>,
}

impl TooltipState {
    pub fn update(&mut self, now: Instant, control: Option<u64>) -> bool {
        match (self.hovered, control) {
            (Some((id, _)), Some(next)) if id == next => self.is_due(now),
            (_, Some(next)) => {
                self.hovered = Some((next, now));
                self.last_hidden
                    .map(|last| now.saturating_duration_since(last) <= REPEAT_WINDOW)
                    .unwrap_or(false)
            }
            (Some(_), None) => {
                self.hovered = None;
                self.last_hidden = Some(now);
                false
            }
            (None, None) => false,
        }
    }

    fn is_due(&self, now: Instant) -> bool {
        self.hovered
            .map(|(_, since)| now.saturating_duration_since(since) >= INITIAL_DELAY)
            .unwrap_or(false)
    }
}

/// Keep the popup inside the window viewport, rather than a scrolling
/// container's clip. Prefer right-of-control; use the required left placement
/// if the right side would clip.
pub fn place(anchor: RectD, popup_w: f32, popup_h: f32, viewport: RectD) -> Placement {
    let right_x = anchor.right() + 6.0;
    let use_left = right_x + popup_w > viewport.right();
    let x = if use_left {
        (anchor.x - 6.0 - popup_w).max(viewport.x)
    } else {
        right_x.max(viewport.x)
    };
    let y = (anchor.center_y() - popup_h / 2.0)
        .clamp(viewport.y, (viewport.bottom() - popup_h).max(viewport.y));
    Placement {
        rect: RectD::new(x, y, popup_w.min(viewport.w), popup_h.min(viewport.h)),
        side: if use_left { Side::Left } else { Side::Right },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_and_repeat_hover_timing_follow_the_design_tokens() {
        let start = Instant::now();
        let mut state = TooltipState::default();
        assert!(!state.update(start, Some(1)));
        assert!(!state.update(start + Duration::from_millis(449), Some(1)));
        assert!(state.update(start + INITIAL_DELAY, Some(1)));
        assert!(!state.update(start + INITIAL_DELAY, None));
        assert!(state.update(start + INITIAL_DELAY + Duration::from_millis(299), Some(2)));
        assert!(!state.update(start + INITIAL_DELAY + Duration::from_millis(301), None));
        assert!(!state.update(start + INITIAL_DELAY + Duration::from_millis(602), Some(3)));
    }

    #[test]
    fn placement_switches_left_and_ignores_scroll_clip() {
        let viewport = RectD::new(0.0, 0.0, 400.0, 192.0);
        let right = place(RectD::new(20.0, 80.0, 28.0, 28.0), 120.0, 32.0, viewport);
        assert_eq!(right.side, Side::Right);
        let left = place(RectD::new(365.0, 80.0, 28.0, 28.0), 120.0, 32.0, viewport);
        assert_eq!(left.side, Side::Left);
        assert!(left.rect.x >= viewport.x && left.rect.right() <= viewport.right());
    }
}
