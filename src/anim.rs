//! anim.rs — real cubic-bezier motion and the on-demand frame scheduler helpers.

use std::time::{Duration, Instant};

/// Evaluate a CSS-style cubic-bezier for an x (time) in 0..1.
///
/// Solves x(t) = x with Newton-Raphson, falling back to bisection. The y value
/// is then read from the same parameter. No lookup tables or approximations.
pub fn cubic_bezier(curve: [f32; 4], x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let (x1, y1, x2, y2) = (curve[0], curve[1], curve[2], curve[3]);
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;

    let sample_x = |t: f32| ((ax * t + bx) * t + cx) * t;
    let sample_dx = |t: f32| (3.0 * ax * t + 2.0 * bx) * t + cx;

    let mut t = x;
    let mut solved = false;
    for _ in 0..8 {
        let error = sample_x(t) - x;
        if error.abs() < 1e-5 {
            solved = true;
            break;
        }
        let d = sample_dx(t);
        if d.abs() < 1e-6 {
            break;
        }
        t -= error / d;
        if !(0.0..=1.0).contains(&t) {
            break;
        }
    }
    if !solved {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            t = (lo + hi) * 0.5;
            if sample_x(t) < x {
                lo = t;
            } else {
                hi = t;
            }
        }
        t = (lo + hi) * 0.5;
    }
    ((ay * t + by) * t + cy) * t
}

/// A single tween between two scalar values.
#[derive(Debug, Clone, Copy)]
pub struct Tween {
    pub start: Instant,
    pub duration: Duration,
    pub from: f32,
    pub to: f32,
    pub curve: [f32; 4],
}

impl Tween {
    pub fn new(start: Instant, duration_ms: u32, from: f32, to: f32, curve: [f32; 4]) -> Self {
        Self {
            start,
            duration: Duration::from_millis(duration_ms as u64),
            from,
            to,
            curve,
        }
    }

    pub fn value(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32();
        let total = self.duration.as_secs_f32().max(0.0001);
        let t = (elapsed / total).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * cubic_bezier(self.curve, t)
    }

    pub fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design;

    #[test]
    fn bezier_hits_endpoints() {
        assert!((cubic_bezier(design::EASE_CONTENT, 0.0) - 0.0).abs() < 1e-4);
        assert!((cubic_bezier(design::EASE_CONTENT, 1.0) - 1.0).abs() < 1e-4);
        let midpoint = cubic_bezier(design::EASE_FLYOUT, 0.5);
        assert!(midpoint > 0.5 && midpoint < 1.0);
    }

    #[test]
    fn tween_reaches_its_target() {
        let now = Instant::now();
        let tween = Tween::new(now, 100, 0.0, 1.0, design::EASE_CONTENT);
        assert_eq!(tween.value(now), 0.0);
        assert_eq!(tween.value(now + Duration::from_millis(150)), 1.0);
        assert!(tween.done(now + Duration::from_millis(150)));
    }
}
