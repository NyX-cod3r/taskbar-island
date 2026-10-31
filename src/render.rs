//! render.rs — GPU rendering for the pill.
//!
//! Pipeline: D3D11 device → DXGI swapchain (flip, `DXGI_ALPHA_MODE_PREMULTIPLIED`)
//! hosted in a DirectComposition visual tree on the HWND → Direct2D device
//! context draws into the backbuffer. This is what gives the window per-pixel
//! alpha without `WS_EX_LAYERED`, so the pill blends with the real taskbar
//! pixels underneath (no blur, just alpha — matching the taskbar highlight).
//!
//! Renders on demand only: once at startup, on resize, and (later phases)
//! while an animation is active. There is no frame loop.

use crate::activities::media::MediaUpdate;
use crate::log::{log, log_err};
use crate::theme::Theme;
use core::mem::ManuallyDrop;
use windows::core::{w, Interface, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateDevice, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Image,
    ID2D1SolidColorBrush, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows_numerics::Vector2;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11Texture2D, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::DCompositionCreateDevice;
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED as DXGI_ALPHA_PREMULTIPLIED, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

/// Taskbar-highlight colors come from theme.rs; the radius is the design spec.
const CORNER_RADIUS_DIP: f32 = 8.0;
/// Hover feedback: highlight-alpha multipliers. Applied only on the
/// mouse-enter/leave transition events, never per frame.
const HOVER_FILL_MULT: f32 = 2.6;
const HOVER_BORDER_MULT: f32 = 2.0;
/// Media row layout (DIP): the bars column sits on the left, the track title
/// starts right of it and is clipped at the right padding. The artist line is
/// a touch smaller and appears only while expanded.
const TEXT_SIZE_DIP: f32 = 12.0;
const ARTIST_SIZE_DIP: f32 = 11.0;
const TEXT_LEFT_DIP: f32 = 34.0;
const TEXT_RIGHT_PAD_DIP: f32 = 6.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    Idle,
    Media,
    /// Wired up when timer activities render (later phase).
    #[allow(dead_code)]
    Timer,
}

pub struct Renderer {
    swapchain: IDXGISwapChain1,
    // Keep the device alive for the lifetime of its target/visual tree.
    #[allow(dead_code)]
    comp_device: IDCompositionDevice,
    // DirectComposition does not guarantee that the HWND target or root visual
    // outlive their last COM references. Keep both alive with the swapchain.
    #[allow(dead_code)]
    comp_target: IDCompositionTarget,
    #[allow(dead_code)]
    comp_visual: IDCompositionVisual,
    d2d_context: ID2D1DeviceContext,
    fill_brush: ID2D1SolidColorBrush,
    border_brush: ID2D1SolidColorBrush,
    text_format: IDWriteTextFormat,
    /// Artist line format (slightly smaller) for the expanded pill.
    artist_format: IDWriteTextFormat,
    /// Retained: the ellipsis trimming sign is created from this at draw time.
    dwrite_factory: IDWriteFactory,
    text_brush: ID2D1SolidColorBrush,
    /// Artist line brush; recolored per draw with the expansion fade.
    artist_brush: ID2D1SolidColorBrush,
    /// Single-line, ellipsis-trimmed title layout — rebuilt only when the
    /// title or available width changes (keyed cache below).
    text_layout: Option<IDWriteTextLayout>,
    text_layout_key: (String, f32),
    artist_layout: Option<IDWriteTextLayout>,
    artist_layout_key: (String, f32),
    /// Hover-expansion progress (0 compact — 1 expanded), driven by the UI's
    /// animation timer; pure render input, no timing logic here.
    expand: f32,
    /// Backbuffer size in physical pixels.
    size: (u32, u32),
    /// DIPs → physical pixels (dpi / 96).
    scale: f32,
    indicator: Indicator,
    theme: Theme,
    /// Mouse-hover feedback state (WM_MOUSEMOVE/LEAVE transitions).
    hover: bool,
    /// Current media snapshot; `None` = nothing playing.
    media: Option<MediaUpdate>,
}

impl Renderer {
    /// Build the whole device chain for `hwnd` at `size` physical px.
    ///
    /// # Safety
    /// `hwnd` must be a valid window created with `WS_EX_NOREDIRECTIONBITMAP`
    /// (required by `IDCompositionDevice::CreateTargetForHwnd`).
    pub unsafe fn create(hwnd: HWND, size: (u32, u32), scale: f32, theme: Theme) -> Result<Self> {
        log("create: d3d…");
        // --- Direct3D device (BGRA support is required by Direct2D-on-DXGI) ---
        let mut d3d_device: Option<ID3D11Device> = None;
        let mut feature_level = core::mem::MaybeUninit::uninit();
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            Some(feature_level.as_mut_ptr()),
            None,
        )
        .inspect_err(|e| {
            log_err("create.D3D11CreateDevice", e);
        })?;
        let d3d_device = d3d_device.expect("D3D11CreateDevice succeeded without a device");

