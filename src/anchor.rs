//! anchor.rs — all taskbar-geometry logic lives here (AGENTS.md rule).
//!
//! Finds `Shell_TrayWnd`, measures the tray area, and computes the pill's
//! and flyout's on-screen rectangle. Also owns the "should we hide?" policy:
//! fullscreen/presentation mode, auto-hidden taskbar, explorer restarts.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, SHQueryUserNotificationState, ABM_NEW, APPBARDATA, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetWindowRect, IsWindowVisible,
};

/// Logical (96-DPI) metrics per activity — the visual spec.
///
/// Vertical thickness is intentionally NOT 32 DIP: the owner asked for the
/// original chunky band (the first build's ~62 px pill at 125%), so the height
/// follows the tray and starts a few DIP below its top. Widths stay per-activity.
pub const PILL_Y_OFFSET_DIP: f32 = 4.0;

// Compact widths per activity (DIP)
/// Empty pill when nothing is active: just the taskbar material.
pub const IDLE_COMPACT_WIDTH_DIP: f32 = 64.0;
pub const MUSIC_COMPACT_WIDTH_DIP: f32 = 184.0;
pub const CALL_COMPACT_WIDTH_DIP: f32 = 176.0;
pub const SHELF_COMPACT_WIDTH_DIP: f32 = 136.0;
pub const SHELF_DRAG_WIDTH_DIP: f32 = 204.0;
pub const TIMER_COMPACT_WIDTH_DIP: f32 = 116.0;
pub const VOLUME_COMPACT_WIDTH_DIP: f32 = 224.0;

// Expanded flyout sizes per activity (width, height in DIP)
pub const MUSIC_EXPANDED_SIZE_DIP: (f32, f32) = (384.0, 164.0);
pub const CALL_EXPANDED_SIZE_DIP: (f32, f32) = (384.0, 136.0);
pub const SHELF_EXPANDED_SIZE_DIP: (f32, f32) = (384.0, 176.0);
pub const TIMER_EXPANDED_SIZE_DIP: (f32, f32) = (320.0, 124.0);

// Anchoring margins
pub const GAP_LEFT_OF_TRAY_DIP: f32 = 12.0; // 12 px left of the tray
pub const FLYOUT_GAP_ABOVE_TASKBAR_DIP: f32 = 12.0; // 12 px above the taskbar

/// Where (and whether) the pill should be right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Pill rectangle in physical pixels, screen coordinates.
    pub rect: RECT,
    /// `false` → the caller should hide the pill (fullscreen, auto-hidden taskbar, …).
    pub visible: bool,
}

/// Class name of the primary (per-monitor: primary) taskbar window.
const TASKBAR_CLASS: PCWSTR = w!("Shell_TrayWnd");
/// Class name of the notification-area child inside the taskbar.
const TRAY_CLASS: PCWSTR = w!("TrayNotifyWnd");

/// Find the primary taskbar window.
pub fn taskbar_window() -> Option<HWND> {
    let hwnd = unsafe { FindWindowW(TASKBAR_CLASS, PCWSTR::null()).ok()? };
    (!hwnd.0.is_null()).then_some(hwnd)
}

/// Compute where the pill should be. `fallback` is used (hidden) when the
/// taskbar is unavailable, e.g. mid-way through an explorer restart.
pub fn compute(taskbar: HWND, dpi: u32, fallback: RECT, width_dip: f32) -> Placement {
    // 1. Fullscreen 3D app / game / presentation mode / fullscreen store app → hide.
    if is_suppressed() {
        return Placement {
            rect: fallback,
            visible: false,
        };
    }

    // 2. Taskbar rect. Auto-hidden taskbars are invisible or slid off-screen.
    let mut tb = RECT::default();
    if unsafe { GetWindowRect(taskbar, &mut tb) }.is_err()
        || !unsafe { IsWindowVisible(taskbar) }.as_bool()
    {
        return Placement {
            rect: fallback,
            visible: false,
        };
    }

    // 3. If the taskbar no longer intersects its monitor at all it is fully
    //    slid away (auto-hide); hide with it.
    let monitor = unsafe { MonitorFromRect(&tb, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut mi) }.as_bool() && !rects_intersect(tb, mi.rcMonitor)
    {
        return Placement {
            rect: fallback,
            visible: false,
        };
    }

    // 4. Geometry: restore the original vertical thickness. The pill starts
    //    4 DIP below the tray top and is 2 px taller than the tray, matching
    //    the first build's band (62 px at 125%). Horizontally it stays 12 DIP
    //    left of the tray.
    let scale = dpi.max(96) as f32 / 96.0;
    let tray = tray_rect(taskbar).unwrap_or(tb);

    let w = (width_dip * scale).round() as i32;
    let h = (tray.bottom - tray.top + 2).max(1);

    let right = tray.left - (GAP_LEFT_OF_TRAY_DIP * scale).round() as i32;
    let mut left = right - w;
    let min_left = tb.left + (12.0 * scale).round() as i32;
    if left < min_left {
        left = min_left;
    }

    let top = tray.top + (PILL_Y_OFFSET_DIP * scale).round() as i32;
    let bottom = top + h;

    Placement {
        rect: RECT {
            left,
            top,
            right: left + w,
            bottom,
        },
        visible: true,
    }
}

