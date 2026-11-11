//! tooltip.rs — the v0.6 tooltip popup.
//!
//! A separate layered, non-activating, click-through top-most window. It never
//! takes focus and never participates in the island's swapchain, so a tooltip
//! can extend past the card without growing the composition surface.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDC,
    GetMonitorInfoW, ReleaseDC, SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DIB_RGB_COLORS, DT_CALCRECT, DT_LEFT, DT_NOPREFIX, DT_TOP, DT_WORDBREAK, FF_DONTCARE, HDC,
    MONITORINFO, OUT_DEFAULT_PRECIS, TRANSPARENT, VARIABLE_PITCH,
};
use windows::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONEAREST};
use windows::Win32::Graphics::Gdi::{AC_SRC_ALPHA, AC_SRC_OVER};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, CS_DROPSHADOW, HTTRANSPARENT, HWND_TOPMOST, SWP_NOACTIVATE, SW_HIDE,
    SW_SHOWNOACTIVATE, ULW_ALPHA, WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::design;

const CLASS_NAME: PCWSTR = w!("TaskbarIslandTooltip");
static REGISTERED: AtomicBool = AtomicBool::new(false);

/// Everything the popup needs for one appearance. Sizes are DIP; the popup
/// scales them with the anchor monitor's DPI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TipRequest<'a> {
    pub title: &'a str,
    pub detail: Option<&'a str>,
    /// Anchor rectangle in physical screen pixels.
    pub anchor: RECT,
    /// Bottom tile rows want the tip above the tile.
    pub prefer_above: bool,
    /// Rightmost grid column right-aligns the tip with the tile.
    pub align_right: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipPosition {
    pub x: i32,
    pub y: i32,
}

/// Tooltip delay: instant when another tip was visible within the neighbour
/// window, otherwise the standard 450 ms.
pub fn delay_ms(last_hidden: Option<Instant>, now: Instant) -> u32 {
    match last_hidden {
        Some(at)
            if now.saturating_duration_since(at)
                < Duration::from_millis(design::TOOLTIP_NEIGHBOR_MS as u64) =>
        {
            0
        }
        _ => design::TOOLTIP_DELAY_MS,
    }
}

/// Place a `size` popup against `anchor`, keeping it inside `work`.
///
/// Below/above placements anchor to the control's left edge, except when that
/// would run past the work area's right edge: the tip then flips to the
/// control's left side (spec §6, "left of row action buttons"), so it is never
/// clipped by the screen. `align_right` (last column) keeps its right edge and
/// clamps instead.
pub fn place(
    anchor: RECT,
    size: (i32, i32),
    work: RECT,
    above: bool,
    align_right: bool,
    gap: i32,
) -> TipPosition {
    let (w, h) = size;
    let mut x = if align_right {
        anchor.right - w
    } else if anchor.left + w > work.right {
        anchor.left - gap - w
    } else {
        anchor.left
    };
    let mut y = if above {
        anchor.top - gap - h
    } else {
        anchor.bottom + gap
    };
    let max_x = (work.right - w).max(work.left);
    let max_y = (work.bottom - h).max(work.top);
    x = x.clamp(work.left, max_x);
    y = y.clamp(work.top, max_y);
    TipPosition { x, y }
}

/// Work area of the monitor containing `anchor`.
pub fn work_area(anchor: RECT) -> RECT {
    // SAFETY: MonitorFromRect only reads the rectangle.
    let monitor = unsafe { MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: info is a valid writable MONITORINFO.
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        info.rcWork
    } else {
        // Conservative fallback: the primary monitor's 96 DPI work area.
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        }
    }
}

pub struct TooltipWindow {
    hwnd: Option<HWND>,
    visible: bool,
    scale: f32,
}

