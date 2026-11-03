//! anim.rs — real cubic-bezier motion and the on-demand frame scheduler helpers.

use std::time::{Duration, Instant};

use crate::design::{self, RectD};

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

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
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

/// Progress of a delayed, eased segment in 0..1.
pub fn delayed(
    now: Instant,
    start: Instant,
    delay_ms: u32,
    duration_ms: u32,
    curve: [f32; 4],
) -> f32 {
    let elapsed = now.saturating_duration_since(start).as_millis() as u32;
    if elapsed < delay_ms {
        return 0.0;
    }
    let t = ((elapsed - delay_ms) as f32 / duration_ms.max(1) as f32).clamp(0.0, 1.0);
    cubic_bezier(curve, t)
}

/// Rect morph with a curve and duration.
#[derive(Debug, Clone, Copy)]
pub struct Morph {
    pub start: Instant,
    pub from: RectD,
    pub to: RectD,
    pub duration: Duration,
    pub curve: [f32; 4],
}

impl Morph {
    pub fn new(start: Instant, from: RectD, to: RectD, duration_ms: u32, curve: [f32; 4]) -> Self {
        Self {
            start,
            from,
            to,
            duration: Duration::from_millis(duration_ms as u64),
            curve,
        }
    }

    pub fn at(&self, now: Instant) -> RectD {
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32();
        let total = self.duration.as_secs_f32().max(0.0001);
        let t = cubic_bezier(self.curve, (elapsed / total).clamp(0.0, 1.0));
        RectD::lerp(self.from, self.to, t)
    }

    pub fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= self.duration
    }
}

/// The wavy progress state: phase, amplitude easing and pause flattening.
#[derive(Debug, Clone)]
pub struct Wave {
    pub phase: f32,
    amp: f32,
    amp_from: f32,
    amp_to: f32,
    amp_start: Instant,
    playing: bool,
    last: Instant,
}

impl Wave {
    pub fn new(now: Instant) -> Self {
        Self {
            phase: 0.0,
            amp: design::WAVE_AMP,
            amp_from: design::WAVE_AMP,
            amp_to: design::WAVE_AMP,
            amp_start: now,
            playing: true,
            last: now,
        }
    }

    pub fn set_playing(&mut self, playing: bool, now: Instant) {
        if self.playing == playing {
            return;
        }
        self.playing = playing;
        self.amp_from = self.amp;
        self.amp_to = if playing {
            design::WAVE_AMP
        } else {
            design::WAVE_AMP * design::WAVE_FLAT
        };
        self.amp_start = now;
    }

    pub fn tick(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.last).as_secs_f32();
        self.last = now;
        if self.playing {
            self.phase = (self.phase + design::WAVE_SPEED * dt) % design::WAVE_LEN;
        }
        let elapsed = now.saturating_duration_since(self.amp_start).as_secs_f32();
        let t = (elapsed / (design::WAVE_EASE_MS as f32 / 1000.0)).clamp(0.0, 1.0);
        let e = ease_out(t);
        self.amp = self.amp_from + (self.amp_to - self.amp_from) * e;
    }

    pub fn amp(&self) -> f32 {
        self.amp
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn animating(&self, now: Instant) -> bool {
        (now.saturating_duration_since(self.amp_start).as_millis() as u32) < design::WAVE_EASE_MS
    }

    pub fn set_phase(&mut self, phase: f32) {
        self.phase = phase.rem_euclid(design::WAVE_LEN);
    }
}

/// A linear value ramp used for progress x (1 s per second of media time).
#[derive(Debug, Clone, Copy)]
pub struct Ramp {
    pub start: Instant,
    pub from: f32,
    pub to: f32,
    pub duration_ms: u32,
}

impl Ramp {
    pub fn new(start: Instant, from: f32, to: f32, duration_ms: u32) -> Self {
        Self {
            start,
            from,
            to,
            duration_ms,
        }
    }

    pub fn value(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.start).as_millis() as f32;
        let t = (elapsed / self.duration_ms.max(1) as f32).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * t
    }

    pub fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start).as_millis() as u32 >= self.duration_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_hits_endpoints() {
        assert!((cubic_bezier(design::EASE_OPEN, 0.0) - 0.0).abs() < 1e-4);
        assert!((cubic_bezier(design::EASE_OPEN, 1.0) - 1.0).abs() < 1e-4);
        let midpoint = cubic_bezier(design::EASE_CLOSE, 0.5);
        assert!(midpoint > 0.7 && midpoint < 0.85);
    }

    #[test]
    fn overshoot_curve_exceeds_one() {
        assert!(cubic_bezier(design::EASE_ICON, 0.62) > 1.0);
    }

    #[test]
    fn delayed_segment_waits() {
        let now = Instant::now();
        assert_eq!(delayed(now, now, 150, 280, design::EASE_CONTENT), 0.0);
        assert_eq!(
            delayed(
                now + Duration::from_millis(1000),
                now,
                150,
                280,
                design::EASE_CONTENT
            ),
            1.0
        );
    }

    #[test]
    fn morph_returns_end_rects() {
        let now = Instant::now();
        let a = RectD::new(0.0, 0.0, 10.0, 10.0);
        let b = RectD::new(100.0, 20.0, 200.0, 100.0);
        let m = Morph::new(now, a, b, 460, design::EASE_OPEN);
        assert_eq!(m.at(now), a);
        assert_eq!(m.at(now + Duration::from_millis(600)), b);
    }
}
