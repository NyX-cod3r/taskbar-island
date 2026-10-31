//! flyout.rs — Windows 11 style flyout window for Taskbar Island.
//!
//! "Expanded opens as a Windows-style flyout 12 px above the taskbar, centered
//! over the pill, like Quick Settings. The flyout uses the Windows dark flyout
//! material, with the system accent color for controls."
//!
//! Motion: 180 ms fade, 220 ms scale .94 to 1 and rise 6 px, cubic-bezier(.2, .8, .2, 1).
//! Close: 110 ms fade and scale back, no delay.
//! Auto-close: 700 ms after pointer leaves pill + flyout.

use std::cell::RefCell;
use std::mem::{size_of, ManuallyDrop};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

use windows::core::{w, Interface, Result, PCWSTR};
use windows::Win32::Foundation::{
    HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1DeviceContext, ID2D1Image, ID2D1SolidColorBrush,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ELLIPSE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP,
    IDWriteFactory, IDWriteTextFormat,
};
use windows_numerics::Vector2;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
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
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, GetWindowRect, KillTimer, LoadCursorW,
    PostMessageW, RegisterClassExW, SetTimer, SetWindowPos, SetWindowsHookExW, ShowWindow,
    UnhookWindowsHookEx, CS_HREDRAW, CS_VREDRAW, HHOOK, HWND_TOPMOST, IDC_ARROW, MSLLHOOKSTRUCT,
    SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, WH_MOUSE_LL, WM_APP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NCDESTROY, WM_RBUTTONDOWN, WM_TIMER, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::activities::call::CallState;
use crate::activities::media::MediaUpdate;
use crate::activities::shelf::ShelfView;
use crate::activities::timer::TimerView;
use crate::anchor;
use crate::state::ActivityId;
use crate::theme::{self, Theme};

pub const FLYOUT_CLASS: PCWSTR = w!("TaskbarIslandFlyoutClass");
pub const WM_APP_FLYOUT_ACTION: u32 = WM_APP + 10;
/// Posted to the flyout HWND by the low-level mouse hook when the user clicks
/// anywhere outside the flyout and the pill (Design Spec: close on click outside).
pub const WM_APP_FLYOUT_CLICK_OUTSIDE: u32 = WM_APP + 30;

// Actions dispatched to main App
pub const FLYOUT_ACTION_PLAY_PAUSE: usize = 1;
pub const FLYOUT_ACTION_PREV: usize = 2;
pub const FLYOUT_ACTION_NEXT: usize = 3;
pub const FLYOUT_ACTION_MUTE: usize = 4;
pub const FLYOUT_ACTION_SPEAKER: usize = 5;
pub const FLYOUT_ACTION_END_CALL: usize = 6;
pub const FLYOUT_ACTION_CLEAR_SHELF: usize = 7;
pub const FLYOUT_ACTION_OPEN_FILE: usize = 8;
pub const FLYOUT_ACTION_TIMER_PAUSE: usize = 9;
pub const FLYOUT_ACTION_TIMER_ADD_MIN: usize = 10;
pub const FLYOUT_ACTION_TIMER_STOP: usize = 11;
pub const FLYOUT_ACTION_MOUSE_ENTER: usize = 12;
pub const FLYOUT_ACTION_MOUSE_LEAVE: usize = 13;

const FLYOUT_ANIM_TIMER_ID: usize = 101;
const FLYOUT_ANIM_TICK_MS: u32 = 16;
const SHADOW_MARGIN_DIP: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlyoutButton {
    MusicPrev,
    MusicPlayPause,
    MusicNext,
    CallMute,
    CallSpeaker,
    CallEnd,
    ShelfClear,
    ShelfFile(usize),
    TimerPause,
    TimerAddMin,
    TimerStop,
}

thread_local! {
    static FLYOUT: RefCell<Option<FlyoutWindow>> = const { RefCell::new(None) };
}

