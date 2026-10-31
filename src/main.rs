//! TaskbarIsland — a Dynamic-Island-style pill anchored to the Windows 11 taskbar.
//!
//! Phase 1: an anchored, empty pill that matches the taskbar's active-item
//! highlight and disappears when a fullscreen app owns the screen.
//! Phase 2: the pill lives directly inside the taskbar (`WS_CHILD` of
//! `Shell_TrayWnd` — no topmost/z-order fight), answers hover, and renders the
//! active GSMTC media session's track title.
//!
//! The window is borderless and non-activating; it never hosts a taskbar
//! button and gets its per-pixel alpha from a DirectComposition swapchain
//! (`WS_EX_NOREDIRECTIONBITMAP`), not from `WS_EX_LAYERED`.

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
use std::time::Instant;

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
use windows::Win32::UI::Controls::WM_MOUSELEAVE; // verified empirically: leave arrives as 0x2A3 (675)
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, LoadCursorW,
    MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW,
    SetWindowPos, SetTimer, GetParent, SetParent, ShowWindow, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, HWND_TOP, EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_FOCUS, EVENT_SYSTEM_CAPTURESTART,
    IDC_ARROW, MB_ICONERROR, MB_OK, MSG, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, WINEVENT_OUTOFCONTEXT, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_SETTINGCHANGE,
    WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
};

const PILL_CLASS: PCWSTR = w!("TaskbarIslandPillClass");
const PILL_TITLE: PCWSTR = w!("TaskbarIsland");
const MUTEX_NAME: PCWSTR = w!("TaskbarIsland.SingleInstance");

/// Posted by the EVENT_SYSTEM_FOREGROUND hook: re-check fullscreen suppression.
const WM_APP_RECHECK: u32 = WM_APP + 1;
/// Our appbar callback message; `wparam == ABN_POSCHANGED` → taskbar geometry changed.
const APPBAR_MSG: u32 = WM_APP + 2;
const WM_APP_MEDIA: u32 = WM_APP + 3;
/// Hover-expansion animation (see AGENTS.md: the only polling allowed is
/// while animating). Timer-driven ~60 fps ticks for the ~180 ms transition.
const EXPAND_TIMER_ID: usize = 1;
const EXPAND_TICK_MS: u32 = 16;
const EXPAND_SECONDS: f32 = 0.18;

static G_HWND: AtomicIsize = AtomicIsize::new(0);
static G_TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);

