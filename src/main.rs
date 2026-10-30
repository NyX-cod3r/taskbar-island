//! TaskbarIsland — a Dynamic-Island-style pill anchored to the Windows 11 taskbar.
//!
//! Phase 1: an anchored, empty pill that matches the taskbar's active-item
//! highlight and disappears when a fullscreen app owns the screen. Input
//! handling, theme following, and activities arrive in later phases.
//!
//! The window is borderless, topmost, non-activating and click-through; it
//! never hosts a taskbar button (WS_EX_TOOLWINDOW) and gets its per-pixel
//! alpha from a DirectComposition swapchain (WS_EX_NOREDIRECTIONBITMAP),
//! not from WS_EX_LAYERED.

#![windows_subsystem = "windows"]

mod anchor;
mod render;

use std::cell::RefCell;
use std::mem::size_of;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use render::Renderer;
use windows::Win32::Foundation::{
    GetLastError, SetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, RECT, WIN32_ERROR,
    WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EVENT_SYSTEM_FOREGROUND, GetMessageW,
    HWND_TOPMOST, IDC_ARROW, LoadCursorW, MB_ICONERROR, MB_OK, MessageBoxW, MSG, PostMessageW,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetWindowPos, ShowWindow, SW_HIDE,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOWNOACTIVATE, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    WINEVENT_OUTOFCONTEXT, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_NCDESTROY, WM_NCCREATE,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOPMOST, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Shell::ABN_POSCHANGED;
use windows::core::{HSTRING, PCWSTR, w};

const PILL_CLASS: PCWSTR = w!("TaskbarIslandPillClass");
const PILL_TITLE: PCWSTR = w!("TaskbarIsland");
const MUTEX_NAME: PCWSTR = w!("TaskbarIsland.SingleInstance");

/// Posted by the EVENT_SYSTEM_FOREGROUND hook: re-check fullscreen suppression.
const WM_APP_RECHECK: u32 = WM_APP + 1;
/// Our appbar callback message; `wparam == ABN_POSCHANGED` → taskbar geometry changed.
const APPBAR_MSG: u32 = WM_APP + 2;

static G_HWND: AtomicIsize = AtomicIsize::new(0);
static G_TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);

fn main() {
    if let Err(e) = run() {
        // windows-subsystem binary: surface fatal startup errors in a message box.
        let text = HSTRING::from(format!("TaskbarIsland failed to start:\n{e}"));
        let caption = HSTRING::from("TaskbarIsland");
        let _ = unsafe { MessageBoxW(None, &text, &caption, MB_OK | MB_ICONERROR) };
    }
}

fn run() -> windows::core::Result<()> {
    unsafe {
        // Per-monitor v2 DPI before any window exists.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Single instance: the named mutex lives for the whole process (never
        // closed here), so a second launch finds it and exits immediately.
        // SetLastError first: CreateMutexW only sets ERROR_ALREADY_EXISTS when
        // the name was taken, and a stale value must not false-positive.
        SetLastError(WIN32_ERROR(0));
        let _single_instance = CreateMutexW(None, false, MUTEX_NAME)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }

        let hinstance = GetModuleHandleW(None)?.into();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: PILL_CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);

        // RegisterWindowMessage gives us the shell's broadcast id (explorer restart).
        G_TASKBAR_CREATED_MSG.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);

        // The window is created hidden (no WS_VISIBLE); App decides when to show.
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW // no taskbar button, no Alt-Tab entry
                | WS_EX_TOPMOST
                | WS_EX_NOACTIVATE // never steal focus
                | WS_EX_TRANSPARENT // Phase 1: clicks pass through (Phase 2 adds input)
                | WS_EX_NOREDIRECTIONBITMAP, // required for DirectComposition hosting
            PILL_CLASS,
            PILL_TITLE,
            WS_POPUP, // borderless
            0,
            0,
            120,
            32,
            None,
            None,
            Some(hinstance),
            None,
        )?;
        G_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

        let mut app = App::new(hwnd)?;
        app.apply(); // position, render, and show/hide

        APP.with(|a| *a.borrow_mut() = Some(app));

        // Event sources (no polling): foreground changes → fullscreen recheck,
        // appbar notifications → taskbar geometry changed.
        let _hook: HWINEVENTHOOK = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        anchor::register_appbar(hwnd, APPBAR_MSG);

        // Standard blocking message loop → ~0% CPU while idle. Also dispatches
        // out-of-context WinEvents on this thread.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

