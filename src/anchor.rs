//! anchor.rs — all taskbar/tray geometry and visibility policy.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, SHQueryUserNotificationState, ABM_GETSTATE, ABM_NEW, ABS_AUTOHIDE, APPBARDATA,
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetCursorPos, GetWindowRect, IsWindowVisible,
};

use crate::design::{
    RectD, BUBBLE, BUBBLE_GAP, FLY_H, FLY_W, GAP_TASKBAR, GAP_TRAY, PILL_INSET, PILL_W,
    WINDOW_TOP_PAD, WINDOW_W,
};

const TASKBAR_CLASS: PCWSTR = w!("Shell_TrayWnd");
const TRAY_CLASS: PCWSTR = w!("TrayNotifyWnd");
const FALLBACK_TRAY_RESERVE_DIP: f32 = 280.0;

/// Everything the app needs to place its one window and its surfaces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub taskbar: RECT,
    pub work: RECT,
    pub window: RECT,
    pub dpi: u32,
    pub scale: f32,
    /// Screen coordinates in DIPs.
    pub pill: RectD,
    pub bubble: RectD,
    /// Final resting place of the flyout, in screen DIPs.
    pub flyout: RectD,
    /// Screen DIP origin of the window's client area.
    pub origin: (f32, f32),
    pub taskbar_height_dip: f32,
    pub visible: bool,
    pub insufficient_space: bool,
}

