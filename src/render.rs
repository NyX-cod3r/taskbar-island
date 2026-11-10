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
    D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1Geometry, ID2D1Layer, ID2D1LinearGradientBrush, ID2D1PathGeometry,
    ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_PROPERTIES, D2D1_CAP_STYLE_FLAT, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_CUSTOM,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS_NONE,
    D2D1_LAYER_PARAMETERS, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_LINE_JOIN_ROUND,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
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

use crate::design::{self, ActivityKind, Color, RectD};
use crate::icons;
use crate::theme::Theme;
use crate::view::{
    AlbumArt, Body, ConvertBody, ConvertState, Element, FlyoutView, IconId, Leading, MusicBody,
    PillKind, PillView, RepeatMode, Trailing, ViewModel,
};

/// What a masked secret row reads as while it is neither hovered nor focused
/// (spec 3.6): eight bullets, never the stored text.
const MASKED_PREVIEW: &str = "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}";

// --------------------------------------------------------------- draw input

#[derive(Debug, Clone, Copy)]
pub struct ProgressVisual {
    /// Elapsed fraction 0..1.
    #[allow(dead_code)] // Progress visuals land with the convert/media cards (B2).
    pub fraction: f32,
    pub phase: f32,
    #[allow(dead_code)] // Progress visuals land with the convert/media cards (B2).
    pub amplitude: f32,
}

/// Travelling-wave parameters for the compact pill's progress line.
#[derive(Debug, Clone, Copy)]
pub struct WaveVisual {
    pub phase: f32,
    pub amplitude: f32,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Icon swap animation lands with the pill states (A).
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
    /// Flyout open/close scale about the card centre (1.0 at rest).
    pub flyout_scale: f32,
    /// Vertical offset in DIP (the card rises during open, drops during close).
    pub flyout_dy: f32,
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
    /// Wave state of the compact pill's progress line (music/timer).
    pub pill_wave: Option<WaveVisual>,
    #[allow(dead_code)] // Pill icon swap animation lands with pill states (A).
    pub icon_swap: Option<IconSwap>,
    /// Selected rail button, which may run ahead of the fading content while
    /// an activity swap is in flight.
    pub rail_current: Option<ActivityKind>,
    /// Previous rail selection while its indicator slides.
    pub rail_from: Option<ActivityKind>,
    /// Eased 0..1 progress of the rail indicator slide.
    pub rail_t: f32,
    /// Highlight the shelf drop zone while a file drag hovers the card.
    pub drag_over: bool,
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
    /// `0` = the UI family, `1` = the monospace family (code-like clipboard
    /// previews, spec 3.2). Part of the key so both caches stay distinct.
    family: u8,
}

/// `FormatKey::family` for `design::FONT_FAMILY`.
const FAMILY_UI: u8 = 0;
/// `FormatKey::family` for `design::FONT_MONO`.
const FAMILY_MONO: u8 = 1;

#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    text: String,
    format: FormatKey,
    max_x10: i32,
    ellipsis: bool,
}