// Click-outside detection only lives while the flyout is open. The low-level
// mouse hook posts a message and never swallows the click.
static FLYOUT_HWND: AtomicIsize = AtomicIsize::new(0);
static PILL_HWND: AtomicIsize = AtomicIsize::new(0);
static MOUSE_HOOK: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn low_level_mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0
        && (wparam.0 as u32 == WM_LBUTTONDOWN || wparam.0 as u32 == WM_RBUTTONDOWN)
    {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let pt = info.pt;
        let flyout_raw = FLYOUT_HWND.load(Ordering::Relaxed);
        if flyout_raw != 0 {
            let flyout_hwnd = HWND(flyout_raw as *mut core::ffi::c_void);
            let pill_hwnd = HWND(PILL_HWND.load(Ordering::Relaxed) as *mut core::ffi::c_void);
            let inside = |hwnd: HWND| -> bool {
                if hwnd.0.is_null() {
                    return false;
                }
                let mut r = RECT::default();
                unsafe { GetWindowRect(hwnd, &mut r) }.is_ok()
                    && pt.x >= r.left
                    && pt.x < r.right
                    && pt.y >= r.top
                    && pt.y < r.bottom
            };
            if !inside(flyout_hwnd) && !inside(pill_hwnd) {
                let _ = unsafe {
                    PostMessageW(
                        Some(flyout_hwnd),
                        WM_APP_FLYOUT_CLICK_OUTSIDE,
                        WPARAM(0),
                        LPARAM(0),
                    )
                };
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

pub struct FlyoutWindow {
    pub hwnd: HWND,
    parent_hwnd: HWND,
    swapchain: IDXGISwapChain1,
    #[allow(dead_code)]
    comp_device: IDCompositionDevice,
    #[allow(dead_code)]
    comp_target: IDCompositionTarget,
    #[allow(dead_code)]
    comp_visual: IDCompositionVisual,
    d2d_context: ID2D1DeviceContext,
    #[allow(dead_code)]
    dwrite_factory: IDWriteFactory,

    // Formats
    title_format: IDWriteTextFormat,
    bold_format: IDWriteTextFormat,
    sub_format: IDWriteTextFormat,
    badge_format: IDWriteTextFormat,
    timer_num_format: IDWriteTextFormat,
    center_format: IDWriteTextFormat,

    theme: Theme,
    scale: f32,
    size_px: (u32, u32),
    card_size_dip: (f32, f32),

    // State
    pub is_open: bool,
    pub is_animating: bool,
    pub is_closing: bool,
    anim_start: Option<Instant>,
    tracking: bool,
    hover_btn: Option<FlyoutButton>,

    // Content
    pub current_activity: ActivityId,
    pub media: MediaUpdate,
    pub call: CallState,
    pub shelf: ShelfView,
    pub timer: TimerView,

    // Target rect for anchoring
    target_screen_rect: RECT,
}

impl FlyoutWindow {
    /// Registers the flyout window class without creating the window or any
    /// GPU resources. The renderer (D3D11 + DComp + D2D + DWrite) is only
    /// built on first open so an unused flyout costs no RAM at startup.
    pub fn register_class() -> Result<()> {
        let hinstance = unsafe { GetModuleHandleW(None)?.into() };
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(flyout_wnd_proc),
            hInstance: hinstance,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
            lpszClassName: FLYOUT_CLASS,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
        Ok(())
    }

    /// Creates the flyout window + renderer on first use.
    pub fn ensure(parent_hwnd: HWND) -> Result<()> {
        let exists = FLYOUT.with(|f| f.borrow().is_some());
        if exists {
            return Ok(());
        }
        Self::init(parent_hwnd)
    }

    pub fn init(parent_hwnd: HWND) -> Result<()> {
        let hinstance = unsafe { GetModuleHandleW(None)?.into() };

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
                FLYOUT_CLASS,
                w!("TaskbarIslandFlyout"),
                WS_POPUP,
                0,
                0,
                400,
                200,
                None,
                None,
                Some(hinstance),
                None,
            )?
        };

        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let scale = dpi as f32 / 96.0;
        let theme = Theme::current();
        let card_size_dip = anchor::MUSIC_EXPANDED_SIZE_DIP;
        let window_w = ((card_size_dip.0 + SHADOW_MARGIN_DIP * 2.0) * scale).round() as u32;
        let window_h = ((card_size_dip.1 + SHADOW_MARGIN_DIP * 2.0) * scale).round() as u32;

        let flyout = unsafe {
            Self::create_renderer(hwnd, parent_hwnd, (window_w, window_h), card_size_dip, scale, theme)?
        };

        FLYOUT.with(|f| *f.borrow_mut() = Some(flyout));
        Ok(())
    }

    unsafe fn create_renderer(
        hwnd: HWND,
        parent_hwnd: HWND,
        size_px: (u32, u32),
        card_size_dip: (f32, f32),
        scale: f32,
        theme: Theme,
    ) -> Result<Self> {
        let gpu = crate::gpu::shared()?;

        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: size_px.0.max(1),
            Height: size_px.1.max(1),
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

        let comp_device: IDCompositionDevice = gpu.comp.clone();
        let comp_target = comp_device.CreateTargetForHwnd(hwnd, true)?;
        let comp_visual = comp_device.CreateVisual()?;
        comp_visual.SetContent(&swapchain)?;
        comp_target.SetRoot(&comp_visual)?;
        comp_device.Commit()?;

        let d2d_context = gpu.d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

        let dwrite_factory = gpu.dwrite.clone();

        let title_format = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            14.0,
            w!("en-US"),
        )?;
        title_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        title_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        title_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

        let bold_format = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            14.0,
            w!("en-US"),
        )?;
        bold_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        bold_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        bold_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

        let sub_format = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            11.5,
            w!("en-US"),
        )?;
        sub_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        sub_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        sub_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

        let badge_format = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            10.0,
            w!("en-US"),
        )?;
        badge_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        badge_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        badge_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

        let timer_num_format = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Display"),
            None,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            15.0,
            w!("en-US"),
        )?;
        timer_num_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        timer_num_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;

        let center_format = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            12.0,
            w!("en-US"),
        )?;
        center_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        center_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;

        Ok(Self {
            hwnd,
            parent_hwnd,
            swapchain,
            comp_device,
            comp_target,
            comp_visual,
            d2d_context,
            dwrite_factory,
            title_format,
            bold_format,
            sub_format,
            badge_format,
            timer_num_format,
            center_format,
            theme,
            scale,
            size_px,
            card_size_dip,
            is_open: false,
            is_animating: false,
            is_closing: false,
            anim_start: None,
            tracking: false,
            hover_btn: None,
            current_activity: ActivityId::Media,
            media: MediaUpdate::demo(),
            call: CallState::default(),
            shelf: ShelfView::demo(),
            timer: TimerView::demo(),
            target_screen_rect: RECT::default(),
        })
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        if self.is_open {
            let _ = unsafe { self.draw(1.0, 0.0) };
        }
    }

    pub fn open_for(&mut self, activity: ActivityId, taskbar: HWND, pill_rect: RECT) {
        if activity == ActivityId::Volume {
            return; // Volume HUD never expands per design spec!
        }
        if activity == ActivityId::Idle {
            return; // Nothing to expand: the pill has no active content.
        }
        self.current_activity = activity;
        self.card_size_dip = match activity {
            ActivityId::Idle => return,
            ActivityId::Media => anchor::MUSIC_EXPANDED_SIZE_DIP,
            ActivityId::Call => anchor::CALL_EXPANDED_SIZE_DIP,
            ActivityId::Shelf => anchor::SHELF_EXPANDED_SIZE_DIP,
            ActivityId::Timer => anchor::TIMER_EXPANDED_SIZE_DIP,
            ActivityId::Volume => return,
        };

        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
        self.scale = dpi as f32 / 96.0;

        let screen_rect = anchor::compute_flyout(taskbar, dpi, pill_rect, self.card_size_dip);
        self.target_screen_rect = screen_rect;

        // Window size includes shadow margin
        let margin_px = (SHADOW_MARGIN_DIP * self.scale).round() as i32;
        let win_left = screen_rect.left - margin_px;
        let win_top = screen_rect.top - margin_px;
        let win_w = (screen_rect.right - screen_rect.left) + margin_px * 2;
        let win_h = (screen_rect.bottom - screen_rect.top) + margin_px * 2;

        self.size_px = (win_w.max(1) as u32, win_h.max(1) as u32);

        unsafe {
            let _ = self.swapchain.ResizeBuffers(
                0,
                self.size_px.0,
                self.size_px.1,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            );

            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                win_left,
                win_top,
                win_w,
                win_h,
                SWP_NOACTIVATE,
            );

            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }

        // Arm click-outside detection for exactly as long as the flyout is up.
        FLYOUT_HWND.store(self.hwnd.0 as isize, Ordering::Relaxed);
        PILL_HWND.store(self.parent_hwnd.0 as isize, Ordering::Relaxed);
        if MOUSE_HOOK.load(Ordering::Relaxed) == 0 {
            if let Ok(hook) = unsafe {
                SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), None, 0)
            } {
                MOUSE_HOOK.store(hook.0 as isize, Ordering::Relaxed);
            }
        }

        self.is_open = true;
        self.is_closing = false;
        self.is_animating = true;
        self.anim_start = Some(Instant::now());

        unsafe {
            SetTimer(
                Some(self.hwnd),
                FLYOUT_ANIM_TIMER_ID,
                FLYOUT_ANIM_TICK_MS,
                None,
            );
        }

        self.on_anim_tick();
    }

    pub fn close(&mut self) {
        if !self.is_open || self.is_closing {
            return;
        }
        self.is_closing = true;
        self.is_animating = true;
        self.anim_start = Some(Instant::now());

        unsafe {
            SetTimer(
                Some(self.hwnd),
                FLYOUT_ANIM_TIMER_ID,
                FLYOUT_ANIM_TICK_MS,
                None,
            );
        }
    }

    pub fn close_immediate(&mut self) {
        self.is_open = false;
        self.is_closing = false;
        self.is_animating = false;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), FLYOUT_ANIM_TIMER_ID);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        // Disarm click-outside detection.
        let hook = MOUSE_HOOK.swap(0, Ordering::Relaxed);
        if hook != 0 {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(hook as *mut core::ffi::c_void));
            }
        }
        FLYOUT_HWND.store(0, Ordering::Relaxed);
        PILL_HWND.store(0, Ordering::Relaxed);
    }

    pub fn on_anim_tick(&mut self) {
        let Some(started) = self.anim_start else {
            return;
        };
        let elapsed_ms = started.elapsed().as_secs_f32() * 1000.0;

        if self.is_closing {
            // Close: 110 ms fade and scale back, no delay
            let progress = (elapsed_ms / 110.0).min(1.0);
            let alpha = 1.0 - progress;
            let _scale = 1.0 - 0.06 * progress;
            let y_offset = 6.0 * progress;

            let _ = unsafe { self.draw(alpha, y_offset) };

            if progress >= 1.0 {
                self.close_immediate();
            }
        } else {
            // Open: 180 ms fade, 220 ms scale .94 to 1 and rise 6 px, cubic-bezier(.2, .8, .2, 1)
            let fade_t = (elapsed_ms / 180.0).min(1.0);
            let scale_t = (elapsed_ms / 220.0).min(1.0);

            // Cubic bezier (.2, .8, .2, 1.0) approx: fast ease out
            let eased = cubic_bezier_eval(scale_t, 0.2, 0.8, 0.2, 1.0);
            let alpha = cubic_bezier_eval(fade_t, 0.2, 0.8, 0.2, 1.0);
            let y_offset = (1.0 - eased) * 6.0; // starts 6 px lower and rises

            let _ = unsafe { self.draw(alpha, y_offset) };

            if elapsed_ms >= 220.0 {
                self.is_animating = false;
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), FLYOUT_ANIM_TIMER_ID);
                }
                let _ = unsafe { self.draw(1.0, 0.0) };
            }
        }
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

    unsafe fn draw(&mut self, alpha: f32, y_offset_dip: f32) -> Result<()> {
        let back_buffer: ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
        let surface: IDXGISurface = back_buffer.cast()?;
        let dpi = 96.0 * self.scale;
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
        let target = self.d2d_context.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
        self.d2d_context.SetTarget(&target);

        self.d2d_context.BeginDraw();
        self.d2d_context.Clear(None);

        let card_w = self.card_size_dip.0;
        let card_h = self.card_size_dip.1;
        let origin_x = SHADOW_MARGIN_DIP;
        let origin_y = SHADOW_MARGIN_DIP + y_offset_dip;

        // Draw shadow: 0 8 32 black 37% + 0 2 8 black 25%
        for i in 1..=4 {
            let spread = i as f32 * 2.0;
            let shadow_brush = self.d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.06 * alpha },
                None,
            )?;
            let shadow_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: origin_x - spread,
                    top: origin_y + 4.0 - spread / 2.0,
                    right: origin_x + card_w + spread,
                    bottom: origin_y + card_h + 8.0 + spread,
                },
                radiusX: theme::FLYOUT_RADIUS_DIP + spread,
                radiusY: theme::FLYOUT_RADIUS_DIP + spread,
            };
            self.d2d_context.FillRoundedRectangle(&shadow_rect, &shadow_brush);
        }

        // Card background: rgb(40, 40, 40) at 95%
        let fill = self.theme.flyout_fill();
        let fill_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: fill.0, g: fill.1, b: fill.2, a: fill.3 * alpha },
            None,
        )?;
        let border = self.theme.flyout_border();
        let border_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: border.0, g: border.1, b: border.2, a: border.3 * alpha },
            None,
        )?;

        let card_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: origin_x,
                top: origin_y,
                right: origin_x + card_w,
                bottom: origin_y + card_h,
            },
            radiusX: theme::FLYOUT_RADIUS_DIP,
            radiusY: theme::FLYOUT_RADIUS_DIP,
        };
        self.d2d_context.FillRoundedRectangle(&card_rect, &fill_brush);
        self.d2d_context.DrawRoundedRectangle(&card_rect, &border_brush, 1.0, None);

        // Draw activity content
        match self.current_activity {
            ActivityId::Idle => {}
            ActivityId::Media => self.draw_music(origin_x, origin_y, alpha)?,
            ActivityId::Call => self.draw_call(origin_x, origin_y, alpha)?,
            ActivityId::Shelf => self.draw_shelf(origin_x, origin_y, alpha)?,
            ActivityId::Timer => self.draw_timer(origin_x, origin_y, alpha)?,
            ActivityId::Volume => {}
        }

        self.d2d_context.EndDraw(None, None)?;
        self.d2d_context.SetTarget(None::<&ID2D1Image>);
        drop(target);

        let _ = self.swapchain.Present(0, DXGI_PRESENT(0));
        Ok(())
    }

    unsafe fn draw_music(&self, ox: f32, oy: f32, alpha: f32) -> Result<()> {
        let accent = self.theme.accent_color();
        let text = self.theme.text();
        let sec_text = self.theme.secondary_text();

        let accent_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: accent.0, g: accent.1, b: accent.2, a: alpha },
            None,
        )?;
        let text_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: text.0, g: text.1, b: text.2, a: text.3 * alpha },
            None,
        )?;
        let sec_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: sec_text.0, g: sec_text.1, b: sec_text.2, a: sec_text.3 * alpha },
            None,
        )?;

        // Top Row: Album Art (48x48) at (ox + 16, oy + 16)
        let art_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: ox + 16.0,
                top: oy + 16.0,
                right: ox + 64.0,
                bottom: oy + 64.0,
            },
            radiusX: 6.0,
            radiusY: 6.0,
        };
        // Gradient/Cyan cover
        let cover_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.15, g: 0.45, b: 0.75, a: alpha },
            None,
        )?;
        self.d2d_context.FillRoundedRectangle(&art_rect, &cover_brush);

        // Music note / inner icon
        let note_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.5, g: 0.8, b: 1.0, a: 0.8 * alpha },
            None,
        )?;
        self.d2d_context.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: ox + 36.0, Y: oy + 44.0 },
                radiusX: 5.0,
                radiusY: 4.0,
            },
            &note_brush,
        );

        // Track title & artist
        let title_rect = D2D_RECT_F {
            left: ox + 76.0,
            top: oy + 18.0,
            right: ox + 320.0,
            bottom: oy + 38.0,
        };
        let title = if self.media.title.is_empty() { "Deewana Kar Raha Hai" } else { &self.media.title };
        let title_w: Vec<u16> = title.encode_utf16().collect();
        self.draw_text(
            &title_w,
            &self.title_format,
            &title_rect,
            &text_brush,
        );

        let sub_rect = D2D_RECT_F {
            left: ox + 76.0,
            top: oy + 40.0,
            right: ox + 320.0,
            bottom: oy + 58.0,
        };
        let artist = if self.media.artist.is_empty() { "Now playing" } else { &self.media.artist };
        let artist_w: Vec<u16> = artist.encode_utf16().collect();
        self.draw_text(
            &artist_w,
            &self.sub_format,
            &sub_rect,
            &sec_brush,
        );

        // Equalizer icon on right
        let eq_heights = if self.media.playing { [14.0, 8.0, 16.0] } else { [10.0, 10.0, 10.0] };
        for (i, h) in eq_heights.iter().enumerate() {
            let bx = ox + 350.0 + i as f32 * 5.0;
            self.d2d_context.FillRectangle(
                &D2D_RECT_F {
                    left: bx,
                    top: oy + 40.0 - h / 2.0,
                    right: bx + 2.5,
                    bottom: oy + 40.0 + h / 2.0,
                },
                &accent_brush,
            );
        }

        // Progress bar row: elapsed (ox+16), bar (ox+52..ox+332), remaining (ox+336)
        // Use real position/duration from MediaUpdate
        let pos = self.media.position_secs;
        let dur = self.media.duration_secs.max(1);
        let progress = (pos as f32 / dur as f32).min(1.0);
        let elapsed_str = format!("{}:{:02}", pos / 60, pos % 60);
        let remain_secs = dur.saturating_sub(pos);
        let remain_str = format!("-{}:{:02}", remain_secs / 60, remain_secs % 60);

        let elapsed_rect = D2D_RECT_F {
            left: ox + 16.0,
            top: oy + 78.0,
            right: ox + 50.0,
            bottom: oy + 94.0,
        };
        let elapsed_w: Vec<u16> = elapsed_str.encode_utf16().collect();
        self.draw_text(&elapsed_w, &self.sub_format, &elapsed_rect, &sec_brush);

        let remain_rect = D2D_RECT_F {
            left: ox + 334.0,
            top: oy + 78.0,
            right: ox + 372.0,
            bottom: oy + 94.0,
        };
        let remain_w: Vec<u16> = remain_str.encode_utf16().collect();
        self.draw_text(&remain_w, &self.sub_format, &remain_rect, &sec_brush);

        let track_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.15 * alpha },
            None,
        )?;
        let bar_left = ox + 54.0;
        let bar_right = ox + 330.0;
        let bar_w = bar_right - bar_left;

        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: bar_left, top: oy + 84.0, right: bar_right, bottom: oy + 88.0 },
                radiusX: 2.0,
                radiusY: 2.0,
            },
            &track_brush,
        );
        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: bar_left, top: oy + 84.0, right: bar_left + bar_w * progress, bottom: oy + 88.0 },
                radiusX: 2.0,
                radiusY: 2.0,
            },
            &accent_brush,
        );

        // Bottom playback controls: Previous, Play/Pause (36x36 accent circle), Next
        let center_x = ox + 192.0;
        let controls_y = oy + 124.0;

        // Previous button (|<)
        let prev_hover = self.hover_btn == Some(FlyoutButton::MusicPrev);
        let btn_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if prev_hover { 0.25 } else { 0.1 } * alpha },
            None,
        )?;
        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: center_x - 56.0, top: controls_y - 14.0, right: center_x - 28.0, bottom: controls_y + 14.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            },
            &btn_brush,
        );
        let prev_w: Vec<u16> = "|<".encode_utf16().collect();
        self.draw_text(
            &prev_w,
            &self.center_format,
            &D2D_RECT_F { left: center_x - 56.0, top: controls_y - 14.0, right: center_x - 28.0, bottom: controls_y + 14.0 },
            &text_brush,
        );

        // Play/Pause circle button
        let play_hover = self.hover_btn == Some(FlyoutButton::MusicPlayPause);
        let play_bg = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: accent.0,
                g: accent.1,
                b: accent.2,
                a: if play_hover { 1.0 } else { 0.9 } * alpha,
            },
            None,
        )?;
        self.d2d_context.FillEllipse(
            &D2D1_ELLIPSE { point: Vector2 { X: center_x, Y: controls_y }, radiusX: 18.0, radiusY: 18.0 },
            &play_bg,
        );
        let dark_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.1, g: 0.1, b: 0.1, a: alpha },
            None,
        )?;
        if self.media.playing {
            // Pause bars
            self.d2d_context.FillRectangle(
                &D2D_RECT_F { left: center_x - 5.0, top: controls_y - 6.0, right: center_x - 2.0, bottom: controls_y + 6.0 },
                &dark_brush,
            );
            self.d2d_context.FillRectangle(
                &D2D_RECT_F { left: center_x + 2.0, top: controls_y - 6.0, right: center_x + 5.0, bottom: controls_y + 6.0 },
                &dark_brush,
            );
        } else {
            // Play triangle
            let play_sym: Vec<u16> = ">".encode_utf16().collect();
            self.draw_text(
                &play_sym,
                &self.bold_format,
                &D2D_RECT_F { left: center_x - 10.0, top: controls_y - 10.0, right: center_x + 10.0, bottom: controls_y + 10.0 },
                &dark_brush,
            );
        }

        // Next button (>|)
        let next_hover = self.hover_btn == Some(FlyoutButton::MusicNext);
        let next_btn_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if next_hover { 0.25 } else { 0.1 } * alpha },
            None,
        )?;
        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: center_x + 28.0, top: controls_y - 14.0, right: center_x + 56.0, bottom: controls_y + 14.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            },
            &next_btn_brush,
        );
        let next_w: Vec<u16> = ">|".encode_utf16().collect();
        self.draw_text(
            &next_w,
            &self.center_format,
            &D2D_RECT_F { left: center_x + 28.0, top: controls_y - 14.0, right: center_x + 56.0, bottom: controls_y + 14.0 },
            &text_brush,
        );

        Ok(())
    }

    unsafe fn draw_call(&self, ox: f32, oy: f32, alpha: f32) -> Result<()> {
        let call_green = Theme::call_color();
        let end_red = Theme::end_color();
        let text = self.theme.text();
        let sec_text = self.theme.secondary_text();

        let green_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: call_green.0, g: call_green.1, b: call_green.2, a: alpha },
            None,
        )?;
        let text_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: text.0, g: text.1, b: text.2, a: text.3 * alpha },
            None,
        )?;
        let sec_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: sec_text.0, g: sec_text.1, b: sec_text.2, a: sec_text.3 * alpha },
            None,
        )?;

        // Avatar: 40x40 circle with 'P' at (ox + 16, oy + 16)
        self.d2d_context.FillEllipse(
            &D2D1_ELLIPSE { point: Vector2 { X: ox + 36.0, Y: oy + 36.0 }, radiusX: 20.0, radiusY: 20.0 },
            &green_brush,
        );
        let letter_w: Vec<u16> = "P".encode_utf16().collect();
        self.draw_text(
            &letter_w,
            &self.bold_format,
            &D2D_RECT_F { left: ox + 16.0, top: oy + 16.0, right: ox + 56.0, bottom: oy + 56.0 },
            &text_brush,
        );

        // Name & timer
        let name_rect = D2D_RECT_F { left: ox + 66.0, top: oy + 16.0, right: ox + 280.0, bottom: oy + 36.0 };
        let name_w: Vec<u16> = self.call.name.encode_utf16().collect();
        self.draw_text(&name_w, &self.title_format, &name_rect, &text_brush);

        let sub_rect = D2D_RECT_F { left: ox + 66.0, top: oy + 36.0, right: ox + 280.0, bottom: oy + 54.0 };
        let sub_text = format!("Voice call · {}", self.call.duration_str);
        let sub_w: Vec<u16> = sub_text.encode_utf16().collect();
        self.draw_text(&sub_w, &self.sub_format, &sub_rect, &sec_brush);

        // Voice wave visual on right
        let wave_heights = [8.0, 14.0, 10.0, 16.0, 6.0];
        for (i, h) in wave_heights.iter().enumerate() {
            let bx = ox + 330.0 + i as f32 * 5.0;
            self.d2d_context.FillRectangle(
                &D2D_RECT_F {
                    left: bx,
                    top: oy + 36.0 - h / 2.0,
                    right: bx + 2.5,
                    bottom: oy + 36.0 + h / 2.0,
                },
                &green_brush,
            );
        }

        // Row of controls: Mute, Speaker, End (sitting in one row)
        // Mute button: (ox + 16, oy + 76, ox + 120, oy + 112)
        let mute_hover = self.hover_btn == Some(FlyoutButton::CallMute);
        let mute_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if mute_hover { 0.20 } else { 0.10 } * alpha },
            None,
        )?;
        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: ox + 16.0, top: oy + 76.0, right: ox + 126.0, bottom: oy + 112.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            },
            &mute_brush,
        );
        let mute_text = if self.call.is_muted { "Unmute" } else { "Mute" };
        let mute_w: Vec<u16> = mute_text.encode_utf16().collect();
        self.draw_text(
            &mute_w,
            &self.center_format,
            &D2D_RECT_F { left: ox + 16.0, top: oy + 76.0, right: ox + 126.0, bottom: oy + 112.0 },
            &text_brush,
        );

        // Speaker button: (ox + 138, oy + 76, ox + 248, oy + 112)
        let speaker_hover = self.hover_btn == Some(FlyoutButton::CallSpeaker);
        let speaker_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if speaker_hover { 0.20 } else { 0.10 } * alpha },
            None,
        )?;
        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: ox + 138.0, top: oy + 76.0, right: ox + 248.0, bottom: oy + 112.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            },
            &speaker_brush,
        );
        let speaker_w: Vec<u16> = "Speaker".encode_utf16().collect();
        self.draw_text(
            &speaker_w,
            &self.center_format,
            &D2D_RECT_F { left: ox + 138.0, top: oy + 76.0, right: ox + 248.0, bottom: oy + 112.0 },
            &text_brush,
        );

        // End button: (ox + 260, oy + 76, ox + 368, oy + 112) filled in End red #C42B1C
        let end_hover = self.hover_btn == Some(FlyoutButton::CallEnd);
        let end_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: end_red.0,
                g: end_red.1,
                b: end_red.2,
                a: if end_hover { 1.0 } else { 0.9 } * alpha,
            },
            None,
        )?;
        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: ox + 260.0, top: oy + 76.0, right: ox + 368.0, bottom: oy + 112.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            },
            &end_brush,
        );
        let end_w: Vec<u16> = "End".encode_utf16().collect();
        self.draw_text(
            &end_w,
            &self.center_format,
            &D2D_RECT_F { left: ox + 260.0, top: oy + 76.0, right: ox + 368.0, bottom: oy + 112.0 },
            &text_brush,
        );

        Ok(())
    }

    unsafe fn draw_shelf(&self, ox: f32, oy: f32, alpha: f32) -> Result<()> {
        let text = self.theme.text();
        let sec_text = self.theme.secondary_text();

        let text_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: text.0, g: text.1, b: text.2, a: text.3 * alpha },
            None,
        )?;
        let sec_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: sec_text.0, g: sec_text.1, b: sec_text.2, a: sec_text.3 * alpha },
            None,
        )?;

        // Header: "Shelf 3 files" + "Clear" button
        let header_str = format!("Shelf  {} files", self.shelf.count);
        let header_w: Vec<u16> = header_str.encode_utf16().collect();
        self.draw_text(
            &header_w,
            &self.title_format,
            &D2D_RECT_F { left: ox + 16.0, top: oy + 12.0, right: ox + 280.0, bottom: oy + 36.0 },
            &text_brush,
        );

        // Clear button
        let clear_hover = self.hover_btn == Some(FlyoutButton::ShelfClear);
        let clear_bg = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if clear_hover { 0.20 } else { 0.08 } * alpha },
            None,
        )?;
        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: ox + 314.0, top: oy + 12.0, right: ox + 368.0, bottom: oy + 36.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            },
            &clear_bg,
        );
        let clear_w: Vec<u16> = "Clear".encode_utf16().collect();
        self.draw_text(
            &clear_w,
            &self.center_format,
            &D2D_RECT_F { left: ox + 314.0, top: oy + 12.0, right: ox + 368.0, bottom: oy + 36.0 },
            &text_brush,
        );

        // 3 file tiles: (PDF, PNG, ZIP)
        let files = if self.shelf.files.is_empty() {
            ShelfView::demo().files
        } else {
            self.shelf.files.clone()
        };

        for (i, file) in files.iter().take(3).enumerate() {
            let tx = ox + 16.0 + i as f32 * 120.0;
            let ty = oy + 46.0;
            let tile_hover = self.hover_btn == Some(FlyoutButton::ShelfFile(i));

            let tile_bg = self.d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if tile_hover { 0.12 } else { 0.06 } * alpha },
                None,
            )?;
            let tile_border = self.d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if tile_hover { 0.25 } else { 0.10 } * alpha },
                None,
            )?;

            let tile_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: tx, top: ty, right: tx + 112.0, bottom: ty + 80.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            self.d2d_context.FillRoundedRectangle(&tile_rect, &tile_bg);
            self.d2d_context.DrawRoundedRectangle(&tile_rect, &tile_border, 1.0, None);

            // Badge (PDF, PNG, ZIP)
            let badge_color = match file.badge.as_str() {
                "PDF" => (0.85, 0.25, 0.20),
                "PNG" | "JPG" => (0.20, 0.65, 0.85),
                "ZIP" | "RAR" => (0.85, 0.65, 0.15),
                _ => (0.45, 0.45, 0.55),
            };
            let badge_bg = self.d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: badge_color.0, g: badge_color.1, b: badge_color.2, a: alpha },
                None,
            )?;
            self.d2d_context.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: tx + 8.0, top: ty + 8.0, right: tx + 40.0, bottom: ty + 24.0 },
                    radiusX: 4.0,
                    radiusY: 4.0,
                },
                &badge_bg,
            );
            let badge_w: Vec<u16> = file.badge.encode_utf16().collect();
            self.draw_text(
                &badge_w,
                &self.badge_format,
                &D2D_RECT_F { left: tx + 8.0, top: ty + 8.0, right: tx + 40.0, bottom: ty + 24.0 },
                &text_brush,
            );

            // Filename
            let name_w: Vec<u16> = file.name.encode_utf16().collect();
            self.draw_text(
                &name_w,
                &self.center_format,
                &D2D_RECT_F { left: tx + 6.0, top: ty + 34.0, right: tx + 106.0, bottom: ty + 54.0 },
                &text_brush,
            );

            // Filesize
            let size_w: Vec<u16> = file.size_str.encode_utf16().collect();
            self.draw_text(
                &size_w,
                &self.sub_format,
                &D2D_RECT_F { left: tx + 6.0, top: ty + 56.0, right: tx + 106.0, bottom: ty + 74.0 },
                &sec_brush,
            );
        }

        // Footer hint: "Drag files out to move them. Click a file to open it."
        let hint_w: Vec<u16> = "Drag files out to move them. Click a file to open it.".encode_utf16().collect();
        self.draw_text(
            &hint_w,
            &self.sub_format,
            &D2D_RECT_F { left: ox + 16.0, top: oy + 142.0, right: ox + 368.0, bottom: oy + 164.0 },
            &sec_brush,
        );

        Ok(())
    }

    unsafe fn draw_timer(&self, ox: f32, oy: f32, alpha: f32) -> Result<()> {
        let accent = self.theme.accent_color();
        let text = self.theme.text();
        let sec_text = self.theme.secondary_text();

        let text_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: text.0, g: text.1, b: text.2, a: text.3 * alpha },
            None,
        )?;
        let sec_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: sec_text.0, g: sec_text.1, b: sec_text.2, a: sec_text.3 * alpha },
            None,
        )?;

        // Circular ring: center (ox + 48, oy + 62), radius 34 DIP
        let ring_center = Vector2 { X: ox + 52.0, Y: oy + 62.0 };
        let track_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 * alpha },
            None,
        )?;
        self.d2d_context.DrawEllipse(
            &D2D1_ELLIPSE { point: ring_center, radiusX: 32.0, radiusY: 32.0 },
            &track_brush,
            3.5,
            None,
        );
        // Accent ring — draw partial arc representing remaining time as fraction of total
        let ring_frac = if self.timer.total_secs > 0 {
            (self.timer.remaining_secs as f32 / self.timer.total_secs as f32).min(1.0)
        } else {
            1.0
        };
        // Full circle when nearly full, or skip inner segment by drawing at accent alpha
        let arc_alpha = if ring_frac > 0.01 { alpha } else { 0.0 };
        let arc_brush = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: accent.0, g: accent.1, b: accent.2, a: arc_alpha },
            None,
        )?;
        self.d2d_context.DrawEllipse(
            &D2D1_ELLIPSE { point: ring_center, radiusX: 32.0, radiusY: 32.0 },
            &arc_brush,
            3.5,
            None,
        );

        // Time digits inside ring ("12:40")
        let time_str = if self.timer.title.is_empty() { "12:40" } else { &self.timer.title };
        let time_w: Vec<u16> = time_str.encode_utf16().collect();
        self.draw_text(
            &time_w,
            &self.timer_num_format,
            &D2D_RECT_F { left: ox + 20.0, top: oy + 46.0, right: ox + 84.0, bottom: oy + 78.0 },
            &text_brush,
        );

        // Label: "Study session", status: "Running"
        let label_w: Vec<u16> = self.timer.label.encode_utf16().collect();
        self.draw_text(
            &label_w,
            &self.bold_format,
            &D2D_RECT_F { left: ox + 104.0, top: oy + 24.0, right: ox + 300.0, bottom: oy + 46.0 },
            &text_brush,
        );

        let status_str = if self.timer.running { "Running" } else { "Paused" };
        let status_w: Vec<u16> = status_str.encode_utf16().collect();
        self.draw_text(
            &status_w,
            &self.sub_format,
            &D2D_RECT_F { left: ox + 104.0, top: oy + 46.0, right: ox + 300.0, bottom: oy + 64.0 },
            &sec_brush,
        );

        // Controls row: Pause (||), +1 min, Stop (X)
        // Pause button
        let pause_hover = self.hover_btn == Some(FlyoutButton::TimerPause);
        let pause_bg = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if pause_hover { 0.22 } else { 0.10 } * alpha },
            None,
        )?;
        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: ox + 104.0, top: oy + 76.0, right: ox + 144.0, bottom: oy + 108.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            },
            &pause_bg,
        );
        let pause_sym: Vec<u16> = "||".encode_utf16().collect();
        self.draw_text(
            &pause_sym,
            &self.center_format,
            &D2D_RECT_F { left: ox + 104.0, top: oy + 76.0, right: ox + 144.0, bottom: oy + 108.0 },
            &text_brush,
        );

        // +1 min button
        let add_hover = self.hover_btn == Some(FlyoutButton::TimerAddMin);
        let add_bg = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if add_hover { 0.22 } else { 0.10 } * alpha },
            None,
        )?;
        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: ox + 154.0, top: oy + 76.0, right: ox + 224.0, bottom: oy + 108.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            },
            &add_bg,
        );
        let add_w: Vec<u16> = "+1 min".encode_utf16().collect();
        self.draw_text(
            &add_w,
            &self.center_format,
            &D2D_RECT_F { left: ox + 154.0, top: oy + 76.0, right: ox + 224.0, bottom: oy + 108.0 },
            &text_brush,
        );

        // Stop button (X)
        let stop_hover = self.hover_btn == Some(FlyoutButton::TimerStop);
        let stop_bg = self.d2d_context.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if stop_hover { 0.22 } else { 0.10 } * alpha },
            None,
        )?;
        self.d2d_context.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: ox + 234.0, top: oy + 76.0, right: ox + 274.0, bottom: oy + 108.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            },
            &stop_bg,
        );
        let stop_sym: Vec<u16> = "X".encode_utf16().collect();
        self.draw_text(
            &stop_sym,
            &self.center_format,
            &D2D_RECT_F { left: ox + 234.0, top: oy + 76.0, right: ox + 274.0, bottom: oy + 108.0 },
            &text_brush,
        );

        Ok(())
    }

    pub fn hit_test(&self, x_dip: f32, y_dip: f32) -> Option<FlyoutButton> {
        let ox = SHADOW_MARGIN_DIP;
        let oy = SHADOW_MARGIN_DIP;

        match self.current_activity {
            ActivityId::Media => {
                let center_x = ox + 192.0;
                let controls_y = oy + 124.0;
                if x_dip >= center_x - 56.0 && x_dip <= center_x - 28.0 && (y_dip - controls_y).abs() <= 15.0 {
                    Some(FlyoutButton::MusicPrev)
                } else if (x_dip - center_x).hypot(y_dip - controls_y) <= 19.0 {
                    Some(FlyoutButton::MusicPlayPause)
                } else if x_dip >= center_x + 28.0 && x_dip <= center_x + 56.0 && (y_dip - controls_y).abs() <= 15.0 {
                    Some(FlyoutButton::MusicNext)
                } else {
                    None
                }
            }
            ActivityId::Call => {
                if y_dip >= oy + 76.0 && y_dip <= oy + 112.0 {
                    if x_dip >= ox + 16.0 && x_dip <= ox + 126.0 {
                        Some(FlyoutButton::CallMute)
                    } else if x_dip >= ox + 138.0 && x_dip <= ox + 248.0 {
                        Some(FlyoutButton::CallSpeaker)
                    } else if x_dip >= ox + 260.0 && x_dip <= ox + 368.0 {
                        Some(FlyoutButton::CallEnd)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            ActivityId::Shelf => {
                if x_dip >= ox + 314.0 && x_dip <= ox + 368.0 && y_dip >= oy + 12.0 && y_dip <= oy + 36.0 {
                    Some(FlyoutButton::ShelfClear)
                } else if y_dip >= oy + 46.0 && y_dip <= oy + 126.0 {
                    for i in 0..3 {
                        let tx = ox + 16.0 + i as f32 * 120.0;
                        if x_dip >= tx && x_dip <= tx + 112.0 {
                            return Some(FlyoutButton::ShelfFile(i));
                        }
                    }
                    None
                } else {
                    None
                }
            }
            ActivityId::Timer => {
                if y_dip >= oy + 76.0 && y_dip <= oy + 108.0 {
                    if x_dip >= ox + 104.0 && x_dip <= ox + 144.0 {
                        Some(FlyoutButton::TimerPause)
                    } else if x_dip >= ox + 154.0 && x_dip <= ox + 224.0 {
                        Some(FlyoutButton::TimerAddMin)
                    } else if x_dip >= ox + 234.0 && x_dip <= ox + 274.0 {
                        Some(FlyoutButton::TimerStop)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            ActivityId::Idle => None,
            ActivityId::Volume => None,
        }
    }
}

/// Evaluates cubic bezier curve at parameter t
fn cubic_bezier_eval(t: f32, _x1: f32, y1: f32, _x2: f32, y2: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t
}

unsafe extern "system" fn flyout_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            FLYOUT.with(|f| {
                if let Ok(mut borrow) = f.try_borrow_mut() {
                    if let Some(flyout) = borrow.as_mut() {
                        if !flyout.tracking {
                            let mut tme = TRACKMOUSEEVENT {
                                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                                dwFlags: TME_LEAVE,
                                hwndTrack: hwnd,
                                dwHoverTime: 0,
                            };
                            let _ = TrackMouseEvent(&mut tme);
                            flyout.tracking = true;
                        }
                        let x = (lparam.0 & 0xFFFF) as i16 as f32 / flyout.scale;
                        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / flyout.scale;
                        let new_hover = flyout.hit_test(x, y);
                        if new_hover != flyout.hover_btn {
                            flyout.hover_btn = new_hover;
                            let _ = flyout.draw(1.0, 0.0);
                        }
                        // Notify parent: mouse is inside flyout (cancels auto-close)
                        let _ = PostMessageW(Some(flyout.parent_hwnd), WM_APP_FLYOUT_ACTION, WPARAM(FLYOUT_ACTION_MOUSE_ENTER), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            FLYOUT.with(|f| {
                if let Ok(mut borrow) = f.try_borrow_mut() {
                    if let Some(flyout) = borrow.as_mut() {
                        flyout.tracking = false;
                        if flyout.hover_btn.is_some() {
                            flyout.hover_btn = None;
                            let _ = flyout.draw(1.0, 0.0);
                        }
                        // Notify parent: mouse left flyout (starts 700 ms auto-close)
                        let _ = PostMessageW(Some(flyout.parent_hwnd), WM_APP_FLYOUT_ACTION, WPARAM(FLYOUT_ACTION_MOUSE_LEAVE), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let mut action = 0;
            let mut file_idx = 0;
            FLYOUT.with(|f| {
                if let Ok(mut borrow) = f.try_borrow_mut() {
                    if let Some(flyout) = borrow.as_mut() {
                        let x = (lparam.0 & 0xFFFF) as i16 as f32 / flyout.scale;
                        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / flyout.scale;
                        if let Some(btn) = flyout.hit_test(x, y) {
                            match btn {
                                FlyoutButton::MusicPrev => action = FLYOUT_ACTION_PREV,
                                FlyoutButton::MusicPlayPause => {
                                    flyout.media.playing = !flyout.media.playing;
                                    action = FLYOUT_ACTION_PLAY_PAUSE;
                                    let _ = flyout.draw(1.0, 0.0);
                                }
                                FlyoutButton::MusicNext => action = FLYOUT_ACTION_NEXT,
                                FlyoutButton::CallMute => {
                                    flyout.call.is_muted = !flyout.call.is_muted;
                                    action = FLYOUT_ACTION_MUTE;
                                    let _ = flyout.draw(1.0, 0.0);
                                }
                                FlyoutButton::CallSpeaker => {
                                    flyout.call.is_speaker = !flyout.call.is_speaker;
                                    action = FLYOUT_ACTION_SPEAKER;
                                    let _ = flyout.draw(1.0, 0.0);
                                }
                                FlyoutButton::CallEnd => {
                                    action = FLYOUT_ACTION_END_CALL;
                                    flyout.close();
                                }
                                FlyoutButton::ShelfClear => {
                                    action = FLYOUT_ACTION_CLEAR_SHELF;
                                    flyout.shelf.count = 0;
                                    flyout.shelf.files.clear();
                                    flyout.close();
                                }
                                FlyoutButton::ShelfFile(idx) => {
                                    action = FLYOUT_ACTION_OPEN_FILE;
                                    file_idx = idx;
                                }
                                FlyoutButton::TimerPause => {
                                    flyout.timer.running = !flyout.timer.running;
                                    action = FLYOUT_ACTION_TIMER_PAUSE;
                                    let _ = flyout.draw(1.0, 0.0);
                                }
                                FlyoutButton::TimerAddMin => {
                                    flyout.timer.remaining_secs += 60;
                                    let min = flyout.timer.remaining_secs / 60;
                                    let sec = flyout.timer.remaining_secs % 60;
                                    flyout.timer.title = format!("{min}:{sec:02}");
                                    action = FLYOUT_ACTION_TIMER_ADD_MIN;
                                    let _ = flyout.draw(1.0, 0.0);
                                }
                                FlyoutButton::TimerStop => {
                                    action = FLYOUT_ACTION_TIMER_STOP;
                                    flyout.close();
                                }
                            }
                            if action != 0 {
                                let _ = PostMessageW(
                                    Some(flyout.parent_hwnd),
                                    WM_APP_FLYOUT_ACTION,
                                    WPARAM(action),
                                    LPARAM(file_idx as isize),
                                );
                            }
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_APP_FLYOUT_CLICK_OUTSIDE => {
            FLYOUT.with(|f| {
                if let Ok(mut borrow) = f.try_borrow_mut() {
                    if let Some(flyout) = borrow.as_mut() {
                        if flyout.is_open && !flyout.is_closing {
                            flyout.close();
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_NCDESTROY => {
            // Never leave a global mouse hook armed behind a destroyed window.
            let hook = MOUSE_HOOK.swap(0, Ordering::Relaxed);
            if hook != 0 {
                unsafe {
                    let _ = UnhookWindowsHookEx(HHOOK(hook as *mut core::ffi::c_void));
                }
            }
            FLYOUT_HWND.store(0, Ordering::Relaxed);
            PILL_HWND.store(0, Ordering::Relaxed);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_TIMER if wparam.0 == FLYOUT_ANIM_TIMER_ID => {
            FLYOUT.with(|f| {
                if let Ok(mut borrow) = f.try_borrow_mut() {
                    if let Some(flyout) = borrow.as_mut() {
                        flyout.on_anim_tick();
                    }
                }
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn get_flyout() -> &'static std::thread::LocalKey<RefCell<Option<FlyoutWindow>>> {
    &FLYOUT
}
