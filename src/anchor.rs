//! anchor.rs — all taskbar/tray geometry and visibility policy.
//!
//! §8 island placement: the taskbar's free region is discovered on watcher
//! threads (the `TaskbarAl` registry value via `RegNotifyChangeKeyValue` and
//! the taskbar's UI Automation structure via change events, debounced by the
//! window) and stored as a [`Space`]; every rectangle the app draws is
//! derived from it here. No other module touches `Shell_TrayWnd` or the
//! taskbar's UIA tree.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use windows::core::{implement, w, Ref, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    SAFEARRAY,
};
use windows::Win32::System::Registry::{
    RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY,
    REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_DWORD,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationElement,
    IUIAutomationStructureChangedEventHandler, IUIAutomationStructureChangedEventHandler_Impl,
    IUIAutomationTreeWalker, SetWinEventHook, StructureChangeType, TreeScope_Descendants,
    HWINEVENTHOOK,
};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, SHQueryUserNotificationState, ABM_GETSTATE, ABS_AUTOHIDE, APPBARDATA,
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowExW, FindWindowW, GetMessageW, GetWindowRect, IsWindowVisible,
    PostMessageW, PostThreadMessageW, TranslateMessage, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_SHOW, MSG, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WM_APP,
};

use crate::design::{
    RectD, BUBBLE, BUBBLE_GAP, FLY_H, FLY_W, GAP_TASKBAR, PILL_INSET, PILL_W, SHADOW_HALO,
    SLOT_GAP_DIP, WINDOW_TOP_PAD, WINDOW_W,
};

const TASKBAR_CLASS: PCWSTR = w!("Shell_TrayWnd");
const TRAY_CLASS: PCWSTR = w!("TrayNotifyWnd");
const FALLBACK_TRAY_RESERVE_DIP: f32 = 280.0;
/// §8: the island's own margin from the screen edge. The pill uses it when
/// it parks in the empty side of a centered taskbar and the flyout uses it
/// when it clamps to the work area, so the pill and the card it opens line
/// up on the same edge (owner request: match the card's border spacing).
const EDGE_MARGIN_DIP: f32 = 12.0;
/// §8: `TaskbarAl` lives in this key (0 = left, 1 = centered).
const ADVANCED_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced");
/// The XAML taskbar's container chain (probe-verified in both alignments).
const BRIDGE_CLASS: &str = "Windows.UI.Composition.DesktopWindowContentBridge";
const SITE_CLASS: &str = "Windows.UI.Input.InputSite.WindowClass";
const FRAME_ID: &str = "TaskbarFrame";
const REPEATER_ID: &str = "TaskbarFrameRepeater";
const WIDGETS_ID: &str = "WidgetsButton";

/// §8 messages. The window debounces geometry changes (+50), asks the
/// watcher thread to rescan (+52/+53), and the scan lands back as an apply
/// (+51). Thread messages never reach a window procedure, so the watcher
/// handles its two inline in its own pump.
pub const WM_APP_SPACE_CHANGED: u32 = WM_APP + 50;
pub const WM_APP_SPACE_APPLY: u32 = WM_APP + 51;
pub const WM_APP_SPACE_SCAN: u32 = WM_APP + 52;
pub const WM_APP_SPACE_REWATCH: u32 = WM_APP + 53;
/// §7/auto-hide: the taskbar window itself moved (the auto-hide bar
/// revealing or hiding). The window re-aligns the pill child at once.
pub const WM_APP_TASKBAR_MOVED: u32 = WM_APP + 54;

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

/// §8: where the taskbar's content sat when the last scan ran. Rects are
/// physical pixels (UIA bounding rectangles).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Space {
    /// `TaskbarAl = 1`: the icon group is centered, so the left side of the
    /// bar is free and that is where the pill belongs.
    pub centered: bool,
    /// Union of the app-button group — Start included, Widgets excluded.
    pub group: RECT,
    /// Right edge of the Widgets button when it sits left of the icon group
    /// (the pill then goes right after it, per spec §8).
    pub widgets_right: Option<i32>,
}

/// §8 (v0.11 owner feedback): placement is Auto-only — there is no user
/// position choice any more (`Island position` and its tray submenu were
/// removed), the app decides where the pill sits: the empty left side when
/// the icons are centered, beside the tray when they are left-aligned.
///
/// Which edge of its free band the pill hugs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Left,
    Right,
}