#[derive(Default)]
pub struct Painter {
    formats: HashMap<FormatKey, IDWriteTextFormat>,
    layouts: HashMap<LayoutKey, IDWriteTextLayout>,
    icons: HashMap<IconId, ID2D1PathGeometry>,
    app_icons: HashMap<(icons::AppIcon, usize), ID2D1PathGeometry>,
    app_fills: HashMap<icons::AppIcon, ID2D1LinearGradientBrush>,
    app_shines: HashMap<icons::AppIcon, ID2D1LinearGradientBrush>,
    ellipsis: Option<IDWriteInlineObject>,
    round_stroke: Option<ID2D1StrokeStyle>,
    dash_stroke: Option<ID2D1StrokeStyle>,
    target_key: isize,
    brushes: HashMap<(u32, u32, u32, u32), ID2D1SolidColorBrush>,
    art_target: isize,
    /// Album art and shelf thumbnails share one cache, keyed by `AlbumArt::id`
    /// (a track id or a path/mtime hash).
    art: HashMap<u64, ID2D1Bitmap>,
    placeholder: Option<ID2D1LinearGradientBrush>,
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
            self.app_fills.clear();
            self.app_shines.clear();
            self.placeholder = None;
        }
        if self.art_target != key {
            self.art_target = key;
            self.art.clear();
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
        // The island only exists while something is live. With no view model
        // the window stays fully transparent and click-through.
        if let Some(pill) = input.view.pill() {
            self.draw_pill(rt, input, pill, &mut hits);
        }
        if let Some(second) = &input.view.second {
            if let Some(rect) = input.bubble {
                self.draw_bubble(rt, input, second, rect, &mut hits);
            }
        }
        if let (Some(mut fly), Some(flyout)) = (input.flyout, input.view.flyout()) {
            fly.y += input.flyout_dy;
            let scale = input.flyout_scale.clamp(0.01, 1.0);
            if (scale - 1.0).abs() > 0.0005 {
                // SAFETY: the render target is valid for this draw call.
                unsafe {
                    let cx = fly.x + fly.w * 0.5;
                    let cy = fly.y + fly.h * 0.5;
                    rt.SetTransform(&Matrix3x2 {
                        M11: input.scale * scale,
                        M22: input.scale * scale,
                        M31: input.scale * cx * (1.0 - scale),
                        M32: input.scale * cy * (1.0 - scale),
                        ..Matrix3x2::identity()
                    });
                }
                self.draw_flyout(rt, input, flyout, fly, &mut hits);
                // SAFETY: restore the base DIP-to-pixel transform afterwards.
                unsafe { rt.SetTransform(&base) };
            } else {
                self.draw_flyout(rt, input, flyout, fly, &mut hits);
            }
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
            rt.DrawRoundedRectangle(&rr, &brush, width, style.as_ref());
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
        // Cascadia Mono for code-like previews (spec 3.2), with the UI family
        // as the last resort so a machine without either monospace font still
        // draws. `CreateTextFormat` accepts an unknown family and lets DWrite
        // substitute, so these chains only matter when the call itself fails.
        let ui: [windows::core::HSTRING; 2] = [
            windows::core::HSTRING::from(design::FONT_FAMILY),
            windows::core::HSTRING::from(design::FONT_FALLBACK),
        ];
        let mono: [windows::core::HSTRING; 4] = [
            windows::core::HSTRING::from(design::FONT_MONO),
            windows::core::HSTRING::from(design::FONT_MONO_FALLBACK),
            ui[0].clone(),
            ui[1].clone(),
        ];
        let chain: &[windows::core::HSTRING] = if spec.family == FAMILY_MONO {
            &mono
        } else {
            &ui
        };
        let mut format = make(&chain[0]);
        for family in &chain[1..] {
            if format.is_err() {
                format = make(family);
            }
        }
        let format = format?;
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
                    family: FAMILY_UI,
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
                family: FAMILY_UI,
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
        self.text_family(
            rt, text, rect, size, weight, align, line, color, ellipsis, FAMILY_UI,
        )
    }

    /// `text` in an explicit font family (`FAMILY_MONO` for code-like
    /// clipboard previews, spec 3.2).
    #[allow(clippy::too_many_arguments)]
    fn text_family(
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
        family: u8,
    ) -> f32 {
        if rect.w <= 0.0 || rect.h <= 0.0 || color.a <= 0.0 {
            return 0.0;
        }
        let spec = FormatKey {
            size_x100: (size * 100.0) as i32,
            weight,
            align,
            line_x100: (line * 100.0) as i32,
            family,
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
                rt.FillGeometry(&geometry, &brush, None);
            } else {
                let style = self.solid_style(rt).ok();
                rt.DrawGeometry(
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

    #[allow(clippy::too_many_arguments)]
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

    // ------------------------------------------------------------ app icons

    fn app_geometry(
        &mut self,
        rt: &ID2D1RenderTarget,
        icon: icons::AppIcon,
        layer: usize,
    ) -> Option<ID2D1PathGeometry> {
        if let Some(g) = self.app_icons.get(&(icon, layer)) {
            return Some(g.clone());
        }
        let factory = unsafe { rt.GetFactory().ok()? };
        let geometry = icons::build_app(&factory, icon, layer).ok()?;
        self.app_icons.insert((icon, layer), geometry.clone());
        Some(geometry)
    }

    fn app_gradient(
        &mut self,
        rt: &ID2D1RenderTarget,
        icon: icons::AppIcon,
    ) -> Option<ID2D1LinearGradientBrush> {
        if let Some(brush) = self.app_fills.get(&icon) {
            return Some(brush.clone());
        }
        let spec = icons::app_spec(icon);
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: color_f(spec.top),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: color_f(spec.bottom),
            },
        ];
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            // Start and end are the SVG's own gradient axis in 64-unit space.
            startPoint: Vector2::new(8.0, 4.0),
            endPoint: Vector2::new(56.0, 60.0),
        };
        // SAFETY: stops and properties are local and valid for the call.
        let brush = unsafe {
            let collection = rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                .ok()?;
            rt.CreateLinearGradientBrush(&props, None, &collection)
                .ok()?
        };
        self.app_fills.insert(icon, brush.clone());
        Some(brush)
    }

    fn app_shine(
        &mut self,
        rt: &ID2D1RenderTarget,
        icon: icons::AppIcon,
    ) -> Option<ID2D1LinearGradientBrush> {
        if let Some(brush) = self.app_shines.get(&icon) {
            return Some(brush.clone());
        }
        let spec = icons::app_spec(icon);
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: color_f(design::white(spec.shine)),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: color_f(design::white(0.0)),
            },
        ];
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: Vector2::new(32.0, 0.0),
            endPoint: Vector2::new(32.0, 32.0),
        };
        // SAFETY: stops and properties are local and valid for the call.
        let brush = unsafe {
            let collection = rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                .ok()?;
            rt.CreateLinearGradientBrush(&props, None, &collection)
                .ok()?
        };
        self.app_shines.insert(icon, brush.clone());
        Some(brush)
    }

    /// Album-art placeholder gradient (`135deg #2B4A52 -> #101C20`), defined
    /// in unit space so callers can stretch it across any rect.
    fn placeholder_gradient(&mut self, rt: &ID2D1RenderTarget) -> Option<ID2D1LinearGradientBrush> {
        if let Some(brush) = &self.placeholder {
            return Some(brush.clone());
        }
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: color_f(design::rgb(0x2B, 0x4A, 0x52)),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: color_f(design::rgb(0x10, 0x1C, 0x20)),
            },
        ];
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: Vector2::new(0.0, 0.0),
            endPoint: Vector2::new(1.0, 1.0),
        };
        // SAFETY: stops and properties are local and valid for the call.
        let brush = unsafe {
            let collection = rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                .ok()?;
            rt.CreateLinearGradientBrush(&props, None, &collection)
                .ok()?
        };
        self.placeholder = Some(brush.clone());
        Some(brush)
    }

    /// Draws one of the four activity identity tiles (call/music/shelf/timer
    /// SVGs at 64 units): gradient square, sheen, then the white glyph.
    fn draw_app_icon(
        &mut self,
        rt: &ID2D1RenderTarget,
        icon: icons::AppIcon,
        r: RectD,
        scale: f32,
        alpha: f32,
    ) {
        if r.w <= 0.0 || r.h <= 0.0 || alpha <= 0.001 {
            return;
        }
        let spec = icons::app_spec(icon);
        let side = r.w.min(r.h);
        let s = (side / 64.0) * scale;
        let matrix = Matrix3x2 {
            M11: s,
            M22: s,
            M31: r.x * scale,
            M32: r.y * scale,
            ..Matrix3x2::identity()
        };
        let style = self.solid_style(rt).ok();
        let square = d2d_rect(RectD::new(0.0, 0.0, 64.0, 64.0));
        // SAFETY: geometry, brushes and transforms stay local to the icon.
        unsafe {
            rt.SetTransform(&matrix);
            if let Some(brush) = self.app_gradient(rt, icon) {
                brush.SetOpacity(alpha);
                let rr = D2D1_ROUNDED_RECT {
                    rect: square,
                    radiusX: spec.radius,
                    radiusY: spec.radius,
                };
                rt.FillRoundedRectangle(&rr, &brush);
            }
            if spec.shine > 0.0 {
                if let Some(brush) = self.app_shine(rt, icon) {
                    brush.SetOpacity(alpha);
                    // The SVG's sheen is its own rounded rect; a square one
                    // would poke outside the tile's top corners.
                    let rr = D2D1_ROUNDED_RECT {
                        rect: d2d_rect(RectD::new(0.0, 0.0, 64.0, 32.0)),
                        radiusX: spec.radius,
                        radiusY: spec.radius,
                    };
                    rt.FillRoundedRectangle(&rr, &brush);
                }
            }
            for (i, layer) in spec.glyph.iter().enumerate() {
                let Some(geometry) = self.app_geometry(rt, icon, i) else {
                    continue;
                };
                if layer.fill > 0.0 {
                    if let Ok(brush) = self.brush(rt, design::white(alpha * layer.fill)) {
                        rt.FillGeometry(&geometry, &brush, None);
                    }
                }
                if layer.stroke > 0.0 {
                    if let Ok(brush) = self.brush(rt, design::white(alpha)) {
                        rt.DrawGeometry(&geometry, &brush, layer.stroke, style.as_ref());
                    }
                }
            }
            let base = Matrix3x2 {
                M11: scale,
                M22: scale,
                ..Matrix3x2::identity()
            };
            rt.SetTransform(&base);
        }
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
        let accent = accent_of(input.view);
        let open = input.flyout.is_some();
        let pressed = input.pressed_amount(Element::Pill).is_some();
        let active_mix = if open || pressed {
            1.0
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

        let content_x = r.x + design::PILL_PAD + design::PILL_TILE + design::PILL_TILE_GAP;
        let content_right = r.right() - design::PILL_PAD;
        let tile = RectD::new(
            r.x + design::PILL_PAD,
            r.y + (r.h - design::PILL_TILE) * 0.5,
            design::PILL_TILE,
            design::PILL_TILE,
        );
        // Music and timer carry a travelling wave. Its baseline sits on the
        // icon's bottom edge, so the crests have the space below it.
        let wavy = matches!(pill.icon, IconId::Music | IconId::Clock) && pill.progress.is_some();
        let text_dy = if wavy { design::PILL_TEXT_DY } else { 0.0 };

        // Bottom progress line, clipped to the pill's rounded shape. On the
        // music and timer pills it spans the words above it: first letter of
        // the label to the last glyph of the value.
        if let Some(fraction) = pill.progress {
            if pill.kind != PillKind::DragOver {
                let clip = rounded_layer(rt, r, theme.pill_radius());
                if let Some((_layer, _geom)) = &clip {
                    let (line_x, line_w) = if wavy {
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
                        let end = if value_w > 0.0 {
                            content_right
                        } else {
                            let label_w = self
                                .measure(
                                    rt,
                                    &pill.label,
                                    design::FONT_PILL_LABEL,
                                    500,
                                    design::LINE_PILL,
                                )
                                .min(content_right - content_x);
                            content_x + label_w
                        }
                        .clamp(content_x, content_right);
                        (content_x, (end - content_x).max(0.0))
                    } else {
                        (r.x, r.w)
                    };
                    let f = fraction.clamp(0.0, 1.0);
                    if wavy {
                        let cy = tile.bottom() - design::PILL_WAVE_STROKE * 0.5;
                        if let Ok(style) = self.solid_style(rt) {
                            if let Ok(track) = self.brush(rt, theme.track().set_alpha(0.10 * alpha))
                            {
                                // SAFETY: ordinary stroke.
                                unsafe {
                                    rt.DrawLine(
                                        Vector2::new(line_x, cy),
                                        Vector2::new(line_x + line_w, cy),
                                        &track,
                                        design::PILL_WAVE_STROKE,
                                        &style,
                                    );
                                }
                            }
                            if line_w * f > 0.5 {
                                if let Some(wave) = input.pill_wave {
                                    if let Ok(fill) =
                                        self.brush(rt, accent.with_alpha(0.90 * alpha))
                                    {
                                        if let Some(path) = wave_geometry(
                                            rt,
                                            line_x,
                                            cy,
                                            line_w * f,
                                            wave.phase,
                                            wave.amplitude,
                                        ) {
                                            // SAFETY: path/brush/style are alive.
                                            unsafe {
                                                rt.DrawGeometry(
                                                    &path,
                                                    &fill,
                                                    design::PILL_WAVE_STROKE,
                                                    Some(&style),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        let line_y = r.bottom() - design::PILL_PROGRESS_INSET;
                        let track_color = theme.track().set_alpha(0.10 * alpha);
                        self.fill_rect(
                            rt,
                            RectD::new(line_x, line_y, line_w, design::PILL_PROGRESS_H),
                            track_color,
                        );
                        self.fill_rect(
                            rt,
                            RectD::new(line_x, line_y, line_w * f, design::PILL_PROGRESS_H),
                            accent.with_alpha(0.90 * alpha),
                        );
                    }
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

        // Tile: the music pill shows the album cover when one is available;
        // otherwise the full-color identity icon (or the tinted tile glyph).
        let bob = if pill.kind == PillKind::DragOver {
            -2.0 * ((input.drag_bob * std::f32::consts::TAU).sin() * 0.5 + 0.5)
        } else {
            0.0
        };
        let tile = RectD::new(tile.x, tile.y + bob, tile.w, tile.h);
        let drew_art = pill
            .art
            .as_ref()
            .map(|art| {
                self.draw_art_tile(rt, art, tile, design::R_TILE_PILL, pill.tile.color, alpha)
            })
            .unwrap_or(false);
        if !drew_art {
            if let Some(app) = icons::AppIcon::of(pill.icon) {
                self.draw_app_icon(rt, app, tile, input.scale, alpha);
            } else {
                let tile_color = pill.tile.color.with_alpha(pill.tile.alpha * alpha);
                self.fill_round(rt, tile, design::R_TILE_PILL, tile_color);
                let glyph_color = pill.tile.glyph.with_alpha(alpha);
                self.icon_at(
                    rt,
                    pill.icon,
                    tile.x + (tile.w - design::PILL_GLYPH) * 0.5,
                    tile.y + (tile.h - design::PILL_GLYPH) * 0.5,
                    design::PILL_GLYPH,
                    glyph_color,
                    input.scale,
                    design::ICON_STROKE,
                );
            }
        }
        if matches!(pill.kind, PillKind::Clipboard { paused: true }) {
            let dot = RectD::new(
                tile.right() - design::CLIP_PAUSED_DOT,
                tile.y,
                design::CLIP_PAUSED_DOT,
                design::CLIP_PAUSED_DOT,
            );
            self.fill_round(
                rt,
                dot,
                design::CLIP_PAUSED_DOT * 0.5,
                design::RAIL_TIMER.with_alpha(alpha),
            );
        }

        let mut value_w = if pill.value.is_empty() {
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
        if pill.icon == IconId::Music && value_w > 0.0 {
            // A long artist must not run under the icon and hide the
            // title; give the value at most 60 % of the content width.
            value_w = value_w.min((content_right - content_x) * 0.6);
        }
        let value_x = content_right - value_w;
        let label_w = (value_x - content_x - if value_w > 0.0 { 8.0 } else { 0.0 }).max(0.0);
        self.text(
            rt,
            &pill.label,
            RectD::new(content_x, r.y + text_dy, label_w, r.h),
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
                RectD::new(value_x, r.y + text_dy, value_w, r.h),
                design::FONT_PILL_VALUE,
                600,
                0,
                design::LINE_PILL,
                theme.pill_value(accent).with_alpha(alpha),
                pill.icon == IconId::Music,
            );
        }

        // Hover sheen: flat strips, no gradient, clipped by the rounded pill.
        if let Some(t) = input.sheen {
            if active_mix > 0.0 && (0.0..=1.0).contains(&t) {
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
        let fill = theme.fly_fill().with_alpha(alpha);
        self.fill_round(rt, fly, design::R_SURFACE, fill);

        let clip = rounded_layer(rt, fly, design::R_SURFACE);
        self.draw_rail(
            rt,
            input,
            input.rail_current.unwrap_or(view.kind),
            fly,
            alpha,
            hits,
        );
        self.draw_card(rt, input, view, fly, hits);
        drop_rounded_layer(rt, clip);

        let px = 1.0 / input.scale;
        self.stroke_round(
            rt,
            fly.inset(px * 0.5),
            design::R_SURFACE,
            px,
            theme.fly_border().with_alpha(alpha),
            false,
        );
    }

    // ------------------------------------------------------------------ rail

    /// 56 DIP activity rail: five 32 x 32 icon buttons, the active indicator
    /// and the 1 DIP divider. Never scaled or moved by content changes.
    fn draw_rail(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        current: ActivityKind,
        fly: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let scale = input.scale;
        let rail = RectD::new(fly.x, fly.y, design::RAIL_W, fly.h);
        self.fill_rect(rt, rail, theme.card_rail().with_alpha(alpha));
        let divider_x0 = snap(fly.x + design::RAIL_W, scale);
        let divider_x1 = snap(fly.x + design::RAIL_W + design::RAIL_DIVIDER, scale);
        self.fill_rect(
            rt,
            RectD::new(
                divider_x0,
                fly.y,
                (divider_x1 - divider_x0).max(1.0 / scale),
                fly.h,
            ),
            theme.card_divider().with_alpha(alpha),
        );

        let stroke_units = design::RAIL_STROKE * 24.0 / design::RAIL_GLYPH;
        for (index, kind) in design::RAIL_ORDER.iter().enumerate() {
            let kind = *kind;
            let rect = snap_rect(rail_button_rect(fly, index), scale);
            let element = Element::Rail(kind);
            let pressed = input.pressed_amount(element).is_some();
            let active = kind == current;
            let fill = if pressed {
                theme.rail_press_fill()
            } else if active {
                theme.rail_active_fill()
            } else if input.hovered(element) {
                theme.rail_hover_fill()
            } else {
                Color::TRANSPARENT
            };
            self.fill_round(rt, rect, design::R_SURFACE, fill.with_alpha(alpha));
            self.icon_center(
                rt,
                rail_icon(kind),
                rect,
                design::RAIL_GLYPH,
                theme.rail_glyph(active).with_alpha(alpha),
                scale,
                stroke_units,
            );
            hits.add(element, rect, design::R_SURFACE);
        }

        // Indicator slides between button centres in 160 ms. It is drawn last
        // so the rounded glyph never paints over it.
        let target = rail_index(current) as f32;
        let from = input
            .rail_from
            .map(rail_index)
            .map(|i| i as f32)
            .unwrap_or(target);
        let pos = from + (target - from) * input.rail_t.clamp(0.0, 1.0);
        let top = fly.y + design::RAIL_INSET + pos * (design::RAIL_BTN + design::RAIL_GAP);
        let indicator = RectD::new(
            snap(fly.x, scale),
            snap(top + design::RAIL_IND_DY, scale),
            design::RAIL_IND_W,
            design::RAIL_IND_H,
        );
        self.fill_round(
            rt,
            indicator,
            design::RAIL_IND_R,
            theme
                .rail_indicator(design::rail_accent(current))
                .with_alpha(alpha),
        );
    }

    // ------------------------------------------------------------------ card

    fn draw_card(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        view: &FlyoutView,
        fly: RectD,
        hits: &mut HitMap,
    ) {
        let top_alpha = input.top_alpha.clamp(0.0, 1.0);
        let body_alpha = input.body_alpha.clamp(0.0, 1.0);
        let content = crate::view::content_rect(fly);
        if let Body::Music(music) = &view.body {
            let alpha = top_alpha.min(body_alpha);
            if alpha > 0.002 {
                self.draw_music_card(rt, input, music, content, input.body_dy, alpha, hits);
            }
            return;
        }
        if let Body::Convert(convert) = &view.body {
            let alpha = top_alpha.min(body_alpha);
            if alpha > 0.002 {
                // The card owns the shared content column from
                // `card_content_rect()` and lays out inside it.
                let mut area = content;
                area.y += input.top_dy;
                self.draw_convert_card(rt, input, convert, area, alpha, hits);
            }
            return;
        }
        // v0.6 shelf: no standard header. The card owns the whole content
        // column and lays out its own 28 DIP toolbar.
        if let Body::Shelf(shelf) = &view.body {
            let alpha = top_alpha.min(body_alpha);
            if alpha > 0.002 {
                self.draw_shelf_card(rt, input, shelf, fly, alpha, hits);
            }
            return;
        }
        // Clipboard: the same shared content column as the shelf. Phase 1
        // draws only the empty-state glyph; the list arrives in a later phase.
        if let Body::Clipboard(clip) = &view.body {
            let alpha = top_alpha.min(body_alpha);
            if alpha > 0.002 {
                let mut area = content;
                area.y += input.top_dy;
                self.draw_clipboard_card(rt, input, clip, area, alpha, hits);
            }
            return;
        }
        let (gap, body_h) = match &view.body {
            Body::Timer(_) => (
                design::CARD_GAP,
                design::LINE_TIMER_READOUT
                    + design::CARD_GAP
                    + design::TIMER_PROGRESS_H
                    + design::CARD_GAP
                    + design::TIMER_CTRL_H,
            ),
            Body::Call(_) => (
                design::CARD_GAP,
                design::CALL_STATUS_H + design::CARD_GAP + design::CALL_BTN_H,
            ),
            Body::Shelf(_) | Body::Music(_) | Body::Clipboard(_) | Body::Convert(_) => {
                unreachable!()
            }
        };
        let total = design::HEADER_H + gap + body_h;
        let start_y = content.y + (content.h - total) * 0.5 + input.top_dy;
        let header = RectD::new(content.x, start_y, content.w, design::HEADER_H);
        if top_alpha > 0.002 {
            self.draw_card_header(rt, input, view, header, top_alpha, hits);
        }
        if body_alpha <= 0.002 {
            return;
        }
        let body = RectD::new(
            content.x,
            start_y + design::HEADER_H + gap,
            content.w,
            body_h,
        );
        match &view.body {
            Body::Timer(t) => self.draw_timer_card(rt, input, t, body, body_alpha, hits),
            Body::Call(c) => self.draw_call_card(rt, input, c, body, body_alpha, hits),
            Body::Shelf(_) | Body::Music(_) | Body::Clipboard(_) | Body::Convert(_) => {
                unreachable!()
            }
        }
    }

    /// Clipboard history card.  This deliberately mirrors the shelf's
    /// icon-only toolbar so every affordance is available in the empty and
    /// paused states too.
    fn draw_clipboard_card(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        body: &crate::view::ClipboardBody,
        rect: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let toolbar = RectD::new(rect.x, rect.y, rect.w, design::CLIP_TOOLBAR_H);
        let mut x = toolbar.x + design::CLIP_FILTER_PAD;
        for filter in crate::view::ClipFilter::ALL {
            let tab = RectD::new(
                x,
                toolbar.y + design::CLIP_FILTER_PAD,
                design::CLIP_FILTER_TAB_W,
                design::CLIP_FILTER_TAB_H,
            );
            let active = body.filter == filter;
            if active || input.hovered(Element::ClipFilter(filter)) {
                self.fill_round(
                    rt,
                    tab,
                    design::R_SURFACE,
                    if active {
                        design::CLIP_FILTER_ACTIVE.with_alpha(alpha)
                    } else {
                        theme.surface_button_hover().with_alpha(alpha)
                    },
                );
            }
            self.icon_center(
                rt,
                filter.icon(),
                tab,
                design::CLIP_FILTER_GLYPH,
                theme.text_primary().with_alpha(alpha),
                input.scale,
                design::ICON_STROKE,
            );
            hits.add(Element::ClipFilter(filter), tab, design::R_SURFACE);
            x += design::CLIP_FILTER_TAB_W;
        }
        let clear = RectD::new(
            toolbar.right() - design::CLIP_TOOL_BTN,
            toolbar.y,
            design::CLIP_TOOL_BTN,
            design::CLIP_TOOL_BTN,
        );
        // Armed Clear (spec 3.3): the trash is replaced by a red ✓ with a ✕
        // beside it, and Pause keeps its own slot so the toolbar reads the same
        // in every state (spec 5.6).
        let cancel_slot = body.clear_confirm.is_some();
        let cancel = RectD::new(
            clear.x - design::CLIP_TOOL_GAP - design::CLIP_TOOL_BTN,
            toolbar.y,
            design::CLIP_TOOL_BTN,
            design::CLIP_TOOL_BTN,
        );
        let pause = RectD::new(
            if cancel_slot {
                cancel.x - design::CLIP_TOOL_GAP - design::CLIP_TOOL_BTN
            } else {
                cancel.x
            },
            toolbar.y,
            design::CLIP_TOOL_BTN,
            design::CLIP_TOOL_BTN,
        );
        let pause_fill = body.paused.then_some(design::RAIL_CLIP.with_alpha(alpha));
        self.flat_icon_button(
            rt,
            input,
            Element::ClipPause,
            pause,
            if body.paused {
                IconId::Play
            } else {
                IconId::Pause
            },
            theme.text_primary(),
            pause_fill,
            alpha,
            true,
            hits,
        );
        match body.clear_confirm {
            Some(_) => {
                let danger_fill = Some(design::CLIP_DANGER.with_alpha(0.16 * alpha));
                self.flat_icon_button(
                    rt,
                    input,
                    Element::ClipClearConfirm,
                    clear,
                    IconId::Check,
                    design::CLIP_DANGER,
                    danger_fill,
                    alpha,
                    true,
                    hits,
                );
                self.flat_icon_button(
                    rt,
                    input,
                    Element::ClipClearCancel,
                    cancel,
                    IconId::Close,
                    theme.text_primary(),
                    None,
                    alpha,
                    true,
                    hits,
                );
            }
            None => {
                self.flat_icon_button(
                    rt,
                    input,
                    Element::ClipClear,
                    clear,
                    IconId::Trash,
                    theme.text_primary(),
                    None,
                    alpha,
                    body.total > body.pinned,
                    hits,
                );
            }
        }
        self.text(
            rt,
            &body.count_text,
            RectD::new(x + 4.0, toolbar.y, (pause.x - x - 8.0).max(0.0), toolbar.h),
            design::FONT_BUTTON,
            400,
            1,
            design::LINE_HINT,
            theme.text_tertiary().with_alpha(alpha),
            false,
        );
        let list = RectD::new(
            rect.x,
            rect.y + design::CLIP_TOOLBAR_H + design::CLIP_TOOLBAR_GAP,
            rect.w - design::CLIP_SCROLL_W - design::CLIP_SCROLL_GAP,
            design::CLIP_LIST_H,
        );
        if body.total == 0 {
            let icon = RectD::new(
                list.center_x() - design::CLIP_EMPTY_GLYPH * 0.5,
                list.center_y() - design::CLIP_EMPTY_GLYPH * 0.5,
                design::CLIP_EMPTY_GLYPH,
                design::CLIP_EMPTY_GLYPH,
            );
            let stroke_units = design::CLIP_EMPTY_STROKE * 24.0 / design::CLIP_EMPTY_GLYPH;
            self.icon_center(
                rt,
                IconId::Clipboard,
                icon,
                design::CLIP_EMPTY_GLYPH,
                theme.text_tertiary().with_alpha(alpha),
                input.scale,
                stroke_units,
            );
            if body.total == 0 && body.paused {
                let dot = RectD::new(
                    icon.right() - design::CLIP_PAUSED_DOT,
                    icon.y,
                    design::CLIP_PAUSED_DOT,
                    design::CLIP_PAUSED_DOT,
                );
                self.fill_round(
                    rt,
                    dot,
                    design::CLIP_PAUSED_DOT * 0.5,
                    design::RAIL_TIMER.with_alpha(alpha),
                );
            }
        }
        unsafe { rt.PushAxisAlignedClip(&d2d_rect(list), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        for row in &body.rows {
            // Rows are not a fixed stride (image rows are taller), so each row
            // carries its content-space offset (spec 7).
            let y = list.y + row.y - body.scroll;
            let row_rect = RectD::new(list.x, y, list.w, row.kind.height());
            // The row hit goes in first: `HitMap::hit` scans in reverse, so the
            // action buttons added below have to come after it or every click
            // would resolve to the row (the shelf adds tile then remove the
            // same way). The buttons keep the row active while they are
            // hovered, otherwise moving onto one would make it vanish.
            hits.add(Element::ClipRow(row.id), row_rect, design::CLIP_ROW_R);
            let active = body.active_row == Some(row.index)
                || input.hovered(Element::ClipRow(row.id))
                || matches!(
                    input.hover,
                    Some(Element::ClipPin(id) | Element::ClipCopy(id) | Element::ClipDelete(id))
                        if id == row.id
                );
            if active {
                self.fill_round(
                    rt,
                    row_rect,
                    design::CLIP_ROW_R,
                    design::CLIP_ROW_HOVER.with_alpha(alpha),
                );
            }
            // Leading slot (spec 7): a 64 × 40 image thumbnail — natural size
            // on a checkerboard when the image is smaller than the box,
            // letterboxed cover when it is larger, never stretched — the small
            // 24 × 18 slot for a single image file, a color swatch, or the
            // type glyph. A thumbnail that fails to load falls back to the
            // glyph, so a row is never blank.
            let image_row = row.image;
            let swatch = if let crate::view::ClipRowKind::Color { r, g, b, .. } = &row.kind {
                Some(design::rgb(*r, *g, *b))
            } else {
                None
            };
            let (thumb_w, thumb_h) = if image_row {
                (design::CLIP_ROW_THUMB_W, design::CLIP_ROW_THUMB_H)
            } else {
                (design::CLIP_THUMB_W, design::CLIP_THUMB_H)
            };
            let thumb_rect = RectD::new(
                row_rect.x + design::CLIP_ROW_PAD,
                row_rect.y + (row_rect.h - thumb_h) * 0.5,
                thumb_w,
                thumb_h,
            );
            let mut drew_thumb = false;
            if let Some(art) = row.thumb.as_ref() {
                let clip = rounded_layer(rt, thumb_rect, design::CLIP_ROW_R * 0.66);
                let natural =
                    image_row && art.width as f32 <= thumb_w && art.height as f32 <= thumb_h;
                if natural {
                    // A 37 × 13 image must be obviously tiny: draw it at its
                    // natural size over the checkerboard (spec 7).
                    self.draw_checker(rt, thumb_rect, alpha);
                    let (nw, nh) = (art.width as f32, art.height as f32);
                    drew_thumb = self.draw_thumb(
                        rt,
                        art,
                        RectD::new(
                            thumb_rect.center_x() - nw * 0.5,
                            thumb_rect.center_y() - nh * 0.5,
                            nw,
                            nh,
                        ),
                        alpha,
                    );
                } else {
                    drew_thumb = self.draw_thumb(rt, art, thumb_rect, alpha);
                }
                drop_rounded_layer(rt, clip);
            }
            if !drew_thumb && image_row {
                self.icon_center(
                    rt,
                    IconId::Image,
                    thumb_rect,
                    design::CLIP_GLYPH,
                    theme.text_secondary().with_alpha(alpha),
                    input.scale,
                    design::ICON_STROKE,
                );
            } else if let Some(color) = swatch.filter(|_| !drew_thumb) {
                // Color rows swap the glyph for the swatch; the ring keeps a
                // near-white value readable on a light card (spec 7).
                let ring = RectD::new(
                    row_rect.x + design::CLIP_ROW_PAD,
                    row_rect.y + (row_rect.h - 18.0) * 0.5,
                    18.0,
                    18.0,
                );
                self.fill_round(
                    rt,
                    ring,
                    5.0,
                    theme.text_tertiary().with_alpha(0.35 * alpha),
                );
                self.fill_round(
                    rt,
                    RectD::new(ring.x + 1.0, ring.y + 1.0, 16.0, 16.0),
                    4.0,
                    color.with_alpha(alpha),
                );
            } else if !drew_thumb {
                self.icon_center(
                    rt,
                    row.icon,
                    RectD::new(
                        row_rect.x + design::CLIP_ROW_PAD,
                        row_rect.y + (row_rect.h - design::CLIP_GLYPH) * 0.5,
                        design::CLIP_GLYPH,
                        design::CLIP_GLYPH,
                    ),
                    design::CLIP_GLYPH,
                    if row.link {
                        design::CLIP_LINK.with_alpha(alpha)
                    } else {
                        theme.text_secondary().with_alpha(if row.dimmed {
                            alpha * 0.4
                        } else {
                            alpha
                        })
                    },
                    input.scale,
                    design::ICON_STROKE,
                );
            }
            let leading = if image_row {
                design::CLIP_ROW_THUMB_W + 6.0
            } else if drew_thumb {
                design::CLIP_THUMB_W + 6.0
            } else if swatch.is_some() {
                18.0 + 6.0
            } else {
                design::CLIP_GLYPH_SLOT
            };
            // A pinned row keeps a small pin marker left of the age column.
            let pin_w = if row.pinned && !active {
                design::CLIP_PIN_MARK + design::CLIP_PIN_MARK_GAP
            } else {
                0.0
            };
            let age_w = if active {
                design::CLIP_ACTION_BTN * 3.0 + design::CLIP_ACTION_GAP * 2.0
            } else {
                design::CLIP_AGE_W
            };
            // Masked secrets read as bullets until the row is hovered or
            // focused (spec 3.6); revealing on hover is the whole point of the
            // "masked" state, so `active` decides, not just the focus ring.
            let masked_now = row.masked && !active;
            let preview = if masked_now {
                MASKED_PREVIEW
            } else {
                &row.preview
            };
            let preview_rect = RectD::new(
                row_rect.x + design::CLIP_ROW_PAD + leading,
                row_rect.y,
                (row_rect.w - age_w - pin_w - leading - design::CLIP_ROW_PAD * 2.0).max(0.0),
                row_rect.h,
            );
            let color =
                theme
                    .text_primary()
                    .with_alpha(if row.dimmed { alpha * 0.4 } else { alpha });
            let muted =
                theme
                    .text_tertiary()
                    .with_alpha(if row.dimmed { alpha * 0.4 } else { alpha });
            match &row.kind {
                // Image rows: bold dimensions over a muted `PNG · 48 KB`
                // beside the thumbnail (spec 7).
                crate::view::ClipRowKind::Image { w, h, bytes } if !masked_now => {
                    let top = preview_rect.y + (preview_rect.h - design::LINE_HINT * 2.0) * 0.5;
                    let dims = format!("{w} \u{00d7} {h}");
                    let size_line = format!("PNG \u{00b7} {}", crate::view::fmt_size(*bytes));
                    self.text(
                        rt,
                        &dims,
                        RectD::new(preview_rect.x, top, preview_rect.w, design::LINE_HINT),
                        design::FONT_BUTTON,
                        600,
                        0,
                        design::LINE_HINT,
                        color,
                        false,
                    );
                    self.text(
                        rt,
                        &size_line,
                        RectD::new(
                            preview_rect.x,
                            top + design::LINE_HINT,
                            preview_rect.w,
                            design::LINE_HINT,
                        ),
                        design::FONT_BUTTON,
                        400,
                        0,
                        design::LINE_HINT,
                        muted,
                        true,
                    );
                }
                // Color rows: `#RRGGBB` in mono, then the muted `rgb(…)` form.
                crate::view::ClipRowKind::Color { hex, rgb, .. } if !masked_now => {
                    let hw = self.text_family(
                        rt,
                        hex,
                        preview_rect,
                        design::FONT_BUTTON,
                        500,
                        0,
                        design::LINE_HINT,
                        color,
                        false,
                        FAMILY_MONO,
                    );
                    let mut rgb_rect = preview_rect;
                    rgb_rect.x += hw + 6.0;
                    rgb_rect.w = (preview_rect.w - hw - 6.0).max(0.0);
                    self.text_family(
                        rt,
                        rgb,
                        rgb_rect,
                        design::FONT_BUTTON,
                        400,
                        0,
                        design::LINE_HINT,
                        muted,
                        true,
                        FAMILY_MONO,
                    );
                }
                // Link rows: domain in the accent, the path muted after it.
                crate::view::ClipRowKind::Link { domain, path } if !masked_now => {
                    let link_color =
                        design::CLIP_LINK.with_alpha(if row.dimmed { alpha * 0.4 } else { alpha });
                    let dw = self.text(
                        rt,
                        domain,
                        preview_rect,
                        design::FONT_BUTTON,
                        500,
                        0,
                        design::LINE_HINT,
                        link_color,
                        true,
                    );
                    if !path.is_empty() {
                        let mut path_rect = preview_rect;
                        path_rect.x += dw + 2.0;
                        path_rect.w = (preview_rect.w - dw - 2.0).max(0.0);
                        self.text(
                            rt,
                            path,
                            path_rect,
                            design::FONT_BUTTON,
                            400,
                            0,
                            design::LINE_HINT,
                            muted,
                            true,
                        );
                    }
                }
                // Text/code/file rows: one trimmed line. Code-like previews
                // are set in Cascadia Mono (spec 3.2); without the ellipsis a
                // copied paragraph would paint straight under the age and the
                // Pin/Copy/Delete cluster.
                _ => {
                    if row.mono {
                        self.text_family(
                            rt,
                            preview,
                            preview_rect,
                            design::FONT_BUTTON,
                            400,
                            0,
                            design::LINE_HINT,
                            color,
                            true,
                            FAMILY_MONO,
                        );
                    } else {
                        self.text(
                            rt,
                            preview,
                            preview_rect,
                            design::FONT_BUTTON,
                            400,
                            0,
                            design::LINE_HINT,
                            color,
                            true,
                        );
                    }
                }
            }
            if row.pinned && !active {
                self.icon_center(
                    rt,
                    IconId::Pin,
                    RectD::new(
                        row_rect.right()
                            - design::CLIP_AGE_W
                            - design::CLIP_PIN_MARK_GAP
                            - design::CLIP_PIN_MARK,
                        row_rect.y + (row_rect.h - design::CLIP_PIN_MARK) * 0.5,
                        design::CLIP_PIN_MARK,
                        design::CLIP_PIN_MARK,
                    ),
                    design::CLIP_PIN_MARK,
                    theme.text_tertiary().with_alpha(alpha),
                    input.scale,
                    design::ICON_STROKE,
                );
            }
            if active {
                let mut bx = row_rect.right() - age_w;
                for (element, icon) in [
                    (Element::ClipPin(row.id), IconId::Pin),
                    (Element::ClipCopy(row.id), IconId::Copy),
                    (Element::ClipDelete(row.id), IconId::Trash),
                ] {
                    let button = RectD::new(
                        bx,
                        row_rect.y + 4.0,
                        design::CLIP_ACTION_BTN,
                        design::CLIP_ACTION_BTN,
                    );
                    let enabled = !row.dimmed || !matches!(element, Element::ClipCopy(_));
                    self.flat_icon_button(
                        rt,
                        input,
                        element,
                        button,
                        icon,
                        theme.text_primary(),
                        None,
                        alpha,
                        enabled,
                        hits,
                    );
                    if !enabled {
                        // Dead file row: Copy stays hoverable so its tooltip can
                        // explain the no-op ("File moved or deleted", spec 4).
                        hits.add(element, button, design::R_SURFACE);
                    }
                    bx += design::CLIP_ACTION_BTN + design::CLIP_ACTION_GAP;
                }
            } else {
                self.text(
                    rt,
                    &row.age,
                    RectD::new(
                        row_rect.right() - design::CLIP_AGE_W,
                        row_rect.y,
                        design::CLIP_AGE_W,
                        row_rect.h,
                    ),
                    design::FONT_BUTTON,
                    400,
                    2,
                    design::LINE_HINT,
                    if row.expiring {
                        design::RAIL_TIMER.with_alpha(alpha)
                    } else {
                        theme.text_tertiary().with_alpha(alpha)
                    },
                    false,
                );
            }
        }
        unsafe { rt.PopAxisAlignedClip() };
        if body.max_scroll > 0.0 {
            let track = RectD::new(
                rect.right() - design::CLIP_SCROLL_W,
                list.y,
                design::CLIP_SCROLL_W,
                list.h,
            );
            let thumb_h = (list.h * list.h / body.content_h).clamp(design::CLIP_SCROLL_MIN, list.h);
            let thumb_y = list.y + (list.h - thumb_h) * (body.scroll / body.max_scroll);
            self.fill_round(
                rt,
                RectD::new(track.x, thumb_y, track.w, thumb_h),
                track.w * 0.5,
                theme.text_tertiary().with_alpha(0.55 * alpha),
            );
            hits.add(Element::ClipScrollbar, track, track.w * 0.5);
        }
        // The 450 ms hover preview (spec 7): a 206 × 150 panel, centered
        // horizontally and seated on the toolbar's bottom edge so it covers
        // the toolbar gap and the list and never leaves the card. Its hit
        // maps back to the row it belongs to, which keeps the pointer "on"
        // that row while the panel is up instead of dismissing it.
        if let Some(preview) = &body.preview {
            let card = RectD::new(
                rect.x + (rect.w - design::CLIP_PREVIEW_W) * 0.5,
                rect.y + rect.h - design::CLIP_PREVIEW_H,
                design::CLIP_PREVIEW_W,
                design::CLIP_PREVIEW_H,
            );
            hits.add(Element::ClipRow(preview.id), card, design::TOOLTIP_R);
            self.draw_shadow(rt, card, theme, alpha);
            // Everything inside the rounded panel is clipped to it, so no
            // caption can paint over the card's edge.
            let clip = rounded_layer(rt, card, design::TOOLTIP_R);
            self.fill_round(
                rt,
                card,
                design::TOOLTIP_R,
                theme.fly_fill().set_alpha(alpha),
            );
            self.draw_clipboard_preview(rt, input, preview, card, alpha);
            drop_rounded_layer(rt, clip);
            self.stroke_round(
                rt,
                card,
                design::TOOLTIP_R,
                1.0,
                theme.fly_border().with_alpha(alpha),
                false,
            );
        }
    }

    /// Contents of the hover-preview panel: the 192 × 118 picture over its
    /// caption for image rows, the full (≤ 200 character) text for everything
    /// else, then the `from <app>` line (spec 7).
    fn draw_clipboard_preview(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        preview: &crate::view::ClipPreviewView,
        card: RectD,
        alpha: f32,
    ) {
        let theme = input.theme;
        // One 192 DIP column, horizontally centered, that picture and copy
        // both align to (7 DIP of panel on either side).
        let column_x = card.x + (card.w - design::CLIP_PREVIEW_IMG_W) * 0.5;
        let column_w = design::CLIP_PREVIEW_IMG_W;
        let source_h = if preview.source.is_some() {
            design::CLIP_PREVIEW_SOURCE_H
        } else {
            0.0
        };
        let top = card.y + design::CLIP_PREVIEW_PAD;
        let bottom = card.y + design::CLIP_PREVIEW_H - source_h;
        if preview.image {
            // Picture and caption are one block, centered in whatever the
            // source line leaves: with a source line it lands exactly on the
            // 6 DIP top pad (the bands fill the panel by construction).
            let block = design::CLIP_PREVIEW_IMG_H + design::CLIP_PREVIEW_CAPTION_H;
            let picture_y = top + ((bottom - top) - block).max(0.0) * 0.5;
            let picture = RectD::new(column_x, picture_y, column_w, design::CLIP_PREVIEW_IMG_H);
            self.draw_checker(rt, picture, alpha);
            let drew = match preview.art.as_ref() {
                Some(art) => {
                    // Natural size over the checkerboard when the picture is
                    // smaller than the box, cover-crop when it is larger —
                    // never stretched (spec 7).
                    let (aw, ah) = (art.width as f32, art.height as f32);
                    let (w, h) = if aw <= picture.w && ah <= picture.h {
                        (aw, ah)
                    } else {
                        let scale = (picture.w / aw).min(picture.h / ah);
                        (aw * scale, ah * scale)
                    };
                    self.draw_thumb(
                        rt,
                        art,
                        RectD::new(
                            picture.center_x() - w * 0.5,
                            picture.center_y() - h * 0.5,
                            w,
                            h,
                        ),
                        alpha,
                    )
                }
                // Still decoding: the caption carries the row meanwhile, so
                // the panel is never blank.
                None => false,
            };
            if !drew {
                self.icon_center(
                    rt,
                    IconId::Image,
                    picture,
                    design::CLIP_GLYPH,
                    theme.text_tertiary().with_alpha(alpha),
                    input.scale,
                    design::ICON_STROKE,
                );
            }
            self.text(
                rt,
                &preview.body,
                RectD::new(
                    column_x,
                    picture_y + design::CLIP_PREVIEW_IMG_H,
                    column_w,
                    design::CLIP_PREVIEW_CAPTION_H,
                ),
                design::FONT_BUTTON,
                500,
                0,
                design::LINE_HINT,
                theme.text_primary().with_alpha(alpha),
                true,
            );
        } else {
            self.text_family(
                rt,
                &preview.body,
                RectD::new(column_x, top, column_w, (bottom - top).max(0.0)),
                design::FONT_BUTTON,
                400,
                0,
                design::LINE_HINT,
                theme.text_primary().with_alpha(alpha),
                true,
                if preview.mono { FAMILY_MONO } else { FAMILY_UI },
            );
        }
        if let Some(source) = &preview.source {
            self.text(
                rt,
                &format!("from {source}"),
                RectD::new(
                    column_x,
                    card.y + design::CLIP_PREVIEW_H - design::CLIP_PREVIEW_SOURCE_H,
                    column_w,
                    design::CLIP_PREVIEW_SOURCE_H,
                ),
                design::FONT_HINT,
                400,
                0,
                design::CLIP_PREVIEW_SOURCE_H,
                theme.text_tertiary().with_alpha(alpha),
                true,
            );
        }
    }

    fn draw_card_header(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        view: &FlyoutView,
        rect: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let tile = RectD::new(
            rect.x,
            rect.y + (rect.h - design::HEADER_TILE) * 0.5,
            design::HEADER_TILE,
            design::HEADER_TILE,
        );
        self.draw_leading(rt, input, &view.leading, tile, alpha);

        let mut right = rect.right();
        match &view.trailing {
            Trailing::Close => {
                let side = 28.0;
                let r = RectD::new(right - side, rect.y + (rect.h - side) * 0.5, side, side);
                if input.hovered(Element::ConvertClose) {
                    self.fill_round(
                        rt,
                        r,
                        design::R_CONTROL,
                        theme.surface_button_hover().with_alpha(alpha),
                    );
                }
                self.icon_center(
                    rt,
                    IconId::Close,
                    r,
                    design::CONVERT_CLOSE,
                    theme.text_secondary().with_alpha(alpha),
                    input.scale,
                    0.0,
                );
                hits.add(Element::ConvertClose, r, design::R_CONTROL);
                right = r.x - 8.0;
            }
            Trailing::None => {}
        }
        let title_x = tile.right() + design::HEADER_GAP;
        if !view.status.is_empty() && right - title_x > 12.0 {
            let width = self
                .measure(
                    rt,
                    &view.status,
                    design::FONT_FLY_SUBTITLE,
                    500,
                    design::LINE_CARD_STATUS,
                )
                .min(right - title_x - 8.0);
            self.text(
                rt,
                &view.status,
                RectD::new(right - width, rect.y, width, rect.h),
                design::FONT_FLY_SUBTITLE,
                500,
                2,
                design::LINE_CARD_STATUS,
                theme
                    .card_status(design::rail_accent(view.kind), view.status_accent)
                    .with_alpha(alpha),
                true,
            );
            right -= width + 8.0;
        }
        self.text(
            rt,
            &view.title,
            RectD::new(title_x, rect.y, (right - title_x).max(0.0), rect.h),
            design::FONT_FLY_TITLE,
            600,
            0,
            design::LINE_FLY_TITLE,
            theme.text_primary().with_alpha(alpha),
            true,
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

    fn draw_leading(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        leading: &Leading,
        r: RectD,
        alpha: f32,
    ) {
        match leading {
            Leading::Art { art, accent } => {
                let drew_art = art
                    .as_ref()
                    .map(|art| self.draw_art_tile(rt, art, r, design::R_TILE_LEAD, *accent, alpha))
                    .unwrap_or(false);
                if !drew_art {
                    self.draw_app_icon(rt, icons::AppIcon::Music, r, input.scale, alpha);
                }
            }
            Leading::Glyph { icon, bg, fg } => {
                if let Some(app) = icons::AppIcon::of(*icon) {
                    self.draw_app_icon(rt, app, r, input.scale, alpha);
                } else {
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
            }
        }
    }

    /// Paints `art` as a center-cropped square filling `r`, clipped to
    /// `radius`. Returns false when the bitmap could not be created so the
    /// caller can fall back to an icon.
    fn draw_art_tile(
        &mut self,
        rt: &ID2D1RenderTarget,
        art: &AlbumArt,
        r: RectD,
        radius: f32,
        accent: Color,
        alpha: f32,
    ) -> bool {
        let Some(bitmap) = self.art_bitmap(rt, art) else {
            return false;
        };
        self.fill_round(rt, r, radius, accent.with_alpha(alpha));
        let clip = rounded_layer(rt, r, radius);
        if let Some(_clip) = &clip {
            let side = art.width.min(art.height) as f32;
            let sx = (art.width as f32 - side) * 0.5;
            let sy = (art.height as f32 - side) * 0.5;
            let src = d2d_rect(RectD::new(sx, sy, side, side));
            let dest = d2d_rect(r);
            // SAFETY: bitmap is alive and the target is valid.
            unsafe {
                rt.DrawBitmap(
                    &bitmap,
                    Some(&dest),
                    1.0,
                    windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    Some(&src),
                );
            }
        }
        drop_rounded_layer(rt, clip);
        true
    }

    #[allow(dead_code)] // A's shelf toolbar calls it; ours draws flat buttons.
    #[allow(clippy::too_many_arguments)]
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

    #[allow(clippy::too_many_arguments)]
    fn draw_music_card(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        m: &MusicBody,
        content: RectD,
        dy: f32,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let accent = accent_of(input.view);
        let content = content.offset(0.0, dy);
        let gap = design::CARD_GAP;
        let total = design::MUSIC_ART + gap + design::MUSIC_PROGRESS_H + gap + design::MUSIC_PLAY_H;
        let y0 = content.y + (content.h - total) * 0.5;

        // Info row: 64 DIP art, title, `Artist · App`.
        let art = RectD::new(content.x, y0, design::MUSIC_ART, design::MUSIC_ART);
        let drew = m
            .art
            .as_ref()
            .map(|a| self.draw_art_tile(rt, a, art, design::R_ART, accent, alpha))
            .unwrap_or(false);
        if !drew {
            self.draw_art_placeholder(rt, input, art, alpha);
        }
        let tx = art.right() + design::MUSIC_INFO_GAP;
        let tw = (content.right() - tx).max(0.0);
        let block = design::LINE_MUSIC_TITLE + design::MUSIC_LINE_GAP + design::LINE_MUSIC_META;
        let ty = art.center_y() - block * 0.5;
        self.text(
            rt,
            &m.title,
            RectD::new(tx, ty, tw, design::LINE_MUSIC_TITLE),
            design::FONT_MUSIC_TITLE,
            600,
            0,
            design::LINE_MUSIC_TITLE,
            theme.text_primary().with_alpha(alpha),
            true,
        );
        let meta_y = ty + design::LINE_MUSIC_TITLE + design::MUSIC_LINE_GAP;
        let source_color = if theme.has_accent {
            theme.accent
        } else {
            design::rgb(0x6C, 0xB4, 0xEE)
        };
        if m.artist.is_empty() {
            self.text(
                rt,
                &m.source,
                RectD::new(tx, meta_y, tw, design::LINE_MUSIC_META),
                design::FONT_MUSIC_META,
                400,
                0,
                design::LINE_MUSIC_META,
                source_color.with_alpha(alpha),
                true,
            );
        } else {
            let artist_w = self
                .measure(
                    rt,
                    &m.artist,
                    design::FONT_MUSIC_META,
                    400,
                    design::LINE_MUSIC_META,
                )
                .min(tw);
            self.text(
                rt,
                &m.artist,
                RectD::new(tx, meta_y, artist_w + 1.0, design::LINE_MUSIC_META),
                design::FONT_MUSIC_META,
                400,
                0,
                design::LINE_MUSIC_META,
                theme.music_meta().with_alpha(alpha),
                true,
            );
            let dot = " · ";
            let dot_w = self.measure(
                rt,
                dot,
                design::FONT_MUSIC_META,
                400,
                design::LINE_MUSIC_META,
            );
            self.text(
                rt,
                dot,
                RectD::new(tx + artist_w, meta_y, dot_w + 1.0, design::LINE_MUSIC_META),
                design::FONT_MUSIC_META,
                400,
                0,
                design::LINE_MUSIC_META,
                theme.text_tertiary().with_alpha(alpha),
                false,
            );
            let sx = (tx + artist_w + dot_w).min(content.right());
            self.text(
                rt,
                &m.source,
                RectD::new(
                    sx,
                    meta_y,
                    (content.right() - sx).max(0.0),
                    design::LINE_MUSIC_META,
                ),
                design::FONT_MUSIC_META,
                400,
                0,
                design::LINE_MUSIC_META,
                source_color.with_alpha(alpha),
                true,
            );
        }

        // Progress row: 30 elapsed | wave | 34 remaining.
        let py = y0 + design::MUSIC_ART + gap;
        self.text(
            rt,
            &m.elapsed,
            RectD::new(
                content.x,
                py,
                design::MUSIC_TIME_W,
                design::MUSIC_PROGRESS_H,
            ),
            design::FONT_TIME_LABEL,
            400,
            0,
            design::LINE_TIME,
            theme.music_meta().with_alpha(alpha),
            false,
        );
        let wave_x = content.x + design::MUSIC_TIME_W + design::MUSIC_TIME_GAP;
        let wave_w =
            (content.right() - design::MUSIC_REMAIN_W - design::MUSIC_TIME_GAP - wave_x).max(160.0);
        let wave = RectD::new(
            wave_x,
            py,
            wave_w.min(content.right() - wave_x),
            design::MUSIC_PROGRESS_H,
        );
        self.draw_music_wave(rt, input, wave, m.fraction, m.playing, alpha);
        self.text(
            rt,
            &m.remaining,
            RectD::new(
                content.right() - design::MUSIC_REMAIN_W,
                py,
                design::MUSIC_REMAIN_W,
                design::MUSIC_PROGRESS_H,
            ),
            design::FONT_TIME_LABEL,
            400,
            2,
            design::LINE_TIME,
            theme.music_meta().with_alpha(alpha),
            false,
        );
        if m.controls.seek && alpha > 0.15 {
            hits.add(Element::ProgressRow, wave, 4.0);
        }

        // Controls row.
        let cy = py + design::MUSIC_PROGRESS_H + gap;
        let step = design::MUSIC_BTN + design::MUSIC_BTN_GAP;
        let total_w = step * 4.0 + design::MUSIC_PLAY_W;
        let mut x = content.center_x() - total_w * 0.5;
        let small_y = cy + (design::MUSIC_PLAY_H - design::MUSIC_BTN) * 0.5;

        let idle = theme.music_meta();
        let active_fill = accent.with_alpha(0.14);
        let r = RectD::new(x, small_y, design::MUSIC_BTN, design::MUSIC_BTN);
        self.flat_icon_button(
            rt,
            input,
            Element::MusicShuffle,
            r,
            IconId::Shuffle,
            if m.shuffle { accent } else { idle },
            if m.shuffle { Some(active_fill) } else { None },
            alpha,
            m.controls.shuffle,
            hits,
        );
        x += step;
        let r = RectD::new(x, small_y, design::MUSIC_BTN, design::MUSIC_BTN);
        self.flat_icon_button(
            rt,
            input,
            Element::MusicPrev,
            r,
            IconId::Previous,
            theme.text_primary(),
            None,
            alpha,
            m.controls.previous,
            hits,
        );
        x += step;
        let r = RectD::new(x, cy, design::MUSIC_PLAY_W, design::MUSIC_PLAY_H);
        let draw = pressed_rect(r, input.pressed_amount(Element::MusicPlay));
        let fill = if input.hovered(Element::MusicPlay) {
            darken(accent, 0.92)
        } else {
            accent
        };
        self.fill_round(rt, draw, design::R_SURFACE, fill.with_alpha(alpha));
        let icon = if m.playing {
            IconId::Pause
        } else {
            IconId::Play
        };
        self.icon_center(
            rt,
            icon,
            draw,
            design::MUSIC_GLYPH,
            design::DARK_GLYPH.with_alpha(alpha),
            input.scale,
            0.0,
        );
        if alpha > 0.15 && m.controls.toggle {
            hits.add(Element::MusicPlay, r, design::R_SURFACE);
        }
        x += design::MUSIC_PLAY_W + design::MUSIC_BTN_GAP;
        let r = RectD::new(x, small_y, design::MUSIC_BTN, design::MUSIC_BTN);
        self.flat_icon_button(
            rt,
            input,
            Element::MusicNext,
            r,
            IconId::Next,
            theme.text_primary(),
            None,
            alpha,
            m.controls.next,
            hits,
        );
        x += step;
        let r = RectD::new(x, small_y, design::MUSIC_BTN, design::MUSIC_BTN);
        self.flat_icon_button(
            rt,
            input,
            Element::MusicRepeat,
            r,
            IconId::Repeat,
            if m.repeat != RepeatMode::None {
                accent
            } else {
                idle
            },
            if m.repeat != RepeatMode::None {
                Some(active_fill)
            } else {
                None
            },
            alpha,
            m.controls.repeat,
            hits,
        );
    }

    fn draw_art_placeholder(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        rect: RectD,
        alpha: f32,
    ) {
        self.fill_round(
            rt,
            rect,
            design::R_ART,
            design::rgb(0x10, 0x1C, 0x20).with_alpha(alpha),
        );
        if let Some(brush) = self.placeholder_gradient(rt) {
            let clip = rounded_layer(rt, rect, design::R_ART);
            // SAFETY: the cached brush and local transform are valid.
            unsafe {
                brush.SetOpacity(alpha);
                brush.SetTransform(&Matrix3x2 {
                    M11: rect.w,
                    M22: rect.h,
                    M31: rect.x,
                    M32: rect.y,
                    ..Matrix3x2::identity()
                });
                let unit = D2D1_ROUNDED_RECT {
                    rect: d2d_rect(RectD::new(0.0, 0.0, 1.0, 1.0)),
                    radiusX: 0.0,
                    radiusY: 0.0,
                };
                rt.FillRoundedRectangle(&unit, &brush);
                brush.SetTransform(&Matrix3x2::identity());
            }
            drop_rounded_layer(rt, clip);
        }
        self.icon_center(
            rt,
            IconId::Music,
            rect,
            26.0,
            design::rgb(0x8F, 0xC3, 0xCF).with_alpha(alpha),
            input.scale,
            1.6 * 24.0 / 26.0,
        );
    }

    fn draw_music_wave(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        row: RectD,
        fraction: f32,
        playing: bool,
        alpha: f32,
    ) {
        let theme = input.theme;
        let f = fraction.clamp(0.0, 1.0);
        let cy = row.y + row.h * 0.5;
        let px = row.x + row.w * f;
        let Ok(style) = self.solid_style(rt) else {
            return;
        };
        // Remaining part: a straight 3 DIP line.
        if row.right() - px > 0.5 {
            if let Ok(track) = self.brush(rt, theme.music_track().with_alpha(alpha)) {
                // SAFETY: ordinary stroke.
                unsafe {
                    rt.DrawLine(
                        Vector2::new(px, cy),
                        Vector2::new(row.right(), cy),
                        &track,
                        design::MUSIC_TRACK_STROKE,
                        &style,
                    );
                }
            }
        }
        if px > row.x + 0.5 {
            let clip = d2d_rect(RectD::new(row.x, row.y, px - row.x, row.h));
            // SAFETY: clip and geometry are local.
            unsafe { rt.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
            let phase = input.progress.map(|p| p.phase).unwrap_or(0.0);
            let amplitude = if playing {
                design::MUSIC_WAVE_AMP
            } else {
                design::MUSIC_WAVE_AMP * design::WAVE_FLAT
            };
            if let Some(path) = wave_geometry_len(
                rt,
                row.x,
                cy,
                px - row.x,
                phase,
                amplitude,
                design::MUSIC_WAVE_LEN,
            ) {
                if let Ok(brush) = self.brush(rt, accent_of(input.view).with_alpha(alpha)) {
                    // SAFETY: path/brush/style are alive.
                    unsafe {
                        rt.DrawGeometry(&path, &brush, design::MUSIC_WAVE_STROKE, Some(&style));
                    }
                }
            }
            // SAFETY: balances the push above.
            unsafe { rt.PopAxisAlignedClip() };
        }
        let thumb_x =
            (px - design::MUSIC_THUMB_W * 0.5).clamp(row.x, row.right() - design::MUSIC_THUMB_W);
        self.fill_round(
            rt,
            RectD::new(
                thumb_x,
                cy - design::MUSIC_THUMB_H * 0.5,
                design::MUSIC_THUMB_W,
                design::MUSIC_THUMB_H,
            ),
            design::R_THUMB,
            design::white(alpha),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn flat_icon_button(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        element: Element,
        rect: RectD,
        icon: IconId,
        color: Color,
        active_fill: Option<Color>,
        alpha: f32,
        enabled: bool,
        hits: &mut HitMap,
    ) {
        let pressed = input.pressed_amount(element).is_some();
        let draw = pressed_rect(rect, input.pressed_amount(element));
        let fill = if pressed {
            input.theme.rail_press_fill()
        } else if input.hovered(element) && enabled {
            input.theme.rail_hover_fill()
        } else if let Some(fill) = active_fill {
            fill
        } else {
            Color::TRANSPARENT
        };
        self.fill_round(rt, draw, design::R_SURFACE, fill.with_alpha(alpha));
        let c = if enabled {
            color.with_alpha(alpha)
        } else {
            color.with_alpha(0.35 * alpha)
        };
        self.icon_center(rt, icon, draw, design::MUSIC_GLYPH, c, input.scale, 0.0);
        if enabled && alpha > 0.15 {
            hits.add(element, rect, design::R_SURFACE);
        }
    }

    fn draw_call_card(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        c: &crate::view::CallBody,
        body: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let accent = design::rail_accent(ActivityKind::Call);

        // Status row: four signal bars then one line of status.
        let status = RectD::new(body.x, body.y, body.w, design::CALL_STATUS_H);
        let bars_y = status.center_y() - design::SIGNAL_H * 0.5;
        for (i, h) in [6.0, 9.5, 13.0, 16.0].iter().enumerate() {
            let a = if i == 3 { 0.35 } else { 1.0 };
            self.fill_round(
                rt,
                RectD::new(
                    status.x + i as f32 * 5.0,
                    bars_y + design::SIGNAL_H - h,
                    3.0,
                    *h,
                ),
                1.0,
                accent.with_alpha(a * alpha),
            );
        }
        let text = if c.live {
            if c.device.is_empty() {
                c.app_name.clone()
            } else {
                format!("{} \u{00b7} {}", c.device, c.elapsed)
            }
        } else {
            "No active call".to_string()
        };
        self.text(
            rt,
            &text,
            RectD::new(
                status.x + design::SIGNAL_W + 10.0,
                status.y,
                (status.w - design::SIGNAL_W - 10.0).max(0.0),
                status.h,
            ),
            design::FONT_CALL_TITLE,
            600,
            0,
            design::LINE_CALL_TITLE,
            theme.call_status_text().with_alpha(alpha),
            true,
        );

        // Controls: mute, speaker, then the flex-width Open call button.
        let row_y = body.y + design::CALL_STATUS_H + design::CARD_GAP;
        let mut x = body.x;
        for (element, icon, active) in [
            (
                Element::CallMic,
                if c.mic_muted {
                    IconId::MicMuted
                } else {
                    IconId::Mic
                },
                c.mic_muted,
            ),
            (
                Element::CallOutput,
                if c.output_muted {
                    IconId::SpeakerMuted
                } else {
                    IconId::Speaker
                },
                c.output_muted,
            ),
        ] {
            let r = RectD::new(x, row_y, design::CALL_BTN_W, design::CALL_BTN_H);
            let pressed = input.pressed_amount(element);
            let draw = pressed_rect(r, pressed);
            let fill = if active {
                theme.text_primary()
            } else if pressed.is_some() {
                theme.card_button_press()
            } else if input.hovered(element) {
                theme.card_button_hover()
            } else {
                theme.card_button()
            };
            self.fill_round(rt, draw, design::R_SURFACE, fill.with_alpha(alpha));
            let glyph = if active {
                design::DARK_GLYPH
            } else {
                theme.card_button_text()
            };
            self.icon_center(
                rt,
                icon,
                draw,
                18.0,
                glyph.with_alpha(alpha),
                input.scale,
                0.0,
            );
            if alpha > 0.15 {
                hits.add(element, r, design::R_SURFACE);
            }
            x += design::CALL_BTN_W + design::CALL_BTN_GAP;
        }
        let open_r = RectD::new(x, row_y, (body.right() - x).max(0.0), design::CALL_BTN_H);
        let pressed = input.pressed_amount(Element::CallOpen);
        let draw = pressed_rect(open_r, pressed);
        let fill = if pressed.is_some() {
            theme.card_button_press()
        } else if input.hovered(Element::CallOpen) {
            theme.card_button_hover()
        } else {
            theme.open_call_fill()
        };
        self.fill_round(rt, draw, design::R_SURFACE, fill.with_alpha(alpha));
        self.text(
            rt,
            &format!("Open {}", c.app_name),
            draw,
            design::FONT_OPEN_CALL,
            600,
            1,
            design::LINE_CALL_TITLE,
            theme.open_call_text(accent).with_alpha(alpha),
            true,
        );
        if alpha > 0.15 {
            hits.add(Element::CallOpen, open_r, design::R_SURFACE);
        }
    }

    /// v0.6 shelf: full content column, 12 DIP padding, a 28 DIP toolbar and a
    /// 4-column scrolling grid (virtualized upstream).
    fn draw_shelf_card(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        s: &crate::view::ShelfBody,
        fly: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let accent = design::rail_accent(ActivityKind::Shelf);
        // Shared content column from `card_content_rect()`, animated by the
        // card-open tween.
        let mut inner = crate::view::content_rect(fly);
        inner.y += input.top_dy;
        let toolbar = RectD::new(inner.x, inner.y, inner.w, design::SHELF_TOOLBAR_H);
        let disabled = s.converting;

        // Toolbar: item count on the left, icon-only controls on the right.
        let icon = RectD::new(
            toolbar.x,
            toolbar.y + (toolbar.h - design::SHELF_COUNT_GLYPH) * 0.5,
            design::SHELF_COUNT_GLYPH,
            design::SHELF_COUNT_GLYPH,
        );
        self.icon_center(
            rt,
            IconId::Tray,
            icon,
            design::SHELF_COUNT_GLYPH,
            theme.text_secondary().with_alpha(alpha),
            input.scale,
            1.8 * 24.0 / design::SHELF_COUNT_GLYPH,
        );
        let count = s.total.to_string();
        let count_w = self.measure(rt, &count, design::FONT_BUTTON, 600, design::LINE_HINT) + 8.0;
        self.text(
            rt,
            &count,
            RectD::new(icon.right() + 6.0, toolbar.y, count_w, toolbar.h),
            design::FONT_BUTTON,
            600,
            0,
            design::LINE_HINT,
            theme.text_primary().with_alpha(alpha),
            false,
        );
        hits.add(
            Element::ShelfCount,
            RectD::new(icon.x, toolbar.y, icon.w + 6.0 + count_w, toolbar.h),
            design::R_CONTROL,
        );

        let mut right = toolbar.right();
        let convert = RectD::new(
            right - design::SHELF_CONVERT_W,
            toolbar.y,
            design::SHELF_CONVERT_W,
            design::SHELF_CONVERT_H,
        );
        right = convert.x - design::SHELF_TOOL_GAP;
        let clear = RectD::new(
            right - design::SHELF_TOOL_BTN,
            toolbar.y,
            design::SHELF_TOOL_BTN,
            design::SHELF_TOOL_BTN,
        );
        right = clear.x - design::SHELF_TOOL_GAP;
        let select = RectD::new(
            right - design::SHELF_TOOL_BTN,
            toolbar.y,
            design::SHELF_TOOL_BTN,
            design::SHELF_TOOL_BTN,
        );

        let convert_enabled = !disabled && s.convertible > 0;
        let clear_enabled = !disabled && s.total > 0;
        let select_enabled = !disabled && s.total > 0 && s.selected < s.total;
        self.shelf_tool_button(
            rt,
            input,
            Element::ShelfSelectAll,
            select,
            IconId::SelectAll,
            select_enabled,
            alpha,
            hits,
        );
        self.shelf_tool_button(
            rt,
            input,
            Element::ShelfClear,
            clear,
            IconId::Trash,
            clear_enabled,
            alpha,
            hits,
        );
        self.shelf_convert_button(rt, input, convert, convert_enabled, alpha, hits);

        let viewport = RectD::new(
            inner.x,
            inner.y + design::SHELF_TOOLBAR_H + design::SHELF_TOOLBAR_GAP,
            inner.w - design::SHELF_SCROLL_W - design::SHELF_SCROLL_GAP,
            inner.h - design::SHELF_TOOLBAR_H - design::SHELF_TOOLBAR_GAP,
        );

        if s.total == 0 {
            let zone = viewport;
            self.stroke_round(
                rt,
                zone.inset(design::SHELF_DROP_BORDER * 0.5),
                design::R_SURFACE,
                design::SHELF_DROP_BORDER,
                if input.drag_over {
                    accent.with_alpha(alpha)
                } else {
                    theme.drop_border().with_alpha(alpha)
                },
                true,
            );
            let glyph = RectD::new(
                zone.center_x() - design::SHELF_GLYPH * 0.5,
                zone.center_y() - design::SHELF_GLYPH * 0.5,
                design::SHELF_GLYPH,
                design::SHELF_GLYPH,
            );
            self.icon_center(
                rt,
                IconId::Tray,
                glyph,
                design::SHELF_GLYPH,
                accent.with_alpha(if input.drag_over { alpha } else { 0.85 * alpha }),
                input.scale,
                1.8 * 24.0 / design::SHELF_GLYPH,
            );
            self.draw_shelf_scrollbar(rt, input, s, inner, viewport, alpha, hits);
            return;
        }

        // SAFETY: axis-aligned clip keeps partially scrolled rows inside the
        // viewport; it is popped before returning.
        unsafe {
            rt.PushAxisAlignedClip(&d2d_rect(viewport), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        self.draw_shelf_tiles(rt, input, s, viewport, alpha, hits);
        self.draw_shelf_rubber(rt, input, s, viewport, alpha);
        // SAFETY: matches the push above.
        unsafe { rt.PopAxisAlignedClip() };
        self.draw_shelf_scrollbar(rt, input, s, inner, viewport, alpha, hits);
    }

    #[allow(clippy::too_many_arguments)]
    fn shelf_tool_button(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        element: Element,
        rect: RectD,
        icon: IconId,
        enabled: bool,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let hovered = enabled && input.hovered(element);
        let pressed = enabled.then(|| input.pressed_amount(element)).flatten();
        let draw = pressed_rect(rect, pressed);
        if pressed.is_some() {
            self.fill_round(
                rt,
                draw,
                design::R_CONTROL,
                theme.surface_button_press().with_alpha(alpha),
            );
        } else if hovered {
            self.fill_round(
                rt,
                draw,
                design::R_CONTROL,
                theme.surface_button_hover().with_alpha(alpha),
            );
        }
        let fg = if enabled {
            theme.text_primary().with_alpha(0.9 * alpha)
        } else {
            theme.text_tertiary().with_alpha(0.45 * alpha)
        };
        self.icon_center(
            rt,
            icon,
            draw,
            design::SHELF_TOOL_GLYPH,
            fg,
            input.scale,
            design::ICON_STROKE,
        );
        hits.add(element, rect, design::R_CONTROL);
    }

    fn shelf_convert_button(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        rect: RectD,
        enabled: bool,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let accent = design::rail_accent(ActivityKind::Shelf);
        let hovered = enabled && input.hovered(Element::ConvertButton);
        let pressed = enabled
            .then(|| input.pressed_amount(Element::ConvertButton))
            .flatten();
        let draw = pressed_rect(rect, pressed);
        let level = if pressed.is_some() {
            0.82
        } else if hovered {
            1.0
        } else {
            0.94
        };
        let fill = if enabled {
            accent.with_alpha(level * alpha)
        } else {
            accent.with_alpha(0.35 * alpha)
        };
        self.fill_round(rt, draw, design::SHELF_CONVERT_R, fill);
        let fg = if enabled {
            design::CONVERT_ON_ACCENT.with_alpha(alpha)
        } else {
            design::CONVERT_ON_ACCENT.with_alpha(0.55 * alpha)
        };
        self.icon_center(
            rt,
            IconId::Convert,
            draw,
            design::SHELF_TOOL_GLYPH,
            fg,
            input.scale,
            design::ICON_STROKE,
        );
        hits.add(Element::ConvertButton, rect, design::SHELF_CONVERT_R);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_shelf_scrollbar(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        s: &crate::view::ShelfBody,
        inner: RectD,
        viewport: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        if s.max_scroll <= 0.0 || s.content_h <= 0.0 {
            return;
        }
        let theme = input.theme;
        let track = RectD::new(
            inner.right() - design::SHELF_SCROLL_W,
            viewport.y,
            design::SHELF_SCROLL_W,
            viewport.h,
        );
        self.fill_round(
            rt,
            track,
            design::SHELF_SCROLL_W * 0.5,
            theme.text_secondary().with_alpha(0.14 * alpha),
        );
        let thumb_h = (viewport.h * viewport.h / s.content_h)
            .max(design::SHELF_SCROLL_MIN)
            .min(viewport.h);
        let t = (s.scroll / s.max_scroll).clamp(0.0, 1.0);
        let thumb = RectD::new(
            track.x,
            track.y + (track.h - thumb_h) * t,
            design::SHELF_SCROLL_W,
            thumb_h,
        );
        let strong = input.hover == Some(Element::ShelfScrollbar);
        self.fill_round(
            rt,
            thumb,
            design::SHELF_SCROLL_W * 0.5,
            theme
                .text_secondary()
                .with_alpha(if strong { 0.85 } else { 0.55 } * alpha),
        );
        hits.add(
            Element::ShelfScrollbar,
            RectD::new(track.x - 4.0, track.y, track.w + 8.0, track.h),
            design::SHELF_SCROLL_W * 0.5,
        );
    }

    fn draw_shelf_tiles(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        s: &crate::view::ShelfBody,
        viewport: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let accent = design::rail_accent(ActivityKind::Shelf);
        let pitch_x = design::SHELF_TILE_W + design::SHELF_TILE_GAP;
        for file in &s.files {
            let r = RectD::new(
                viewport.x + file.col as f32 * pitch_x,
                viewport.y + file.row as f32 * design::SHELF_ROW_H - s.scroll,
                design::SHELF_TILE_W,
                design::SHELF_TILE_H,
            );
            if r.bottom() <= viewport.y || r.y >= viewport.bottom() {
                continue;
            }
            let element = Element::ShelfTile(file.index);
            let hovered = input.hovered(element);
            let pressed = input.pressed_amount(element);
            let draw = pressed_rect(r, pressed);
            let fill = if pressed.is_some() {
                theme.surface_button_press()
            } else if hovered {
                design::white(design::SHELF_TILE_HOVER_A)
            } else {
                design::white(design::SHELF_TILE_A)
            };
            self.fill_round(rt, draw, design::SHELF_TILE_R, fill.with_alpha(alpha));

            // Content is clipped to the tile's rounded corners so thumbnails
            // and their name strip follow the corner radius.
            let clip = rounded_layer(rt, draw, design::SHELF_TILE_R);
            let mut drew_thumb = false;
            if let Some(thumb) = &file.thumb {
                drew_thumb = self.draw_thumb(rt, thumb, draw, alpha);
            }
            if !drew_thumb {
                let chip = RectD::new(
                    draw.center_x() - design::SHELF_CHIP * 0.5,
                    draw.y
                        + (design::SHELF_TILE_H - design::SHELF_NAME_H - design::SHELF_CHIP) * 0.5,
                    design::SHELF_CHIP,
                    design::SHELF_CHIP,
                );
                self.fill_round(
                    rt,
                    chip,
                    design::SHELF_CHIP_R,
                    file.badge_color.with_alpha(alpha),
                );
                self.icon_center(
                    rt,
                    file.icon,
                    chip,
                    design::SHELF_CHIP_GLYPH,
                    design::white(0.96 * alpha),
                    input.scale,
                    design::ICON_STROKE,
                );
            }
            let strip = RectD::new(
                draw.x,
                draw.bottom() - design::SHELF_NAME_H,
                draw.w,
                design::SHELF_NAME_H,
            );
            self.fill_rect(rt, strip, design::black(design::SHELF_NAME_ALPHA * alpha));
            self.text(
                rt,
                &file.name,
                RectD::new(strip.x + 4.0, strip.y, strip.w - 8.0, strip.h),
                design::FONT_FILE_NAME,
                500,
                1,
                design::SHELF_NAME_H,
                design::white(0.94 * alpha),
                true,
            );
            drop_rounded_layer(rt, clip);

            let px = 1.0 / input.scale;
            if file.selected {
                self.stroke_round(
                    rt,
                    draw.inset(design::SHELF_SELECT_RING * 0.5),
                    design::SHELF_TILE_R,
                    design::SHELF_SELECT_RING,
                    accent.with_alpha(alpha),
                    false,
                );
                let check = RectD::new(
                    draw.x + 6.0,
                    draw.y + 6.0,
                    design::SHELF_CHECK,
                    design::SHELF_CHECK,
                );
                self.fill_round(rt, check, check.h * 0.5, accent.with_alpha(alpha));
                self.icon_center(
                    rt,
                    IconId::Check,
                    check,
                    design::SHELF_CHECK - 6.0,
                    design::DARK_GLYPH.with_alpha(alpha),
                    input.scale,
                    0.0,
                );
            } else {
                self.stroke_round(
                    rt,
                    draw.inset(px * 0.5),
                    design::SHELF_TILE_R,
                    px,
                    design::white(design::SHELF_TILE_BORDER_A),
                    false,
                );
            }

            hits.add(element, r, design::R_CONTROL);
            if hovered && !file.selected {
                let remove = RectD::new(
                    r.right() - design::SHELF_REMOVE - 5.0,
                    r.y + 5.0,
                    design::SHELF_REMOVE,
                    design::SHELF_REMOVE,
                );
                let hot = input.hovered(Element::ShelfRemove(file.index));
                self.fill_round(
                    rt,
                    remove,
                    remove.h * 0.5,
                    design::black(if hot { 0.85 } else { 0.62 } * alpha),
                );
                self.icon_center(
                    rt,
                    IconId::Close,
                    remove,
                    design::SHELF_REMOVE - 7.0,
                    design::white(0.94 * alpha),
                    input.scale,
                    0.0,
                );
                hits.add(Element::ShelfRemove(file.index), remove, remove.h * 0.5);
            }
        }
    }

    /// Rubber-band selection overlay, clipped to the grid viewport.
    fn draw_shelf_rubber(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        s: &crate::view::ShelfBody,
        viewport: RectD,
        alpha: f32,
    ) {
        let Some((x, y, w, h)) = s.rubber else {
            return;
        };
        let band = RectD::new(
            x.max(viewport.x),
            y.max(viewport.y),
            (x + w).min(viewport.right()) - x.max(viewport.x),
            (y + h).min(viewport.bottom()) - y.max(viewport.y),
        );
        if band.w <= 0.0 || band.h <= 0.0 {
            return;
        }
        let accent = design::rail_accent(ActivityKind::Shelf);
        self.fill_rect(rt, band, accent.with_alpha(0.12 * alpha));
        let px = 1.0 / input.scale;
        self.stroke_round(
            rt,
            band.inset(px * 0.5),
            0.0,
            px,
            accent.with_alpha(0.9 * alpha),
            false,
        );
    }

    /// Cover-crops a cached thumbnail into a tile. Returns false when the
    /// bitmap could not be created so the caller can draw the type chip.
    /// Light checkerboard under an image that renders at its natural size, so
    /// the empty area of the 64 × 40 slot reads as "small image" (spec 7).
    fn draw_checker(&mut self, rt: &ID2D1RenderTarget, r: RectD, alpha: f32) {
        let cell = design::CLIP_CHECKER;
        if cell <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        unsafe { rt.PushAxisAlignedClip(&d2d_rect(r), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        let rows = (r.h / cell).ceil() as usize + 1;
        let cols = (r.w / cell).ceil() as usize + 1;
        for row in 0..rows {
            for col in 0..cols {
                let square =
                    RectD::new(r.x + col as f32 * cell, r.y + row as f32 * cell, cell, cell);
                let color = if (row + col) % 2 == 0 {
                    design::CLIP_CHECKER_A
                } else {
                    design::CLIP_CHECKER_B
                };
                self.fill_rect(rt, square, color.with_alpha(alpha));
            }
        }
        unsafe { rt.PopAxisAlignedClip() };
    }

    fn draw_thumb(&mut self, rt: &ID2D1RenderTarget, art: &AlbumArt, r: RectD, alpha: f32) -> bool {
        let Some(bitmap) = self.art_bitmap(rt, art) else {
            return false;
        };
        let scale = (r.w / art.width as f32).max(r.h / art.height as f32);
        let src_w = (r.w / scale).min(art.width as f32);
        let src_h = (r.h / scale).min(art.height as f32);
        let sx = (art.width as f32 - src_w) * 0.5;
        let sy = (art.height as f32 - src_h) * 0.5;
        let src = d2d_rect(RectD::new(sx, sy, src_w, src_h));
        let dest = d2d_rect(r);
        // SAFETY: bitmap is alive and the target is valid.
        unsafe {
            rt.DrawBitmap(
                &bitmap,
                Some(&dest),
                alpha,
                windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                Some(&src),
            );
        }
        true
    }

    fn draw_convert_card(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        c: &ConvertBody,
        area: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        match &c.state {
            ConvertState::Working(w) => self.draw_convert_working(rt, input, w, area, alpha, hits),
            ConvertState::Done(d) => self.draw_convert_done(rt, input, d, area, alpha, hits),
            ConvertState::Failed { title, detail } => {
                self.draw_convert_failed(rt, input, title, detail, area, alpha, c, hits)
            }
            ConvertState::Grid | ConvertState::Notice { .. } | ConvertState::Waiting => {
                self.draw_convert_choice(rt, input, c, area, alpha, hits)
            }
        }
    }

    fn draw_convert_choice(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        c: &ConvertBody,
        area: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        // The stack is top-aligned: header, tabs, then the grid fills every
        // remaining DIP of the content column (design system §1: body fills).
        let stack_y = area.y;

        // Header: 24 DIP thumbnail/type tile, one truncated name, close.
        let header = RectD::new(area.x, stack_y, area.w, design::CONVERT_HEADER_H);
        let tile = RectD::new(
            header.x,
            header.y + (header.h - design::CONVERT_HEADER_TILE) * 0.5,
            design::CONVERT_HEADER_TILE,
            design::CONVERT_HEADER_TILE,
        );
        if c.stacked {
            let back = RectD::new(tile.x + 3.0, tile.y + 3.0, tile.w, tile.h);
            self.fill_round(rt, back, 6.0, c.badge_color.with_alpha(0.45 * alpha));
            self.fill_round(rt, tile, 6.0, c.badge_color.with_alpha(alpha));
        } else if c
            .thumb
            .as_ref()
            .is_none_or(|art| !self.draw_thumb(rt, art, tile, alpha))
        {
            self.fill_round(rt, tile, 6.0, c.badge_color.with_alpha(alpha));
            self.icon_center(
                rt,
                c.icon,
                tile,
                14.0,
                design::white(0.94 * alpha),
                input.scale,
                0.0,
            );
        }

        let close = RectD::new(
            header.right() - 28.0,
            header.y + (header.h - 28.0) * 0.5,
            28.0,
            28.0,
        );
        if input.hovered(Element::ConvertClose) {
            self.fill_round(
                rt,
                close,
                design::R_CONTROL,
                theme.surface_button_hover().with_alpha(alpha),
            );
        }
        self.icon_center(
            rt,
            IconId::Close,
            close,
            design::CONVERT_CLOSE,
            theme.text_secondary().with_alpha(alpha),
            input.scale,
            0.0,
        );
        hits.add(Element::ConvertClose, close, design::R_CONTROL);

        let name_x = tile.right() + design::CONVERT_HEADER_GAP;
        // Format badge ("JPG", "PDF") between the name and close: a rounded
        // chip in the format colour with white bold text (design §7 chip).
        let badge_w = if c.badge.is_empty() {
            0.0
        } else {
            self.measure(rt, &c.badge, 10.5, 600, 14.0) + 12.0
        };
        let name_right = if badge_w > 0.0 {
            close.x - 6.0 - badge_w - 6.0
        } else {
            close.x - 8.0
        };
        self.text(
            rt,
            &c.title,
            RectD::new(name_x, header.y, (name_right - name_x).max(0.0), header.h),
            design::CONVERT_NAME_FONT,
            600,
            0,
            header.h,
            theme.text_primary().with_alpha(alpha),
            true,
        );
        if badge_w > 0.0 {
            let chip = RectD::new(
                close.x - 6.0 - badge_w,
                header.y + (header.h - 15.0) * 0.5,
                badge_w,
                15.0,
            );
            self.fill_round(rt, chip, 4.0, c.badge_color.with_alpha(alpha));
            self.text(
                rt,
                &c.badge,
                chip,
                10.5,
                600,
                1,
                14.0,
                design::white(0.96 * alpha),
                false,
            );
        }

        // Tabs: 24 DIP icon-only pills in a 28 DIP strip, centered. §5: the
        // strip is pointless when only one tab exists, so an empty tab list
        // collapses it and the grid takes the space it leaves behind.
        let strip_h = if c.tabs.is_empty() {
            0.0
        } else {
            design::CONVERT_TABS_H + design::CONVERT_STACK_GAP
        };
        let tabs = RectD::new(
            area.x,
            header.bottom() + design::CONVERT_STACK_GAP,
            area.w,
            design::CONVERT_TABS_H,
        );
        let grid_y = header.bottom() + design::CONVERT_STACK_GAP + strip_h;
        let grid = RectD::new(area.x, grid_y, area.w, (area.bottom() - grid_y).max(0.0));
        match &c.state {
            ConvertState::Grid => {
                let gap = design::CONVERT_TAB_GAP;
                let count = c.tabs.len() as f32;
                let strip_w = count * design::CONVERT_TAB + (count - 1.0).max(0.0) * gap;
                let mut x = tabs.center_x() - strip_w * 0.5;
                for tab in &c.tabs {
                    let r = RectD::new(
                        x,
                        tabs.y + (tabs.h - design::CONVERT_TAB) * 0.5,
                        design::CONVERT_TAB,
                        design::CONVERT_TAB,
                    );
                    let element = Element::ConvertTab(tab.tab);
                    let active = tab.tab == c.active_tab;
                    let pressed = input.pressed_amount(element);
                    if pressed.is_some() {
                        self.fill_round(
                            rt,
                            pressed_rect(r, pressed),
                            design::R_CONTROL,
                            theme.convert_option_hover().with_alpha(alpha),
                        );
                    } else if active {
                        self.fill_round(
                            rt,
                            r,
                            design::R_CONTROL,
                            theme.convert_tab_active().with_alpha(alpha),
                        );
                    } else if input.hovered(element) {
                        self.fill_round(
                            rt,
                            r,
                            design::R_CONTROL,
                            theme.surface_button_hover().with_alpha(alpha),
                        );
                    }
                    let fg = if active {
                        theme.text_primary()
                    } else {
                        theme.text_secondary()
                    };
                    self.icon_center(
                        rt,
                        tab.tab.icon(),
                        r,
                        design::CONVERT_TAB_GLYPH,
                        fg.with_alpha(alpha),
                        input.scale,
                        0.0,
                    );
                    hits.add(element, r, design::R_CONTROL);
                    x += design::CONVERT_TAB + gap;
                }

                // Rows split the grid height: one row takes all of it (106),
                // two rows share it (49 each), never below the 40 DIP minimum.
                let visible = c.options.len().min(design::CONVERT_MAX_OPTIONS);
                let rows = visible.div_ceil(design::CONVERT_OPTION_COLS).max(1);
                let tile_h = ((grid.h - design::CONVERT_OPTION_GAP * (rows as f32 - 1.0))
                    / rows as f32)
                    .max(design::CONVERT_OPTION_H);
                for (index, option) in c
                    .options
                    .iter()
                    .take(design::CONVERT_MAX_OPTIONS)
                    .enumerate()
                {
                    let row = index / design::CONVERT_OPTION_COLS;
                    let col = index % design::CONVERT_OPTION_COLS;
                    let r = RectD::new(
                        grid.x
                            + col as f32 * (design::CONVERT_OPTION_W + design::CONVERT_OPTION_GAP),
                        grid.y + row as f32 * (tile_h + design::CONVERT_OPTION_GAP),
                        design::CONVERT_OPTION_W,
                        tile_h,
                    );
                    let element = option.action.map(Element::ConvertOption);
                    let pressed = element.and_then(|e| input.pressed_amount(e));
                    let hovered = element.map(|e| input.hovered(e)).unwrap_or(false);
                    let drop_target = c.drag_target.is_some() && option.action == c.drag_target;
                    let a = if option.needs {
                        // §5 rule 3: not enough input yet — the tip says what.
                        alpha * design::CONVERT_NEEDS_A
                    } else if c.drag_target.is_some() && !drop_target {
                        alpha * design::CONVERT_DROP_DIM_A
                    } else {
                        alpha
                    };
                    let draw = pressed_rect(r, pressed);
                    let fill = if drop_target {
                        theme
                            .convert_accent()
                            .with_alpha(design::CONVERT_DROP_FILL_A * alpha)
                    } else if pressed.is_some() {
                        theme.convert_option_hover().with_alpha(a)
                    } else if hovered {
                        theme.convert_option_hover().with_alpha(0.8 * a)
                    } else {
                        theme.convert_option_fill().with_alpha(a)
                    };
                    self.fill_round(rt, draw, design::CONVERT_OPTION_R, fill);
                    let px = 1.0 / input.scale;
                    if drop_target {
                        self.stroke_round(
                            rt,
                            draw.inset(px * 0.75),
                            design::CONVERT_OPTION_R,
                            px * 1.5,
                            theme.convert_accent().with_alpha(alpha),
                            false,
                        );
                    } else {
                        self.stroke_round(
                            rt,
                            draw.inset(px * 0.5),
                            design::CONVERT_OPTION_R,
                            px,
                            theme.convert_option_border().with_alpha(a),
                            false,
                        );
                    }
                    let fg = theme.text_primary().with_alpha(if option.action.is_some() {
                        0.94 * a
                    } else {
                        0.55 * a
                    });
                    if let Some(format) = &option.format {
                        self.text(
                            rt,
                            format,
                            draw,
                            design::CONVERT_OPTION_FONT,
                            600,
                            1,
                            draw.h,
                            fg,
                            false,
                        );
                    } else if let Some(icon) = option.icon {
                        let icon_rect = RectD::new(
                            draw.center_x() - design::CONVERT_OPTION_GLYPH * 0.5,
                            draw.center_y() - design::CONVERT_OPTION_GLYPH * 0.5,
                            design::CONVERT_OPTION_GLYPH,
                            design::CONVERT_OPTION_GLYPH,
                        );
                        self.icon_center(
                            rt,
                            icon,
                            icon_rect,
                            design::CONVERT_OPTION_GLYPH,
                            fg,
                            input.scale,
                            0.0,
                        );
                    }
                    if let Some(element) = element {
                        hits.add(element, r, design::CONVERT_OPTION_R);
                    }
                }
            }
            ConvertState::Notice { title, detail } => {
                let block_y = grid.y + 12.0;
                self.text(
                    rt,
                    title,
                    RectD::new(grid.x, block_y, grid.w, 20.0),
                    14.0,
                    600,
                    1,
                    20.0,
                    theme.text_primary().with_alpha(alpha),
                    true,
                );
                self.text(
                    rt,
                    detail,
                    RectD::new(grid.x, block_y + 24.0, grid.w, 16.0),
                    design::FONT_HINT + 0.5,
                    400,
                    1,
                    16.0,
                    theme.text_secondary().with_alpha(alpha),
                    true,
                );
            }
            ConvertState::Waiting => {
                let block_y = grid.y + 12.0;
                self.text(
                    rt,
                    "Waiting for the file",
                    RectD::new(grid.x, block_y, grid.w, 20.0),
                    14.0,
                    600,
                    1,
                    20.0,
                    theme.text_primary().with_alpha(alpha),
                    true,
                );
                self.text(
                    rt,
                    "Keep dragging; the options appear as it reaches the island.",
                    RectD::new(grid.x, block_y + 24.0, grid.w, 16.0),
                    design::FONT_HINT + 0.5,
                    400,
                    1,
                    16.0,
                    theme.text_secondary().with_alpha(alpha),
                    true,
                );
            }
            _ => {}
        }
    }

    fn draw_convert_working(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        w: &crate::view::WorkingView,
        area: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let tile = design::CONVERT_STAGE_TILE;
        let gap = design::CONVERT_STAGE_GAP;
        let arrow_w = design::CONVERT_STAGE_ARROW;
        let top = area.y + 18.0;
        let source = RectD::new(area.x, top, tile, tile);
        let arrow = RectD::new(source.right() + gap, top, arrow_w, tile);
        let target = RectD::new(arrow.right() + gap, top, tile, tile);

        self.fill_round(
            rt,
            source,
            design::CONVERT_STAGE_R,
            w.source_color.with_alpha(alpha),
        );
        self.text(
            rt,
            &w.source_badge,
            source,
            13.5,
            600,
            1,
            18.0,
            design::white(0.96 * alpha),
            false,
        );

        self.fill_round(
            rt,
            target,
            design::CONVERT_STAGE_R,
            theme.surface_button().with_alpha(0.55 * alpha),
        );
        let px = 1.0 / input.scale;
        self.stroke_round(
            rt,
            target.inset(px * 0.75),
            design::CONVERT_STAGE_R,
            px * 1.5,
            theme.convert_accent().with_alpha(0.7 * alpha),
            true,
        );
        self.text(
            rt,
            &w.target_badge,
            target,
            13.5,
            600,
            1,
            18.0,
            theme.text_secondary().with_alpha(alpha),
            false,
        );

        self.text(
            rt,
            "\u{2192}",
            arrow,
            design::CONVERT_STAGE_ARROW,
            600,
            1,
            tile,
            theme.convert_accent().with_alpha(alpha),
            false,
        );

        // Cancel is the only control on the row: a plain x at the far right.
        let cancel = RectD::new(area.right() - 28.0, top + (tile - 28.0) * 0.5, 28.0, 28.0);
        if input.hovered(Element::ConvertCancel) && !w.cancelling {
            self.fill_round(
                rt,
                cancel,
                design::R_CONTROL,
                theme.surface_button_hover().with_alpha(alpha),
            );
        }
        self.icon_center(
            rt,
            IconId::Close,
            cancel,
            16.0,
            theme
                .text_secondary()
                .with_alpha(if w.cancelling { 0.35 * alpha } else { alpha }),
            input.scale,
            0.0,
        );
        if !w.cancelling {
            hits.add(Element::ConvertCancel, cancel, design::R_CONTROL);
        }

        let percent = format!("{}%", w.percent);
        let percent_w = self.measure(rt, &percent, design::CONVERT_PERCENT_FONT, 600, tile);
        self.text(
            rt,
            &percent,
            RectD::new(cancel.x - 12.0 - percent_w, top, percent_w, tile),
            design::CONVERT_PERCENT_FONT,
            600,
            0,
            tile,
            theme.convert_accent().with_alpha(alpha),
            false,
        );

        let bar_y = area.y + 96.0;
        let track = RectD::new(area.x, bar_y, area.w, design::CONVERT_PROGRESS_H);
        self.fill_round(
            rt,
            track,
            design::CONVERT_PROGRESS_H * 0.5,
            theme.surface_button().with_alpha(alpha),
        );
        let fill_w = area.w * (f32::from(w.percent) / 100.0);
        if fill_w > 0.5 {
            self.fill_round(
                rt,
                RectD::new(track.x, track.y, fill_w, track.h),
                design::CONVERT_PROGRESS_H * 0.5,
                theme.convert_accent().with_alpha(alpha),
            );
        }

        // Reading -> converting -> saving: done stages become a green check,
        // the current one carries the accent, later ones stay gray.
        let dot = design::CONVERT_STAGE_ICON;
        let link = design::CONVERT_STAGE_LINK;
        let stage_y = area.y + 116.0;
        let mut x = area.x;
        for index in 0..3usize {
            let circle = RectD::new(x, stage_y, dot, dot);
            let (fill, fg, icon) = if index < usize::from(w.stage) {
                (
                    design::CONVERT_DONE,
                    design::white(0.98 * alpha),
                    IconId::Check,
                )
            } else if index == usize::from(w.stage) {
                (
                    theme.convert_accent(),
                    design::CONVERT_ON_ACCENT,
                    stage_icon(index),
                )
            } else {
                (
                    theme.surface_button(),
                    theme.text_tertiary(),
                    stage_icon(index),
                )
            };
            self.fill_round(rt, circle, dot * 0.5, fill.with_alpha(alpha));
            self.icon_center(
                rt,
                icon,
                circle,
                14.0,
                fg.with_alpha(alpha),
                input.scale,
                0.0,
            );
            hits.add(Element::ConvertStage(index), circle, dot * 0.5);
            x += dot;
            if index < 2 {
                let join = design::CONVERT_STAGE_LINK_H;
                let rule = RectD::new(
                    x + 4.0,
                    stage_y + (dot - join) * 0.5,
                    (link - 8.0).max(2.0),
                    join,
                );
                let color = if index < usize::from(w.stage) {
                    theme.convert_accent()
                } else {
                    theme.convert_tile_border()
                };
                self.fill_round(rt, rule, join * 0.5, color.with_alpha(alpha));
                x += link;
            }
        }
    }

    fn draw_convert_done(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        d: &crate::view::DoneView,
        area: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let check = RectD::new(
            area.x,
            area.y + 16.0,
            design::CONVERT_DONE_CHECK,
            design::CONVERT_DONE_CHECK,
        );
        self.fill_round(
            rt,
            check,
            design::CONVERT_DONE_CHECK * 0.5,
            design::CONVERT_DONE.with_alpha(alpha),
        );
        self.icon_center(
            rt,
            IconId::Check,
            check,
            22.0,
            design::white(0.98 * alpha),
            input.scale,
            0.0,
        );

        let text_x = check.right() + 14.0;
        self.text(
            rt,
            &d.name,
            RectD::new(text_x, area.y + 18.0, area.right() - text_x, 20.0),
            14.0,
            600,
            0,
            20.0,
            theme.text_primary().with_alpha(alpha),
            true,
        );
        self.text(
            rt,
            &d.meta,
            RectD::new(text_x, area.y + 40.0, area.right() - text_x, 16.0),
            11.5,
            500,
            0,
            16.0,
            if d.meta_accent {
                design::CONVERT_DONE.with_alpha(alpha)
            } else if d.meta_grew {
                design::TIMER_AMBER.with_alpha(alpha)
            } else {
                theme.text_secondary().with_alpha(alpha)
            },
            true,
        );

        if d.has_file {
            let gap = 6.0;
            let button_y = area.y + 78.0;
            let button_w = (area.w - gap * 2.0) / 3.0;
            let show = RectD::new(area.x, button_y, button_w, design::CONVERT_DONE_BTN_H);
            let copy = RectD::new(
                show.right() + gap,
                button_y,
                button_w,
                design::CONVERT_DONE_BTN_H,
            );
            let undo = RectD::new(
                copy.right() + gap,
                button_y,
                button_w,
                design::CONVERT_DONE_BTN_H,
            );
            self.draw_icon_button(
                rt,
                input,
                Element::ConvertShow,
                show,
                IconId::Folder,
                theme.text_primary(),
                alpha,
                hits,
            );
            self.draw_icon_button(
                rt,
                input,
                Element::ConvertCopy,
                copy,
                IconId::Copy,
                theme.text_primary(),
                alpha,
                hits,
            );
            self.draw_icon_button(
                rt,
                input,
                Element::ConvertUndo,
                undo,
                IconId::Undo,
                design::CONVERT_UNDO,
                alpha,
                hits,
            );
        }

        let line_y = area.bottom() - 18.0;
        let height = design::CONVERT_DONE_COUNTDOWN_H;
        self.fill_round(
            rt,
            RectD::new(area.x, line_y, area.w, height),
            height * 0.5,
            theme.convert_tile_border().with_alpha(alpha),
        );
        let remaining = area.w * d.countdown.clamp(0.0, 1.0);
        if remaining > 0.5 {
            self.fill_round(
                rt,
                RectD::new(area.x, line_y, remaining, height),
                height * 0.5,
                theme.convert_accent().with_alpha(alpha),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_convert_failed(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        title: &str,
        detail: &str,
        area: RectD,
        alpha: f32,
        c: &ConvertBody,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let close = RectD::new(area.right() - 28.0, area.y, 28.0, 28.0);
        if input.hovered(Element::ConvertClose) {
            self.fill_round(
                rt,
                close,
                design::R_CONTROL,
                theme.surface_button_hover().with_alpha(alpha),
            );
        }
        self.icon_center(
            rt,
            IconId::Close,
            close,
            design::CONVERT_CLOSE,
            theme.text_secondary().with_alpha(alpha),
            input.scale,
            0.0,
        );
        hits.add(Element::ConvertClose, close, design::R_CONTROL);

        self.text(
            rt,
            title,
            RectD::new(area.x, area.y + 34.0, area.w - 36.0, 20.0),
            14.0,
            600,
            0,
            20.0,
            theme.text_primary().with_alpha(alpha),
            true,
        );
        self.text(
            rt,
            detail,
            RectD::new(area.x, area.y + 62.0, area.w - 36.0, 18.0),
            design::FONT_HINT + 0.5,
            400,
            0,
            18.0,
            theme.text_secondary().with_alpha(alpha),
            true,
        );
        self.text(
            rt,
            &c.hint,
            RectD::new(
                area.x,
                area.bottom() - design::CONVERT_HINT_H,
                area.w,
                design::CONVERT_HINT_H,
            ),
            design::FONT_HINT,
            400,
            0,
            design::CONVERT_HINT_H,
            theme.card_hint().with_alpha(alpha),
            true,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_icon_button(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        element: Element,
        rect: RectD,
        icon: IconId,
        fg: crate::design::Color,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let pressed = input.pressed_amount(element);
        let hovered = input.hovered(element);
        let draw = pressed_rect(rect, pressed);
        let fill = if pressed.is_some() {
            theme.surface_button_press()
        } else if hovered {
            theme.surface_button_hover()
        } else {
            theme.surface_button()
        };
        self.fill_round(rt, draw, design::CONVERT_DONE_BTN_R, fill.with_alpha(alpha));
        self.icon_center(
            rt,
            icon,
            draw,
            design::CONVERT_DONE_ICON,
            fg.with_alpha(alpha),
            input.scale,
            0.0,
        );
        hits.add(element, rect, design::CONVERT_DONE_BTN_R);
    }

    fn draw_timer_card(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        t: &crate::view::TimerBody,
        body: RectD,
        alpha: f32,
        hits: &mut HitMap,
    ) {
        let theme = input.theme;
        let accent = design::rail_accent(ActivityKind::Timer);
        let has_timer = t.running || t.armed || t.finished;

        // 36/40 readout; idle stays grey.
        self.text(
            rt,
            &t.remaining,
            RectD::new(body.x, body.y, body.w, design::LINE_TIMER_READOUT),
            design::FONT_TIMER_READOUT,
            300,
            0,
            design::LINE_TIMER_READOUT,
            if has_timer {
                theme.text_primary()
            } else {
                theme.timer_idle()
            }
            .with_alpha(alpha),
            false,
        );

        // 4 DIP track inside the 14 DIP progress row, thumb at the playhead.
        let py = body.y + design::LINE_TIMER_READOUT + design::CARD_GAP;
        let track = RectD::new(
            body.x,
            py + (design::TIMER_PROGRESS_H - design::TIMER_TRACK_H) * 0.5,
            body.w,
            design::TIMER_TRACK_H,
        );
        self.fill_round(
            rt,
            track,
            design::TIMER_TRACK_H * 0.5,
            theme.card_track().with_alpha(alpha),
        );
        let f = t.fraction.clamp(0.0, 1.0);
        if has_timer {
            if f > 0.0 {
                self.fill_round(
                    rt,
                    RectD::new(track.x, track.y, track.w * f, track.h),
                    track.h * 0.5,
                    accent.with_alpha(alpha),
                );
            }
            let thumb_x = (track.x + track.w * f - design::TIMER_THUMB_W * 0.5)
                .clamp(track.x, track.right() - design::TIMER_THUMB_W);
            self.fill_round(
                rt,
                RectD::new(
                    thumb_x,
                    py + (design::TIMER_PROGRESS_H - design::TIMER_THUMB_H) * 0.5,
                    design::TIMER_THUMB_W,
                    design::TIMER_THUMB_H,
                ),
                design::R_THUMB,
                design::white(alpha),
            );
        }

        // Start/pause, four preset chips, reset. 304 DIP total.
        let row_y = py + design::TIMER_PROGRESS_H + design::CARD_GAP;
        let chips = design::TIMER_PRESETS;
        let total_w = design::TIMER_START_W
            + design::TIMER_CHIP_GAP
            + chips.len() as f32 * design::TIMER_CHIP_W
            + chips.len() as f32 * design::TIMER_CHIP_GAP
            + design::TIMER_RESET_W;
        let mut x = body.center_x() - total_w * 0.5;

        let start = RectD::new(x, row_y, design::TIMER_START_W, design::TIMER_CTRL_H);
        let pressed = input.pressed_amount(Element::TimerToggle);
        let draw = pressed_rect(start, pressed);
        let fill = if input.hovered(Element::TimerToggle) {
            darken(accent, 0.92)
        } else {
            accent
        };
        self.fill_round(rt, draw, design::R_SURFACE, fill.with_alpha(alpha));
        self.icon_center(
            rt,
            if t.running {
                IconId::Pause
            } else {
                IconId::Play
            },
            draw,
            16.0,
            design::DARK_GLYPH.with_alpha(alpha),
            input.scale,
            0.0,
        );
        if alpha > 0.15 {
            hits.add(Element::TimerToggle, start, design::R_SURFACE);
        }
        x += design::TIMER_START_W + design::TIMER_CHIP_GAP;

        for minutes in chips {
            let r = RectD::new(x, row_y, design::TIMER_CHIP_W, design::TIMER_CTRL_H);
            let element = Element::TimerPreset(minutes);
            let pressed = input.pressed_amount(element);
            let draw = pressed_rect(r, pressed);
            let fill = if pressed.is_some() {
                theme.card_button_press()
            } else if input.hovered(element) {
                theme.card_button_hover()
            } else {
                theme.card_button()
            };
            self.fill_round(rt, draw, design::R_SURFACE, fill.with_alpha(alpha));
            self.text(
                rt,
                &format!("{minutes}m"),
                draw,
                design::FONT_CHIP,
                600,
                1,
                design::LINE_CALL_TITLE,
                theme.card_button_text().with_alpha(alpha),
                false,
            );
            if alpha > 0.15 {
                hits.add(element, r, design::R_SURFACE);
            }
            x += design::TIMER_CHIP_W + design::TIMER_CHIP_GAP;
        }

        let reset = RectD::new(x, row_y, design::TIMER_RESET_W, design::TIMER_CTRL_H);
        let pressed = input.pressed_amount(Element::TimerStop);
        let draw = pressed_rect(reset, pressed);
        if pressed.is_some() || input.hovered(Element::TimerStop) {
            let fill = if pressed.is_some() {
                theme.rail_press_fill()
            } else {
                theme.rail_hover_fill()
            };
            self.fill_round(rt, draw, design::R_SURFACE, fill.with_alpha(alpha));
        }
        self.icon_center(
            rt,
            IconId::Close,
            draw,
            16.0,
            theme.text_secondary().with_alpha(alpha),
            input.scale,
            2.0 * 24.0 / 16.0,
        );
        if alpha > 0.15 {
            hits.add(Element::TimerStop, reset, design::R_SURFACE);
        }
    }

    // ----------------------------------------------------------- decorations

    fn draw_focus_ring(
        &mut self,
        rt: &ID2D1RenderTarget,
        input: &RenderInput<'_>,
        target: HitTarget,
    ) {
        let px = 1.0 / input.scale;
        // The v0.4 rail ring is the system accent, 2 DIP at a 1 DIP offset.
        let gap = 1.0;
        let r = target.rect.inset(-gap);
        self.stroke_round(
            rt,
            r,
            target.radius + gap,
            design::FOCUS_RING_W * px,
            input.theme.rail_indicator(input.theme.accent),
            false,
        );
    }

    // --------------------------------------------------------------- artwork

    fn art_bitmap(&mut self, rt: &ID2D1RenderTarget, art: &AlbumArt) -> Option<ID2D1Bitmap> {
        if self.art_target == rt.as_raw() as isize {
            if let Some(bitmap) = self.art.get(&art.id) {
                return Some(bitmap.clone());
            }
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
        // A shelf can hold hundreds of files; keep only the recent window so
        // scrolled-away bitmaps do not pin GPU memory.
        if self.art.len() >= 24 {
            self.art.clear();
        }
        self.art.insert(art.id, bitmap.clone());
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

fn color_f(color: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
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

fn accent_of(view: &ViewModel) -> Color {
    view.main
        .as_ref()
        .map(|m| m.accent)
        .unwrap_or(design::VOLUME_WHITE)
}

fn darken(c: Color, k: f32) -> Color {
    Color {
        r: c.r * k,
        g: c.g * k,
        b: c.b * k,
        a: c.a,
    }
}

/// Snap a DIP coordinate to a whole device pixel so 1 DIP lines stay crisp at
/// fractional DPI scales.
fn snap(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}

fn snap_rect(r: RectD, scale: f32) -> RectD {
    let x = snap(r.x, scale);
    let y = snap(r.y, scale);
    RectD::new(
        x,
        y,
        (snap(r.right(), scale) - x).max(0.0),
        (snap(r.bottom(), scale) - y).max(0.0),
    )
}

/// Rail buttons are a fixed 40 x 40 stack, centred vertically in the card.
fn rail_button_rect(fly: RectD, index: usize) -> RectD {
    RectD::new(
        fly.x + (design::RAIL_W - design::RAIL_BTN) * 0.5,
        fly.y + design::RAIL_INSET + index as f32 * (design::RAIL_BTN + design::RAIL_GAP),
        design::RAIL_BTN,
        design::RAIL_BTN,
    )
}

fn rail_index(kind: ActivityKind) -> usize {
    design::RAIL_ORDER
        .iter()
        .position(|k| *k == kind)
        .unwrap_or(0)
}

fn stage_icon(index: usize) -> IconId {
    match index {
        0 => IconId::File,
        1 => IconId::Convert,
        _ => IconId::Save,
    }
}

fn rail_icon(kind: ActivityKind) -> IconId {
    match kind {
        ActivityKind::Music => IconId::RailMusic,
        ActivityKind::Timer => IconId::RailTimer,
        ActivityKind::Call => IconId::RailCall,
        ActivityKind::Shelf => IconId::RailShelf,
        ActivityKind::Clipboard => IconId::Clipboard,
    }
}

#[allow(dead_code)] // Rail hover labels land with the rail interaction (A).
fn rail_label(kind: ActivityKind) -> &'static str {
    match kind {
        ActivityKind::Music => "Music",
        ActivityKind::Timer => "Timer",
        ActivityKind::Call => "Call",
        ActivityKind::Shelf => "Shelf",
        ActivityKind::Clipboard => "Clipboard",
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
    wave_geometry_len(rt, x, cy, width, phase, amplitude, design::WAVE_LEN)
}

/// Sine polyline with an explicit wavelength (the v0.4 music card uses 14 DIP).
fn wave_geometry_len(
    rt: &ID2D1RenderTarget,
    x: f32,
    cy: f32,
    width: f32,
    phase: f32,
    amplitude: f32,
    wavelength: f32,
) -> Option<ID2D1PathGeometry> {
    if width <= 0.5 || wavelength <= 0.1 {
        return None;
    }
    let factory = unsafe { rt.GetFactory().ok()? };
    let path = unsafe { factory.CreatePathGeometry().ok()? };
    let sink = unsafe { path.Open().ok()? };
    let steps = width.ceil().max(2.0) as usize;
    let tau = std::f32::consts::TAU / wavelength;
    // SAFETY: sink belongs to the path and is closed below.
    unsafe {
        sink.BeginFigure(
            Vector2::new(x, cy + amplitude * (phase * tau).sin()),
            D2D1_FIGURE_BEGIN_HOLLOW,
        );
        for i in 1..=steps {
            let dx = width * i as f32 / steps as f32;
            let y = cy + amplitude * ((dx + phase) * tau).sin();
            sink.AddLine(Vector2::new(x + dx, y));
        }
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        if sink.Close().is_err() {
            return None;
        }
    }
    Some(path)
}

// Working and done layouts fit the inner card: tiles 18..74, bar 96..104,
// stage dots 116..142 of the 178 DIP content column, and the done state's
// check row, buttons and countdown line. Checked at compile time in every
// build, not just under `cargo test`.
const _: () = assert!(18.0 + design::CONVERT_STAGE_TILE < 96.0);
const _: () = assert!(96.0 + design::CONVERT_PROGRESS_H < 116.0);
const _: () = assert!(116.0 + design::CONVERT_STAGE_ICON < design::CONVERT_INNER_H);
const _: () = assert!(78.0 + design::CONVERT_DONE_BTN_H < design::CONVERT_INNER_H - 18.0);
const _: () = assert!((design::CONVERT_INNER_W - 12.0) / 3.0 > design::CONVERT_DONE_BTN_H * 2.0);

#[cfg(test)]
mod tests {
    use super::*;

    /// Clear stays in the toolbar for every shelf state: the empty state only
    /// returns after the toolbar and its buttons were drawn.
    #[test]
    fn clear_is_present_in_empty_populated_and_converting_shelves() {
        let source = include_str!("render.rs");
        let card = source
            .split("fn draw_shelf_card")
            .nth(1)
            .expect("draw_shelf_card exists")
            .split("fn shelf_tool_button")
            .next()
            .expect("toolbar helper follows");
        let clear = card.find("Element::ShelfClear").expect("Clear hit target");
        let empty = card.find("if s.total == 0").expect("empty state branch");
        assert!(clear < empty, "Clear is drawn before the early return");
        assert!(
            card.contains("let disabled = s.converting"),
            "the converting state disables the same toolbar instead of hiding it"
        );
    }

    #[test]
    fn stages_use_file_convert_and_save_glyphs() {
        assert!(matches!(stage_icon(0), IconId::File));
        assert!(matches!(stage_icon(1), IconId::Convert));
        assert!(matches!(stage_icon(2), IconId::Save));
        // Out-of-range indices clamp to the last stage, never panic.
        assert!(matches!(stage_icon(9), IconId::Save));
    }
}
