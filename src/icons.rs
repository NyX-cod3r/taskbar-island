//! icons.rs — the SVG-path icons from section 9 of the spec.
//!
//! Paths are kept verbatim so they can be diffed against the document. A small
//! parser converts M/L/H/V/C/Q/A/Z (absolute and relative) into a Direct2D
//! geometry. Everything is device independent; callers scale the geometry.

use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING,
};
use windows::Win32::Graphics::Direct2D::{ID2D1Factory, ID2D1PathGeometry};
use windows_numerics::Vector2;

use crate::view::IconId;

pub struct IconSpec {
    pub path: &'static str,
    pub filled: bool,
    /// Stroke width in the 24 x 24 viewBox.
    pub stroke: f32,
}

pub fn spec(icon: IconId) -> IconSpec {
    match icon {
        IconId::Music => IconSpec {
            path: "M9 18V6l10-2v12M9 18a3 3 0 1 1-3-3 3 3 0 0 1 3 3zm10-2a3 3 0 1 1-3-3 3 3 0 0 1 3 3z",
            filled: false,
            stroke: 2.0,
        },
        IconId::Phone => IconSpec {
            path: "M5 4h4l2 5-2.5 1.5a11 11 0 0 0 5 5L15 13l5 2v4a2 2 0 0 1-2 2A16 16 0 0 1 3 6a2 2 0 0 1 2-2z",
            filled: false,
            stroke: 2.0,
        },
        IconId::Folder => IconSpec {
            path: "M3.5 7a2 2 0 0 1 2-2H10l2 2.5h6.5a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z",
            filled: false,
            stroke: 2.0,
        },
        IconId::Clock => IconSpec {
            path: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3 2",
            filled: false,
            stroke: 2.0,
        },
        IconId::Speaker => IconSpec {
            path: "M4 9.5v5h3.5l4.5 4v-13l-4.5 4zM16 9a4 4 0 0 1 0 6M18.5 6.5a8 8 0 0 1 0 11",
            filled: false,
            stroke: 2.0,
        },
        IconId::SpeakerMuted => IconSpec {
            path: "M4 9.5v5h3.5l4.5 4v-13l-4.5 4zM16 9a4 4 0 0 1 0 6M18.5 6.5a8 8 0 0 1 0 11M3 3l18 18",
            filled: false,
            stroke: 2.0,
        },
        IconId::DownArrow => IconSpec {
            path: "M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19.5h14",
            filled: false,
            stroke: 2.0,
        },
        IconId::Mic => IconSpec {
            path: "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-3 3 3 3 0 0 1-3-3V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3",
            filled: false,
            stroke: 2.0,
        },
        IconId::MicMuted => IconSpec {
            path: "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-3 3 3 3 0 0 1-3-3V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3M4 4l16 16",
            filled: false,
            stroke: 2.0,
        },
        IconId::Signal => IconSpec {
            path: "M4 20v-4M10 20V11M16 20V7M22 20V3",
            filled: false,
            stroke: 2.0,
        },
        IconId::Shuffle => IconSpec {
            path: "M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5",
            filled: false,
            stroke: 2.0,
        },
        IconId::Repeat => IconSpec {
            path: "M17 2l4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4M21 13v2a3 3 0 0 1-3 3H3",
            filled: false,
            stroke: 2.0,
        },
        IconId::Play => IconSpec {
            path: "M7.5 4.5v15l12-7.5z",
            filled: true,
            stroke: 0.0,
        },
        IconId::Pause => IconSpec {
            path: "M6.5 4.5h4v15h-4zM13.5 4.5h4v15h-4z",
            filled: true,
            stroke: 0.0,
        },
        IconId::Previous => IconSpec {
            path: "M6 5h2.2v14H6zM20 5.5v13L9.5 12z",
            filled: true,
            stroke: 0.0,
        },
        IconId::Next => IconSpec {
            path: "M15.8 5H18v14h-2.2zM4 5.5v13L14.5 12z",
            filled: true,
            stroke: 0.0,
        },
        IconId::Close => IconSpec {
            path: "M6 6l12 12M18 6L6 18",
            filled: false,
            stroke: 2.0,
        },
    }
}

// ------------------------------------------------------------------ parser

#[derive(Clone)]
enum Seg {
    Line(Vector2),
    Cubic(Vector2, Vector2, Vector2),
}