impl TooltipWindow {
    pub fn new() -> Self {
        Self {
            hwnd: None,
            visible: false,
            scale: 1.0,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn hide(&mut self) {
        if let Some(hwnd) = self.hwnd {
            if self.visible {
                // SAFETY: the handle belongs to this module.
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
        }
        self.visible = false;
    }

    /// Shows `request` at `scale` physical pixels per DIP. Failures are
    /// cosmetic: a missing tooltip never blocks the card.
    pub fn show(&mut self, request: &TipRequest<'_>, scale: f32) {
        self.scale = scale.max(1.0);
        let Some(hwnd) = self.ensure_window() else {
            return;
        };
        let Some(bitmap) = self.paint(request) else {
            return;
        };
        let (w, h, x, y) = bitmap.frame;
        // SAFETY: hwnd is our layered popup; the bitmap-owned DC is valid only
        // inside `present`, which owns the underlying DIB through the call.
        if !self.present(hwnd, bitmap) {
            return;
        }
        // SAFETY: positioning a non-activating top-most popup.
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        self.visible = true;
    }

    fn ensure_window(&mut self) -> Option<HWND> {
        if let Some(hwnd) = self.hwnd {
            return Some(hwnd);
        }
        if !REGISTERED.load(Ordering::Relaxed) {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DROPSHADOW,
                lpfnWndProc: Some(tip_proc),
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            // SAFETY: class is fully initialized.
            unsafe {
                let _ = RegisterClassExW(&class);
            }
            REGISTERED.store(true, Ordering::Relaxed);
        }
        // SAFETY: the class is registered above; the new window is hidden and
        // owned by this module.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST
                    | WS_EX_TRANSPARENT,
                CLASS_NAME,
                w!("Taskbar Island tooltip"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
        }
        .ok()?;
        self.hwnd = Some(hwnd);
        Some(hwnd)
    }

    fn paint(&self, request: &TipRequest<'_>) -> Option<LayeredBitmap> {
        let s = self.scale;
        let px = |dip: f32| (dip * s).round().max(1.0) as i32;
        let pad_x = px(design::TOOLTIP_PAD_X);
        let pad_y = px(design::TOOLTIP_PAD_Y);
        let radius = px(design::TOOLTIP_R);
        let gap = px(design::TOOLTIP_GAP);
        let max_w = px(design::TOOLTIP_MAX_W);
        let border = 1;

        let title_h = px(design::TOOLTIP_LINE);
        let sub_h = px(design::TOOLTIP_SUB_LINE);

        // SAFETY: measuring text on a screen DC with a temporary font.
        let (mut text_w, text_h, _dc, font, sub_font) = unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            ReleaseDC(None, screen);
            let font = make_font(s, design::TOOLTIP_FONT, 400);
            let sub_font = make_font(s, design::TOOLTIP_SUB_FONT, 400);
            let old = SelectObject(dc, font.into());
            let mut title = HSTRING::from(request.title).to_vec();
            let mut rc = RECT {
                left: 0,
                top: 0,
                right: max_w - pad_x * 2,
                bottom: 0,
            };
            DrawTextW(
                dc,
                &mut title,
                &mut rc,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX | DT_LEFT,
            );
            let mut w = rc.right;
            let h_title = rc.bottom.max(title_h);
            let mut h = h_title;
            if let Some(detail_text) = request.detail {
                let mut detail = HSTRING::from(detail_text).to_vec();
                let mut rc2 = RECT {
                    left: 0,
                    top: 0,
                    right: max_w - pad_x * 2,
                    bottom: 0,
                };
                DrawTextW(
                    dc,
                    &mut detail,
                    &mut rc2,
                    DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX | DT_LEFT,
                );
                w = w.max(rc2.right);
                h += rc2.bottom.max(sub_h);
            }
            SelectObject(dc, old);
            (w, h, dc, font, sub_font)
        };

        let w = (text_w + pad_x * 2).clamp(px(24.0), max_w);
        let h = text_h + pad_y * 2;
        text_w = w - pad_x * 2;

        let work = work_area(request.anchor);
        let pos = place(
            request.anchor,
            (w, h),
            work,
            request.prefer_above,
            request.align_right,
            gap,
        );

        // SAFETY: create a top-down 32-bpp DIB, paint it, and hand its DC to
        // UpdateLayeredWindow inside this call.
        let mut frame = unsafe { LayeredBitmap::new(w, h)? };
        // Rounded background and border.
        paint_surface(&mut frame, radius, border);

        // SAFETY: text is painted into the same DC after the rounded fill.
        unsafe {
            let old = SelectObject(frame.dc, font.into());
            SetBkMode(frame.dc, TRANSPARENT);
            SetTextColor(frame.dc, COLORREF(0x00F2F2F2));
            let mut title = HSTRING::from(request.title).to_vec();
            let mut rc = RECT {
                left: pad_x,
                top: pad_y,
                right: pad_x + text_w,
                bottom: pad_y + title_h,
            };
            DrawTextW(
                frame.dc,
                &mut title,
                &mut rc,
                DT_WORDBREAK | DT_NOPREFIX | DT_LEFT | DT_TOP,
            );
            if let Some(detail_text) = request.detail {
                let old_sub = SelectObject(frame.dc, sub_font.into());
                SetTextColor(frame.dc, COLORREF(0x00A8A8A8));
                let mut detail = HSTRING::from(detail_text).to_vec();
                let mut rc = RECT {
                    left: pad_x,
                    top: pad_y + title_h,
                    right: pad_x + text_w,
                    bottom: h - pad_y,
                };
                DrawTextW(
                    frame.dc,
                    &mut detail,
                    &mut rc,
                    DT_WORDBREAK | DT_NOPREFIX | DT_LEFT | DT_TOP,
                );
                SelectObject(frame.dc, old_sub);
            }
            SelectObject(frame.dc, old);
            // GDI text leaves the DIB's alpha byte alone; restore full alpha
            // inside the rounded surface so glyph pixels are visible.
            frame.restore_alpha(radius, border);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(sub_font.into());
        }

        frame.frame = (w, h, pos.x, pos.y);
        Some(frame)
    }

    fn present(&mut self, hwnd: HWND, frame: LayeredBitmap) -> bool {
        let (w, h, x, y) = frame.frame;
        let pos = POINT { x, y };
        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        // SAFETY: frame.dc and its DIB are alive for this call.
        let result = unsafe {
            UpdateLayeredWindow(
                hwnd,
                None,
                Some(&pos),
                Some(&size),
                Some(frame.dc),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        };
        drop(frame);
        result.is_ok()
    }
}

impl Drop for TooltipWindow {
    fn drop(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            // SAFETY: the popup belongs to this module and the UI thread.
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

fn make_font(scale: f32, size_dip: f32, weight: i32) -> windows::Win32::Graphics::Gdi::HFONT {
    let height = -((size_dip * scale).round() as i32);
    // SAFETY: font creation only reads the constant face name.
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (FF_DONTCARE.0 | VARIABLE_PITCH.0) as u32,
            w!("Segoe UI Variable Text"),
        )
    }
}

/// Owns the memory DC, the DIB section and its pixels for one paint.
struct LayeredBitmap {
    dc: HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    pixels: *mut u32,
    old: windows::Win32::Graphics::Gdi::HGDIOBJ,
    w: i32,
    h: i32,
    /// (width, height, x, y)
    frame: (i32, i32, i32, i32),
}

impl LayeredBitmap {
    unsafe fn new(w: i32, h: i32) -> Option<Self> {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        ReleaseDC(None, screen);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: info and bits out-pointer are valid; the section is NULL.
        let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let old = SelectObject(dc, bitmap.into());
        Some(Self {
            dc,
            bitmap,
            pixels: bits as *mut u32,
            old,
            w,
            h,
            frame: (w, h, 0, 0),
        })
    }

    fn pixel(&mut self, x: i32, y: i32) -> &mut u32 {
        // SAFETY: x/y are inside the DIB created with these dimensions.
        unsafe { &mut *self.pixels.add((y * self.w + x) as usize) }
    }

    /// Alpha channel after GDI text work: opaque inside the rounded surface,
    /// transparent outside.
    unsafe fn restore_alpha(&mut self, radius: i32, border: i32) {
        for y in 0..self.h {
            for x in 0..self.w {
                let inside = rounded_inside(x, y, self.w, self.h, radius, border);
                let px = self.pixel(x, y);
                if inside {
                    *px |= 0xFF00_0000;
                } else {
                    *px &= 0x00FF_FFFF;
                }
            }
        }
    }
}

impl Drop for LayeredBitmap {
    fn drop(&mut self) {
        // SAFETY: both handles belong to this bitmap.
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Fills the rounded rectangle and its 1 px border into a fresh DIB. Coverage
/// is 4x4 supersampled so the corners stay smooth without a graphics library.
fn paint_surface(frame: &mut LayeredBitmap, radius: i32, border: i32) {
    let (w, h) = (frame.w, frame.h);
    for y in 0..h {
        for x in 0..w {
            let outer = coverage(x, y, w, h, radius);
            if outer == 0.0 {
                continue;
            }
            let inner = coverage(
                x + border,
                y + border,
                w - border * 2,
                h - border * 2,
                (radius - border).max(0),
            );
            let (r, g, b) = if inner > 0.0 {
                (
                    (design::TOOLTIP_FILL.r * 255.0) as u32,
                    (design::TOOLTIP_FILL.g * 255.0) as u32,
                    (design::TOOLTIP_FILL.b * 255.0) as u32,
                )
            } else {
                (
                    (design::TOOLTIP_BORDER.r * 255.0) as u32,
                    (design::TOOLTIP_BORDER.g * 255.0) as u32,
                    (design::TOOLTIP_BORDER.b * 255.0) as u32,
                )
            };
            let a = (outer * 255.0).round() as u32;
            let pr = (r * a / 255) & 0xFF;
            let pg = (g * a / 255) & 0xFF;
            let pb = (b * a / 255) & 0xFF;
            *frame.pixel(x, y) = (a << 24) | (pr << 16) | (pg << 8) | pb;
        }
    }
}

/// Fraction of a pixel covered by the rounded rect `(x, y, w, h)`.
fn coverage(x: i32, y: i32, w: i32, h: i32, radius: i32) -> f32 {
    if w <= 0 || h <= 0 {
        return 0.0;
    }
    let r = radius.min(w / 2).min(h / 2).max(1);
    let left = x < r;
    let right = x >= w - r;
    let top = y < r;
    let bottom = y >= h - r;
    if !((left || right) && (top || bottom)) {
        return 1.0;
    }
    let rf = r as f32;
    let cx = if left { rf - 0.5 } else { (w - r) as f32 - 0.5 };
    let cy = if top { rf - 0.5 } else { (h - r) as f32 - 0.5 };
    let mut hits = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let px = x as f32 + (sx as f32 + 0.5) / 4.0;
            let py = y as f32 + (sy as f32 + 0.5) / 4.0;
            let dx = px - cx;
            let dy = py - cy;
            if dx * dx + dy * dy <= rf * rf {
                hits += 1;
            }
        }
    }
    hits as f32 / 16.0
}

fn rounded_inside(x: i32, y: i32, w: i32, h: i32, radius: i32, border: i32) -> bool {
    coverage(
        x + border,
        y + border,
        w - border * 2,
        h - border * 2,
        (radius - border).max(0),
    ) > 0.0
}

unsafe extern "system" fn tip_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
        RECT {
            left: l,
            top: t,
            right: r,
            bottom: b,
        }
    }

    #[test]
    fn tips_sit_below_by_default_and_clamp_to_the_work_area() {
        let work = rect(0, 0, 1920, 1040);
        let anchor = rect(900, 400, 940, 430);
        let pos = place(anchor, (120, 36), work, false, false, 4);
        assert_eq!(pos, TipPosition { x: 900, y: 434 });

        // Near the right edge the default placement would clip, so the tip
        // flips to the left of the control instead (spec §6).
        let near_edge = rect(1900, 1000, 1920, 1020);
        let pos = place(near_edge, (200, 40), work, false, false, 4);
        assert_eq!(pos.x, 1900 - 4 - 200, "left of the control");
        assert!(pos.x + 200 <= work.right, "never clipped");
        assert!(pos.y + 40 <= work.bottom);

        // Left of the control can itself overflow; then clamp to the edge.
        let left_edge = rect(0, 400, 40, 430);
        let pos = place(left_edge, (200, 36), work, false, false, 4);
        assert_eq!(pos.x, 0, "clamped to the left work edge");
    }

    #[test]
    fn above_placement_for_the_bottom_row() {
        let work = rect(0, 0, 1920, 1040);
        let anchor = rect(100, 500, 171, 562);
        let pos = place(anchor, (150, 36), work, true, true, 4);
        assert_eq!(pos.y, 500 - 4 - 36);
        assert_eq!(pos.x, 171 - 150, "right-aligned to the tile");
    }

    #[test]
    fn neighbour_tips_are_instant_for_300_ms() {
        let now = Instant::now();
        assert_eq!(delay_ms(None, now), 450);
        assert_eq!(delay_ms(Some(now - Duration::from_millis(299)), now), 0);
        assert_eq!(delay_ms(Some(now - Duration::from_millis(301)), now), 450);
    }
}
