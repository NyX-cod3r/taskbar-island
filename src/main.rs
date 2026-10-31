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

mod activities;
mod anchor;
mod log;
mod render;
mod state;
mod theme;

use std::cell::RefCell;
use std::mem::size_of;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use render::Renderer;
use windows::core::{w, Error, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, SetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT,
    WIN32_ERROR, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::ABN_POSCHANGED;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW, MessageBoxW,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetWindowPos,
    GetParent, SetParent, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    EVENT_SYSTEM_FOREGROUND,
    EVENT_OBJECT_FOCUS, EVENT_SYSTEM_CAPTURESTART, IDC_ARROW, MB_ICONERROR, MB_OK, MSG,
    SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, WINEVENT_OUTOFCONTEXT, WM_APP, WM_DISPLAYCHANGE,
    WM_DPICHANGED, WM_NCCREATE, WM_NCDESTROY, WM_SETTINGCHANGE, WNDCLASSEXW, WS_CHILD,
    WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TRANSPARENT,
};

const PILL_CLASS: PCWSTR = w!("TaskbarIslandPillClass");
const PILL_TITLE: PCWSTR = w!("TaskbarIsland");
const MUTEX_NAME: PCWSTR = w!("TaskbarIsland.SingleInstance");

/// Posted by the EVENT_SYSTEM_FOREGROUND hook: re-check fullscreen suppression.
const WM_APP_RECHECK: u32 = WM_APP + 1;
/// Our appbar callback message; `wparam == ABN_POSCHANGED` → taskbar geometry changed.
const APPBAR_MSG: u32 = WM_APP + 2;
const WM_APP_MEDIA: u32 = WM_APP + 3;

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
        let taskbar = match anchor::taskbar_window() {
            Some(taskbar) => taskbar,
            None => return Err(Error::new(HRESULT(0x80004005u32 as i32), "taskbar unavailable")),
        };

        // RegisterWindowMessage gives us the shell's broadcast id (explorer restart).
        G_TASKBAR_CREATED_MSG.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );

        // The window is created hidden (no WS_VISIBLE); App decides when to show.
        let hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE // never steal focus
                | WS_EX_TRANSPARENT // Phase 1: clicks pass through (Phase 2 adds input)
                | WS_EX_NOREDIRECTIONBITMAP, // required for DirectComposition hosting
            PILL_CLASS,
            PILL_TITLE,
            WS_CHILD, // hosted directly inside Shell_TrayWnd
            0,
            0,
            120,
            32,
            Some(taskbar),
            None,
            Some(hinstance),
            None,
        )?;
        G_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

        // Position and suppression are decided before creating the
        // DirectComposition target: suppressed startups remain fully hidden.
        let initial_visible = {
            let tb = taskbar;
            let dpi = GetDpiForWindow(hwnd).max(96);
            let placement = anchor::compute(
                tb,
                dpi,
                RECT {
                    left: 0,
                    top: 0,
                    right: 120,
                    bottom: 32,
                },
            );
            let w = (placement.rect.right - placement.rect.left).max(1);
            let h = (placement.rect.bottom - placement.rect.top).max(1);
            let _ = set_window_rect(hwnd, placement.rect, w, h);
            placement.visible
        };
        if initial_visible {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }

        let mut app = App::new(hwnd, initial_visible, theme::Theme::current())?;
        app.apply(); // position, render, and show/hide

        APP.with(|a| *a.borrow_mut() = Some(app));

        let (media_tx, media_rx) = mpsc::channel();
        let media_snapshot = Arc::new(Mutex::new(None));
        let bridge_snapshot = Arc::clone(&media_snapshot);
        let hwnd_value = hwnd.0 as isize;
        std::thread::spawn(move || {
            while let Ok(update) = media_rx.recv() {
                *bridge_snapshot
                    .lock()
                    .expect("media snapshot mutex poisoned") = Some(update);
                let _ = PostMessageW(
                    Some(HWND(hwnd_value as *mut _)),
                    WM_APP_MEDIA,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        });
        if let Err(error) = activities::media::subscribe(media_tx) {
            log::log_err("media.subscribe", &error);
        }
        APP.with(|a| {
            a.borrow_mut()
                .as_mut()
                .expect("app initialized")
                .media_snapshot = media_snapshot
        });

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
        // A taskbar click can change the topmost ordering without changing
        // the foreground window. Recheck when the taskbar captures the mouse.
        let _capture_hook: HWINEVENTHOOK = SetWinEventHook(
            EVENT_SYSTEM_CAPTURESTART,
            EVENT_SYSTEM_CAPTURESTART,
            None,
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        let _focus_hook: HWINEVENTHOOK = SetWinEventHook(
            EVENT_OBJECT_FOCUS,
            EVENT_OBJECT_FOCUS,
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
    renderer: Option<Renderer>,
    /// Last applied pill rect (physical px); keeps the pill parked in place
    /// while explorer is mid-restart and we can't measure the taskbar.
    pill: RECT,
    visible: bool,
    media_snapshot: Arc<Mutex<Option<activities::media::MediaUpdate>>>,
    theme: theme::Theme,
}

impl App {
    fn new(hwnd: HWND, visible: bool, theme: theme::Theme) -> windows::core::Result<Self> {
        let rect = window_rect(hwnd).unwrap_or(RECT {
            left: 0,
            top: 0,
            right: 120,
            bottom: 32,
        });
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let scale = dpi.max(96) as f32 / 96.0;
        let size = (
            (rect.right - rect.left).max(1) as u32,
            (rect.bottom - rect.top).max(1) as u32,
        );
        // # Safety: hwnd was created with WS_EX_NOREDIRECTIONBITMAP.
        // DirectComposition is initialized only while the host window is
        // visible. This avoids a startup flash when fullscreen suppression is
        // active and avoids creating a DComp target for a hidden HWND.
        let renderer = if visible {
            let mut renderer = unsafe { Renderer::create(hwnd, size, scale, theme)? };
            if let Err(e) = unsafe { renderer.draw() } {
                log::log_err("App::new.draw", &e);
            }
            Some(renderer)
        } else {
            None
        };
        log::log(&format!("App::new: rect={:?} dpi={dpi}", rect));
        Ok(Self {
            hwnd,
            renderer,
            pill: rect,
            visible,
            media_snapshot: Arc::new(Mutex::new(None)),
            theme,
        })
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
        if unsafe { GetParent(self.hwnd) }.ok() != Some(taskbar) {
            unsafe {
                let _ = SetParent(self.hwnd, Some(taskbar));
            }
        }
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
        let placement = anchor::compute(taskbar, dpi, current);
        log::log(&format!(
            "apply: placement={:?} visible={} (taskbar dpi={dpi})",
            placement.rect, placement.visible
        ));

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
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        unsafe {
            let resized = renderer.resize(size, scale);
            if let Err(e) = &resized {
                log::log_err("sync_renderer.resize", e);
            }
            let drawn = resized.and_then(|()| renderer.draw());
            if let Err(e) = &drawn {
                log::log_err("sync_renderer.draw", e);
            }
            if drawn.is_err() {
                log::log("sync_renderer: recreating device chain");
                if let Ok(renderer) = Renderer::create(self.hwnd, size, scale, self.theme) {
                    self.renderer = Some(renderer);
                    if let Err(e) = self
                        .renderer
                        .as_mut()
                        .expect("renderer was just set")
                        .draw()
                    {
                        log::log_err("sync_renderer.recreate.draw", &e);
                    }
                } else {
                    log::log("sync_renderer: recreate FAILED");
                }
            }

        }
    }

    fn set_visible(&mut self, visible: bool) {
        if visible != self.visible {
            // SW_SHOWNOACTIVATE: show without activating (window is NOACTIVATE anyway).
            unsafe {
                let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
            };
            self.visible = visible;
            if visible {
                if self.renderer.is_none() {
                    let rect = window_rect(self.hwnd).unwrap_or(self.pill);
                    let size = (
                        (rect.right - rect.left).max(1) as u32,
                        (rect.bottom - rect.top).max(1) as u32,
                    );
                    let scale = unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32 / 96.0;
                    match unsafe { Renderer::create(self.hwnd, size, scale, self.theme) } {
                        Ok(renderer) => self.renderer = Some(renderer),
                        Err(e) => {
                            log::log_err("set_visible.create", &e);
                            return;
                        }
                    }
                }
                // Present + Commit only now: DXGI drops presents issued while
                // hidden, and DWM needs a fresh frame for a shown target.
                if let Err(e) =
                    unsafe { self.renderer.as_mut().expect("renderer initialized").draw() }
                {
                    log::log_err("set_visible.draw", &e);
                }
            }
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
            WM_SETTINGCHANGE => {
                self.theme = theme::Theme::current();
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.set_theme(self.theme);
                    if let Err(error) = unsafe { renderer.draw() } {
                        log::log_err("theme.draw", &error);
                    }
                }
                Some(LRESULT(0))
            }
            WM_APP_RECHECK => {
                // Foreground window changed → fullscreen state may have changed.
                self.apply();
                Some(LRESULT(0))
            }
            WM_APP_MEDIA => {
                if let Some(update) = self
                    .media_snapshot
                    .lock()
                    .expect("media snapshot mutex poisoned")
                    .as_ref()
                {
                    log::log(&format!("media: {} — {}", update.title, update.artist));
                }
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.set_indicator(render::Indicator::Media);
                    if let Err(error) = unsafe { renderer.draw() } {
                        log::log_err("media.draw", &error);
                    }
                }
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
    let parent = unsafe { GetParent(hwnd) }?;
    let mut origin = POINT {
        x: rect.left,
        y: rect.top,
    };
    unsafe {
        if !ScreenToClient(parent, &mut origin).as_bool() {
            return Err(Error::new(HRESULT(0x80004005u32 as i32), "ScreenToClient failed"));
        }
    }
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            origin.x,
            origin.y,
            w,
            h,
            SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
        )?;
    }

    Ok(())
}