/// §8 placement inside one free band, in DIP.
///
/// The band's right edge is always a safe gap in front of the tray (or the
/// icon group), so the pill clamps *into* the band in every branch: it can
/// never slide under `TrayNotifyWnd`, whatever a slot offset asks for. A
/// band narrower than the pill puts the pill flush against that right gap
/// (reaching back over the last icons instead — the owner's rule is "never
/// overlap the tray") and returns `insufficient`, which arms the one-time
/// tray notice. Returns `(pill_left, insufficient)`.
fn place(band: (f32, f32), mode: Place, slot: f32) -> (f32, bool) {
    let (left, right) = band;
    let insufficient = right - left < PILL_W;
    // The furthest right the pill may sit: its right edge stays a full
    // PILL_W clear of the band's right edge.
    let max_x = right - PILL_W;
    if insufficient {
        return (max_x, true);
    }
    let wanted = match mode {
        Place::Right => right - PILL_W + slot,
        Place::Left => left + slot,
    };
    (wanted.clamp(left, max_x), false)
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
        // §8: the placement rule reads the last scan — no registry or UIA
        // work happens on this (UI) thread. v0.11: Auto-only, there is no
        // tray position choice any more.
        let space = current_space();
        let mut anchor = Self::layout(tb, tray, work, dpi, slot_offset_dip, space.as_ref());
        // §8: a squeezed pill still shows (with the tray notice); hiding is
        // only about full-screen/presentation state now.
        anchor.visible = is_allowed() && taskbar_is_shown(tb, monitor_rect);
        Some(anchor)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn layout(
        taskbar: RECT,
        tray: RECT,
        work: RECT,
        dpi: u32,
        slot_offset_dip: i32,
        space: Option<&Space>,
    ) -> Self {
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

        // §8: with no scan yet the group hugs the bar's left edge, which
        // leaves the whole bar as one free band (the pre-§8 behaviour).
        let fallback = Space {
            centered: false,
            group: RECT {
                left: taskbar.left,
                top: taskbar.top,
                right: taskbar.left,
                bottom: taskbar.bottom,
            },
            widgets_right: None,
        };
        let space = space.unwrap_or(&fallback);

        // Two free bands, in DIP: the empty left side (only meaningful when
        // the icons are centered) and the space between the last icon and
        // the tray. Element-to-element gaps are the spec's 8 px at 100 %;
        // the empty side starts at the shared edge margin so the pill lines
        // up with the card it opens.
        let group_left = space.group.left as f32 / scale;
        let group_right = space.group.right as f32 / scale;
        let widgets_right = space
            .widgets_right
            .map(|right| right as f32 / scale)
            .filter(|right| *right < group_left);
        let head = (
            widgets_right.map_or(tb_left + EDGE_MARGIN_DIP, |right| right + SLOT_GAP_DIP),
            group_left - SLOT_GAP_DIP,
        );
        let tail = (group_right + SLOT_GAP_DIP, tray_left - SLOT_GAP_DIP);

        // Auto-only (v0.11): the empty left side when the icons are
        // centered and it fits, otherwise the space beside the tray.
        let (band, mode) = auto_band(space.centered, head, tail);
        let (pill_left, insufficient_space) = place(band, mode, slot_offset_dip as f32);

        // Equal 4 DIP margins top and bottom keep the band inside the
        // taskbar and vertically center it at any taskbar height (§8: use
        // the real height, never a constant).
        let pill_h = (t - PILL_INSET * 2.0).max(0.0);
        let pill_top = tb_top + PILL_INSET;
        let pill = RectD::new(pill_left, pill_top, PILL_W, pill_h);
        // The bubble slot stays reserved just left of the pill.
        let bubble = RectD::new(pill_left - BUBBLE_GAP - BUBBLE, pill_top, BUBBLE, BUBBLE);

        let work_left = work.left as f32 / scale;
        let work_right = work.right as f32 / scale;
        let mut fly_left = pill.center_x() - FLY_W / 2.0;
        let min_left = work_left + EDGE_MARGIN_DIP;
        let max_left = (work_right - EDGE_MARGIN_DIP - FLY_W).max(min_left);
        fly_left = fly_left.clamp(min_left, max_left);
        let fly_bottom = tb_top - GAP_TASKBAR;
        let flyout = RectD::new(fly_left, fly_bottom - FLY_H, FLY_W, FLY_H);

        let win_w = WINDOW_W;
        let win_h = t + GAP_TASKBAR + FLY_H + WINDOW_TOP_PAD;
        // §8: the pill can sit at either screen edge (centered alignment, a
        // cramped band), and the flyout is clamped to the work area there —
        // so nudge the window until it covers *both* surfaces. It may start
        // no further right than either surface's left edge and its right
        // edge must reach the flyout's shadow halo, or the open card would
        // be clipped by its own window (a hard edge with no shadow on that
        // side). FLY_W + SHADOW_HALO < WINDOW_W, so this bound always stays
        // left of the flyout's own left edge and cannot lose coverage.
        let win_left = (pill.center_x() - win_w / 2.0)
            .min(fly_left)
            .min(pill.x)
            .max(fly_left + FLY_W + SHADOW_HALO - win_w)
            .max(pill.right() - win_w);
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
            // §8: visibility no longer depends on the band width — the
            // squeeze case shows the pill plus a tray notice instead.
            visible: true,
            insufficient_space,
        }
    }

    /// Convert a screen-DIP rect to window-local DIPs.
    pub fn local(&self, r: RectD) -> RectD {
        RectD::new(r.x - self.origin.0, r.y - self.origin.1, r.w, r.h)
    }
}

/// Auto's band: the empty left side when the icons are centered (when it
/// actually fits), otherwise the space beside the tray.
fn auto_band(centered: bool, head: (f32, f32), tail: (f32, f32)) -> ((f32, f32), Place) {
    if centered && head.1 - head.0 >= PILL_W {
        (head, Place::Left)
    } else {
        (tail, Place::Right)
    }
}

// ---------------------------------------------------------------------------
// §8 shared state.