thread_local! {
    /// Everything lives on the UI thread: wnd_proc and out-of-context WinEvents
    /// are both dispatched from `run`'s message loop.
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Owned state for the pill window.
struct App {
    hwnd: HWND,
    renderer: Renderer,
    /// Last applied pill rect (physical px); keeps the pill parked in place
    /// while explorer is mid-restart and we can't measure the taskbar.
    pill: RECT,
    visible: bool,
}

impl App {
    fn new(hwnd: HWND) -> windows::core::Result<Self> {
        let rect = window_rect(hwnd).unwrap_or(RECT { left: 0, top: 0, right: 120, bottom: 32 });
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let scale = dpi.max(96) as f32 / 96.0;
        let size = ((rect.right - rect.left).max(1) as u32, (rect.bottom - rect.top).max(1) as u32);
        // # Safety: hwnd was created with WS_EX_NOREDIRECTIONBITMAP.
        let mut renderer = unsafe { Renderer::create(hwnd, size, scale)? };
        // Draw once up front: `apply` only redraws when the rect changes, and a
        // first paint must not depend on that happening.
        let _ = unsafe { renderer.draw() };
        Ok(Self { hwnd, renderer, pill: rect, visible: false })
    }

    /// Re-derive the pill position/visibility and sync the window to it.
    fn apply(&mut self) {
        let current = window_rect(self.hwnd).unwrap_or(self.pill);
        let taskbar = match anchor::taskbar_window() {
            Some(t) => t,
            // Explorer is restarting; keep the last known position, stay hidden.
            None => {
                self.set_visible(false);
                return;
            }
        };
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
        let placement = anchor::compute(taskbar, dpi, current);

        let w = (placement.rect.right - placement.rect.left).max(1);
        let h = (placement.rect.bottom - placement.rect.top).max(1);
        if placement.rect != self.pill {
            // SWP_NOACTIVATE: never yank focus from whatever the user is doing.
            if set_window_rect(self.hwnd, placement.rect, w, h).is_err() {
                return; // window is gone; WM_NCDESTROY will follow
            }
            self.pill = placement.rect;
            self.sync_renderer((w as u32, h as u32), dpi as f32 / 96.0);
        }
        self.set_visible(placement.visible);
    }

    /// Resize + redraw; recreate the device chain if the GPU device was lost
    /// (driver reset, RDP session switch, hybrid-GPU flip).
    fn sync_renderer(&mut self, size: (u32, u32), scale: f32) {
        unsafe {
            let resized = self.renderer.resize(size, scale);
            let drawn = resized.and_then(|()| self.renderer.draw());
            if drawn.is_err() {
                if let Ok(renderer) = Renderer::create(self.hwnd, size, scale) {
                    self.renderer = renderer;
                    let _ = self.renderer.draw();
                }
            }
        }
    }

    fn set_visible(&mut self, visible: bool) {
        if visible != self.visible {
            // SW_SHOWNOACTIVATE: show without activating (window is NOACTIVATE anyway).
            unsafe { let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE }); };
            self.visible = visible;
        }
    }

    fn on_message(&mut self, msg: u32, wparam: WPARAM) -> Option<LRESULT> {
        match msg {
            m if m == G_TASKBAR_CREATED_MSG.load(Ordering::Relaxed) => {
                // Explorer restarted: re-register the appbar and re-measure.
                anchor::register_appbar(self.hwnd, APPBAR_MSG);
                self.apply();
                Some(LRESULT(0))
            }
            m if m == APPBAR_MSG && wparam.0 as u32 == ABN_POSCHANGED => {
                self.apply();
                Some(LRESULT(0))
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                self.apply();
                Some(LRESULT(0))
            }
            WM_APP_RECHECK => {
                // Foreground window changed → fullscreen state may have changed.
                self.apply();
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

/// # Safety
/// `hwnd` must be our pill window. Only called by the OS on the UI thread.
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => return DefWindowProcW(hwnd, msg, wparam, lparam),
        WM_NCDESTROY => {
            APP.with(|a| *a.borrow_mut() = None);
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }

    // Reentrancy note: on_message only calls SetWindowPos/ShowWindow, whose
    // resulting messages (WM_WINDOWPOS*, WM_PAINT) fall through to DefWindowProc
    // and never re-borrow APP, so a RefCell borrow here cannot panic.
    let handled = APP.with(|a| match a.borrow_mut().as_mut() {
        Some(app) => app.on_message(msg, wparam),
        None => None,
    });
    match handled {
        Some(result) => result,
        None => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// WinEvent out-of-context hook: called on our message-loop thread whenever the
/// foreground window changes. We only post to ourselves — cheapest possible path.
///
/// # Safety
/// Called by user32 on the hooking thread.
unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    let me = HWND(G_HWND.load(Ordering::Relaxed) as *mut _);
    if !me.0.is_null() {
        let _ = PostMessageW(Some(me), WM_APP_RECHECK, WPARAM(0), LPARAM(0));
    }
}

fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect) }.ok()?;
    Some(rect)
}

fn set_window_rect(hwnd: HWND, rect: RECT, w: i32, h: i32) -> windows::core::Result<()> {
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST), // stay pinned above the taskbar
            rect.left,
            rect.top,
            w,
            h,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )?;
    }
    Ok(())
}