        // --- DXGI pieces: the factory that created the device's adapter, plus
        //     the device itself for swapchain/DComp interop. ---
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let adapter = dxgi_device.GetAdapter()?;
        if let Ok(desc) = adapter.GetDesc() {
            let name: Vec<u16> = desc
                .Description
                .iter()
                .copied()
                .take_while(|&c| c != 0)
                .collect();
            log(&format!(
                "create: adapter = {}",
                String::from_utf16_lossy(&name)
            ));
        }
        let factory: IDXGIFactory2 = adapter.GetParent()?;
        log("create: dxgi ok");

        // --- Premultiplied-alpha flip swapchain hosted by the window. ---
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: size.0,
            Height: size.1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_PREMULTIPLIED,
            ..Default::default()
        };
        // --- Premultiplied-alpha flip swapchain hosted in the composition tree.
        //     CreateSwapChainForHwnd rejects DXGI_ALPHA_MODE_PREMULTIPLIED on
        //     current Windows 11 (verified: 0x887A0001 for every swap effect),
        //     so the swapchain is created for composition instead. The HWND
        //     still owns the on-screen region via the DComp target below, and
        //     we drive Present/ResizeBuffers ourselves. ---
        let swapchain = factory
            .CreateSwapChainForComposition(&d3d_device, &desc, None)
            .inspect_err(|e| {
                log_err("create.CreateSwapChainForComposition", e);
            })?;
        log("create: swapchain ok");

        // --- DirectComposition tree: target(root(visual(content = swapchain))) ---
        let comp_device: IDCompositionDevice =
            DCompositionCreateDevice(&dxgi_device).inspect_err(|e| {
                log_err("create.DCompositionCreateDevice", e);
            })?;
        let comp_target = comp_device.CreateTargetForHwnd(hwnd, true).inspect_err(|e| {
            log_err("create.CreateTargetForHwnd", e);
        })?;
        let comp_visual = comp_device.CreateVisual()?;
        comp_visual.SetContent(&swapchain)?;
        comp_target.SetRoot(&comp_visual)?;
        // Commit BEFORE the first Present so the visual<->swapchain binding is
        // on record with DWM ahead of any frame (spec order: Commit, then Present).
        comp_device.Commit().inspect_err(|e| {
            log_err("create.dcomp.Commit", e);
        })?;
        log("create: dcomp ok (commit-before-present)");

        // --- Direct2D device + context + the two theme brushes. Brushes are
        //     context-bound, not backbuffer-bound, so they survive resizes. ---
        let d2d_device: ID2D1Device = D2D1CreateDevice(&dxgi_device, None)?;
        let d2d_context = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let fill_brush = d2d_context.CreateSolidColorBrush(&Self::color(theme.fill()), None)?;
        let border_brush = d2d_context.CreateSolidColorBrush(&Self::color(theme.border()), None)?;

        // Track-title text format via DirectWrite (windows-rs routes D2D text
        // through IDWriteTextFormat). Sizes are DIPs — the context works in
        // DIPs because the backbuffer bitmap carries the scaled DPI.
        let text_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let text_format = text_factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            TEXT_SIZE_DIP,
            w!("en-US"),
        )?;
        text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let artist_format = text_factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            ARTIST_SIZE_DIP,
            w!("en-US"),
        )?;
        artist_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        artist_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let text_brush = d2d_context.CreateSolidColorBrush(&Self::color(theme.text()), None)?;
        let artist_brush = d2d_context.CreateSolidColorBrush(&Self::color(theme.text()), None)?;

        log(&format!("create ok: size={:?} scale={scale}", size));
        Ok(Self {
            swapchain,
            comp_device,
            comp_target,
            comp_visual,
            d2d_context,
            fill_brush,
            border_brush,
            text_format,
            artist_format,
            dwrite_factory: text_factory,
            text_brush,
            artist_brush,
            text_layout: None,
            text_layout_key: (String::new(), 0.0),
            artist_layout: None,
            artist_layout_key: (String::new(), 0.0),
            size,
            scale,
            indicator: Indicator::Idle,
            theme,
            hover: false,
            media: None,
            expand: 0.0,
        })
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        let fill = Self::color(theme.fill());
        let border = Self::color(theme.border());
        unsafe {
            self.fill_brush.SetColor(&fill);
            self.border_brush.SetColor(&border);
        }
    }

    /// Wrap the swapchain's *current* back buffer as the D2D target bitmap.
    /// Must be called before every draw: flip-model Present rotates the buffer
    /// queue, so a bitmap cached across a Present points at a stale buffer.
    unsafe fn wrap_backbuffer(
        ctx: &ID2D1DeviceContext,
        swapchain: &IDXGISwapChain1,
        scale: f32,
    ) -> Result<ID2D1Bitmap1> {
        // Release the previous wrapper first — no refs may overlap a GetBuffer.
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

    fn color((r, g, b, a): (f32, f32, f32, f32)) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r, g, b, a }
    }

    /// Current backbuffer size in physical pixels (used by later phases).
    #[allow(dead_code)]
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Kept for later phases (timer bars); media state now flows through
    /// `set_media`, which derives the indicator itself.
    #[allow(dead_code)]
    pub fn set_indicator(&mut self, indicator: Indicator) {
        self.indicator = indicator;
    }

    /// Hover feedback. Returns `true` on a transition so callers redraw only
    /// on enter/leave events.
    pub fn set_hover(&mut self, hover: bool) -> bool {
        if self.hover == hover {
            return false;
        }
        self.hover = hover;
        true
    }

    /// Replace the media snapshot (`None` = nothing playing). Kept deduped:
    /// returns `true` when the visible state changed, so callers can skip
    /// no-op redraws. Derives the indicator internally so the two states can
    /// never disagree.
    pub fn set_media(&mut self, media: Option<MediaUpdate>) -> bool {
        if self.media == media {
            return false;
        }
        self.indicator = if media.is_some() {
            Indicator::Media
        } else {
            Indicator::Idle
        };
        self.media = media;
        true
    }

    /// Hover-expansion progress (0..1), set by the animation timer ticks.
    pub fn set_expand(&mut self, expand: f32) {
        self.expand = expand;
    }

    /// Single-line ellipsized layout, cached by (text, max width); rebuilt
    /// only when either changes. Shared by the title and artist lines.
    ///
    /// # Safety
    /// DirectWrite factory/format COM calls; callers draw on the UI thread
    /// inside existing unsafe blocks.
    unsafe fn ensure_layout<'a>(
        factory: &IDWriteFactory,
        format: &IDWriteTextFormat,
        cache: &'a mut Option<IDWriteTextLayout>,
        cache_key: &mut (String, f32),
        text: &str,
        max_width: f32,
        max_height: f32,
    ) -> Result<&'a IDWriteTextLayout> {
        if cache_key.0 != text || (cache_key.1 - max_width).abs() > f32::EPSILON {
            let chars: Vec<u16> = text.encode_utf16().collect();
            let layout =
                factory.CreateTextLayout(&chars, format, max_width, max_height)?;
            layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            // A plain "…" is attached through the sign object.
            let sign = factory.CreateEllipsisTrimmingSign(&layout)?;
            layout.SetTrimming(
                &DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                },
                &sign,
            )?;
            *cache = Some(layout);
            *cache_key = (text.to_owned(), max_width);
        }
        Ok(cache.as_ref().expect("layout cached"))
    }

    /// Resize the backbuffer for a new physical-pixel size. No-op if unchanged.
    pub unsafe fn resize(&mut self, size: (u32, u32), scale: f32) -> Result<()> {
        if size == self.size && scale == self.scale {
            return Ok(());
        }
        log(&format!(
            "resize: {:?} -> {:?} scale {scale}",
            self.size, size
        ));
        // No target/bitmap is cached across draws (each draw wraps the current
        // backbuffer and drops it before Present), so no backbuffer reference
        // is outstanding here — ResizeBuffers is safe.
        self.swapchain
            .ResizeBuffers(
                0,
                size.0,
                size.1,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )
            .inspect_err(|e| {
                log_err("resize.ResizeBuffers", e);
            })?;
        self.size = size;
        self.scale = scale;
        Ok(())
    }

    /// Draw the pill once and present. Static content → single Present, then
    /// let DirectComposition pick up the new frame.
    pub unsafe fn draw(&mut self) -> Result<()> {
        let w = self.size.0 as f32 / self.scale;
        let h = self.size.1 as f32 / self.scale;
        let radius = CORNER_RADIUS_DIP.min(w.min(h) / 2.0);
        let shape = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: h,
            },
            radiusX: radius,
            radiusY: radius,
        };
        // Appearance per event: hover bumps the highlight, pause dims the
        // content, expansion fades the artist line in. All re-color the
        // existing brushes — no allocations on the draw path.
        let playing = self.media.as_ref().is_none_or(|media| media.playing);
        let dim = if playing { 1.0 } else { 0.55 };
        let mut fill = self.theme.fill();
        let mut border = self.theme.border();
        if self.hover {
            fill.3 = (fill.3 * HOVER_FILL_MULT).min(1.0);
            border.3 = (border.3 * HOVER_BORDER_MULT).min(1.0);
        }
        let text_base = self.theme.text();
        self.fill_brush.SetColor(&Self::color(fill));
        self.border_brush.SetColor(&Self::color(border));
        self.text_brush.SetColor(&Self::color((
            text_base.0,
            text_base.1,
            text_base.2,
            text_base.3 * dim,
        )));
        self.artist_brush.SetColor(&Self::color((
            text_base.0,
            text_base.1,
            text_base.2,
            text_base.3 * 0.8 * self.expand.clamp(0.0, 1.0) * dim,
        )));
        // Wrap the *current* back buffer for this draw only. The bitmap (and
        // the buffer reference it holds) MUST be dropped before Present: with
        // CreateSwapChainForComposition, DWM cannot scan out a frame while any
        // reference to the back buffer is outstanding — Present still returns
        // S_OK, but the frame is silently dropped. So the target lives in a
        // local scope here and is never cached on `self`.
        {
            let target = Self::wrap_backbuffer(&self.d2d_context, &self.swapchain, self.scale)?;

            self.d2d_context.BeginDraw();
            self.d2d_context.Clear(None); // fully transparent — taskbar shows through
            self.d2d_context
                .FillRoundedRectangle(&shape, &self.fill_brush);
            if self.indicator != Indicator::Idle {
                // Playing: uneven bars; paused: flat bars, still dimmed text.
                let heights: &[f32] = if playing {
                    &[7.0, 13.0, 9.0, 16.0]
                } else {
                    &[11.0, 11.0, 11.0, 11.0]
                };
                for (index, height) in heights.iter().enumerate() {
                    let x = 12.0 + index as f32 * 4.0;
                    self.d2d_context.FillRectangle(
                        &D2D_RECT_F {
                            left: x,
                            top: (h - height) / 2.0,
                            right: x + 2.0,
                            bottom: (h + height) / 2.0,
                        },
                        &self.border_brush,
                    );
                }
            }
            // Track title, single-line ellipsized; while expanded the title
            // rides up and the artist line fades in underneath. Layouts are
            // cached by (text, max width) and rebuilt only on change.
            if let Some(media) = &self.media {
                let title = &media.title;
                if !title.is_empty() {
                    let max_width = w - TEXT_LEFT_DIP - TEXT_RIGHT_PAD_DIP;
                    let artist_line = self.expand > 0.05 && !media.artist.is_empty();
                    let (title_top, artist_top) = if artist_line {
                        let block = TEXT_SIZE_DIP + 2.0 + ARTIST_SIZE_DIP;
                        ((h - block) / 2.0, (h - block) / 2.0 + TEXT_SIZE_DIP + 2.0)
                    } else {
                        ((h - TEXT_SIZE_DIP) / 2.0, 0.0)
                    };
                    let title_layout = Self::ensure_layout(
                        &self.dwrite_factory,
                        &self.text_format,
                        &mut self.text_layout,
                        &mut self.text_layout_key,
                        title,
                        max_width,
                        TEXT_SIZE_DIP * 1.4,
                    )?;
                    self.d2d_context.DrawTextLayout(
                        Vector2 {
                            X: TEXT_LEFT_DIP,
                            Y: title_top,
                        },
                        title_layout,
                        &self.text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                    if artist_line {
                        let artist_layout = Self::ensure_layout(
                            &self.dwrite_factory,
                            &self.artist_format,
                            &mut self.artist_layout,
                            &mut self.artist_layout_key,
                            &media.artist,
                            max_width,
                            ARTIST_SIZE_DIP * 1.4,
                        )?;
                        self.d2d_context.DrawTextLayout(
                            Vector2 {
                                X: TEXT_LEFT_DIP,
                                Y: artist_top,
                            },
                            artist_layout,
                            &self.artist_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                        );
                    }
                }
            }
            self.d2d_context.EndDraw(None, None).inspect_err(|e| {
                log_err("draw.EndDraw", e);
            })?;

            // Release the target + its back-buffer reference before Present.
            self.d2d_context.SetTarget(None::<&ID2D1Image>);
            drop(target);
        }

        // The visual tree was committed when it was created. Presenting a
        // composition swapchain updates that visual directly; calling Commit
        // again here can fail with DXGI_ERROR_INVALID_CALL while the initial
        // asynchronous commit is still pending.
        // Do not wait for a compositor refresh on the UI thread.  The visual
        // is static and updates only on events, so allowing DWM to pick up the
        // latest frame asynchronously is both sufficient and keeps input and
        // taskbar notifications responsive.
        let hr = self.swapchain.Present(0, DXGI_PRESENT(0));
        if hr.is_err() {
            log_err("draw.Present", &windows::core::Error::from(hr));
            return Err(windows::core::Error::from(hr));
        }
        log(&format!("draw ok: size={:?}", self.size));
        Ok(())
    }
}

// The ID2D1Device that produced d2d_context is kept alive by the context
// itself (COM refcount), so dropping it here is fine.