impl Anchor {
    pub fn compute(taskbar: HWND, dpi: u32, slot_offset_dip: i32) -> Option<Self> {
        let mut tb = RECT::default();
        // SAFETY: GetWindowRect writes one RECT to a valid stack pointer.
        unsafe { GetWindowRect(taskbar, &mut tb) }.ok()?;
        if tb.right <= tb.left || tb.bottom <= tb.top {
            return None;
        }
        let fallback_tray = RECT {
            left: tb.right
                - ((FALLBACK_TRAY_RESERVE_DIP * dpi.max(96) as f32 / 96.0).round() as i32),
            top: tb.top,
            right: tb.right,
            bottom: tb.bottom,
        };
        let tray = tray_rect(taskbar, fallback_tray);
        let monitor = unsafe { MonitorFromWindow(taskbar, MONITOR_DEFAULTTOPRIMARY) };
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: mi.cbSize is set and mi is writable.
        let work = if unsafe { GetMonitorInfoW(monitor, &mut mi) }.as_bool() {
            mi.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: tb.right,
                bottom: tb.top,
            }
        };
        let monitor_rect = if unsafe { GetMonitorInfoW(monitor, &mut mi) }.as_bool() {
            mi.rcMonitor
        } else {
            RECT {
                left: 0,
                top: 0,
                right: tb.right,
                bottom: tb.bottom,
            }
        };
        let mut anchor = Self::layout(tb, tray, work, dpi, slot_offset_dip);
        anchor.visible =
            is_allowed() && taskbar_is_shown(tb, monitor_rect) && !anchor.insufficient_space;
        Some(anchor)
    }

    /// Deterministic geometry for `--snapshot` (no Explorer required).
    pub fn synthetic(dpi: u32, screen_w: i32, screen_h: i32, taskbar_h: i32) -> Self {
        let tb = RECT {
            left: 0,
            top: screen_h - taskbar_h,
            right: screen_w,
            bottom: screen_h,
        };
        let scale = dpi as f32 / 96.0;
        let tray_left = screen_w - (280.0 * scale) as i32;
        let tray = RECT {
            left: tray_left,
            top: tb.top,
            right: screen_w,
            bottom: tb.bottom,
        };
        let work = RECT {
            left: 0,
            top: 0,
            right: screen_w,
            bottom: tb.top,
        };
        let mut anchor = Self::layout(tb, tray, work, dpi, 0);
        anchor.visible = true;
        anchor
    }

    pub fn layout(taskbar: RECT, tray: RECT, work: RECT, dpi: u32, slot_offset_dip: i32) -> Self {
        let scale = dpi.max(96) as f32 / 96.0;
        let tb_left = taskbar.left as f32 / scale;
        let tb_top = taskbar.top as f32 / scale;
        let tb_bottom = taskbar.bottom as f32 / scale;
        let t = (taskbar.bottom - taskbar.top) as f32 / scale;
        let tray_left = if tray.right > tray.left {
            tray.left as f32 / scale
        } else {
            taskbar.right as f32 / scale - FALLBACK_TRAY_RESERVE_DIP
        };

        let pill_right = tray_left - GAP_TRAY + slot_offset_dip as f32;
        let pill_left = pill_right - PILL_W;
        // Equal 4 DIP margins top and bottom keep the band inside the taskbar
        // and match the height of a taskbar icon's hit box instead of
        // overhanging the screen edge.
        let pill_h = (t - PILL_INSET * 2.0).max(0.0);
        let pill_top = tb_top + PILL_INSET;
        let pill = RectD::new(pill_left, pill_top, PILL_W, pill_h);
        // The bubble slot stays reserved but now sits just left of the pill so
        // the pill itself keeps the old 12 DIP gap to the tray.
        let bubble = RectD::new(pill_left - BUBBLE_GAP - BUBBLE, pill_top, BUBBLE, BUBBLE);

        let work_left = work.left as f32 / scale;
        let work_right = work.right as f32 / scale;
        let mut fly_left = pill.center_x() - FLY_W / 2.0;
        let min_left = work_left + 12.0;
        let max_left = (work_right - 12.0 - FLY_W).max(min_left);
        fly_left = fly_left.clamp(min_left, max_left);
        let fly_bottom = tb_top - GAP_TASKBAR;
        let flyout = RectD::new(fly_left, fly_bottom - FLY_H, FLY_W, FLY_H);

        let win_w = WINDOW_W;
        let win_h = t + GAP_TASKBAR + FLY_H + WINDOW_TOP_PAD;
        let win_left = pill.center_x() - win_w / 2.0;
        let win_top = tb_bottom - win_h;
        let window = RECT {
            left: (win_left * scale).round() as i32,
            top: (win_top * scale).round() as i32,
            right: ((win_left + win_w) * scale).round() as i32,
            bottom: (tb_bottom * scale).round() as i32,
        };
        // Use the snapped physical origin for local-coordinate conversion so
        // hit testing and drawing cannot drift by a rounding fraction.
        let origin = (window.left as f32 / scale, window.top as f32 / scale);

        let insufficient_space = pill_left < tb_left + 12.0;
        Self {
            taskbar,
            work,
            window,
            dpi,
            scale,
            pill,
            bubble,
            flyout,
            origin,
            taskbar_height_dip: t,
            visible: !insufficient_space,
            insufficient_space,
        }
    }

    /// Convert a screen-DIP rect to window-local DIPs.
    pub fn local(&self, r: RectD) -> RectD {
        RectD::new(r.x - self.origin.0, r.y - self.origin.1, r.w, r.h)
    }

    /// Local DIP point of the cursor, if the app window exists.
    pub fn cursor_local(&self) -> Option<(f32, f32)> {
        let mut pt = POINT::default();
        // SAFETY: GetCursorPos writes one POINT.
        unsafe { GetCursorPos(&mut pt) }.ok()?;
        Some((
            pt.x as f32 / self.scale - self.origin.0,
            pt.y as f32 / self.scale - self.origin.1,
        ))
    }

    pub fn screen_client_rect(&self) -> RectD {
        RectD::new(
            0.0,
            0.0,
            (self.window.right - self.window.left) as f32 / self.scale,
            (self.window.bottom - self.window.top) as f32 / self.scale,
        )
    }
}

/// Effective DPI of the display the taskbar lives on.
pub fn dpi_for_window(hwnd: HWND) -> u32 {
    // SAFETY: GetDpiForWindow only reads properties of the passed window.
    let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd) };
    if dpi == 0 {
        96
    } else {
        dpi
    }
}

pub fn taskbar_window() -> Option<HWND> {
    // SAFETY: class lookup only.
    let hwnd = unsafe { FindWindowW(TASKBAR_CLASS, PCWSTR::null()) }.ok()?;
    (!hwnd.0.is_null()).then_some(hwnd)
}

fn tray_rect(taskbar: HWND, fallback: RECT) -> RECT {
    // SAFETY: child lookup only; the result is checked before use.
    let tray = unsafe { FindWindowExW(Some(taskbar), None, TRAY_CLASS, PCWSTR::null()) }.ok();
    let Some(tray) = tray else {
        return fallback;
    };
    if tray.0.is_null() {
        return fallback;
    }
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(tray, &mut rect) }.is_err() || rect.right <= rect.left {
        fallback
    } else {
        rect
    }
}

pub fn is_allowed() -> bool {
    let Ok(state) = (unsafe { SHQueryUserNotificationState() }) else {
        return true;
    };
    !matches!(
        state,
        QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE
    )
}