struct Subpath {
    start: Vector2,
    segs: Vec<Seg>,
    closed: bool,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            i: 0,
        }
    }

    fn skip_sep(&mut self) {
        while self.i < self.bytes.len() {
            let c = self.bytes[self.i];
            if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' || c == b',' {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip_sep();
        let c = *self.bytes.get(self.i)?;
        if c.is_ascii_alphabetic() {
            self.i += 1;
            Some(c)
        } else {
            None
        }
    }

    fn number(&mut self) -> Option<f32> {
        self.skip_sep();
        let start = self.i;
        if matches!(self.bytes.get(self.i), Some(b'+') | Some(b'-')) {
            self.i += 1;
        }
        let mut digits = false;
        while matches!(self.bytes.get(self.i), Some(b'0'..=b'9')) {
            self.i += 1;
            digits = true;
        }
        if self.bytes.get(self.i) == Some(&b'.') {
            self.i += 1;
            while matches!(self.bytes.get(self.i), Some(b'0'..=b'9')) {
                self.i += 1;
                digits = true;
            }
        }
        if !digits {
            self.i = start;
            return None;
        }
        if matches!(self.bytes.get(self.i), Some(b'e') | Some(b'E')) {
            self.i += 1;
            if matches!(self.bytes.get(self.i), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            while matches!(self.bytes.get(self.i), Some(b'0'..=b'9')) {
                self.i += 1;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.i])
            .ok()?
            .parse()
            .ok()
    }
}

#[derive(Clone, Copy)]
struct Builder {
    cur: Vector2,
    start: Vector2,
    cmd: u8,
}

fn parse(path: &str) -> Vec<Subpath> {
    let mut lex = Lexer::new(path);
    let mut out: Vec<Subpath> = Vec::new();
    let mut cur = Vector2::new(0.0, 0.0);
    let mut sub_start = cur;
    let mut open: Option<Subpath> = None;
    let mut cmd = b'M';

    while let Some(c) = lex.command() {
        cmd = c;
        loop {
            lex.skip_sep();
            if lex.command().is_some() {
                // Put the command back by remembering it; the outer loop
                // expects to consume it. The lexer advanced, so restore.
                lex.i -= 1;
                break;
            }
            let rel = cmd.is_ascii_lowercase();
            let base = if rel { cur } else { Vector2::new(0.0, 0.0) };
            match cmd.to_ascii_uppercase() {
                b'M' => {
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    if let Some(mut p) = open.take() {
                        p.closed = false;
                        out.push(p);
                    }
                    cur = Vector2::new(base.X + x, base.Y + y);
                    sub_start = cur;
                    open = Some(Subpath {
                        start: cur,
                        segs: Vec::new(),
                        closed: false,
                    });
                    // Additional pairs after M are implicit lines.
                    cmd = if rel { b'l' } else { b'L' };
                }
                b'L' => {
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    let to = Vector2::new(base.X + x, base.Y + y);
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Line(to));
                    }
                    cur = to;
                }
                b'H' => {
                    let Some(x) = lex.number() else { break };
                    let to = Vector2::new(if rel { cur.X + x } else { x }, cur.Y);
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Line(to));
                    }
                    cur = to;
                }
                b'V' => {
                    let Some(y) = lex.number() else { break };
                    let to = Vector2::new(cur.X, if rel { cur.Y + y } else { y });
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Line(to));
                    }
                    cur = to;
                }
                b'C' => {
                    let Some(x1) = lex.number() else { break };
                    let Some(y1) = lex.number() else { break };
                    let Some(x2) = lex.number() else { break };
                    let Some(y2) = lex.number() else { break };
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    let c1 = Vector2::new(base.X + x1, base.Y + y1);
                    let c2 = Vector2::new(base.X + x2, base.Y + y2);
                    let to = Vector2::new(base.X + x, base.Y + y);
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Cubic(c1, c2, to));
                    }
                    cur = to;
                }
                b'Q' => {
                    let Some(qx) = lex.number() else { break };
                    let Some(qy) = lex.number() else { break };
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    let q = Vector2::new(base.X + qx, base.Y + qy);
                    let to = Vector2::new(base.X + x, base.Y + y);
                    let c1 = Vector2::new(
                        cur.X + (q.X - cur.X) * 2.0 / 3.0,
                        cur.Y + (q.Y - cur.Y) * 2.0 / 3.0,
                    );
                    let c2 = Vector2::new(
                        to.X + (q.X - to.X) * 2.0 / 3.0,
                        to.Y + (q.Y - to.Y) * 2.0 / 3.0,
                    );
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Cubic(c1, c2, to));
                    }
                    cur = to;
                }
                b'A' => {
                    let Some(rx) = lex.number() else { break };
                    let Some(ry) = lex.number() else { break };
                    let Some(rot) = lex.number() else { break };
                    let Some(large) = lex.number() else { break };
                    let Some(sweep) = lex.number() else { break };
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    let to = Vector2::new(base.X + x, base.Y + y);
                    let mut arc = Vec::new();
                    arc_to_cubics(cur, rx, ry, rot, large != 0.0, sweep != 0.0, to, &mut arc);
                    if let Some(p) = open.as_mut() {
                        for seg in arc {
                            p.segs.push(seg);
                        }
                    }
                    cur = to;
                }
                b'Z' => {
                    if let Some(mut p) = open.take() {
                        p.closed = true;
                        out.push(p);
                    }
                    cur = sub_start;
                    break;
                }
                _ => break,
            }
            if cmd.to_ascii_uppercase() == b'M' {
                // unreachable (M rewritten above)
            }
            // Z consumes no numbers; all others can repeat.
            if cmd.to_ascii_uppercase() == b'Z' {
                break;
            }
        }
    }
    if let Some(mut p) = open.take() {
        p.closed = false;
        out.push(p);
    }
    let _ = Builder {
        cur,
        start: sub_start,
        cmd,
    };
    out
}