fn main() {
    // panic = "abort" release profiles print nothing anywhere — hook panics
    // into the TBI log so a crash leaves evidence behind.
    std::panic::set_hook(Box::new(|info| {
        log::log(&format!(
            "PANIC: {info} at {}",
            info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default()
        ));
    }));
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
                anchor::PILL_WIDTH_DIP,
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
        let (toggle_tx, toggle_rx) = mpsc::channel::<()>();
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
        if let Err(error) = activities::media::subscribe(media_tx, toggle_rx) {
            log::log_err("media.subscribe", &error);
        }
        APP.with(|a| {
            let mut app = a.borrow_mut();
            let app = app.as_mut().expect("app initialized");
            app.media_snapshot = media_snapshot;
            app.media_toggle = toggle_tx;
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
    /// Mouse-hover feedback (mirrors the renderer's; they resync on redraws).
    hover: bool,
    /// `TrackMouseEvent` registered exactly once per hover episode.
    tracking: bool,
    /// App-owned media source of truth: the renderer is recreated on device
    /// loss or when re-showing, and is pushed fresh state from here.
    media_state: Option<activities::media::MediaUpdate>,
    /// UI → media-worker play/pause toggle requests.
    media_toggle: mpsc::Sender<()>,
    /// Hover-expansion animation state (compact → expanded pill). Only the
    /// WM_TIMER ticks advance it; there is no polling when idle.
    expand: f32,
    expand_target: f32,
    expanding: bool,
    animate_started: Option<Instant>,
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
            hover: false,
            tracking: false,
            media_state: None,
            expand: 0.0,
            expand_target: 0.0,
            expanding: false,
            animate_started: None,
            // Placeholder replaced right after App::constructs (see main()).
            media_toggle: mpsc::channel().0,
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
        // The taskbar's own composition content bridge
        // (Windows.UI.Composition.DesktopWindowContentBridge) covers the whole
        // tray strip and sits ABOVE us in the child z-order — it eats every
        // mouse message aimed at the pill while its transparent regions still
        // let the pill's frames show through ("renders but no hover").
        // Assert our z above it on every re-apply: explorer re-raises its
        // bridge content over time.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
        // Hover expansion changes the pill's width; the animation ticks own
        // the progress, and every other placement source respects it. All
        // geometry math stays inside anchor.rs.
        let width = anchor::PILL_WIDTH_DIP
            + (anchor::EXPANDED_WIDTH_DIP - anchor::PILL_WIDTH_DIP) * self.expand;
        let placement = anchor::compute(taskbar, dpi, current, width);
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
                        Ok(mut renderer) => {
                            // Don't lose the activity state across a hide
                            // cycle: the fresh renderer starts empty.
                            renderer.set_media(self.media_state.clone());
                            self.renderer = Some(renderer);
                        }
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
            } else {
                // Hovering can't continue in a hidden window; reset so the
                // next show starts the episode cleanly.
                self.tracking = false;
                self.set_hover(false);
            }
        }
    }

    /// Hover-status transition. The renderer dedupes no-op transitions; the
    /// draw only happens for real changes while visible.
    fn set_hover(&mut self, hover: bool) -> bool {
        self.hover = hover;
        let Some(renderer) = self.renderer.as_mut() else {
            return false;
        };
        if renderer.set_hover(hover) {
            log::log(&format!("hover: {hover}"));
            if self.visible {
                if let Err(error) = unsafe { renderer.draw() } {
                    log::log_err("hover.draw", &error);
                }
            }
            return true;
        }
        false
    }

    /// Start (or re-target) the hover-expansion animation. Purely event
    /// triggered: runs a short WM_TIMER-driven tween while active, then kills
    /// the timer — no periodic work while idle.
    fn set_expansion(&mut self, expanded: bool) {
        let target = if expanded { 1.0 } else { 0.0 };
        if self.expanding && (self.expand_target - target).abs() < f32::EPSILON {
            return; // already animating toward this target
        }
        self.expand_target = target;
        if !self.expanding {
            self.animate_started = Some(Instant::now());
            self.expanding = true;
            unsafe {
                SetTimer(
                    Some(self.hwnd),
                    EXPAND_TIMER_ID,
                    EXPAND_TICK_MS,
                    None::<unsafe extern "system" fn(HWND, u32, usize, u32)>,
                );
            }
        }
    }

    /// Advance the expansion tween one tick; resize + redraw if anything
    /// changed. Renders only while the transition runs.
    fn on_anim_tick(&mut self) {
        let Some(started) = self.animate_started else {
            self.stop_expansion();
            return;
        };
        let t = (started.elapsed().as_secs_f32() / EXPAND_SECONDS).min(1.0);
        let eased = t * t * (3.0 - 2.0 * t); // smoothstep
        self.expand = if self.expand_target >= 0.5 {
            eased
        } else {
            1.0 - eased
        };
        if t >= 1.0 {
            self.expand = self.expand_target;
            self.stop_expansion();
        }
        self.apply_expansion();
    }

    fn stop_expansion(&mut self) {
        if self.expanding {
            self.expanding = false;
            unsafe {
                let _ = KillTimer(Some(self.hwnd), EXPAND_TIMER_ID);
            }
        }
    }

    /// Sync the window to the animated width and draw at the eased progress.
    fn apply_expansion(&mut self) {
        let taskbar = match anchor::taskbar_window() {
            Some(taskbar) => taskbar,
            None => return, // explorer restarting; apply() will recover
        };
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
        let width = anchor::PILL_WIDTH_DIP
            + (anchor::EXPANDED_WIDTH_DIP - anchor::PILL_WIDTH_DIP) * self.expand;
        let placement = anchor::compute(taskbar, dpi, self.pill, width);
        let w = (placement.rect.right - placement.rect.left).max(1);
        let h = (placement.rect.bottom - placement.rect.top).max(1);
        let resized = placement.rect != self.pill;
        if resized {
            if set_window_rect(self.hwnd, placement.rect, w, h).is_err() {
                self.stop_expansion();
                return; // window gone; WM_NCDESTROY follows
            }
            self.pill = placement.rect;
        }
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_expand(self.expand);
            if resized {
                if let Err(error) = unsafe { renderer.resize((w as u32, h as u32), dpi as f32 / 96.0) } {
                    log::log_err("anim.resize", &error);
                }
            }
            if self.visible {
                if let Err(error) = unsafe { renderer.draw() } {
                    log::log_err("anim.draw", &error);
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
                let locked = self
                    .media_snapshot
                    .lock()
                    .expect("media snapshot mutex poisoned")
                    .clone();
                // An empty title is the "nothing playing" sentinel from
                // media.rs (only sent on playback-status changes, never polled).
                let media = locked.filter(|update| !update.title.is_empty());
                if let Some(update) = &media {
                    log::log(&format!("media: {} — {}", update.title, update.artist));
                } else if self.media_state.is_some() {
                    log::log("media: stopped");
                }
                self.media_state = media.clone();
                if let Some(renderer) = self.renderer.as_mut() {
                    if renderer.set_media(media) {
                        if let Err(error) = unsafe { renderer.draw() } {
                            log::log_err("media.draw", &error);
                        }
                    }
                }
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                // First move of an episode: ask for the leave notification
                // once, then pin the highlight state. The hover timeout event
                // would lag the visual, so the move itself drives it.
                if !self.tracking {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: self.hwnd,
                        dwHoverTime: 0,
                    };
                    if unsafe { TrackMouseEvent(&mut tme) }.is_ok() {
                        self.tracking = true;
                    }
                }
                self.set_hover(true);
                if self.hover && self.visible {
                    self.set_expansion(true);
                }
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                if self.set_hover(false) {
                    self.set_expansion(false);
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => {
                // Swallow so the taskbar doesn't react underneath the pill.
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                // Toggle the current session's play state; the media worker's
                // PlaybackInfoChanged event republishes the new state to us.
                let _ = self.media_toggle.send(());
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == EXPAND_TIMER_ID => {
                self.on_anim_tick();
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
            // try_borrow: WM_NCDESTROY can itself arrive nested (see the
            // reentrancy note below) while an outer handler holds the APP
            // borrow; clearing the slot is then best-effort and harmless.
            let _ = APP.try_with(|a| {
                if let Ok(mut app) = a.try_borrow_mut() {
                    *app = None;
                }
            });
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }

    // Reentrancy: on_message calls SetWindowPos (apply()/apply_expansion()
    // raise the pill above the taskbar's full-strip content bridge), and
    // SetWindowPos synchronously dispatches WM_WINDOWPOSCHANGING,
    // WM_NCCALCSIZE and WM_WINDOWPOSCHANGED into wnd_proc *inside that call*,
    // while the outer message handler still holds the APP borrow. None of
    // those nested messages read App state (the dispatcher below never
    // handled them), and DefWindowProc processes them exactly as before, so
    // on a borrow conflict we defer to the default proc instead of panicking.
    // Empirically confirmed: nested 0x46 WM_WINDOWPOSCHANGING, 0x83
    // WM_NCCALCSIZE, 0x85 WM_NCPAINT, 0x14 WM_ERASEBKGND, 0x47
    // WM_WINDOWPOSCHANGED (+0x3/0x5 WM_MOVE/WM_SIZE from DSProcW of 0x47) on
    // every width-changing SetWindowPos of the expand animation.
    let handled = APP
        .try_with(|a| match a.try_borrow_mut() {
            Ok(mut app) => app.as_mut().and_then(|app| app.on_message(msg, wparam)),
            // Borrow conflict ⇒ an outer handler that called SetWindowPos is
            // still on the stack; the nested cluster needs no App state and
            // DSProcW handles it exactly as before, so just defer there.
            Err(_) => None,
        })
        .unwrap_or(None);
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
