//! render.rs — Direct2D/DirectWrite drawing for the pill, bubble and flyout.
//!
//! Everything is drawn from `PillView`/`FlyoutView`. The same painter is used by
//! the on-screen swapchain and by the headless WIC snapshot path, so the PNGs
//! acceptance-tested here are the same pixels the product draws.

use std::collections::HashMap;
use std::mem::ManuallyDrop;

use windows::core::{Interface, Result};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN,
    D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1Geometry, ID2D1Layer, ID2D1PathGeometry, ID2D1RenderTarget,
    ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT,
    D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_PROPERTIES, D2D1_CAP_STYLE_FLAT,
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_CUSTOM, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, D2D1_LINE_JOIN_ROUND, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES, D2D1_SWEEP_DIRECTION_CLOCKWISE,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteInlineObject, IDWriteTextFormat, IDWriteTextLayout, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_LINE_SPACING_METHOD_UNIFORM,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows_numerics::{Matrix3x2, Vector2};

use crate::design::{self, Color, RectD};
use crate::icons;
use crate::theme::Theme;
use crate::view::{
    AlbumArt, Body, Element, FlyoutView, IconId, Leading, MusicBody, PillKind, PillView,
    RepeatMode, TileView, Trailing, ViewModel,
};

// --------------------------------------------------------------- draw input

#[derive(Debug, Clone, Copy)]
pub struct ProgressVisual {
    /// Elapsed fraction 0..1.
    pub fraction: f32,
    pub phase: f32,
    pub amplitude: f32,
    pub playing: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct IconSwap {
    pub from_playing: bool,
    pub t: f32,
}

#[derive(Debug, Clone)]
pub struct RenderInput<'a> {
    pub view: &'a ViewModel,
    pub theme: Theme,
    pub scale: f32,
    /// Screen-local DIP surfaces. The flyout rect changes while morphing.
    pub pill: RectD,
    pub bubble: Option<RectD>,
    pub flyout: Option<RectD>,
    /// Overall flyout fill/shadow opacity (0..1).
    pub flyout_alpha: f32,
    pub pill_alpha: f32,
    pub top_alpha: f32,
    pub body_alpha: f32,
    pub top_dy: f32,
    pub body_dy: f32,
    pub hover: Option<Element>,
    /// Element plus press progress (0..1 over the 120 ms press).
    pub pressed: Option<(Element, f32)>,
    pub focus: Option<Element>,
    pub hover_mix: f32,
    pub sheen: Option<f32>,
    pub flash_t: Option<f32>,
    pub drag_bob: f32,
    pub progress: Option<ProgressVisual>,
    pub icon_swap: Option<IconSwap>,
}

impl RenderInput<'_> {
    fn pressed_amount(&self, element: Element) -> Option<f32> {
        self.pressed
            .as_ref()
            .filter(|(e, _)| *e == element)
            .map(|(_, t)| *t)
    }

    fn hovered(&self, element: Element) -> bool {
        self.hover == Some(element)
    }
}

// ---------------------------------------------------------------- hit map

#[derive(Debug, Clone, Copy)]
pub struct HitTarget {
    pub element: Element,
    pub rect: RectD,
    pub radius: f32,
}

#[derive(Debug, Clone, Default)]
pub struct HitMap {
    pub targets: Vec<HitTarget>,
}

impl HitMap {
    fn add(&mut self, element: Element, rect: RectD, radius: f32) {
        self.targets.push(HitTarget {
            element,
            rect,
            radius,
        });
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<Element> {
        self.targets
            .iter()
            .rev()
            .find(|t| t.rect.contains(x, y))
            .map(|t| t.element)
    }

    pub fn target(&self, element: Element) -> Option<HitTarget> {
        self.targets
            .iter()
            .rev()
            .find(|t| t.element == element)
            .copied()
    }
}

// --------------------------------------------------------------- painter

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FormatKey {
    size_x100: i32,
    weight: i32,
    align: i32,
    line_x100: i32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    text: String,
    format: FormatKey,
    max_x10: i32,
    ellipsis: bool,
}

pub struct Painter {
    formats: HashMap<FormatKey, IDWriteTextFormat>,
    layouts: HashMap<LayoutKey, IDWriteTextLayout>,
    icons: HashMap<IconId, ID2D1PathGeometry>,
    ellipsis: Option<IDWriteInlineObject>,
    round_stroke: Option<ID2D1StrokeStyle>,
    dash_stroke: Option<ID2D1StrokeStyle>,
    target_key: isize,
    brushes: HashMap<(u32, u32, u32, u32), ID2D1SolidColorBrush>,
    art_target: isize,
    art: Option<(u64, ID2D1Bitmap)>,
}

impl Default for Painter {
    fn default() -> Self {
        Self {
            formats: HashMap::new(),
            layouts: HashMap::new(),
            icons: HashMap::new(),
            ellipsis: None,
            round_stroke: None,
            dash_stroke: None,
            target_key: 0,
            brushes: HashMap::new(),
            art_target: 0,
            art: None,
        }
    }
}

impl Painter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn draw(&mut self, rt: &ID2D1RenderTarget, input: &RenderInput<'_>) -> Result<HitMap> {
        let key = rt.as_raw() as isize;
        if self.target_key != key {
            self.target_key = key;
            self.brushes.clear();
        }
        let base = Matrix3x2 {
            M11: input.scale,
            M22: input.scale,
            ..Matrix3x2::identity()
        };
        // SAFETY: the render target is valid for this entire call.
        unsafe {
            rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            rt.SetTransform(&base);
        }

        let mut hits = HitMap::default();
        let idle = unreachable_pill();
        let pill = input.view.pill().unwrap_or(idle);
        self.draw_pill(rt, input, pill, &mut hits);
        if let Some(second) = &input.view.second {
            if let Some(rect) = input.bubble {
                self.draw_bubble(rt, input, second, rect, &mut hits);
            }
        }
        if let (Some(fly), Some(flyout)) = (input.flyout, input.view.flyout()) {
            self.draw_flyout(rt, input, flyout, fly, &mut hits);
        }
        if let Some(focus) = input.focus {
            if let Some(target) = hits.target(focus) {
                self.draw_focus_ring(rt, input, target);
            }
        }
        Ok(hits)
    }

    // ------------------------------------------------------------- brushes

    fn brush(&mut self, rt: &ID2D1RenderTarget, color: Color) -> Result<ID2D1SolidColorBrush> {
        let key = (
            color.r.to_bits(),
            color.g.to_bits(),
            color.b.to_bits(),
            color.a.to_bits(),
        );
        if let Some(b) = self.brushes.get(&key) {
            return Ok(b.clone());
        }
        let c = D2D1_COLOR_F {
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
        };
        // SAFETY: target is valid; the brush is held in the cache.
        let brush = unsafe { rt.CreateSolidColorBrush(&c, None)? };
        self.brushes.insert(key, brush.clone());
        Ok(brush)
    }

    fn fill_rect(&mut self, rt: &ID2D1RenderTarget, r: RectD, color: Color) {
        if r.w <= 0.0 || r.h <= 0.0 || color.a <= 0.0 {
            return;
        }
        let Ok(brush) = self.brush(rt, color) else {
            return;
        };
        let rect = d2d_rect(r);
        // SAFETY: ordinary fill call.
        unsafe { rt.FillRectangle(&rect, &brush) };
    }

    fn fill_round(&mut self, rt: &ID2D1RenderTarget, r: RectD, radius: f32, color: Color) {
        if r.w <= 0.0 || r.h <= 0.0 || color.a <= 0.0 {
            return;
        }
        let Ok(brush) = self.brush(rt, color) else {
            return;
        };
        let rr = round_rect(r, radius);
        // SAFETY: ordinary fill call.
        unsafe { rt.FillRoundedRectangle(&rr, &brush) };
    }

    fn stroke_round(
        &mut self,
        rt: &ID2D1RenderTarget,
        r: RectD,
        radius: f32,
        width: f32,
        color: Color,
        dashed: bool,
    ) {
        if r.w <= 0.0 || r.h <= 0.0 || color.a <= 0.0 {
            return;
        }
        let Ok(brush) = self.brush(rt, color) else {
            return;
        };
        let style = if dashed {
            self.dash_style(rt).ok()
        } else {
            None
        };
        let rr = round_rect(r, radius);
        // SAFETY: ordinary stroke call. The style is owned by the painter.
        unsafe {
            let _ = rt.DrawRoundedRectangle(&rr, &brush, width, style.as_ref());
        }
    }