/// Last scanned free region; `None` until the first scan lands.
static SPACE: Mutex<Option<Space>> = Mutex::new(None);
/// Island window the watchers post back to.
static SPACE_HWND: AtomicIsize = AtomicIsize::new(0);
/// UIA watcher thread id (0 = not started / already gone).
static WATCH_THREAD: AtomicU32 = AtomicU32::new(0);
/// `true` while the squeeze notice may fire. The first notice clears it;
/// `notice_clear` re-arms it once the space is back — one notice per
/// episode, never one per scan.
static NOTICE_ARMED: AtomicBool = AtomicBool::new(true);
/// `Shell_TrayWnd` as last seen by the move hook (0 = never). Every other
/// window that moves re-resolves the class once and re-caches, so the hot
/// path for the frequent events is a single atomic load.
static TASKBAR_HOOK_TARGET: AtomicIsize = AtomicIsize::new(0);

fn store_space(space: Space) {
    *SPACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(space);
}

fn current_space() -> Option<Space> {
    *SPACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// May the squeeze notice fire now? Consumes the one allowed firing.
pub fn notice_due() -> bool {
    NOTICE_ARMED
        .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

/// The free space came back — arm the notice for the next episode.
pub fn notice_clear() {
    NOTICE_ARMED.store(true, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Effective DPI of the display the taskbar lives on.

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

/// The pill child's rect in `Shell_TrayWnd` client coordinates (§7 Stage A).
///
/// The child window is placed at exactly the island window's screen position
/// — client (0, 0) lands on the island's own origin, so both windows share
/// one coordinate space and the drawing, hit-testing and DWM-region numbers
/// are identical for each (one rendered frame, two windows). The bottom is
/// stretched to the taskbar's bottom edge so the child's own rect still
/// covers the pill band: the island window itself sits *above* the taskbar,
/// and a child is clipped to its parent, never the other way round.
///
/// `Shell_TrayWnd` has no non-client area, so client coordinates are the
/// window rect translated by its origin — spike-verified: a child placed at
/// client (8, 13) measured at screen (8, 1153) with the taskbar at
/// (0, 1140).
pub fn pill_child(window: RECT, taskbar: RECT) -> RECT {
    RECT {
        left: window.left - taskbar.left,
        top: window.top - taskbar.top,
        right: window.right - taskbar.left,
        bottom: window.bottom.max(taskbar.bottom) - taskbar.top,
    }
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

// ---------------------------------------------------------------------------
// §8: alignment, scans and watchers.

/// `TaskbarAl` — 1 = icons centered, anything else (or absent) = left.
/// One synchronous read; the *watch* on the value is the registry thread
/// below, never a poll.
fn taskbar_alignment() -> bool {
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: value and size point at live locals; the rest are plain
    // parameters (null type pointer, no data buffer beyond ours).
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            ADVANCED_KEY,
            w!("TaskbarAl"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    status.0 == 0 && value == 1
}

/// §8: one bounded scan before the first `Anchor::compute`, so the very
/// first frame is already in the right slot. The scan runs on its own MTA
/// thread and never blocks the UI thread past `budget`; a result that
/// misses the budget is still stored and picked up by the next anchor sync.
pub fn scan_initial(budget: Duration) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        init_mta();
        let found = if let Some(space) = scan_space() {
            store_space(space);
            true
        } else {
            false
        };
        // SAFETY: matched with the CoInitializeEx above on this thread.
        unsafe { CoUninitialize() };
        let _ = tx.send(found);
    });
    match rx.recv_timeout(budget) {
        Ok(true) => {}
        Ok(false) => crate::log::log("anchor.scan: no taskbar space at startup"),
        Err(_) => crate::log::log("anchor.scan: startup scan still running"),
    }
}

/// §8: start the two geometry watchers. Both are fire-and-forget threads
/// (like the media/call/clipboard workers): the process exit ends them.
/// The first debounced rescan closes the registration race with
/// `scan_initial`.
pub fn start_watching(hwnd: HWND) {
    SPACE_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
    std::thread::spawn(registry_watcher);
    std::thread::spawn(uia_watcher);
    install_move_hook();
    request_space_changed(false);
}

/// §7/auto-hide: watch `Shell_TrayWnd` for moves, shows and hides. An
/// auto-hidden bar reveals by sliding the taskbar window (a visibility
/// transition can end it), and the pill child is parented to it — its
/// parent-relative rect is only right for the bar's *current* position.
/// Without these events a reveal left the child baked against the hidden
/// rect until the next 2 s guard tick, so the pill appeared up to 2 s late
/// (the "sometimes instant" cases were the tick having just run). Three
/// OUTOFCONTEXT hooks on this pumping thread, installed once for the process
/// lifetime (the OS drops them on exit, like the watcher threads); the
/// callback only posts a message.
fn install_move_hook() {
    const EVENTS: &[u32] = &[
        EVENT_OBJECT_LOCATIONCHANGE,
        EVENT_OBJECT_SHOW,
        EVENT_OBJECT_HIDE,
    ];
    for event in EVENTS {
        // SAFETY: `move_hook_proc` lives as long as the process, and
        // OUTOFCONTEXT means the system only queues it to this thread.
        let hook = unsafe {
            SetWinEventHook(
                *event,
                *event,
                None,
                Some(move_hook_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.0.is_null() {
            crate::log::log(&format!("anchor.watch: taskbar hook {event} failed"));
        }
    }
}

/// SAFETY: called by the system on this thread only (OUTOFCONTEXT).
unsafe extern "system" fn move_hook_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    idobject: i32,
    _idchild: i32,
    _ideventthread: u32,
    _eventtime: u32,
) {
    // Window-level moves only; every other window re-resolves the class
    // once (the taskbar gets a new handle after an Explorer restart) and
    // re-caches, so the frequent path is one atomic load.
    if hwnd.0.is_null() || idobject != OBJID_WINDOW.0 {
        return;
    }
    let raw = hwnd.0 as isize;
    if raw != TASKBAR_HOOK_TARGET.load(Ordering::Relaxed) {
        if find_taskbar_hwnd().0 as isize != raw {
            return;
        }
        TASKBAR_HOOK_TARGET.store(raw, Ordering::Relaxed);
    }
    post_island(WM_APP_TASKBAR_MOVED, false);
}

/// The window's debounce timer expired: ask the watcher to rescan.
pub fn request_scan() {
    let thread = WATCH_THREAD.load(Ordering::Relaxed);
    if thread != 0 {
        // SAFETY: thread messages are fire-and-forget; a dead id fails
        // harmlessly and the next event tries again.
        unsafe {
            let _ = PostThreadMessageW(thread, WM_APP_SPACE_SCAN, WPARAM(0), LPARAM(0));
        }
    }
}

/// Explorer restarted (`TaskbarCreated`): the watched taskbar element died,
/// so re-register the structure handler on the new one. It rescans and
/// applies on its own.
pub fn rewatch() {
    let thread = WATCH_THREAD.load(Ordering::Relaxed);
    if thread != 0 {
        // SAFETY: same fire-and-forget contract as `request_scan`.
        unsafe {
            let _ = PostThreadMessageW(thread, WM_APP_SPACE_REWATCH, WPARAM(0), LPARAM(0));
        }
    }
}

/// Ask the island window to re-place the pill (debounced by its timer when
/// this is a geometry-change signal). `alignment` marks a `TaskbarAl` flip,
/// which needs the longer debounce: Windows slides the icons into their new
/// alignment for ~550 ms and a scan in between reads a half-way group.
fn request_space_changed(alignment: bool) {
    post_island(WM_APP_SPACE_CHANGED, alignment);
}

fn post_island(message: u32, alignment: bool) {
    let raw = SPACE_HWND.load(Ordering::Relaxed);
    if raw == 0 {
        return;
    }
    let hwnd = HWND(raw as *mut core::ffi::c_void);
    // SAFETY: the island window stored by `start_watching`; posting is
    // asynchronous and never touches state from here.
    unsafe {
        let _ = PostMessageW(Some(hwnd), message, WPARAM(alignment as usize), LPARAM(0));
    }
}

/// One blocking registry watch: `RegNotifyChangeKeyValue` with no event
/// handle returns only when a value under `Explorer\Advanced` changed, then
/// we re-arm. The alignment (`TaskbarAl`) lives there and is the only value
/// this watches for — a write to any sibling value is read, compared and
/// ignored. No polling, no timers — the thread simply sleeps inside the API
/// call.
fn registry_watcher() {
    let mut key = HKEY(std::ptr::null_mut());
    // SAFETY: standard open into a writable local.
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, ADVANCED_KEY, None, KEY_NOTIFY, &mut key) };
    if status.0 != 0 {
        crate::log::log(&format!(
            "anchor.watch: Advanced key unavailable ({})",
            status.0
        ));
        return;
    }
    // Remember the alignment this watch was armed on: Explorer and other
    // apps write unrelated values under the same key often, and each of
    // those used to run a 900 ms alignment wait plus a full rescan (a whole
    // class of spurious "the island reacts slowly" moments). Only a real
    // `TaskbarAl` flip is the alignment signal; structure changes still
    // reach the scanner through the UIA watcher within 150 ms.
    let mut armed = taskbar_alignment();
    loop {
        // SAFETY: the key is open with KEY_NOTIFY; no event handle means
        // this call blocks until the value changes (synchronous watch).
        let status =
            unsafe { RegNotifyChangeKeyValue(key, true, REG_NOTIFY_CHANGE_LAST_SET, None, false) };
        if status.0 != 0 {
            crate::log::log(&format!(
                "anchor.watch: registry watch ended ({})",
                status.0
            ));
            return;
        }
        let alignment = taskbar_alignment();
        if alignment == armed {
            continue; // a sibling value under `Explorer\Advanced`
        }
        armed = alignment;
        // The icons slide for ~550 ms, so the window uses the longer debounce.
        request_space_changed(true);
    }
}

/// The UIA watcher thread: registers the taskbar's structure-change handler
/// once and pumps its own queue forever. Structure events, rescan requests
/// (+52) and re-register requests (+53) all resolve to one debounced rescan
/// whose result the window applies with the 200 ms slide.
fn uia_watcher() {
    init_mta();
    // SAFETY: MTA on this worker; the id is needed before the first post.
    WATCH_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    let automation = match create_automation() {
        Ok(automation) => Some(automation),
        Err(error) => {
            crate::log::log(&format!("anchor.watch: UIA client unavailable: {error}"));
            None
        }
    };
    let mut registration: Option<(
        IUIAutomationElement,
        IUIAutomationStructureChangedEventHandler,
    )> = None;
    if let Some(automation) = &automation {
        registration = register_structure_handler(automation);
    }
    loop {
        let mut message = MSG::default();
        // SAFETY: our own thread's queue; 0 = WM_QUIT, <0 = error.
        let got = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        match message.message {
            WM_APP_SPACE_SCAN => {
                if let Some(automation) = &automation {
                    // A registration that failed at startup is retried on
                    // every rescan, so one transient error is enough.
                    if registration.is_none() {
                        registration = register_structure_handler(automation);
                    }
                    rescan(Some(automation));
                } else {
                    rescan(None);
                }
            }
            WM_APP_SPACE_REWATCH => {
                if let Some(automation) = &automation {
                    if let Some((element, handler)) = registration.take() {
                        // SAFETY: the pair was returned by our own Add call.
                        let _ = unsafe {
                            automation.RemoveStructureChangedEventHandler(&element, &handler)
                        };
                    }
                    registration = register_structure_handler(automation);
                    rescan(Some(automation));
                } else {
                    rescan(None);
                }
            }
            _ => {
                // SAFETY: a normal dispatch of a message we just read —
                // UIA delivers its callbacks through this pump.
                unsafe {
                    let _ = TranslateMessage(&message);
                    let _ = DispatchMessageW(&message);
                }
            }
        }
    }
    WATCH_THREAD.store(0, Ordering::Relaxed);
    // SAFETY: matched with the CoInitializeEx above on this thread.
    unsafe { CoUninitialize() };
}

/// One rescan (watcher thread); the window applies the result with the
/// 200 ms slide when it receives `WM_APP_SPACE_APPLY`.
fn rescan(automation: Option<&IUIAutomation>) {
    let space = taskbar_window().and_then(|bar| {
        automation
            .and_then(|automation| scan_space_uia(automation, bar))
            .or_else(|| scan_space_win32(bar))
    });
    match space {
        Some(space) => store_space(space),
        None => crate::log::log("anchor.scan: no taskbar space found"),
    }
    post_island(WM_APP_SPACE_APPLY, false);
}

/// Register for structure changes (icons pinned/unpinned, windows opening)
/// under the taskbar. The handler only posts the debounce message; the
/// window collapses any event storm into one 150 ms wait.
fn register_structure_handler(
    automation: &IUIAutomation,
) -> Option<(
    IUIAutomationElement,
    IUIAutomationStructureChangedEventHandler,
)> {
    let bar = taskbar_window()?;
    // SAFETY: querying UIA for our own taskbar window.
    let element = unsafe { automation.ElementFromHandle(bar) }.ok()?;
    let handler: IUIAutomationStructureChangedEventHandler = SpaceHandler {}.into();
    // SAFETY: registration lives as long as this thread (the pair is kept
    // alive by the pump, and removed before re-registering).
    let registered = unsafe {
        automation.AddStructureChangedEventHandler(
            &element,
            TreeScope_Descendants,
            None::<&IUIAutomationCacheRequest>,
            &handler,
        )
    };
    match registered {
        Ok(()) => Some((element, handler)),
        Err(error) => {
            crate::log::log(&format!("anchor.watch: structure hook failed: {error}"));
            None
        }
    }
}

/// The structure-change callback: nothing but a post — the scan itself
/// happens later on the watcher thread, behind the window's debounce.
#[implement(IUIAutomationStructureChangedEventHandler)]
struct SpaceHandler {}

impl IUIAutomationStructureChangedEventHandler_Impl for SpaceHandler_Impl {
    fn HandleStructureChangedEvent(
        &self,
        _sender: Ref<'_, IUIAutomationElement>,
        _changetype: StructureChangeType,
        _runtimeid: *const SAFEARRAY,
    ) -> windows::core::Result<()> {
        // A structure change (pin, window, tray icon) is not an alignment
        // flip: the short debounce is right, and it must not extend an
        // alignment wait that is already running.
        request_space_changed(false);
        Ok(())
    }
}

fn init_mta() {
    // SAFETY: COINIT_MULTITHREADED on a worker thread; RPC_E_CHANGED_MODE
    // is harmless (the thread never hosted STA).
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

fn create_automation() -> windows::core::Result<IUIAutomation> {
    // SAFETY: class id and interface pair match the UIA core server.
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
}

/// One full scan on the calling thread (startup helper only). UIA first,
/// Win32 fallback when the XAML chain cannot be resolved.
fn scan_space() -> Option<Space> {
    let bar = taskbar_window()?;
    let uia = create_automation()
        .ok()
        .and_then(|automation| scan_space_uia(&automation, bar));
    uia.or_else(|| scan_space_win32(bar))
}

/// §8: the targeted raw-view descent that finds the icon group. `FindAll`
/// cannot do this — it walks the *control view*, which hides the
/// `TaskbarFrame` container — so every step is an explicit walker step
/// (probe-verified in both alignments).
fn scan_space_uia(automation: &IUIAutomation, bar: HWND) -> Option<Space> {
    // SAFETY: querying UIA for our own taskbar window.
    let root = unsafe { automation.ElementFromHandle(bar) }.ok()?;
    // SAFETY: the raw walker is a plain UIA object.
    let walker = unsafe { automation.RawViewWalker() }.ok()?;
    // taskbar -> composition bridge -> input site -> TaskbarFrame.
    let bridge = find_child(&walker, &root, |element| {
        element_class(element) == BRIDGE_CLASS
    })?;
    let site = find_child(&walker, &bridge, |element| {
        element_class(element) == SITE_CLASS
    })?;
    let frame = find_child(&walker, &site, |element| {
        let id = element_id(element);
        id == FRAME_ID || id == REPEATER_ID
    })
    .or_else(|| find_descendant(&walker, &root, FRAME_ID, 4))?;
    let children = collect_children(&walker, &frame);
    if children.is_empty() {
        return None;
    }
    // Widgets (when shown) leaves the group and is remembered: the pill
    // goes right after it. It is looked up at both levels because builds
    // ship it either as a frame child or inside the repeater.
    let widgets = children.iter().find_map(|child| {
        if is_widgets(child) {
            return element_rect(child);
        }
        collect_children(&walker, child)
            .iter()
            .find(|grand| is_widgets(grand))
            .and_then(element_rect)
    });
    let group = group_rect(&walker, &children)?;
    if group.right <= group.left {
        return None;
    }
    Some(Space {
        centered: taskbar_alignment(),
        group,
        widgets_right: widgets.map(|rect| rect.right),
    })
}

/// Union of the frame's buttons. One container child (the repeater) means
/// *its* children are the real buttons — unioning those also drops a
/// Widgets button parked at either edge of the cluster, which is what puts
/// the pill right after it instead of before it.
fn group_rect(walker: &IUIAutomationTreeWalker, children: &[IUIAutomationElement]) -> Option<RECT> {
    let rects: Vec<RECT> = if children.len() == 1 {
        let grands: Vec<RECT> = collect_children(walker, &children[0])
            .iter()
            .filter(|element| !is_widgets(element))
            .filter_map(element_rect)
            .collect();
        // Two or more grandchildren are buttons; one would be an icon
        // inside a lone button (too small to be the group).
        if grands.len() >= 2 {
            grands
        } else {
            child_rects(children)
        }
    } else {
        child_rects(children)
    };
    let mut group: Option<RECT> = None;
    for rect in rects {
        extend(&mut group, Some(rect));
    }
    group
}

fn child_rects(children: &[IUIAutomationElement]) -> Vec<RECT> {
    children
        .iter()
        .filter(|element| !is_widgets(element))
        .filter_map(element_rect)
        .collect()
}

/// Win32 fallback when the UIA chain cannot be resolved: the legacy group
/// windows plus Start. The `ReBarWindow32` rect is stale by one unpinned
/// running button (probe-verified), so this path is only a fallback — the
/// flush-right Auto placement barely depends on the exact group right edge.
fn scan_space_win32(bar: HWND) -> Option<Space> {
    let mut group: Option<RECT> = None;
    extend(&mut group, child_rect(bar, w!("Start")));
    extend(
        &mut group,
        child_rect(bar, w!("ReBarWindow32")).or_else(|| child_rect(bar, w!("MSTaskSwWClass"))),
    );
    let group = group?;
    if group.right <= group.left {
        return None;
    }
    Some(Space {
        centered: taskbar_alignment(),
        group,
        widgets_right: None,
    })
}

fn child_rect(parent: HWND, class: PCWSTR) -> Option<RECT> {
    // SAFETY: class lookup under our own parent window.
    let child = unsafe { FindWindowExW(Some(parent), None, class, PCWSTR::null()) }.ok()?;
    if child.0.is_null() {
        return None;
    }
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(child, &mut rect) }.is_err() || rect.right <= rect.left {
        None
    } else {
        Some(rect)
    }
}

/// The frame's direct children, walked raw (budgeted so a pathological
/// tree can never spin).
fn collect_children(
    walker: &IUIAutomationTreeWalker,
    parent: &IUIAutomationElement,
) -> Vec<IUIAutomationElement> {
    let mut out = Vec::new();
    // SAFETY: walker and element are live UIA objects from this thread.
    let mut child = unsafe { walker.GetFirstChildElement(parent) }.ok();
    while let Some(element) = child {
        out.push(element.clone());
        if out.len() >= 128 {
            break;
        }
        // No sibling is reported as an error — that ends the list.
        // SAFETY: same live objects as above.
        child = unsafe { walker.GetNextSiblingElement(&element) }.ok();
    }
    out
}

fn find_child(
    walker: &IUIAutomationTreeWalker,
    parent: &IUIAutomationElement,
    predicate: impl Fn(&IUIAutomationElement) -> bool,
) -> Option<IUIAutomationElement> {
    collect_children(walker, parent).into_iter().find(predicate)
}

/// Breadth-first raw search for one AutomationId, used when the canonical
/// bridge -> site -> frame path does not resolve (structure variants).
fn find_descendant(
    walker: &IUIAutomationTreeWalker,
    root: &IUIAutomationElement,
    id: &str,
    max_depth: u32,
) -> Option<IUIAutomationElement> {
    let mut frontier = vec![root.clone()];
    let mut visited = 0usize;
    for _ in 0..max_depth {
        let mut next = Vec::new();
        for element in &frontier {
            for child in collect_children(walker, element) {
                visited += 1;
                if visited > 256 {
                    return None;
                }
                if element_id(&child) == id {
                    return Some(child);
                }
                next.push(child);
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    None
}

fn is_widgets(element: &IUIAutomationElement) -> bool {
    let id = element_id(element);
    id == WIDGETS_ID || id == "Widgets" || element_name(element) == "Widgets"
}

fn element_id(element: &IUIAutomationElement) -> String {
    // SAFETY: element is a live UIA proxy; the returned BSTR is ours.
    unsafe { element.CurrentAutomationId() }
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn element_class(element: &IUIAutomationElement) -> String {
    // SAFETY: same as `element_id`.
    unsafe { element.CurrentClassName() }
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn element_name(element: &IUIAutomationElement) -> String {
    // SAFETY: same as `element_id`.
    unsafe { element.CurrentName() }
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn element_rect(element: &IUIAutomationElement) -> Option<RECT> {
    // SAFETY: same as `element_id`; UIA rects are physical pixels.
    unsafe { element.CurrentBoundingRectangle() }.ok()
}

fn extend(target: &mut Option<RECT>, add: Option<RECT>) {
    let Some(add) = add else {
        return;
    };
    *target = Some(match *target {
        Some(existing) => RECT {
            left: existing.left.min(add.left),
            top: existing.top.min(add.top),
            right: existing.right.max(add.right),
            bottom: existing.bottom.max(add.bottom),
        },
        None => add,
    });
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

    fn space(centered: bool, group: RECT, widgets_right: Option<i32>) -> Space {
        Space {
            centered,
            group,
            widgets_right,
        }
    }

    /// One bar geometry every placement test shares: 1920x1200 screen, a
    /// 60 px taskbar at the bottom, work area above it.
    fn bar() -> (RECT, RECT, RECT) {
        (
            rect(0, 1140, 1920, 1200),
            rect(1519, 1140, 1920, 1200),
            rect(0, 0, 1920, 1140),
        )
    }

    #[test]
    fn pill_child_shares_the_island_origin_and_covers_the_pill_band() {
        // Real v0.11 layout at 125%: island window (card + halo + pill span)
        // above a 60 px taskbar; the pill lives at screen y 1145..1195.
        let window = rect(1059, 837, 1669, 1200);
        let taskbar = rect(0, 1140, 1920, 1200);
        let child = pill_child(window, taskbar);
        // Client (0, 0) is the island window's screen origin, so pill/card
        // coordinates are identical in both windows.
        assert_eq!((child.left, child.top), (1059, -303));
        assert_eq!(child.right - child.left, window.right - window.left);
        // The bottom reaches the taskbar's bottom edge, so the pill band
        // (taskbar-client y 5..55) lies inside the child's own rect instead
        // of being clipped away by it.
        assert_eq!(child.bottom, taskbar.bottom - taskbar.top);
        // Even a window that ends above the taskbar is stretched down.
        let high = rect(1059, 837, 1669, 1132);
        assert_eq!(pill_child(high, taskbar).bottom, 60);
        // A window that already reaches the taskbar bottom is not grown past
        // it (the child must not extend below its parent's client area).
        let deep = rect(1059, 837, 1669, 1260);
        assert_eq!(pill_child(deep, taskbar).bottom, 120);
    }

    #[test]
    fn pill_keeps_the_old_width_and_sits_8_dip_left_of_the_tray() {
        // Auto with no scan falls back to a group at the bar's left edge,
        // so the tail band ends SLOT_GAP_DIP (§8's 8 px at 100%) before
        // the tray.
        let a = Anchor::layout(
            rect(0, 1040, 1920, 1080),
            rect(1600, 1040, 1920, 1080),
            rect(0, 0, 1920, 1040),
            96,
            0,
            None,
        );
        assert_eq!(a.pill.w, design::PILL_W);
        assert_eq!(a.pill.h, a.taskbar_height_dip - design::PILL_INSET * 2.0);
        assert_eq!(a.pill.bottom(), 1080.0 - design::PILL_INSET);
        assert_eq!(a.pill.y, 1040.0 + design::PILL_INSET);
        assert_eq!(a.pill.right(), 1600.0 - design::SLOT_GAP_DIP);
        assert!(!a.insufficient_space);
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
            None,
        );
        let b = Anchor::layout(
            rect(0, 1040, 1920, 1080),
            rect(1600, 1040, 1920, 1080),
            rect(0, 0, 1920, 1040),
            96,
            -24,
            None,
        );
        assert_eq!(
            a.window.right - a.window.left,
            b.window.right - b.window.left
        );
        assert_ne!(a.pill.x, b.pill.x);
    }

    #[test]
    fn flyout_is_clamped_not_the_pill() {
        // A band squeezed to 84 DIP between the icons and a tray parked at
        // the far left: the pill cannot fit, so it clamps flush against the
        // tray's safe gap (never under the tray — owner rule) and reports
        // the squeeze. The card still clamps to the work area.
        let a = Anchor::layout(
            rect(0, 1040, 1920, 1080),
            rect(100, 1040, 320, 1080),
            rect(0, 0, 1920, 1040),
            96,
            0,
            None,
        );
        assert!(a.flyout.x >= 12.0);
        assert!(a.insufficient_space);
        assert_eq!(a.pill.right(), 100.0 - design::SLOT_GAP_DIP);
        assert!(a.visible, "§8: a squeezed pill still shows");
    }

    #[test]
    fn dpi_scales_window_and_surfaces() {
        let a = Anchor::layout(
            rect(0, 1300, 2400, 1350),
            rect(2000, 1300, 2400, 1350),
            rect(0, 0, 2400, 1300),
            120,
            0,
            None,
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

    #[test]
    fn centered_icons_use_the_empty_left_side() {
        // Probe geometry for TaskbarAl = 1: the group starts at x 547 and
        // the whole left side is free.
        let (taskbar, tray, work) = bar();
        let icons = space(true, rect(547, 1140, 1373, 1200), None);
        let a = Anchor::layout(taskbar, tray, work, 96, 0, Some(&icons));
        // Left margin 12 px — the same edge spacing the card clamps to, so
        // the pill and the flyout it opens line up on the screen border…
        assert_eq!(a.pill.x, EDGE_MARGIN_DIP);
        // …and a full gap before the first icon.
        assert!(a.pill.right() <= 547.0 - design::SLOT_GAP_DIP);
        assert!(!a.insufficient_space);
    }

    #[test]
    fn widgets_button_pushes_the_pill_to_its_right() {
        let (taskbar, tray, work) = bar();
        // Widgets standing left of the cluster: the pill goes right after it.
        // The band from Widgets to the icons must fit the pill (300 + 8 to
        // 600 - 8 = 284 DIP wide) or Auto would fall back to the tail.
        let beside = space(true, rect(600, 1140, 1400, 1200), Some(300));
        let a = Anchor::layout(taskbar, tray, work, 96, 0, Some(&beside));
        assert_eq!(a.pill.x, 300.0 + design::SLOT_GAP_DIP);
        assert!(a.pill.right() <= 600.0 - design::SLOT_GAP_DIP);
        // Widgets inside the cluster does not claim the left side: Auto
        // still means "empty left side, screen-edge margin".
        let inside = space(true, rect(600, 1140, 1400, 1200), Some(700));
        let b = Anchor::layout(taskbar, tray, work, 96, 0, Some(&inside));
        assert_eq!(b.pill.x, EDGE_MARGIN_DIP);
    }

    #[test]
    fn left_aligned_auto_hugs_the_tray_with_the_8_dip_gap() {
        // Probe geometry for TaskbarAl = 0: icons end at x 839, tray starts
        // at 1519.
        let (taskbar, tray, work) = bar();
        let icons = space(false, rect(0, 1140, 839, 1200), None);
        let a = Anchor::layout(taskbar, tray, work, 96, 0, Some(&icons));
        assert_eq!(a.pill.right(), 1519.0 - design::SLOT_GAP_DIP);
        assert!(a.pill.x >= 839.0 + design::SLOT_GAP_DIP);
        assert!(!a.insufficient_space);
    }

    #[test]
    fn left_aligned_auto_never_reaches_under_the_tray() {
        // "Left"-style placement (flush after the icons) is gone with the
        // menu: Auto in a left-aligned bar hugs the tray, and a positive
        // slot offset can no longer push the pill into the tray either.
        let (taskbar, tray, work) = bar();
        let icons = space(false, rect(0, 1140, 839, 1200), None);
        let a = Anchor::layout(taskbar, tray, work, 96, 240, Some(&icons));
        assert_eq!(a.pill.right(), 1519.0 - design::SLOT_GAP_DIP);
        assert!(!a.insufficient_space);
    }

    #[test]
    fn cramped_free_space_never_covers_the_tray() {
        // 84 DIP between the last icon and the tray: the pill cannot fit.
        // Owner rule: it never slides under the tray — it clamps flush to
        // the tray's safe gap instead (reaching back over the last icons),
        // never shrinks, and arms the one-time notice.
        let (taskbar, _tray, work) = bar();
        let icons = space(false, rect(0, 1140, 1300, 1200), None);
        let a = Anchor::layout(
            taskbar,
            rect(1400, 1140, 1920, 1200),
            work,
            96,
            0,
            Some(&icons),
        );
        assert!(a.insufficient_space);
        assert_eq!(a.pill.w, design::PILL_W, "the pill never shrinks");
        assert_eq!(a.pill.right(), 1400.0 - design::SLOT_GAP_DIP);
        assert!(a.pill.x < 1300.0 + design::SLOT_GAP_DIP);
        assert!(a.visible, "§8: a squeezed pill still shows");
    }

    #[test]
    fn window_covers_pill_and_clamped_flyout_at_the_screen_edge() {
        // Centered alignment parks the pill against the left edge, where the
        // flyout clamps to the work area (12 DIP) while the window keeps its
        // fixed width — the window must be nudged until it holds both, or the
        // open card would be clipped by its own window.
        let (taskbar, tray, work) = bar();
        let icons = space(true, rect(547, 1140, 1373, 1200), None);
        let a = Anchor::layout(taskbar, tray, work, 96, 0, Some(&icons));
        assert_eq!(a.pill.x, EDGE_MARGIN_DIP, "matches the card's edge margin");
        let win_left = a.window.left as f32 / a.scale;
        let win_right = a.window.right as f32 / a.scale;
        assert!(win_left <= a.pill.x, "window must reach the pill's left");
        assert!(
            win_left <= a.flyout.x,
            "window must reach the flyout's left"
        );
        assert!(win_right >= a.flyout.right(), "card must not be clipped");
        assert!(
            win_right >= a.flyout.right() + design::SHADOW_HALO,
            "the card's shadow halo must not be cut off"
        );
        assert!(win_right >= a.pill.right());
        // Only the horizontal origin moved: the swapchain size is untouched.
        assert_eq!(
            a.window.right - a.window.left,
            (design::WINDOW_W * a.scale).round() as i32
        );
    }

    #[test]
    fn squeeze_notice_fires_once_per_episode() {
        // Only this test touches the notice static, so the sequence is
        // deterministic even with the suite running in parallel.
        notice_clear();
        assert!(notice_due(), "first squeeze may notify");
        assert!(!notice_due(), "one notice per episode");
        notice_clear();
        assert!(notice_due(), "the next episode may notify again");
    }
}