/// Compute where the expanded flyout should be in screen coordinates.
/// "Opens 12 px above the taskbar, centered over the pill, like Quick Settings.
/// It never covers the tray."
pub fn compute_flyout(
    taskbar: HWND,
    dpi: u32,
    pill_screen_rect: RECT,
    flyout_size_dip: (f32, f32),
) -> RECT {
    let mut tb = RECT::default();
    if unsafe { GetWindowRect(taskbar, &mut tb) }.is_err() {
        return RECT::default();
    }
    let tray = tray_rect(taskbar).unwrap_or(tb);
    let scale = dpi.max(96) as f32 / 96.0;

    let w = (flyout_size_dip.0 * scale).round() as i32;
    let h = (flyout_size_dip.1 * scale).round() as i32;

    let bottom = tb.top - (FLYOUT_GAP_ABOVE_TASKBAR_DIP * scale).round() as i32;
    let top = bottom - h;

    let pill_center_x = (pill_screen_rect.left + pill_screen_rect.right) / 2;
    let mut left = pill_center_x - w / 2;
    let mut right = left + w;

    // Never cover the tray area: ensure right edge doesn't push into the tray,
    // or clamp to tray.right / monitor right.
    let monitor = unsafe { MonitorFromRect(&tb, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let monitor_right = if unsafe { GetMonitorInfoW(monitor, &mut mi) }.as_bool() {
        mi.rcWork.right - (12.0 * scale).round() as i32
    } else {
        tray.right
    };

    if right > monitor_right {
        let diff = right - monitor_right;
        left -= diff;
        right -= diff;
    }
    let min_left = tb.left + (12.0 * scale).round() as i32;
    if left < min_left {
        left = min_left;
        right = left + w;
    }

    RECT {
        left,
        top,
        right,
        bottom,
    }
}

/// `SHQueryUserNotificationState`: are we allowed to be seen?
pub fn is_suppressed() -> bool {
    let Ok(state) = (unsafe { SHQueryUserNotificationState() }) else {
        return false;
    };
    matches!(
        state,
        QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE
    )
}

/// Register with the shell as an appbar so we receive `ABN_POSCHANGED`.
pub fn register_appbar(hwnd: HWND, callback_msg: u32) -> bool {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: callback_msg,
        ..Default::default()
    };
    unsafe { SHAppBarMessage(ABM_NEW, &mut data) != 0 }
}

/// The notification area (`TrayNotifyWnd`).
fn tray_rect(taskbar: HWND) -> Option<RECT> {
    let tray = unsafe { FindWindowExW(Some(taskbar), None, TRAY_CLASS, PCWSTR::null()).ok()? };
    if tray.0.is_null() {
        return None;
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(tray, &mut rect) }.ok()?;
    Some(rect)
}

pub fn rects_intersect(a: RECT, b: RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects_intersect_works() {
        let a = RECT {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        };
        assert!(rects_intersect(
            a,
            RECT {
                left: 5,
                top: 5,
                right: 15,
                bottom: 15
            }
        ));
        assert!(!rects_intersect(
            a,
            RECT {
                left: 10,
                top: 10,
                right: 20,
                bottom: 20
            }
        ));
    }
}