    fn stroke_style(&mut self, rt: &ID2D1RenderTarget, dashed: bool) -> Result<ID2D1StrokeStyle> {
        if dashed {
            if let Some(style) = &self.dash_stroke {
                return Ok(style.clone());
            }
            let factory = unsafe { rt.GetFactory()? };
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_FLAT,
                endCap: D2D1_CAP_STYLE_FLAT,
                dashCap: D2D1_CAP_STYLE_FLAT,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                dashStyle: D2D1_DASH_STYLE_CUSTOM,
                ..Default::default()
            };
            // 3 on, 3 off expressed in multiples of a 1-device-pixel stroke.
            let dashes = [3.0, 3.0];
            // SAFETY: factory and properties are valid.
            let style = unsafe { factory.CreateStrokeStyle(&props, Some(&dashes))? };
            self.dash_stroke = Some(style.clone());
            Ok(style)
        } else {
            if let Some(style) = &self.round_stroke {
                return Ok(style.clone());
            }
            let factory = unsafe { rt.GetFactory()? };
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                dashStyle: windows::Win32::Graphics::Direct2D::D2D1_DASH_STYLE_SOLID,
                ..Default::default()
            };
            // SAFETY: factory and properties are valid.
            let style = unsafe { factory.CreateStrokeStyle(&props, None)? };
            self.round_stroke = Some(style.clone());
            Ok(style)
        }
    }

    fn dash_style(&mut self, rt: &ID2D1RenderTarget) -> Result<ID2D1StrokeStyle> {
        self.stroke_style(rt, true)
    }

    fn solid_style(&mut self, rt: &ID2D1RenderTarget) -> Result<ID2D1StrokeStyle> {
        self.stroke_style(rt, false)
    }

    // ----------------------------------------------------------------- text

    fn format(&mut self, _rt: &ID2D1RenderTarget, spec: FormatKey) -> Result<IDWriteTextFormat> {
        if let Some(f) = self.formats.get(&spec) {
            return Ok(f.clone());
        }
        // DirectWrite formats are created by the process-wide DWrite factory.
        let dwrite = crate::gpu::shared()?.dwrite.clone();
        let size = spec.size_x100 as f32 / 100.0;
        let line = spec.line_x100 as f32 / 100.0;
        let align = match spec.align {
            1 => DWRITE_TEXT_ALIGNMENT_CENTER,
            2 => DWRITE_TEXT_ALIGNMENT_TRAILING,
            _ => DWRITE_TEXT_ALIGNMENT_LEADING,
        };
        let make = |family: &windows::core::HSTRING| -> Result<IDWriteTextFormat> {
            // SAFETY: family and locale are NUL-terminated wide strings.
            let format = unsafe {
                dwrite.CreateTextFormat(
                    windows::core::PCWSTR(family.as_ptr()),
                    None,
                    DWRITE_FONT_WEIGHT(spec.weight),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    windows::core::w!("en-US"),
                )?
            };
            // SAFETY: format is a live object.
            unsafe {
                format.SetTextAlignment(align)?;
                format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                format.SetLineSpacing(
                    DWRITE_LINE_SPACING_METHOD_UNIFORM,
                    line.max(size),
                    line.max(size) * 0.8,
                )?;
            }
            Ok(format)
        };
        let family = windows::core::HSTRING::from(design::FONT_FAMILY);
        let fallback = windows::core::HSTRING::from(design::FONT_FALLBACK);
        let format = make(&family).or_else(|_| make(&fallback))?;
        self.formats.insert(spec, format.clone());
        Ok(format)
    }

    fn ellipsis_sign(&mut self, rt: &ID2D1RenderTarget) -> Option<IDWriteInlineObject> {
        if let Some(sign) = &self.ellipsis {
            return Some(sign.clone());
        }
        let format = self
            .format(
                rt,
                FormatKey {
                    size_x100: (design::FONT_PILL_LABEL * 100.0) as i32,
                    weight: 400,
                    align: 0,
                    line_x100: (design::LINE_PILL * 100.0) as i32,
                },
            )
            .ok()?;
        let dwrite = crate::gpu::shared().ok()?.dwrite.clone();
        // SAFETY: format is alive; returns a small inline object.
        let sign = unsafe { dwrite.CreateEllipsisTrimmingSign(&format).ok()? };
        self.ellipsis = Some(sign.clone());
        Some(sign)
    }

    fn layout(
        &mut self,
        rt: &ID2D1RenderTarget,
        text: &str,
        spec: FormatKey,
        max_width: f32,
        ellipsis: bool,
    ) -> Option<IDWriteTextLayout> {
        if text.is_empty() || max_width <= 0.5 {
            return None;
        }
        let key = LayoutKey {
            text: text.to_string(),
            format: spec,
            max_x10: (max_width.max(0.0) * 10.0) as i32,
            ellipsis,
        };
        if let Some(layout) = self.layouts.get(&key) {
            return Some(layout.clone());
        }
        let format = self.format(rt, spec).ok()?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        let dwrite = crate::gpu::shared().ok()?.dwrite.clone();
        // SAFETY: strings and format are valid for the duration of the call.
        let layout = unsafe {
            dwrite
                .CreateTextLayout(&wide, &format, max_width, 1000.0)
                .ok()?
        };
        if ellipsis {
            if let Some(sign) = self.ellipsis_sign(rt) {
                let trimming = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                // SAFETY: layout and sign are alive.
                let _ = unsafe { layout.SetTrimming(&trimming, &sign) };
            }
        }
        if self.layouts.len() > 384 {
            self.layouts.clear();
        }
        self.layouts.insert(key, layout.clone());
        Some(layout)
    }

    fn measure(
        &mut self,
        rt: &ID2D1RenderTarget,
        text: &str,
        size: f32,
        weight: i32,
        line: f32,
    ) -> f32 {
        let Some(layout) = self.layout(
            rt,
            text,
            FormatKey {
                size_x100: (size * 100.0) as i32,
                weight,
                align: 0,
                line_x100: (line * 100.0) as i32,
            },
            2000.0,
            false,
        ) else {
            return 0.0;
        };
        let mut metrics = Default::default();
        // SAFETY: layout is live; metrics is writable.
        if unsafe { layout.GetMetrics(&mut metrics) }.is_err() {
            return 0.0;
        }
        metrics.widthIncludingTrailingWhitespace
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        rt: &ID2D1RenderTarget,
        text: &str,
        rect: RectD,
        size: f32,
        weight: i32,
        align: i32,
        line: f32,
        color: Color,
        ellipsis: bool,
    ) -> f32 {
        if rect.w <= 0.0 || rect.h <= 0.0 || color.a <= 0.0 {
            return 0.0;
        }
        let spec = FormatKey {
            size_x100: (size * 100.0) as i32,
            weight,
            align,
            line_x100: (line * 100.0) as i32,
        };
        let Some(layout) = self.layout(rt, text, spec, rect.w, ellipsis) else {
            return 0.0;
        };
        let mut metrics = Default::default();
        // SAFETY: layout is live.
        let _ = unsafe { layout.GetMetrics(&mut metrics) };
        let y = rect.y + (rect.h - metrics.height) * 0.5;
        let Ok(brush) = self.brush(rt, color) else {
            return 0.0;
        };
        // SAFETY: ordinary text draw.
        unsafe {
            rt.DrawTextLayout(
                Vector2::new(rect.x, y),
                &layout,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        metrics.widthIncludingTrailingWhitespace
    }

    // ---------------------------------------------------------------- icons

    fn icon_geometry(&mut self, rt: &ID2D1RenderTarget, icon: IconId) -> Option<ID2D1PathGeometry> {
        if let Some(g) = self.icons.get(&icon) {
            return Some(g.clone());
        }
        let factory = unsafe { rt.GetFactory().ok()? };
        let geometry = icons::build(&factory, icon).ok()?;
        self.icons.insert(icon, geometry.clone());
        Some(geometry)
    }

    #[allow(clippy::too_many_arguments)]
    fn icon_at(
        &mut self,
        rt: &ID2D1RenderTarget,
        icon: IconId,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        scale: f32,
        stroke_units: f32,
    ) {
        if size <= 0.0 || color.a <= 0.0 {
            return;
        }
        let Some(geometry) = self.icon_geometry(rt, icon) else {
            return;
        };
        let Ok(brush) = self.brush(rt, color) else {
            return;
        };
        let s = size / 24.0;
        let matrix = Matrix3x2 {
            M11: s * scale,
            M22: s * scale,
            M31: x * scale,
            M32: y * scale,
            ..Matrix3x2::identity()
        };
        // SAFETY: geometry, brush and matrix are valid for the call.
        unsafe {
            rt.SetTransform(&matrix);
            let spec = icons::spec(icon);
            if spec.filled {
                let _ = rt.FillGeometry(&geometry, &brush, None);
            } else {
                let style = self.solid_style(rt).ok();
                let _ = rt.DrawGeometry(
                    &geometry,
                    &brush,
                    if stroke_units > 0.0 {
                        stroke_units
                    } else {
                        spec.stroke
                    },
                    style.as_ref(),
                );
            }
            let base = Matrix3x2 {
                M11: scale,
                M22: scale,
                ..Matrix3x2::identity()
            };
            rt.SetTransform(&base);
        }
    }

    fn icon_center(
        &mut self,
        rt: &ID2D1RenderTarget,
        icon: IconId,
        rect: RectD,
        size: f32,
        color: Color,
        scale: f32,
        stroke_units: f32,
    ) {
        self.icon_at(
            rt,
            icon,
            rect.center_x() - size * 0.5,
            rect.center_y() - size * 0.5,
            size,
            color,
            scale,
            stroke_units,
        );
    }

    // ---------------------------------------------------------------- pill

    fn draw_pill(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        pill: &PillView,
        hits: &mut HitMap,
    ) {
        let r = input.pill;
        let theme = input.theme;
        let alpha = input.pill_alpha.clamp(0.0, 1.0);
        if alpha <= 0.001 {
            return;
        }
        let px = 1.0 / input.scale;
        let accent = accent_of(input.view, pill);
        let open = input.flyout.is_some();
        let hovered = matches!(input.hover, Some(Element::Pill | Element::Bubble));
        let pressed = input.pressed_amount(Element::Pill).is_some();
        let active_mix = if open || pressed {
            1.0
        } else if hovered {
            input.hover_mix.clamp(0.0, 1.0)
        } else {
            input.hover_mix.clamp(0.0, 1.0)
        };

        let rest_fill = theme.pill_fill();
        let hover_fill = theme.pill_fill_hover();
        let mut fill = Color::lerp(rest_fill, hover_fill, active_mix);
        if pressed {
            fill = theme.pill_fill_press();
        }
        let mut border = Color::lerp(
            theme.pill_border_rest(),
            accent.with_alpha(design::PILL_BORDER_ACTIVE_A),
            active_mix,
        );
        if pill.kind == PillKind::DragOver {
            fill = accent.with_alpha(design::DRAG_FILL_A);
            border = accent.with_alpha(design::PILL_BORDER_DRAG_A);
        }
        if pill.kind == PillKind::Finished {
            if let Some(t) = input.flash_t {
                let cycle = (t / (design::TIMER_FLASH_MS as f32 / 1000.0)).floor();
                if cycle < design::TIMER_FLASHES as f32 {
                    let p = (t % (design::TIMER_FLASH_MS as f32 / 1000.0))
                        / (design::TIMER_FLASH_MS as f32 / 1000.0);
                    let k = 1.0 - (2.0 * p - 1.0).abs();
                    border = accent.with_alpha(
                        design::PILL_BORDER_ACTIVE_A + (1.0 - design::PILL_BORDER_ACTIVE_A) * k,
                    );
                }
            }
        }
        fill.a *= alpha;
        border.a *= alpha;
        self.fill_round(rt, r, theme.pill_radius(), fill);

        // Bottom progress line, clipped to the pill's rounded shape.
        if let Some(fraction) = pill.progress {
            if !matches!(pill.kind, PillKind::Volume { .. }) && pill.kind != PillKind::DragOver {
                let clip = rounded_layer(rt, r, theme.pill_radius());
                if let Some((_layer, _geom)) = &clip {
                    let track = r;
                    let track_color = theme.track().with_alpha(0.10 * alpha);
                    self.fill_rect(
                        rt,
                        RectD::new(
                            track.x,
                            track.bottom() - design::PILL_PROGRESS_H,
                            track.w,
                            design::PILL_PROGRESS_H,
                        ),
                        track_color,
                    );
                    let f = fraction.clamp(0.0, 1.0);
                    self.fill_rect(
                        rt,
                        RectD::new(
                            track.x,
                            track.bottom() - design::PILL_PROGRESS_H,
                            track.w * f,
                            design::PILL_PROGRESS_H,
                        ),
                        accent.with_alpha(0.90 * alpha),
                    );
                }
                drop_rounded_layer(rt, clip);
            }
        }

        // Border.
        let border_rect = r.inset(px * 0.5);
        self.stroke_round(
            rt,
            border_rect,
            theme.pill_radius(),
            px,
            border,
            pill.kind == PillKind::DragOver,
        );

        // Tile.
        let tile_color = pill.tile.color.with_alpha(pill.tile.alpha * alpha);
        let tile = RectD::new(
            r.x + design::PILL_PAD_L,
            r.y + design::PILL_TILE_Y,
            design::PILL_TILE,
            design::PILL_TILE,
        );
        self.fill_round(rt, tile, design::R_TILE_PILL, tile_color);
        let bob = if pill.kind == PillKind::DragOver {
            -2.0 * ((input.drag_bob * std::f32::consts::TAU).sin() * 0.5 + 0.5)
        } else {
            0.0
        };
        let glyph_color = pill.tile.glyph.with_alpha(alpha);
        self.icon_at(
            rt,
            pill.icon,
            tile.x + (tile.w - design::PILL_GLYPH) * 0.5,
            tile.y + (tile.h - design::PILL_GLYPH) * 0.5 + bob,
            design::PILL_GLYPH,
            glyph_color,
            input.scale,
            design::ICON_STROKE,
        );

        let content_x = tile.right() + design::PILL_TILE_GAP;
        let content_right = r.right() - design::PILL_PAD_R;
        if matches!(pill.kind, PillKind::Volume { .. }) {
            let value_w = design::VOL_VALUE_W;
            let value_x = content_right - value_w;
            let slider_x = content_x;
            let slider_w = (value_x - design::VOL_VALUE_GAP - slider_x).max(0.0);
            let y = r.y + (r.h - design::VOL_SLIDER_H) * 0.5;
            self.fill_round(
                rt,
                RectD::new(slider_x, y, slider_w, design::VOL_SLIDER_H),
                design::R_THUMB,
                design::white(design::VOLUME_TRACK_A * alpha),
            );
            let level = match pill.kind {
                PillKind::Volume { level, .. } => level,
                _ => 0.0,
            };
            self.fill_round(
                rt,
                RectD::new(
                    slider_x,
                    y,
                    slider_w * level.clamp(0.0, 1.0),
                    design::VOL_SLIDER_H,
                ),
                design::R_THUMB,
                design::white(alpha),
            );
            self.text(
                rt,
                &pill.value,
                RectD::new(value_x, r.y, value_w, r.h),
                design::FONT_PILL_VALUE,
                600,
                2,
                design::LINE_PILL,
                design::white(alpha),
                false,
            );
        } else {
            let value_w = if pill.value.is_empty() {
                0.0
            } else {
                self.measure(
                    rt,
                    &pill.value,
                    design::FONT_PILL_VALUE,
                    600,
                    design::LINE_PILL,
                )
            };
            let value_x = content_right - value_w;
            let label_w = (value_x - content_x - if value_w > 0.0 { 8.0 } else { 0.0 }).max(0.0);
            self.text(
                rt,
                &pill.label,
                RectD::new(content_x, r.y, label_w, r.h),
                design::FONT_PILL_LABEL,
                500,
                0,
                design::LINE_PILL,
                theme
                    .text_pill_label()
                    .with_alpha(design::TEXT_PILL_LABEL_A * alpha),
                true,
            );
            if value_w > 0.0 {
                self.text(
                    rt,
                    &pill.value,
                    RectD::new(value_x, r.y, value_w, r.h),
                    design::FONT_PILL_VALUE,
                    600,
                    0,
                    design::LINE_PILL,
                    accent.with_alpha(alpha),
                    false,
                );
            }
        }

        // Hover sheen: flat strips, no gradient, clipped by the rounded pill.
        if let Some(t) = input.sheen {
            if active_mix > 0.0 && t >= 0.0 && t <= 1.0 {
                let clip = rounded_layer(rt, r, design::R_SURFACE);
                if let Some(_clip) = &clip {
                    let w = r.w;
                    let center = r.x + (-1.3 + 2.6 * t) * w;
                    let half = 0.35 * w;
                    let strips = 18;
                    for i in 0..strips {
                        let u = (i as f32 + 0.5) / strips as f32;
                        let x = center - half + u * half * 2.0;
                        let profile = (1.0 - (2.0 * u - 1.0).abs()).powi(2);
                        let a = design::SHEEN_ALPHA * profile * alpha;
                        self.fill_rect(
                            rt,
                            RectD::new(x, r.y, (half * 2.0 / strips as f32) + 0.5, r.h),
                            design::white(a),
                        );
                    }
                }
                drop_rounded_layer(rt, clip);
            }
        }
        hits.add(Element::Pill, r, theme.pill_radius());
    }

    fn draw_bubble(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        second: &crate::view::SecondView,
        r: RectD,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let alpha = input.pill_alpha.clamp(0.0, 1.0);
        let mut fill = theme.pill_fill();
        if input.hovered(Element::Bubble) {
            fill = theme.pill_fill_hover();
        }
        fill.a *= alpha;
        let px = 1.0 / input.scale;
        self.fill_round(rt, r, theme.pill_radius(), fill);
        self.stroke_round(
            rt,
            r.inset(px * 0.5),
            theme.pill_radius(),
            px,
            theme.pill_border_rest().with_alpha(alpha),
            false,
        );
        let tile = RectD::new(
            r.center_x() - design::PILL_TILE * 0.5,
            r.center_y() - design::PILL_TILE * 0.5,
            design::PILL_TILE,
            design::PILL_TILE,
        );
        self.fill_round(
            rt,
            tile,
            design::R_TILE_PILL,
            second.accent.with_alpha(alpha),
        );
        self.icon_center(
            rt,
            second.icon,
            tile,
            design::PILL_GLYPH,
            design::DARK_GLYPH.with_alpha(alpha),
            input.scale,
            design::ICON_STROKE,
        );
        hits.add(Element::Bubble, r, design::R_SURFACE);
    }

    // -------------------------------------------------------------- flyout

    fn draw_flyout(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        view: &FlyoutView,
        fly: RectD,
        hits: &mut HitMap,
    ) {
        let alpha = input.flyout_alpha.clamp(0.0, 1.0);
        if alpha <= 0.001 || fly.w <= 1.0 || fly.h <= 1.0 {
            return;
        }
        let theme = input.theme;
        self.draw_shadow(rt, fly, theme, alpha);
        let fill = theme.fly_fill().with_alpha(theme.fly_fill().a * alpha);
        self.fill_round(rt, fly, design::R_SURFACE, fill);

        let clip = rounded_layer(rt, fly, design::R_SURFACE);
        self.draw_top_row(rt, input, view, fly, hits);
        self.draw_body(rt, input, view, fly, hits);
        drop_rounded_layer(rt, clip);

        let px = 1.0 / input.scale;
        self.stroke_round(
            rt,
            fly.inset(px * 0.5),
            design::R_SURFACE,
            px,
            theme.fly_border().with_alpha(theme.fly_border().a * alpha),
            false,
        );
    }

    fn draw_shadow(&mut self, rt: &ID2D1RenderTarget, fly: RectD, theme: Theme, alpha: f32) {
        let steps = design::SHADOW_STEPS.max(4);
        for (offset, blur, base) in [
            (
                design::SHADOW_1_DY,
                design::SHADOW_1_BLUR,
                design::SHADOW_1_A,
            ),
            (
                design::SHADOW_2_DY,
                design::SHADOW_2_BLUR,
                design::SHADOW_2_A,
            ),
        ] {
            for i in (0..steps).rev() {
                let t = i as f32 / (steps - 1) as f32;
                let spread = blur * 0.5 * t;
                let a = theme.shadow_alpha(base) * alpha * (1.0 - t).powi(2) * 0.22;
                if a <= 0.002 {
                    continue;
                }
                let r = RectD::new(
                    fly.x - spread,
                    fly.y - spread + offset,
                    fly.w + spread * 2.0,
                    fly.h + spread * 2.0,
                );
                self.fill_round(rt, r, design::R_SURFACE + spread * 0.25, design::black(a));
            }
        }
    }

    fn draw_top_row(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        view: &FlyoutView,
        fly: RectD,
        hits: &mut HitMap,
    ) {
        let alpha = input.top_alpha.clamp(0.0, 1.0);
        if alpha <= 0.002 {
            return;
        }
        let theme = input.theme;
        let dy = input.top_dy;
        let top = RectD::new(
            fly.x + design::FLY_PAD,
            fly.y + design::FLY_PAD + dy,
            design::FLY_CONTENT_W,
            design::TOP_ROW_H,
        );
        let lead = RectD::new(top.x, top.y, design::LEAD_TILE, design::LEAD_TILE);
        self.draw_leading(rt, input, &view.leading, lead, alpha);

        let trailing_right = top.right();
        let mut trailing_left = trailing_right;
        if let Some(second) = &input.view.second {
            let chip = self.switch_chip(rt, input, second, top, trailing_right, alpha, hits);
            trailing_left = chip.x - design::TRAILING_GAP;
        }
        match &view.trailing {
            Trailing::ClearAll => {
                let w = self.measure(rt, "Clear all", design::FONT_BUTTON, 500, design::LINE_HINT)
                    + 24.0;
                let rect = RectD::new(
                    trailing_left - w,
                    top.y + (design::TOP_ROW_H - design::CHIP_H) * 0.5,
                    w,
                    design::CHIP_H,
                );
                self.draw_small_button(
                    rt,
                    input,
                    Element::ClearAll,
                    rect,
                    "Clear all",
                    alpha,
                    hits,
                );
                trailing_left = rect.x - design::TRAILING_GAP;
            }
            Trailing::TimerTime(text) => {
                let w = self
                    .measure(
                        rt,
                        text,
                        design::FONT_TIMER_BIG,
                        600,
                        design::LINE_TIMER_BIG,
                    )
                    .max(62.0);
                let rect = RectD::new(trailing_left - w, top.y, w, design::TOP_ROW_H);
                self.text(
                    rt,
                    text,
                    rect,
                    design::FONT_TIMER_BIG,
                    600,
                    2,
                    design::LINE_TIMER_BIG,
                    theme.text_primary().with_alpha(alpha),
                    false,
                );
                trailing_left = rect.x - design::TRAILING_GAP;
            }
            Trailing::None => {}
        }

        let text_x = lead.right() + design::ROW_GAP;
        let text_w = (trailing_left - text_x).max(24.0);
        let title_h = design::LINE_FLY_TITLE;
        let sub_h = design::LINE_FLY_SUBTITLE;
        let block_h = title_h + sub_h;
        let y0 = top.y + (design::TOP_ROW_H - block_h) * 0.5;
        self.text(
            rt,
            &view.title,
            RectD::new(text_x, y0, text_w, title_h),
            design::FONT_FLY_TITLE,
            600,
            0,
            design::LINE_FLY_TITLE,
            theme.text_primary().with_alpha(alpha),
            true,
        );
        let mut sub_x = text_x;
        let mut sub_w = text_w;
        if !view.subtitle.prefix.is_empty() {
            let prefix_w = self.text(
                rt,
                &view.subtitle.prefix,
                RectD::new(sub_x, y0 + title_h, sub_w, sub_h),
                design::FONT_FLY_SUBTITLE,
                400,
                0,
                design::LINE_FLY_SUBTITLE,
                theme.text_secondary().with_alpha(alpha),
                false,
            );
            sub_x += prefix_w.min(sub_w);
            sub_w = (sub_w - prefix_w).max(0.0);
        }
        if let Some(accent_text) = &view.subtitle.accent {
            self.text(
                rt,
                accent_text,
                RectD::new(sub_x, y0 + title_h, sub_w, sub_h),
                design::FONT_FLY_SUBTITLE,
                400,
                0,
                design::LINE_FLY_SUBTITLE,
                accent_of(
                    input.view,
                    input
                        .view
                        .main
                        .as_ref()
                        .map(|m| &m.pill)
                        .unwrap_or_else(|| unreachable_pill()),
                )
                .with_alpha(alpha),
                true,
            );
        }
    }

    fn draw_leading(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        leading: &Leading,
        r: RectD,
        alpha: f32,
    ) {
        let theme = input.theme;
        match leading {
            Leading::Art { art, accent } => {
                self.fill_round(rt, r, design::R_TILE_LEAD, accent.with_alpha(alpha));
                if let Some(art) = art {
                    if let Some(bitmap) = self.art_bitmap(rt, art) {
                        let clip = rounded_layer(rt, r, design::R_TILE_LEAD);
                        if let Some(_clip) = &clip {
                            let side = art.width.min(art.height) as f32;
                            let sx = (art.width as f32 - side) * 0.5;
                            let sy = (art.height as f32 - side) * 0.5;
                            let src = d2d_rect(RectD::new(sx, sy, side, side));
                            let dest = d2d_rect(r);
                            // SAFETY: bitmap is alive and the target is valid.
                            unsafe {
                                let _ = rt.DrawBitmap(
                                    &bitmap,
                                    Some(&dest),
                                    1.0,
                                    windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                    Some(&src),
                                );
                            }
                        }
                        drop_rounded_layer(rt, clip);
                    } else {
                        self.icon_center(
                            rt,
                            IconId::Music,
                            r,
                            design::LEAD_GLYPH,
                            design::DARK_GLYPH.with_alpha(alpha),
                            input.scale,
                            design::ICON_STROKE,
                        );
                    }
                } else {
                    self.icon_center(
                        rt,
                        IconId::Music,
                        r,
                        design::LEAD_GLYPH,
                        design::DARK_GLYPH.with_alpha(alpha),
                        input.scale,
                        design::ICON_STROKE,
                    );
                }
            }
            Leading::Glyph { icon, bg, fg } => {
                self.fill_round(rt, r, design::R_TILE_LEAD, bg.with_alpha(alpha));
                self.icon_center(
                    rt,
                    *icon,
                    r,
                    design::LEAD_GLYPH,
                    fg.with_alpha(alpha),
                    input.scale,
                    design::ICON_STROKE,
                );
            }
            Leading::Ring { fraction } => {
                self.fill_round(
                    rt,
                    r,
                    design::R_TILE_LEAD,
                    design::TIMER_AMBER.with_alpha(0.16 * alpha),
                );
                let (cx, cy) = (r.x + design::RING_CENTER, r.y + design::RING_CENTER);
                self.draw_ring(
                    rt,
                    cx,
                    cy,
                    design::RING_RADIUS,
                    design::RING_STROKE,
                    *fraction,
                    design::TIMER_AMBER.with_alpha(alpha),
                    theme
                        .timer_track()
                        .with_alpha(theme.timer_track().a * alpha),
                    input.scale,
                );
            }
            Leading::Initial { ch, bg, fg } => {
                self.fill_round(rt, r, design::R_TILE_LEAD, bg.with_alpha(alpha));
                let text: String = ch.to_string();
                self.text(
                    rt,
                    &text,
                    r,
                    design::FONT_TIMER_BIG,
                    600,
                    1,
                    design::LINE_TIMER_BIG,
                    fg.with_alpha(alpha),
                    false,
                );
            }
        }
    }

    fn switch_chip(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        second: &crate::view::SecondView,
        top: RectD,
        right: f32,
        alpha: f32,
        hits: &mut HitMap,
    ) -> RectD {
        let label_w = self.measure(
            rt,
            &second.label,
            design::FONT_PILL_LABEL,
            500,
            design::LINE_PILL,
        );
        let w = design::CHIP_PAD_L
            + design::CHIP_TILE
            + design::CHIP_TILE_GAP
            + label_w
            + design::CHIP_PAD_R;
        let r = RectD::new(
            right - w,
            top.y + (design::TOP_ROW_H - design::CHIP_H) * 0.5,
            w,
            design::CHIP_H,
        );
        let mut fill = design::white(design::CHIP_FILL_A);
        if input.hovered(Element::SwitchChip) {
            fill = design::white(design::SURFACE_BUTTON_HOVER_A);
        }
        fill.a *= alpha;
        self.fill_round(rt, r, design::R_CONTROL, fill);
        let tile = RectD::new(
            r.x + design::CHIP_PAD_L,
            r.center_y() - design::CHIP_TILE * 0.5,
            design::CHIP_TILE,
            design::CHIP_TILE,
        );
        self.fill_round(
            rt,
            tile,
            design::R_CHIP_TILE,
            second.accent.with_alpha(alpha),
        );
        self.icon_center(
            rt,
            second.icon,
            tile,
            12.0,
            design::DARK_GLYPH.with_alpha(alpha),
            input.scale,
            design::ICON_STROKE,
        );
        self.text(
            rt,
            &second.label,
            RectD::new(
                tile.right() + design::CHIP_TILE_GAP,
                r.y,
                (r.right() - design::CHIP_PAD_R - tile.right() - design::CHIP_TILE_GAP).max(0.0),
                r.h,
            ),
            design::FONT_PILL_LABEL,
            500,
            0,
            design::LINE_PILL,
            input.theme.text_primary().with_alpha(alpha),
            true,
        );
        hits.add(Element::SwitchChip, r, design::R_CONTROL);
        r
    }

    fn draw_small_button(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        element: Element,
        r: RectD,
        label: &str,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let pressed = input.pressed_amount(element);
        let draw = pressed_rect(r, pressed);
        let mut fill = if pressed.is_some() {
            input.theme.surface_button_press()
        } else if input.hovered(element) {
            input.theme.surface_button_hover()
        } else {
            input.theme.surface_button()
        };
        fill.a *= alpha;
        self.fill_round(rt, draw, design::R_CONTROL, fill);
        self.text(
            rt,
            label,
            draw,
            design::FONT_BUTTON,
            500,
            1,
            design::LINE_HINT,
            input.theme.text_primary().with_alpha(alpha),
            true,
        );
        if alpha > 0.15 {
            hits.add(element, r, design::R_CONTROL);
        }
    }

    // --------------------------------------------------------------- bodies

    fn draw_body(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        view: &FlyoutView,
        fly: RectD,
        hits: &mut HitMap,
    ) {
        let alpha = input.body_alpha.clamp(0.0, 1.0);
        if alpha <= 0.002 {
            return;
        }
        let body = crate::view::content_boxes(fly).1.offset(0.0, input.body_dy);
        match &view.body {
            Body::Music(m) => self.draw_music(rt, input, m, body, alpha, hits),
            Body::Call(c) => self.draw_call(rt, input, c, body, alpha, hits),
            Body::Shelf(s) => self.draw_shelf(rt, input, s, body, alpha, hits),
            Body::Timer(t) => self.draw_timer(rt, input, t, body, alpha, hits),
        }
    }

    fn draw_progress_row(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        body: RectD,
        progress: ProgressVisual,
        elapsed: &str,
        right: &str,
        alpha: f32,
        hits: &mut HitMap,
        seekable: bool,
    ) {
        let theme = input.theme;
        let row = RectD::new(
            body.x,
            body.y + (design::BODY_H - design::MUSIC_BODY_H) * 0.5,
            body.w,
            design::PROGRESS_H,
        );
        let elapsed_r = RectD::new(row.x, row.y, design::PROGRESS_LABEL_ELAPSED, row.h);
        self.text(
            rt,
            elapsed,
            elapsed_r,
            design::FONT_TIME_LABEL,
            400,
            0,
            design::LINE_TIME,
            theme.text_secondary().with_alpha(alpha),
            false,
        );
        let right_r = RectD::new(
            row.right() - design::PROGRESS_LABEL_REMAIN,
            row.y,
            design::PROGRESS_LABEL_REMAIN,
            row.h,
        );
        self.text(
            rt,
            right,
            right_r,
            design::FONT_TIME_LABEL,
            400,
            2,
            design::LINE_TIME,
            theme.text_secondary().with_alpha(alpha),
            false,
        );
        let wave_x = row.x + design::PROGRESS_LABEL_ELAPSED + design::PROGRESS_LABEL_GAP;
        let wave_w =
            (row.right() - design::PROGRESS_LABEL_REMAIN - design::PROGRESS_LABEL_GAP - wave_x)
                .max(1.0);
        let wave_rect = RectD::new(wave_x, row.y, wave_w, row.h);
        self.draw_wave(rt, input, wave_rect, progress, alpha);
        if seekable && alpha > 0.15 {
            hits.add(Element::ProgressRow, wave_rect, 4.0);
        }
    }

    fn draw_wave(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        row: RectD,
        progress: ProgressVisual,
        alpha: f32,
    ) {
        let theme = input.theme;
        let fraction = progress.fraction.clamp(0.0, 1.0);
        let cy = row.y + row.h * 0.5;
        let x0 = row.x;
        let x1 = row.right();
        let px = x0 + row.w * fraction;
        let Ok(style) = self.solid_style(rt) else {
            return;
        };
        let Ok(track) = self.brush(rt, theme.track().with_alpha(alpha)) else {
            return;
        };
        let Ok(accent) = self.brush(
            rt,
            accent_of(
                input.view,
                input
                    .view
                    .main
                    .as_ref()
                    .map(|m| &m.pill)
                    .unwrap_or_else(|| unreachable_pill()),
            )
            .with_alpha(alpha),
        ) else {
            return;
        };
        // Remaining track.
        if x1 - px > 0.5 {
            // SAFETY: ordinary stroke.
            unsafe {
                rt.DrawLine(
                    Vector2::new(px, cy),
                    Vector2::new(x1, cy),
                    &track,
                    design::PROGRESS_STROKE,
                    &style,
                );
            }
        }
        // Played part: a wave sampled every DIP, clipped exactly at progressX.
        if px > x0 + 0.5 {
            let clip = d2d_rect(RectD::new(x0, row.y, px - x0, row.h));
            // SAFETY: clip and geometry are local.
            unsafe { rt.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
            if let Some(path) =
                wave_geometry(rt, x0, cy, px - x0, progress.phase, progress.amplitude)
            {
                // SAFETY: path/brush/style are alive.
                unsafe {
                    let _ = rt.DrawGeometry(&path, &accent, design::WAVE_STROKE, Some(&style));
                }
            }
            // SAFETY: balances the push above.
            unsafe { rt.PopAxisAlignedClip() };
        }
        let thumb_x = (px - design::THUMB_W * 0.5).clamp(row.x, row.right() - design::THUMB_W);
        self.fill_round(
            rt,
            RectD::new(
                thumb_x,
                cy - design::THUMB_H * 0.5,
                design::THUMB_W,
                design::THUMB_H,
            ),
            design::R_THUMB,
            design::white(alpha),
        );
    }

    fn draw_music(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        m: &MusicBody,
        body: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let progress = input.progress.unwrap_or(ProgressVisual {
            fraction: m.fraction,
            phase: 0.0,
            amplitude: if m.playing {
                design::WAVE_AMP
            } else {
                design::WAVE_AMP * design::WAVE_FLAT
            },
            playing: m.playing,
        });
        self.draw_progress_row(
            rt,
            input,
            body,
            progress,
            &m.elapsed,
            &m.remaining,
            alpha,
            hits,
            m.controls.seek,
        );

        let row_y = body.y
            + (design::BODY_H - design::MUSIC_BODY_H) * 0.5
            + design::PROGRESS_H
            + design::MUSIC_ROW_GAP;
        let total = design::BUTTON_H_SMALL
            + design::BUTTON_GAP
            + design::BUTTON_PREV
            + design::BUTTON_GAP
            + design::BUTTON_PLAY
            + design::BUTTON_GAP
            + design::BUTTON_PREV
            + design::BUTTON_GAP
            + design::BUTTON_H_SMALL;
        let mut x = body.center_x() - total * 0.5;
        let y = row_y;
        let theme = input.theme;
        let accent = accent_of(
            input.view,
            input
                .view
                .main
                .as_ref()
                .map(|m| &m.pill)
                .unwrap_or_else(|| unreachable_pill()),
        );
        // Shuffle.
        let r = RectD::new(x, y, design::BUTTON_H_SMALL, design::BUTTON_H);
        let supported = m.controls.shuffle;
        self.control_button(
            rt,
            input,
            Element::MusicShuffle,
            r,
            IconId::Shuffle,
            design::CONTROL_GLYPH_SMALL,
            if m.shuffle {
                theme.text_primary()
            } else {
                design::white(0.60)
            },
            if m.shuffle {
                Some(design::white(design::SURFACE_BUTTON_A))
            } else {
                None
            },
            alpha,
            supported,
            hits,
        );
        x += design::BUTTON_H_SMALL + design::BUTTON_GAP;
        // Previous.
        let r = RectD::new(x, y, design::BUTTON_PREV, design::BUTTON_H);
        self.control_button(
            rt,
            input,
            Element::MusicPrev,
            r,
            IconId::Previous,
            design::CONTROL_GLYPH,
            theme.text_primary(),
            None,
            alpha,
            m.controls.previous,
            hits,
        );
        x += design::BUTTON_PREV + design::BUTTON_GAP;
        // Play / pause.
        let r = RectD::new(x, y, design::BUTTON_PLAY, design::BUTTON_H);
        let draw = pressed_rect(r, input.pressed_amount(Element::MusicPlay));
        let fill = if input.hovered(Element::MusicPlay) {
            darken(accent, 0.90)
        } else {
            accent
        };
        self.fill_round(rt, draw, design::R_CONTROL, fill.with_alpha(alpha));
        if let Some(swap) = input.icon_swap {
            let t = swap.t.clamp(0.0, 1.2);
            let from = if swap.from_playing {
                IconId::Pause
            } else {
                IconId::Play
            };
            let to = if swap.from_playing {
                IconId::Play
            } else {
                IconId::Pause
            };
            self.icon_scaled(
                rt,
                from,
                draw,
                design::CONTROL_GLYPH_PLAY,
                design::DARK_GLYPH.with_alpha((1.0 - t).clamp(0.0, 1.0) * alpha),
                input.scale,
                0.4 + 0.6 * (1.0 - t).clamp(0.0, 1.0),
            );
            self.icon_scaled(
                rt,
                to,
                draw,
                design::CONTROL_GLYPH_PLAY,
                design::DARK_GLYPH.with_alpha(t.clamp(0.0, 1.0) * alpha),
                input.scale,
                0.4 + 0.6 * t.clamp(0.0, 1.0),
            );
        } else {
            self.icon_center(
                rt,
                if m.playing {
                    IconId::Pause
                } else {
                    IconId::Play
                },
                draw,
                design::CONTROL_GLYPH_PLAY,
                design::DARK_GLYPH.with_alpha(alpha),
                input.scale,
                0.0,
            );
        }
        if alpha > 0.15 && m.controls.toggle {
            hits.add(Element::MusicPlay, r, design::R_CONTROL);
        }
        x += design::BUTTON_PLAY + design::BUTTON_GAP;
        // Next.
        let r = RectD::new(x, y, design::BUTTON_PREV, design::BUTTON_H);
        self.control_button(
            rt,
            input,
            Element::MusicNext,
            r,
            IconId::Next,
            design::CONTROL_GLYPH,
            theme.text_primary(),
            None,
            alpha,
            m.controls.next,
            hits,
        );
        x += design::BUTTON_PREV + design::BUTTON_GAP;
        // Repeat.
        let r = RectD::new(x, y, design::BUTTON_H_SMALL, design::BUTTON_H);
        self.control_button(
            rt,
            input,
            Element::MusicRepeat,
            r,
            IconId::Repeat,
            design::CONTROL_GLYPH_SMALL,
            if m.repeat != RepeatMode::None {
                theme.text_primary()
            } else {
                design::white(0.60)
            },
            if m.repeat != RepeatMode::None {
                Some(design::white(design::SURFACE_BUTTON_A))
            } else {
                None
            },
            alpha,
            m.controls.repeat,
            hits,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn control_button(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        element: Element,
        r: RectD,
        icon: IconId,
        glyph: f32,
        icon_color: Color,
        active_fill: Option<Color>,
        alpha: f32,
        enabled: bool,
        hits: &mut HitMap,
    ) {
        let pressed = input.pressed_amount(element);
        let draw = pressed_rect(r, pressed);
        let fill = if pressed.is_some() {
            input.theme.surface_button_press()
        } else if input.hovered(element) && enabled {
            design::white(0.10)
        } else if let Some(fill) = active_fill {
            fill
        } else {
            Color::TRANSPARENT
        };
        self.fill_round(rt, draw, design::R_CONTROL, fill.with_alpha(fill.a * alpha));
        let c = if enabled {
            icon_color.with_alpha(icon_color.a * alpha)
        } else {
            icon_color.with_alpha(icon_color.a * 0.35 * alpha)
        };
        self.icon_center(rt, icon, draw, glyph, c, input.scale, 0.0);
        if enabled && alpha > 0.15 {
            hits.add(element, r, design::R_CONTROL);
        }
    }

    fn icon_scaled(
        &mut self,
        rt: &ID2D1RenderTarget,
        icon: IconId,
        rect: RectD,
        size: f32,
        color: Color,
        scale: f32,
        k: f32,
    ) {
        let s = size * k;
        self.icon_at(
            rt,
            icon,
            rect.center_x() - s * 0.5,
            rect.center_y() - s * 0.5,
            s,
            color,
            scale,
            0.0,
        );
    }

    fn draw_call(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        c: &crate::view::CallBody,
        body: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let top = RectD::new(
            body.x,
            body.y + (design::BODY_H - design::CALL_BODY_H) * 0.5,
            body.w,
            design::CALL_BODY_TOP,
        );
        self.icon_at(
            rt,
            IconId::Signal,
            top.x,
            top.y + (top.h - 14.0) * 0.5,
            14.0,
            design::CALL_GREEN.with_alpha(alpha),
            input.scale,
            design::ICON_STROKE,
        );
        self.text(
            rt,
            &c.device,
            RectD::new(top.x + 22.0, top.y, top.w - 22.0, top.h),
            design::FONT_FLY_SUBTITLE,
            400,
            0,
            design::LINE_FLY_SUBTITLE,
            theme.text_secondary().with_alpha(alpha),
            true,
        );
        let row = RectD::new(
            body.x,
            top.y + design::CALL_BODY_TOP + design::CALL_BODY_GAP,
            body.w,
            design::BUTTON_CALL,
        );
        let mic = self.call_button(
            rt,
            input,
            Element::CallMic,
            RectD::new(row.x, row.y, design::BUTTON_CALL, design::BUTTON_CALL),
            if c.mic_muted {
                IconId::MicMuted
            } else {
                IconId::Mic
            },
            c.mic_muted,
            alpha,
            hits,
        );
        let output = self.call_button(
            rt,
            input,
            Element::CallOutput,
            RectD::new(
                mic.right() + design::BUTTON_GAP,
                row.y,
                design::BUTTON_CALL,
                design::BUTTON_CALL,
            ),
            if c.output_muted {
                IconId::SpeakerMuted
            } else {
                IconId::Speaker
            },
            c.output_muted,
            alpha,
            hits,
        );
        let open_x = output.right() + design::BUTTON_GAP;
        let open_r = RectD::new(
            open_x,
            row.y,
            (row.right() - open_x).max(0.0),
            design::BUTTON_CALL,
        );
        let pressed = input.pressed_amount(Element::CallOpen);
        let draw = pressed_rect(open_r, pressed);
        let fill = if pressed.is_some() {
            theme.surface_button_press()
        } else if input.hovered(Element::CallOpen) {
            theme.surface_button_hover()
        } else {
            theme.surface_button()
        };
        self.fill_round(rt, draw, design::R_CONTROL, fill.with_alpha(alpha));
        self.text(
            rt,
            &format!("Open {}", c.app_name),
            draw,
            design::FONT_BUTTON,
            600,
            1,
            design::LINE_HINT,
            theme.text_primary().with_alpha(alpha),
            true,
        );
        if alpha > 0.15 {
            hits.add(Element::CallOpen, open_r, design::R_CONTROL);
        }
        let _ = (input, body);
    }

    #[allow(clippy::too_many_arguments)]
    fn call_button(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        element: Element,
        r: RectD,
        icon: IconId,
        active: bool,
        alpha: f32,
        hits: &mut HitMap,
    ) -> RectD {
        let pressed = input.pressed_amount(element);
        let draw = pressed_rect(r, pressed);
        let (fill, icon_color) = if active {
            (
                if input.hovered(element) {
                    design::white(0.90)
                } else {
                    design::white(1.0)
                },
                design::DARK_GLYPH,
            )
        } else if pressed.is_some() {
            (
                input.theme.surface_button_press(),
                input.theme.text_primary(),
            )
        } else if input.hovered(element) {
            (
                input.theme.surface_button_hover(),
                input.theme.text_primary(),
            )
        } else {
            (input.theme.surface_button(), input.theme.text_primary())
        };
        self.fill_round(rt, draw, design::R_CONTROL, fill.with_alpha(alpha));
        self.icon_center(
            rt,
            icon,
            draw,
            design::CONTROL_GLYPH,
            icon_color.with_alpha(alpha),
            input.scale,
            0.0,
        );
        if alpha > 0.15 {
            hits.add(element, r, design::R_CONTROL);
        }
        r
    }

    fn draw_shelf(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        s: &crate::view::ShelfBody,
        body: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let gap = design::SHELF_TILE_GAP;
        let tile_w = (body.w - gap * 2.0) / 3.0;
        let row = RectD::new(body.x, body.y, body.w, design::SHELF_TILE_H);
        for (i, file) in s.files.iter().take(3).enumerate() {
            let r = RectD::new(
                row.x + i as f32 * (tile_w + gap),
                row.y,
                tile_w,
                design::SHELF_TILE_H,
            );
            let pressed = input.pressed_amount(Element::ShelfTile(i));
            let draw = pressed_rect(r, pressed);
            let fill = if pressed.is_some() {
                theme.surface_button_press()
            } else if input.hovered(Element::ShelfTile(i)) {
                design::white(design::SHELF_TILE_HOVER_A)
            } else {
                design::white(design::SHELF_TILE_A)
            };
            self.fill_round(rt, draw, design::R_CONTROL, fill.with_alpha(alpha));
            let px = 1.0 / input.scale;
            self.stroke_round(
                rt,
                draw.inset(px * 0.5),
                design::R_CONTROL,
                px,
                design::white(design::SHELF_TILE_BORDER_A),
                false,
            );
            let badge_w = self.measure(rt, &file.badge, design::FONT_BADGE, 700, design::LINE_HINT)
                + design::BADGE_PAD_X * 2.0;
            let badge = RectD::new(
                draw.x + design::SHELF_TILE_PAD,
                draw.y + design::SHELF_TILE_PAD,
                badge_w,
                design::BADGE_H,
            );
            self.fill_round(
                rt,
                badge,
                design::R_BADGE,
                file.badge_color.with_alpha(alpha),
            );
            self.text(
                rt,
                &file.badge,
                badge,
                design::FONT_BADGE,
                700,
                1,
                design::BADGE_H,
                design::DARK_GLYPH.with_alpha(alpha),
                false,
            );
            let size_y = draw.bottom() - design::SHELF_TILE_PAD - design::LINE_HINT;
            let name_y = size_y - 2.0 - design::LINE_PILL;
            self.text(
                rt,
                &file.name,
                RectD::new(
                    draw.x + design::SHELF_TILE_PAD,
                    name_y,
                    draw.w - design::SHELF_TILE_PAD * 2.0,
                    design::LINE_PILL,
                ),
                design::FONT_FILE_NAME,
                500,
                0,
                design::LINE_PILL,
                theme.text_primary().with_alpha(alpha),
                true,
            );
            self.text(
                rt,
                &file.size,
                RectD::new(
                    draw.x + design::SHELF_TILE_PAD,
                    size_y,
                    draw.w - design::SHELF_TILE_PAD * 2.0,
                    design::LINE_HINT,
                ),
                design::FONT_FILE_SIZE,
                400,
                0,
                design::LINE_HINT,
                theme.text_secondary().with_alpha(alpha),
                true,
            );
            hits.add(Element::ShelfTile(i), r, design::R_CONTROL);
        }
        let hint_y = row.bottom() + design::SHELF_HINT_GAP;
        self.text(
            rt,
            "Drag a file out to move it. Click to open.",
            RectD::new(body.x, hint_y, body.w, design::SHELF_HINT_H),
            design::FONT_HINT,
            400,
            0,
            design::LINE_HINT,
            theme.text_tertiary().with_alpha(alpha),
            true,
        );
    }

    fn draw_timer(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        t: &crate::view::TimerBody,
        body: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let progress = input.progress.unwrap_or(ProgressVisual {
            fraction: t.fraction,
            phase: 0.0,
            amplitude: if t.paused {
                design::WAVE_AMP * design::WAVE_FLAT
            } else {
                design::WAVE_AMP
            },
            playing: !t.paused,
        });
        self.draw_progress_row(
            rt, input, body, progress, &t.elapsed, &t.total, alpha, hits, false,
        );
        let row_y = body.y
            + (design::BODY_H - design::TIMER_BODY_H) * 0.5
            + design::PROGRESS_H
            + design::MUSIC_ROW_GAP;
        let add_w = self.measure(rt, "+1 min", design::FONT_BUTTON, 500, design::LINE_HINT) + 32.0;
        let total = design::TIMER_TOGGLE_W
            + design::BUTTON_GAP
            + add_w
            + design::BUTTON_GAP
            + design::TIMER_STOP_W;
        let mut x = body.center_x() - total * 0.5;
        let y = row_y;
        // Pause / resume.
        let r = RectD::new(x, y, design::TIMER_TOGGLE_W, design::BUTTON_H);
        let draw = pressed_rect(r, input.pressed_amount(Element::TimerToggle));
        let fill = if input.hovered(Element::TimerToggle) {
            darken(design::TIMER_AMBER, 0.90)
        } else {
            design::TIMER_AMBER
        };
        self.fill_round(rt, draw, design::R_CONTROL, fill.with_alpha(alpha));
        self.icon_center(
            rt,
            if t.paused {
                IconId::Play
            } else {
                IconId::Pause
            },
            draw,
            design::CONTROL_GLYPH,
            design::DARK_GLYPH.with_alpha(alpha),
            input.scale,
            0.0,
        );
        hits.add(Element::TimerToggle, r, design::R_CONTROL);
        x += design::TIMER_TOGGLE_W + design::BUTTON_GAP;
        // +1 minute.
        let r = RectD::new(x, y, add_w, design::BUTTON_H);
        self.small_label_button(rt, input, Element::TimerAdd, r, "+1 min", alpha, hits);
        x += add_w + design::BUTTON_GAP;
        // Stop.
        let r = RectD::new(x, y, design::TIMER_STOP_W, design::BUTTON_H);
        let draw = pressed_rect(r, input.pressed_amount(Element::TimerStop));
        let fill = if input.hovered(Element::TimerStop) {
            input.theme.surface_button_hover()
        } else {
            input.theme.surface_button()
        };
        self.fill_round(rt, draw, design::R_CONTROL, fill.with_alpha(alpha));
        self.icon_center(
            rt,
            IconId::Close,
            draw,
            design::CONTROL_GLYPH_CLOSE,
            input.theme.text_primary().with_alpha(alpha),
            input.scale,
            0.0,
        );
        hits.add(Element::TimerStop, r, design::R_CONTROL);
    }

    fn small_label_button(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        element: Element,
        r: RectD,
        label: &str,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let draw = pressed_rect(r, input.pressed_amount(element));
        let fill = if input.pressed_amount(element).is_some() {
            input.theme.surface_button_press()
        } else if input.hovered(element) {
            input.theme.surface_button_hover()
        } else {
            input.theme.surface_button()
        };
        self.fill_round(rt, draw, design::R_CONTROL, fill.with_alpha(alpha));
        self.text(
            rt,
            label,
            draw,
            design::FONT_BUTTON,
            500,
            1,
            design::LINE_HINT,
            input.theme.text_primary().with_alpha(alpha),
            true,
        );
        hits.add(element, r, design::R_CONTROL);
    }

    // ----------------------------------------------------------- decorations

    #[allow(clippy::too_many_arguments)]
    fn draw_ring(
        &mut self,
        rt: &ID2D1RenderTarget,
        cx: f32,
        cy: f32,
        radius: f32,
        stroke: f32,
        fraction: f32,
        accent: Color,
        track: Color,
        scale: f32,
    ) {
        let Ok(accent_brush) = self.brush(rt, accent) else {
            return;
        };
        let Ok(track_brush) = self.brush(rt, track) else {
            return;
        };
        let Ok(style) = self.solid_style(rt) else {
            return;
        };
        let ellipse = D2D1_ELLIPSE {
            point: Vector2::new(cx, cy),
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: ordinary ellipse stroke.
        unsafe {
            let _ = rt.DrawEllipse(&ellipse, &track_brush, stroke, &style);
        }
        let f = fraction.clamp(0.0, 1.0);
        if f <= 0.001 {
            return;
        }
        if f >= 0.999 {
            // SAFETY: same as above.
            unsafe {
                let _ = rt.DrawEllipse(&ellipse, &accent_brush, stroke, &style);
            }
            return;
        }
        let start = -std::f32::consts::FRAC_PI_2;
        let end = start + f * std::f32::consts::TAU;
        let p0 = Vector2::new(cx + radius * start.cos(), cy + radius * start.sin());
        let p1 = Vector2::new(cx + radius * end.cos(), cy + radius * end.sin());
        let Ok(factory) = (unsafe { rt.GetFactory() }) else {
            return;
        };
        let Ok(path) = (unsafe { factory.CreatePathGeometry() }) else {
            return;
        };
        let Ok(sink) = (unsafe { path.Open() }) else {
            return;
        };
        // SAFETY: sink belongs to the path and is closed below.
        unsafe {
            sink.BeginFigure(p0, D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: p1,
                size: D2D_SIZE_F {
                    width: radius,
                    height: radius,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if f > 0.5 {
                    D2D1_ARC_SIZE_LARGE
                } else {
                    D2D1_ARC_SIZE_SMALL
                },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                let _ = rt.DrawGeometry(&path, &accent_brush, stroke, &style);
            }
        }
        let _ = scale;
    }

    fn draw_focus_ring(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        target: HitTarget,
    ) {
        let px = 1.0 / input.scale;
        let r = target.rect.inset(-design::FOCUS_RING_GAP);
        self.stroke_round(
            rt,
            r,
            target.radius + design::FOCUS_RING_GAP,
            design::FOCUS_RING_W * px,
            design::white(0.90),
            false,
        );
    }

    // --------------------------------------------------------------- artwork

    fn art_bitmap(&mut self, rt: &ID2D1RenderTarget, art: &AlbumArt) -> Option<ID2D1Bitmap> {
        if self.art.as_ref().map(|(id, _)| *id) == Some(art.id)
            && self.art_target == rt.as_raw() as isize
        {
            return self.art.as_ref().map(|(_, b)| b.clone());
        }
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        let size = D2D_SIZE_U {
            width: art.width,
            height: art.height,
        };
        // SAFETY: the BGRA buffer is exactly width * height * 4 bytes.
        let bitmap = unsafe {
            rt.CreateBitmap(
                size,
                Some(art.bgra.as_ptr() as *const core::ffi::c_void),
                art.width * 4,
                &props,
            )
            .ok()?
        };
        self.art_target = rt.as_raw() as isize;
        self.art = Some((art.id, bitmap.clone()));
        Some(bitmap)
    }
}

// ---------------------------------------------------------------- helpers

fn d2d_rect(r: RectD) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.right(),
        bottom: r.bottom(),
    }
}

fn round_rect(r: RectD, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: d2d_rect(r),
        radiusX: radius,
        radiusY: radius,
    }
}

fn rounded_layer(
    rt: &ID2D1RenderTarget,
    r: RectD,
    radius: f32,
) -> Option<(ID2D1Layer, ID2D1Geometry)> {
    let factory = unsafe { rt.GetFactory().ok()? };
    let geometry = unsafe {
        factory
            .CreateRoundedRectangleGeometry(&round_rect(r, radius))
            .ok()?
    };
    let layer = unsafe { rt.CreateLayer(None).ok()? };
    let parameters = D2D1_LAYER_PARAMETERS {
        contentBounds: d2d_rect(r),
        geometricMask: ManuallyDrop::new(Some(geometry.cast().ok()?)),
        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        maskTransform: Matrix3x2::identity(),
        opacity: 1.0,
        opacityBrush: ManuallyDrop::new(None),
        layerOptions: D2D1_LAYER_OPTIONS_NONE,
    };
    // SAFETY: all layer resources are alive until the paired pop.
    unsafe { rt.PushLayer(&parameters, &layer) };
    Some((layer, geometry.cast().ok()?))
}

fn drop_rounded_layer(rt: &ID2D1RenderTarget, clip: Option<(ID2D1Layer, ID2D1Geometry)>) {
    if clip.is_some() {
        // SAFETY: balanced with the push in rounded_layer.
        unsafe { rt.PopLayer() };
    }
}

fn pressed_rect(r: RectD, pressed: Option<f32>) -> RectD {
    let Some(t) = pressed else { return r };
    let k = 1.0 - 0.06 * t.clamp(0.0, 1.0);
    RectD::new(
        r.center_x() - r.w * k * 0.5,
        r.center_y() - r.h * k * 0.5,
        r.w * k,
        r.h * k,
    )
}

fn accent_of(view: &ViewModel, pill: &PillView) -> Color {
    let _ = pill;
    view.main
        .as_ref()
        .map(|m| m.accent)
        .unwrap_or(design::VOLUME_WHITE)
}

/// The compact surface remains visible while no activity is active.
fn unreachable_pill() -> &'static PillView {
    use std::sync::OnceLock;
    static PILL: OnceLock<PillView> = OnceLock::new();
    PILL.get_or_init(|| PillView {
        icon: IconId::Folder,
        tile: TileView::colored(design::SHELF_BLUE),
        label: "Shelf".into(),
        value: "empty".into(),
        progress: None,
        kind: PillKind::Normal,
    })
}

fn darken(c: Color, k: f32) -> Color {
    Color {
        r: c.r * k,
        g: c.g * k,
        b: c.b * k,
        a: c.a,
    }
}

fn wave_geometry(
    rt: &ID2D1RenderTarget,
    x: f32,
    cy: f32,
    width: f32,
    phase: f32,
    amplitude: f32,
) -> Option<ID2D1PathGeometry> {
    if width <= 0.5 {
        return None;
    }
    let factory = unsafe { rt.GetFactory().ok()? };
    let path = unsafe { factory.CreatePathGeometry().ok()? };
    let sink = unsafe { path.Open().ok()? };
    let steps = width.ceil().max(2.0) as usize;
    // SAFETY: sink belongs to the path and is closed below.
    unsafe {
        sink.BeginFigure(
            Vector2::new(
                x,
                cy + amplitude * ((phase) * std::f32::consts::TAU / design::WAVE_LEN).sin(),
            ),
            D2D1_FIGURE_BEGIN_HOLLOW,
        );
        for i in 1..=steps {
            let dx = width * i as f32 / steps as f32;
            let y =
                cy + amplitude * (((dx + phase) * std::f32::consts::TAU / design::WAVE_LEN).sin());
            sink.AddLine(Vector2::new(x + dx, y));
        }
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        if sink.Close().is_err() {
            return None;
        }
    }
    Some(path)
}
