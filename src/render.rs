//! render.rs — GPU rendering for the compact taskbar pill.
//!
//! Pipeline: D3D11 device → DXGI swapchain (flip, `DXGI_ALPHA_MODE_PREMULTIPLIED`)
//! hosted in a DirectComposition visual tree on the HWND → Direct2D device
//! context draws into the backbuffer.
//!
//! The compact pill stays put and never resizes or scales on hover. Only its fill
//! and border brighten.

use crate::activities::call::CallState;
use crate::activities::media::MediaUpdate;
use crate::activities::shelf::ShelfView;
use crate::activities::timer::TimerView;
use crate::activities::volume::VolumeActivity;
use crate::state::ActivityId;
use crate::theme::Theme;
use core::mem::ManuallyDrop;
use std::time::Instant;
use windows::core::{w, Interface, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, ID2D1Image,
    ID2D1SolidColorBrush, ID2D1StrokeStyle,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DASH_STYLE_DASH, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteTextFormat,
};
use windows_numerics::Vector2;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED as DXGI_ALPHA_PREMULTIPLIED, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGISurface, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

const TEXT_SIZE_DIP: f32 = 12.0;

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    Idle,
    Media,
    Call,
    Timer,
    Volume,
    Files(usize),
}

pub struct Renderer {
    swapchain: IDXGISwapChain1,
    #[allow(dead_code)]
    comp_target: IDCompositionTarget,
    #[allow(dead_code)]
    comp_visual: IDCompositionVisual,
    d2d_context: ID2D1DeviceContext,
    text_format: IDWriteTextFormat,
    bold_format: IDWriteTextFormat,
    center_format: IDWriteTextFormat,
    /// Dashed stroke for the drag-over drop target (Design Spec 06).
    dash_style: ID2D1StrokeStyle,
    #[allow(dead_code)]
    dwrite_factory: IDWriteFactory,

    size: (u32, u32),
    scale: f32,
    theme: Theme,
    hover: bool,
    pressed: bool,
    pub drop_hover: bool,
    /// Hover transition (spec: 200 ms ease, fill and border only).
    hover_mix: f32,
    hover_from: f32,
    hover_target: f32,
    hover_anim_start: Option<Instant>,

    // Active state
    pub active_id: ActivityId,
    pub media: Option<MediaUpdate>,
    pub call: Option<CallState>,
    pub shelf: Option<ShelfView>,
    pub timer: Option<TimerView>,
    pub volume: Option<VolumeActivity>,
}

/// cubic-bezier(.2, .8, .2, 1): fast-out, no overshoot (Design Spec motion).
fn ease_out(t: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * 0.8 + 3.0 * u * t * t + t * t * t
}

/// Component-wise RGBA interpolation used by the hover fill/border tween.
fn lerp_rgba(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32), t: f32) -> (f32, f32, f32, f32) {
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
        a.3 + (b.3 - a.3) * t,
    )
}

impl Renderer {
    pub unsafe fn create(hwnd: HWND, size: (u32, u32), scale: f32, theme: Theme) -> Result<Self> {
        let gpu = crate::gpu::shared()?;

        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: size.0.max(1),
            Height: size.1.max(1),
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_PREMULTIPLIED,
            ..Default::default()
        };

        let swapchain = gpu.dxgi_factory.CreateSwapChainForComposition(&gpu.d3d, &desc, None)?;

        let comp_target = gpu.comp.CreateTargetForHwnd(hwnd, true)?;
        let comp_visual = gpu.comp.CreateVisual()?;
        comp_visual.SetContent(&swapchain)?;
        comp_target.SetRoot(&comp_visual)?;
        gpu.comp.Commit()?;

        let d2d_context = gpu.d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

        // Dashed 1 px stroke used only for the drag-over drop target.
        let dash_style = d2d_context.GetFactory()?.CreateStrokeStyle(
            &D2D1_STROKE_STYLE_PROPERTIES {
                dashStyle: D2D1_DASH_STYLE_DASH,
                ..Default::default()
            },
            None,
        )?;

        let text_factory = gpu.dwrite.clone();