#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(
    p0: Vector2,
    rx: f32,
    ry: f32,
    rotation_deg: f32,
    large: bool,
    sweep: bool,
    p1: Vector2,
    out: &mut Vec<Seg>,
) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx <= f32::EPSILON || ry <= f32::EPSILON {
        out.push(Seg::Line(p1));
        return;
    }
    let phi = rotation_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();
    let dx = (p0.X - p1.X) / 2.0;
    let dy = (p0.Y - p1.Y) / 2.0;
    let x1 = cos_phi * dx + sin_phi * dy;
    let y1 = -sin_phi * dx + cos_phi * dy;

    let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let sign = if large != sweep { -1.0 } else { 1.0 };
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let coef = sign * (num.max(0.0) / den.max(f32::EPSILON)).sqrt();
    let cxp = coef * rx * y1 / ry;
    let cyp = -coef * ry * x1 / rx;
    let cx = cos_phi * cxp - sin_phi * cyp + (p0.X + p1.X) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (p0.Y + p1.Y) / 2.0;

    let ux = (x1 - cxp) / rx;
    let uy = (y1 - cyp) / ry;
    let vx = (-x1 - cxp) / rx;
    let vy = (-y1 - cyp) / ry;
    let theta = uy.atan2(ux);
    let mut delta = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    let segments = ((delta.abs() / (std::f32::consts::FRAC_PI_2)).ceil() as usize).max(1);
    let step = delta / segments as f32;
    let mut angle = theta;
    for _ in 0..segments {
        let next = angle + step;
        let alpha = 4.0 / 3.0 * (step / 4.0).tan();
        let (sa, ca) = angle.sin_cos();
        let (sn, cn) = next.sin_cos();
        let c1 = Vector2::new(cx + rx * (ca - alpha * sa), cy + ry * (sa + alpha * ca));
        let c2 = Vector2::new(cx + rx * (cn + alpha * sn), cy + ry * (sn - alpha * cn));
        let to = Vector2::new(cx + rx * cn, cy + ry * sn);
        out.push(Seg::Cubic(c1, c2, to));
        angle = next;
    }
}

pub fn build(factory: &ID2D1Factory, icon: IconId) -> Result<ID2D1PathGeometry> {
    let path = spec(icon).path;
    let subpaths = parse(path);
    let geometry = unsafe { factory.CreatePathGeometry() }?;
    let sink = unsafe { geometry.Open() }?;
    // SAFETY: the sink belongs to `geometry` and is closed below.
    unsafe {
        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
        for sub in &subpaths {
            sink.BeginFigure(
                sub.start,
                if sub.closed {
                    D2D1_FIGURE_BEGIN_FILLED
                } else {
                    D2D1_FIGURE_BEGIN_HOLLOW
                },
            );
            for seg in &sub.segs {
                match seg {
                    Seg::Line(p) => sink.AddLine(*p),
                    Seg::Cubic(c1, c2, p) => {
                        let bezier = D2D1_BEZIER_SEGMENT {
                            point1: *c1,
                            point2: *c2,
                            point3: *p,
                        };
                        sink.AddBezier(&bezier);
                    }
                }
            }
            sink.EndFigure(if sub.closed {
                D2D1_FIGURE_END_CLOSED
            } else {
                D2D1_FIGURE_END_OPEN
            });
        }
        sink.Close()?;
    }
    Ok(geometry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses_to_a_subpath() {
        for icon in [
            IconId::Music,
            IconId::Phone,
            IconId::Folder,
            IconId::Clock,
            IconId::Speaker,
            IconId::SpeakerMuted,
            IconId::DownArrow,
            IconId::Mic,
            IconId::MicMuted,
            IconId::Signal,
            IconId::Shuffle,
            IconId::Repeat,
            IconId::Play,
            IconId::Pause,
            IconId::Previous,
            IconId::Next,
            IconId::Close,
        ] {
            let subpaths = parse(spec(icon).path);
            assert!(!subpaths.is_empty(), "no subpaths for {icon:?}");
            assert!(
                subpaths.iter().any(|p| !p.segs.is_empty()),
                "no segments for {icon:?}"
            );
        }
    }

    #[test]
    fn relative_and_absolute_arcs_end_where_requested() {
        let subs = parse("M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z");
        assert_eq!(subs.len(), 1);
        let last = subs[0].segs.last().unwrap();
        match last {
            Seg::Cubic(_, _, p) => {
                assert!((p.X - 12.0).abs() < 0.1);
                assert!((p.Y - 3.0).abs() < 0.1);
            }
            _ => panic!("expected cubic"),
        }
    }
}