fn taskbar_is_shown(tb: RECT, monitor: RECT) -> bool {
    if !unsafe { IsWindowVisible(find_taskbar_hwnd()).as_bool() } {
        return false;
    }
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    // SAFETY: APPBARDATA is initialized with its size.
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) } as u32;
    if state & ABS_AUTOHIDE == 0 {
        return true;
    }
    // An auto-hidden taskbar slides off the monitor leaving a 1-2 px lip.
    let w = (tb.right - tb.left).min(monitor.right) - tb.left.max(monitor.left);
    let h = (tb.bottom - tb.top).min(monitor.bottom) - tb.top.max(monitor.top);
    w > 3 && h > 3
}

fn find_taskbar_hwnd() -> HWND {
    // SAFETY: class lookup only.
    unsafe { FindWindowW(TASKBAR_CLASS, PCWSTR::null()) }.unwrap_or_default()
}

pub fn register_appbar(hwnd: HWND, callback_msg: u32) -> bool {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: callback_msg,
        ..Default::default()
    };
    // SAFETY: APPBARDATA has a valid hwnd and size.
    unsafe { SHAppBarMessage(ABM_NEW, &mut data) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design;

    fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
        RECT {
            left: l,
            top: t,
            right: r,
            bottom: b,
        }
    }

    #[test]
    fn pill_keeps_the_old_width_and_sits_12_dip_left_of_the_tray() {
        let a = Anchor::layout(
            rect(0, 1040, 1920, 1080),
            rect(1600, 1040, 1920, 1080),
            rect(0, 0, 1920, 1040),
            96,
            0,
        );
        assert_eq!(a.pill.w, design::PILL_W);
        assert_eq!(a.pill.h, a.taskbar_height_dip - design::PILL_INSET * 2.0);
        assert_eq!(a.pill.bottom(), 1080.0 - design::PILL_INSET);
        assert_eq!(a.pill.y, 1040.0 + design::PILL_INSET);
        assert_eq!(a.pill.right(), 1600.0 - 12.0);
        assert_eq!(a.bubble.x, a.pill.x - 8.0 - design::BUBBLE);
        assert_eq!(a.flyout.h, design::FLY_H);
        assert_eq!(a.flyout.bottom(), 1040.0 - design::GAP_TASKBAR);
    }

    #[test]
    fn window_never_moves_with_activity_changes() {
        let a = Anchor::layout(
            rect(0, 1040, 1920, 1080),
            rect(1600, 1040, 1920, 1080),
            rect(0, 0, 1920, 1040),
            96,
            0,
        );
        let b = Anchor::layout(
            rect(0, 1040, 1920, 1080),
            rect(1600, 1040, 1920, 1080),
            rect(0, 0, 1920, 1040),
            96,
            24,
        );
        assert_eq!(
            a.window.right - a.window.left,
            b.window.right - b.window.left
        );
        assert_ne!(a.pill.x, b.pill.x);
    }

    #[test]
    fn flyout_is_clamped_not_the_pill() {
        let a = Anchor::layout(
            rect(0, 1040, 1920, 1080),
            rect(100, 1040, 320, 1080),
            rect(0, 0, 1920, 1040),
            96,
            0,
        );
        assert!(a.flyout.x >= 12.0);
        assert!(a.insufficient_space);
        assert!(a.pill.x < 12.0);
    }

    #[test]
    fn dpi_scales_window_and_surfaces() {
        let a = Anchor::layout(
            rect(0, 1300, 2400, 1350),
            rect(2000, 1300, 2400, 1350),
            rect(0, 0, 2400, 1300),
            120,
            0,
        );
        assert_eq!(a.scale, 1.25);
        assert_eq!(
            a.window.right - a.window.left,
            (design::WINDOW_W * 1.25).round() as i32
        );
        // Each edge is snapped to a physical pixel, so an edge landing on .5 can
        // differ from the ideal height by one pixel. The origin and the
        // taskbar-aligned bottom are the authoritative values.
        let ideal_h = ((50.0_f64 / 1.25
            + design::GAP_TASKBAR as f64
            + design::FLY_H as f64
            + design::WINDOW_TOP_PAD as f64)
            * 1.25)
            .round() as i32;
        let actual_h = a.window.bottom - a.window.top;
        assert!(
            (actual_h - ideal_h).abs() <= 1,
            "window height {actual_h} should be within 1 px of {ideal_h}"
        );
        assert_eq!(a.window.bottom, 1350);
    }
}