        let text_format = text_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            TEXT_SIZE_DIP,
            w!("en-US"),
        )?;
        text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        text_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

        let bold_format = text_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            TEXT_SIZE_DIP,
            w!("en-US"),
        )?;
        bold_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        bold_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        bold_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

        let center_format = text_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            TEXT_SIZE_DIP,
            w!("en-US"),
        )?;
        center_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        center_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        center_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

        Ok(Self {
            swapchain,
            comp_target,
            comp_visual,
            d2d_context,
            text_format,
            bold_format,
            center_format,
            dash_style,
            dwrite_factory: text_factory,
            size,
            scale,
            theme,
            hover: false,
            pressed: false,
            drop_hover: false,
            hover_mix: 0.0,
            hover_from: 0.0,
            hover_target: 0.0,
            hover_anim_start: None,
            active_id: ActivityId::Media,
            media: Some(MediaUpdate::demo()),
            call: None,
            shelf: None,
            timer: None,
            volume: None,
        })
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn set_hover(&mut self, hover: bool) -> bool {
        if self.hover == hover {
            return false;
        }
        self.hover = hover;
        self.retarget_hover();
        true
    }

    pub fn set_pressed(&mut self, pressed: bool) -> bool {
        if self.pressed == pressed {
            return false;
        }
        self.pressed = pressed;
        self.retarget_hover();
        true
    }

    /// Starts the 200 ms ease toward the current hover/press target.
    /// Drop-hover is deliberately instant (it tracks an OLE drag, not the pointer).
    fn retarget_hover(&mut self) {
        let target = if self.hover || self.pressed { 1.0 } else { 0.0 };
        if (target - self.hover_target).abs() < f32::EPSILON {
            return;
        }
        self.hover_from = self.hover_mix;
        self.hover_target = target;
        self.hover_anim_start = Some(Instant::now());
    }

    /// Advances the hover tween. Returns `(needs_redraw, still_animating)`.
    pub fn tick_hover(&mut self, now: Instant) -> (bool, bool) {
        let Some(started) = self.hover_anim_start else {
            return (false, false);
        };
        let t = (now.duration_since(started).as_secs_f32() / 0.2).min(1.0);
        // cubic-bezier(.2, .8, .2, 1), same ease as the flyout open motion.
        let eased = ease_out(t);
        self.hover_mix = self.hover_from + (self.hover_target - self.hover_from) * eased;
        if t >= 1.0 {
            self.hover_mix = self.hover_target;
            self.hover_anim_start = None;
            (true, false)
        } else {
            (true, true)
        }
    }

    pub fn set_drop_hover(&mut self, on: bool) -> bool {
        if self.drop_hover == on {
            return false;
        }
        self.drop_hover = on;
        true
    }

    #[allow(dead_code)]
    pub fn set_media(&mut self, media: Option<MediaUpdate>) -> bool {
        if self.media == media {
            return false;
        }
        self.media = media;
        true
    }

    #[allow(dead_code)]
    pub fn set_shelf(&mut self, shelf: Option<ShelfView>) -> bool {
        if self.shelf == shelf {
            return false;
        }
        self.shelf = shelf;
        true
    }

    #[allow(dead_code)]
    pub fn set_timer(&mut self, timer: Option<TimerView>) -> bool {
        if self.timer == timer {
            return false;
        }
        self.timer = timer;
        true
    }

    #[allow(dead_code)]
    pub fn set_call(&mut self, call: Option<CallState>) -> bool {
        if self.call == call {
            return false;
        }
        self.call = call;
        true
    }

    #[allow(dead_code)]
    pub fn set_volume(&mut self, vol: Option<VolumeActivity>) -> bool {
        if self.volume == vol {
            return false;
        }
        self.volume = vol;
        true
    }

    pub unsafe fn resize(&mut self, size: (u32, u32), scale: f32) -> Result<()> {
        if size == self.size && (scale - self.scale).abs() < f32::EPSILON {
            return Ok(());
        }
        self.swapchain.ResizeBuffers(
            0,
            size.0.max(1),
            size.1.max(1),
            DXGI_FORMAT_UNKNOWN,
            DXGI_SWAP_CHAIN_FLAG(0),
        )?;
        self.size = size;
        self.scale = scale;
        Ok(())
    }

    unsafe fn wrap_backbuffer(
        ctx: &ID2D1DeviceContext,
        swapchain: &IDXGISwapChain1,
        scale: f32,
    ) -> Result<ID2D1Bitmap1> {
        ctx.SetTarget(None::<&ID2D1Image>);
        let back_buffer: ID3D11Texture2D = swapchain.GetBuffer(0)?;
        let surface: IDXGISurface = back_buffer.cast()?;
        let dpi = 96.0 * scale;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi,
            dpiY: dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: ManuallyDrop::new(None),
        };
        let bitmap = ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
        ctx.SetTarget(&bitmap);
        Ok(bitmap)
    }

    unsafe fn draw_text(
        &self,
        string: &[u16],
        format: &IDWriteTextFormat,
        rect: &D2D_RECT_F,
        brush: &ID2D1SolidColorBrush,
    ) {
        self.d2d_context.DrawText(
            string,
            format,
            rect,
            brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }

    pub unsafe fn draw(&mut self) -> Result<()> {
        let w = self.size.0 as f32 / self.scale;
        let h = self.size.1 as f32 / self.scale;
        let radius = self.theme.radius_dip.min(w.min(h) / 2.0);

        let shape = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h },
            radiusX: radius,
            radiusY: radius,
        };

        // Fill & border per spec:
        // Idle: white 9%, border 10%
        // Hover / Pressed: fill 15%, border 20% (200 ms ease)
        let (fill, border) = if self.drop_hover {
            (self.theme.hover_fill(), self.theme.hover_border())
        } else {
            let idle_fill = self.theme.fill();
            let hover_fill = self.theme.hover_fill();
            let idle_border = self.theme.border();
            let hover_border = self.theme.hover_border();
            let m = self.hover_mix;
            (
                lerp_rgba(idle_fill, hover_fill, m),
                lerp_rgba(idle_border, hover_border, m),
            )
        };

        let text_color = self.theme.text();
        let sec_color = self.theme.secondary_text();
        let accent = self.theme.accent_color();

        let fill_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: fill.0, g: fill.1, b: fill.2, a: fill.3 },
            None,
        )?;
        let border_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: border.0, g: border.1, b: border.2, a: border.3 },
            None,
        )?;
        let text_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: text_color.0, g: text_color.1, b: text_color.2, a: text_color.3 },
            None,
        )?;
        let sec_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: sec_color.0, g: sec_color.1, b: sec_color.2, a: sec_color.3 },
            None,
        )?;
        let accent_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: accent.0, g: accent.1, b: accent.2, a: 1.0 },
            None,
        )?;

        {
            let target = Self::wrap_backbuffer(&self.d2d_context, &self.swapchain, self.scale)?;
            self.d2d_context.BeginDraw();
            self.d2d_context.Clear(None);

            self.d2d_context.FillRoundedRectangle(&shape, &fill_brush);

            if self.drop_hover {
                // Dashed drop target border (Design Spec 06).
                self.d2d_context.DrawRoundedRectangle(
                    &shape,
                    &border_brush,
                    1.0,
                    Some(&self.dash_style),
                );
            } else {
                self.d2d_context.DrawRoundedRectangle(&shape, &border_brush, 1.0, None);
            }

            if self.drop_hover {
                // "Drop to hold" with a down arrow (Design Spec 06).
                let drop_sym: Vec<u16> = "\u{2193}  Drop to hold".encode_utf16().collect();
                self.draw_text(
                    &drop_sym,
                    &self.center_format,
                    &D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h },
                    &text_brush,
                );
            } else {
                match self.active_id {
                    ActivityId::Idle => {
                        // Idle pill: material only, no content (Design Spec 01).
                    }
                    ActivityId::Media => {
                        // Compact Music (184 wide)
                        // Art (20x20), vertically centered
                        let art_top = (h - 20.0) / 2.0;
                        let art_rect = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: 6.0, top: art_top, right: 26.0, bottom: art_top + 20.0 },
                            radiusX: 4.0,
                            radiusY: 4.0,
                        };
                        let cover_brush = self.d2d_context.CreateSolidColorBrush(
                            &D2D1_COLOR_F { r: 0.15, g: 0.50, b: 0.85, a: 1.0 },
                            None,
                        )?;
                        self.d2d_context.FillRoundedRectangle(&art_rect, &cover_brush);

                        // Title
                        let title = self.media.as_ref().map(|m| m.title.as_str()).unwrap_or("Deewana Kar Raha ...");
                        let title_w: Vec<u16> = title.encode_utf16().collect();
                        self.draw_text(
                            &title_w,
                            &self.text_format,
                            &D2D_RECT_F { left: 32.0, top: 0.0, right: w - 24.0, bottom: h },
                            &text_brush,
                        );

                        // 3-bar equalizer at right
                        let playing = self.media.as_ref().map(|m| m.playing).unwrap_or(true);
                        let bars = if playing { [12.0, 7.0, 14.0] } else { [9.0, 9.0, 9.0] };
                        for (i, bar_h) in bars.iter().enumerate() {
                            let bx = w - 18.0 + i as f32 * 4.0;
                            self.d2d_context.FillRectangle(
                                &D2D_RECT_F {
                                    left: bx,
                                    top: (h - bar_h) / 2.0,
                                    right: bx + 2.0,
                                    bottom: (h + bar_h) / 2.0,
                                },
                                &accent_brush,
                            );
                        }
                    }
                    ActivityId::Call => {
                        // Compact Call (176 wide)
                        // Green mic dot, vertically centered
                        let green = Theme::call_color();
                        let green_brush = self.d2d_context.CreateSolidColorBrush(
                            &D2D1_COLOR_F { r: green.0, g: green.1, b: green.2, a: 1.0 },
                            None,
                        )?;
                        self.d2d_context.FillEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: 16.0, Y: h / 2.0 }, radiusX: 4.0, radiusY: 4.0 },
                            &green_brush,
                        );

                        // Name: "Priya"
                        let name = self.call.as_ref().map(|c| c.name.as_str()).unwrap_or("Priya");
                        let name_w: Vec<u16> = name.encode_utf16().collect();
                        self.draw_text(
                            &name_w,
                            &self.bold_format,
                            &D2D_RECT_F { left: 28.0, top: 0.0, right: 110.0, bottom: h },
                            &text_brush,
                        );

                        // Timer: "02:14"
                        let duration = self.call.as_ref().map(|c| c.duration_str.as_str()).unwrap_or("02:14");
                        let dur_w: Vec<u16> = duration.encode_utf16().collect();
                        self.draw_text(
                            &dur_w,
                            &self.text_format,
                            &D2D_RECT_F { left: 116.0, top: 0.0, right: w - 8.0, bottom: h },
                            &sec_brush,
                        );
                    }
                    ActivityId::Shelf => {
                        // Compact File shelf (136 wide):
                        // Folder icon at x=14
                        let folder_sym: Vec<u16> = "[F]".encode_utf16().collect();
                        self.draw_text(
                            &folder_sym,
                            &self.bold_format,
                            &D2D_RECT_F { left: 14.0, top: 0.0, right: 38.0, bottom: h },
                            &text_brush,
                        );

                        let count = self.shelf.as_ref().map(|s| s.count).unwrap_or(3);
                        let count_str = format!("{count} files");
                        let count_w: Vec<u16> = count_str.encode_utf16().collect();
                        self.draw_text(
                            &count_w,
                            &self.text_format,
                            &D2D_RECT_F { left: 42.0, top: 0.0, right: w - 8.0, bottom: h },
                            &text_brush,
                        );
                    }
                    ActivityId::Timer => {
                        // Compact Timer (116 wide):
                        // Ring icon, vertically centered
                        self.d2d_context.DrawEllipse(
                            &D2D1_ELLIPSE { point: Vector2 { X: 18.0, Y: h / 2.0 }, radiusX: 6.5, radiusY: 6.5 },
                            &accent_brush,
                            2.0,
                            None,
                        );

                        let time_str = self.timer.as_ref().map(|t| t.title.as_str()).unwrap_or("12:40");
                        let time_w: Vec<u16> = time_str.encode_utf16().collect();
                        self.draw_text(
                            &time_w,
                            &self.bold_format,
                            &D2D_RECT_F { left: 34.0, top: 0.0, right: w - 8.0, bottom: h },
                            &text_brush,
                        );
                    }
                    ActivityId::Volume => {
                        // Compact Volume HUD (224 wide):
                        // Speaker icon at x=14
                        let spk_w: Vec<u16> = "(())".encode_utf16().collect();
                        self.draw_text(
                            &spk_w,
                            &self.bold_format,
                            &D2D_RECT_F { left: 14.0, top: 0.0, right: 40.0, bottom: h },
                            &text_brush,
                        );

                        // Level bar: x=44..176
                        let level = self.volume.as_ref().map(|v| v.level).unwrap_or(42) as f32 / 100.0;
                        let bar_w = 130.0;
                        let track_brush = self.d2d_context.CreateSolidColorBrush(
                            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.15 },
                            None,
                        )?;
                        let bar_top = h / 2.0 - 2.0;
                        self.d2d_context.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: 44.0, top: bar_top, right: 44.0 + bar_w, bottom: bar_top + 4.0 },
                                radiusX: 2.0,
                                radiusY: 2.0,
                            },
                            &track_brush,
                        );
                        self.d2d_context.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: 44.0, top: bar_top, right: 44.0 + bar_w * level, bottom: bar_top + 4.0 },
                                radiusX: 2.0,
                                radiusY: 2.0,
                            },
                            &text_brush,
                        );

                        // Value: "42"
                        let val_str = format!("{}", (level * 100.0).round() as u32);
                        let val_w: Vec<u16> = val_str.encode_utf16().collect();
                        self.draw_text(
                            &val_w,
                            &self.bold_format,
                            &D2D_RECT_F { left: 184.0, top: 0.0, right: w - 8.0, bottom: h },
                            &text_brush,
                        );
                    }
                }
            }

            self.d2d_context.EndDraw(None, None)?;
            self.d2d_context.SetTarget(None::<&ID2D1Image>);
            drop(target);
        }

        let _ = self.swapchain.Present(0, DXGI_PRESENT(0));
        Ok(())
    }
}
