//! TaskbarIsland — a Dynamic-Island-style pill anchored to the Windows 11 taskbar.
//!
//! Follows Design Spec v0.1:
//! 1. The pill stays put: never moves or resizes on hover. Only its fill and border change.
//! 2. Flyout above the taskbar: opens as a Windows-style flyout 12 px above the taskbar,
//!    centered over the pill, like Quick Settings.
//! 3. Windows material: white at 9% on taskbar. Flyout is rgb(40,40,40) at 95% with 1 px
//!    border (white 9%) and 8 px radius. Pill radius 10 px.
//! 4. Motion: 180 ms fade, 220 ms scale .94 to 1 and rise 6 px, cubic-bezier(.2, .8, .2, 1).
//!    Close: 110 ms fade and scale back. Auto-close 700 ms after pointer leaves pill + flyout.
//! 5. Sizes per activity:
//!    - Music: compact 184x32, expanded 384x164
//!    - Call: compact 176x32, expanded 384x136
//!    - File shelf: compact 136x32 (204 on drag), expanded 384x176
//!    - Timer: compact 116x32, expanded 320x124
//!    - Volume HUD: compact 224x32, expanded None (dismisses after 1.8 s)
//! 6. Priority order: Call > Timer finishing > Volume HUD > Music > Shelf.

#![windows_subsystem = "windows"]

mod activities;
mod anchor;
mod flyout;
mod log;
mod prototype;
mod render;
mod settings;
mod state;
mod theme;

use std::cell::RefCell;
use std::mem::size_of;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use flyout::{FlyoutWindow, get_flyout, WM_APP_FLYOUT_ACTION};
use render::Renderer;
use state::{Activity, ActivityId, ActivityQueue, Priority};
use windows::core::{w, Error, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, SetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT,
    WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::OleInitialize;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, RegisterHotKey, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::Shell::{ABN_POSCHANGED, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos as WinGetCursorPos, GetMessageW, GetParent, KillTimer, LoadCursorW, MessageBoxW,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetParent, SetTimer,
    SetWindowPos, ShowWindow, TrackPopupMenu, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    EVENT_OBJECT_FOCUS, EVENT_SYSTEM_CAPTURESTART, EVENT_SYSTEM_FOREGROUND, HWND_TOP,
    IDC_ARROW, MB_ICONERROR, MB_OK, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, SW_SHOWNORMAL, TPM_RIGHTBUTTON, WINEVENT_OUTOFCONTEXT,
    WM_APP, WM_COMMAND, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_RBUTTONUP, WM_SETTINGCHANGE,
    WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
};

const PILL_CLASS: PCWSTR = w!("TaskbarIslandPillClass");
const PILL_TITLE: PCWSTR = w!("TaskbarIsland");
const MUTEX_NAME: PCWSTR = w!("TaskbarIsland.SingleInstance");

const WM_APP_RECHECK: u32 = WM_APP + 1;
const APPBAR_MSG: u32 = WM_APP + 2;
const WM_APP_MEDIA: u32 = WM_APP + 3;

// Timers
const AUTOCLOSE_TIMER_ID: usize = 1;
const AUTOCLOSE_MS: u32 = 700;

const TIMER_TICK_ID: usize = 2;
const TIMER_TICK_MS: u32 = 1000;

const VOLUME_HUD_TIMER_ID: usize = 3;
const VOLUME_HUD_MS: u32 = 1800;

const HOVER_ANIM_TIMER_ID: usize = 4;
const HOVER_ANIM_TICK_MS: u32 = 16;

// Hotkeys
const HOTKEY_TIMER: i32 = 1;
const HOTKEY_MUSIC: i32 = 2;
const HOTKEY_CALL: i32 = 3;
const HOTKEY_SHELF: i32 = 4;
const HOTKEY_TIMER_VIEW: i32 = 5;
const HOTKEY_VOLUME: i32 = 6;
const HOTKEY_COLLAPSE: i32 = 7;

static G_HWND: AtomicIsize = AtomicIsize::new(0);
static G_TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);

fn main() {
    std::panic::set_hook(Box::new(|info| {
        log::log(&format!(
            "PANIC: {info} at {}",
            info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default()
        ));
    }));
    if let Err(e) = run() {
        let text = HSTRING::from(format!("TaskbarIsland failed to start:\n{e}"));
        let caption = HSTRING::from("TaskbarIsland");
        let _ = unsafe { MessageBoxW(None, &text, &caption, MB_OK | MB_ICONERROR) };
    }
}

fn run() -> windows::core::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

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

        G_TASKBAR_CREATED_MSG.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );

        let hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            PILL_CLASS,
            PILL_TITLE,
            WS_CHILD,
            0,
            0,
            184,
            32,
            Some(taskbar),
            None,
            Some(hinstance),
            None,
        )?;
        G_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

        // Flyout window
        let _ = FlyoutWindow::init(hwnd);

        let initial_visible = {
            let tb = taskbar;
            let dpi = GetDpiForWindow(hwnd).max(96);
            let placement = anchor::compute(
                tb,
                dpi,
                RECT { left: 0, top: 0, right: 184, bottom: 32 },
                anchor::MUSIC_COMPACT_WIDTH_DIP,
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
        app.apply();

        APP.with(|a| *a.borrow_mut() = Some(app));

        // Media subscription worker
        let (media_tx, media_rx) = mpsc::channel();
        let (toggle_tx, toggle_rx) = mpsc::channel::<()>();
        let media_snapshot = Arc::new(Mutex::new(None));
        let bridge_snapshot = Arc::clone(&media_snapshot);
        let hwnd_value = hwnd.0 as isize;
        std::thread::spawn(move || {
            while let Ok(update) = media_rx.recv() {
                *bridge_snapshot.lock().expect("mutex poisoned") = Some(update);
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

        // OLE Drag-drop shelf
        if let Err(error) = OleInitialize(None) {
            log::log_err("ole.init", &error);
        }
        let shelf_slot: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        match activities::shelf::register_drop_target(hwnd, Arc::clone(&shelf_slot)) {
            Ok(target) => std::mem::forget(target),
            Err(error) => log::log_err("shelf.register", &error),
        }

        APP.with(|a| {
            let mut app = a.borrow_mut();
            let app = app.as_mut().expect("app initialized");
            app.media_snapshot = media_snapshot;
            app.media_toggle = toggle_tx;
            app.shelf_slot = shelf_slot;
        });

        // Appbar & hooks
        let _hook: HWINEVENTHOOK = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
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

        // Hotkeys for testing/demo per design spec:
        // Ctrl+Alt+1: Music
        // Ctrl+Alt+2: Call
        // Ctrl+Alt+3: File shelf
        // Ctrl+Alt+4: Timer
        // Ctrl+Alt+5: Volume HUD
        // Ctrl+Alt+C: Collapse island
        // Ctrl+Alt+T: Timer countdown
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_TIMER, MOD_CONTROL | MOD_ALT, 'T' as u32);
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_MUSIC, MOD_CONTROL | MOD_ALT, '1' as u32);
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_CALL, MOD_CONTROL | MOD_ALT, '2' as u32);
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_SHELF, MOD_CONTROL | MOD_ALT, '3' as u32);
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_TIMER_VIEW, MOD_CONTROL | MOD_ALT, '4' as u32);
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_VOLUME, MOD_CONTROL | MOD_ALT, '5' as u32);
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_COLLAPSE, MOD_CONTROL | MOD_ALT, 'C' as u32);

        // Check command line arguments: if --prototype is specified, show prototype window
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|arg| arg == "--prototype") {
            prototype::show_prototype_window(hwnd);
        }

        // Standard event loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

struct App {
    hwnd: HWND,
    renderer: Option<Renderer>,
    pill: RECT,
    visible: bool,
    theme: theme::Theme,
    hover: bool,
    pressed: bool,
    tracking: bool,
    drop_hover: bool,

    // Arbitration
    queue: ActivityQueue,
    current_activity: ActivityId,

    // Activities
    media_snapshot: Arc<Mutex<Option<activities::media::MediaUpdate>>>,
    media_state: Option<activities::media::MediaUpdate>,
    media_toggle: mpsc::Sender<()>,

    shelf_slot: Arc<Mutex<Vec<String>>>,
    shelf_paths: Vec<String>,
    shelf: activities::shelf::ShelfActivity,
    shelf_view: Option<activities::shelf::ShelfView>,

    timer: activities::timer::TimerActivity,
    timer_view: Option<activities::timer::TimerView>,
    timer_secs: u64,

    call: activities::call::CallActivity,
    volume: activities::volume::VolumeActivity,
}

impl App {
    fn new(hwnd: HWND, visible: bool, theme: theme::Theme) -> windows::core::Result<Self> {
        let rect = window_rect(hwnd).unwrap_or(RECT {
            left: 0,
            top: 0,
            right: 184,
            bottom: 32,
        });
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let scale = dpi.max(96) as f32 / 96.0;
        let size = (
            (rect.right - rect.left).max(1) as u32,
            (rect.bottom - rect.top).max(1) as u32,
        );

        let queue = ActivityQueue::default();

        let renderer = if visible {
            let mut r = unsafe { Renderer::create(hwnd, size, scale, theme)? };
            r.active_id = ActivityId::Idle;
            let _ = unsafe { r.draw() };
            Some(r)
        } else {
            None
        };

        Ok(Self {
            hwnd,
            renderer,
            pill: rect,
            visible,
            theme,
            hover: false,
            pressed: false,
            tracking: false,
            drop_hover: false,
            queue,
            current_activity: ActivityId::Idle,
            media_snapshot: Arc::new(Mutex::new(None)),
            media_state: None,
            media_toggle: mpsc::channel().0,
            shelf_slot: Arc::new(Mutex::new(Vec::new())),
            shelf_paths: Vec::new(),
            shelf: activities::shelf::ShelfActivity::default(),
            shelf_view: None,
            timer: activities::timer::TimerActivity::default(),
            timer_view: None,
            // TBI_TIMER_SECS exists purely so automated verification can run
            // the countdown end-state quickly; the registry setting is the
            // user-facing length (Tweaks → Timer).
            timer_secs: std::env::var("TBI_TIMER_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|secs| (1..=3600).contains(secs))
                .unwrap_or(settings::Settings::current().timer_minutes as u64 * 60),
            call: activities::call::CallActivity::default(),
            volume: activities::volume::VolumeActivity::default(),
        })
    }

    /// Returns the exact compact width (in DIP) for the current state.
    fn compact_width_dip(&self) -> f32 {
        if self.drop_hover {
            return anchor::SHELF_DRAG_WIDTH_DIP; // 204.0 DIP on drag
        }
        match self.current_activity {
            ActivityId::Idle => anchor::IDLE_COMPACT_WIDTH_DIP, // empty material pill
            ActivityId::Media => anchor::MUSIC_COMPACT_WIDTH_DIP, // 184.0 DIP
            ActivityId::Call => anchor::CALL_COMPACT_WIDTH_DIP,   // 176.0 DIP
            ActivityId::Shelf => anchor::SHELF_COMPACT_WIDTH_DIP, // 136.0 DIP
            ActivityId::Timer => anchor::TIMER_COMPACT_WIDTH_DIP, // 116.0 DIP
            ActivityId::Volume => anchor::VOLUME_COMPACT_WIDTH_DIP, // 224.0 DIP
        }
    }

    /// Apply anchoring and sizing. The compact pill NEVER resizes on hover!
    fn apply(&mut self) {
        let current = window_rect(self.hwnd).unwrap_or(self.pill);
        let taskbar = match anchor::taskbar_window() {
            Some(t) => t,
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
        let width_dip = self.compact_width_dip();
        let placement = anchor::compute(taskbar, dpi, current, width_dip);

        let w = (placement.rect.right - placement.rect.left).max(1);
        let h = (placement.rect.bottom - placement.rect.top).max(1);

        if placement.rect != self.pill {
            if set_window_rect(self.hwnd, placement.rect, w, h).is_err() {
                return;
            }
            self.pill = placement.rect;
            self.sync_renderer((w as u32, h as u32), dpi as f32 / 96.0);
        }

        self.set_visible(placement.visible);
    }

    fn sync_renderer(&mut self, size: (u32, u32), scale: f32) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        unsafe {
            let _ = renderer.resize(size, scale);
            let _ = renderer.draw();
        }
    }

    fn set_visible(&mut self, visible: bool) {
        if visible != self.visible {
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
                    if let Ok(mut r) = unsafe { Renderer::create(self.hwnd, size, scale, self.theme) } {
                        Self::push_activity_to(
                            self.current_activity,
                            &self.media_state,
                            &self.call,
                            &self.shelf_view,
                            &self.timer_view,
                            &self.volume,
                            &mut r,
                        );
                        let _ = unsafe { r.draw() };
                        self.renderer = Some(r);
                    }
                }
            } else {
                self.tracking = false;
                self.set_hover(false);
            }
        }
    }

    fn set_hover(&mut self, hover: bool) -> bool {
        self.hover = hover;
        let Some(renderer) = self.renderer.as_mut() else {
            return false;
        };
        if renderer.set_hover(hover) {
            if self.visible {
                let _ = unsafe { renderer.draw() };
                // 200 ms fill/border ease (Design Spec: Hover).
                unsafe {
                    SetTimer(Some(self.hwnd), HOVER_ANIM_TIMER_ID, HOVER_ANIM_TICK_MS, None);
                }
            }
            return true;
        }
        false
    }

    fn set_pressed(&mut self, pressed: bool) -> bool {
        self.pressed = pressed;
        let Some(renderer) = self.renderer.as_mut() else {
            return false;
        };
        if renderer.set_pressed(pressed) {
            if self.visible {
                let _ = unsafe { renderer.draw() };
                unsafe {
                    SetTimer(Some(self.hwnd), HOVER_ANIM_TIMER_ID, HOVER_ANIM_TICK_MS, None);
                }
            }
            return true;
        }
        false
    }

    fn set_drop_hover(&mut self, on: bool) {
        if self.drop_hover == on {
            return;
        }
        self.drop_hover = on;
        if let Some(r) = self.renderer.as_mut() {
            r.set_drop_hover(on);
        }
        self.apply();
        if let Some(r) = self.renderer.as_mut() {
            let _ = unsafe { r.draw() };
        }
    }

    fn push_activity_to(
        current_activity: ActivityId,
        media: &Option<activities::media::MediaUpdate>,
        call: &activities::call::CallActivity,
        shelf: &Option<activities::shelf::ShelfView>,
        timer: &Option<activities::timer::TimerView>,
        volume: &activities::volume::VolumeActivity,
        r: &mut Renderer,
    ) {
        r.active_id = current_activity;
        r.media = media.clone();
        r.call = if call.active { Some(call.state.clone()) } else { None };
        r.shelf = shelf.clone();
        r.timer = timer.clone();
        r.volume = if volume.active { Some(volume.clone()) } else { None };
    }

    fn refresh_content(&mut self) {
        let winner = self.queue.current().unwrap_or(ActivityId::Idle);
        let activity_changed = winner != self.current_activity;
        self.current_activity = winner;

        if let Some(r) = self.renderer.as_mut() {
            Self::push_activity_to(
                self.current_activity,
                &self.media_state,
                &self.call,
                &self.shelf_view,
                &self.timer_view,
                &self.volume,
                r,
            );
        }

        if activity_changed {
            self.apply();
        }

        if self.visible {
            if let Some(r) = self.renderer.as_mut() {
                let _ = unsafe { r.draw() };
            }
        }
    }

    // Toggle or open the flyout window
    fn toggle_flyout(&mut self) {
        let taskbar = match anchor::taskbar_window() {
            Some(t) => t,
            None => return,
        };
        get_flyout().with(|f| {
            if let Ok(mut borrow) = f.try_borrow_mut() {
                if let Some(flyout) = borrow.as_mut() {
                    if flyout.is_open && !flyout.is_closing {
                        flyout.close();
                    } else {
                        // Sync content to flyout
                        flyout.media = self.media_state.clone().unwrap_or_else(activities::media::MediaUpdate::demo);
                        flyout.call = self.call.state.clone();
                        flyout.shelf = self.shelf_view.clone().unwrap_or_else(activities::shelf::ShelfView::demo);
                        flyout.timer = self.timer_view.clone().unwrap_or_else(activities::timer::TimerView::demo);

                        flyout.open_for(self.current_activity, taskbar, self.pill);
                    }
                }
            }
        });
    }

    fn collapse_flyout(&mut self) {
        get_flyout().with(|f| {
            if let Ok(mut borrow) = f.try_borrow_mut() {
                if let Some(flyout) = borrow.as_mut() {
                    flyout.close();
                }
            }
        });
    }

    fn start_autoclose_timer(&self) {
        unsafe {
            SetTimer(Some(self.hwnd), AUTOCLOSE_TIMER_ID, AUTOCLOSE_MS, None);
        }
    }

    fn stop_autoclose_timer(&self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), AUTOCLOSE_TIMER_ID);
        }
    }

    fn on_flyout_action(&mut self, action: usize, param: usize) {
        match action {
            flyout::FLYOUT_ACTION_PLAY_PAUSE => {
                let _ = self.media_toggle.send(());
                if let Some(m) = self.media_state.as_mut() {
                    m.playing = !m.playing;
                }
                self.refresh_content();
            }
            flyout::FLYOUT_ACTION_PREV | flyout::FLYOUT_ACTION_NEXT => {
                // Previous / Next track
                if let Some(m) = self.media_state.as_mut() {
                    m.title = "Deewana Kar Raha Hai".into();
                    m.playing = true;
                }
                self.refresh_content();
            }
            flyout::FLYOUT_ACTION_MUTE => {
                self.call.toggle_mute();
                self.refresh_content();
            }
            flyout::FLYOUT_ACTION_SPEAKER => {
                self.call.toggle_speaker();
                self.refresh_content();
            }
            flyout::FLYOUT_ACTION_END_CALL => {
                self.call.end_call();
                self.queue.update(ActivityId::Call, Priority::Call, false);
                self.refresh_content();
            }
            flyout::FLYOUT_ACTION_CLEAR_SHELF => {
                self.shelf_paths.clear();
                self.shelf.count = 0;
                self.shelf_view = None;
                self.queue.update(ActivityId::Shelf, Priority::Shelf, false);
                self.refresh_content();
            }
            flyout::FLYOUT_ACTION_OPEN_FILE => {
                if let Some(sv) = &self.shelf_view {
                    if let Some(file) = sv.files.get(param) {
                        let wide: Vec<u16> = file.path.encode_utf16().chain(std::iter::once(0)).collect();
                        unsafe {
                            ShellExecuteW(
                                None,
                                w!("open"),
                                PCWSTR::from_raw(wide.as_ptr()),
                                PCWSTR::null(),
                                PCWSTR::null(),
                                SW_SHOWNORMAL,
                            );
                        }
                    }
                }
            }
            flyout::FLYOUT_ACTION_TIMER_PAUSE => {
                self.timer.toggle_pause(Instant::now());
                if self.timer.is_running() {
                    unsafe {
                        SetTimer(Some(self.hwnd), TIMER_TICK_ID, TIMER_TICK_MS, None);
                    }
                } else {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), TIMER_TICK_ID);
                    }
                }
                self.on_timer_tick();
            }
            flyout::FLYOUT_ACTION_TIMER_ADD_MIN => {
                self.timer.add_seconds(60);
                self.on_timer_tick();
            }
            flyout::FLYOUT_ACTION_TIMER_STOP => {
                self.timer.reset();
                self.timer_view = None;
                self.queue.update(ActivityId::Timer, Priority::Idle, false);
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_TICK_ID);
                }
                log::log("timer.cancel (stop)");
                self.refresh_content();
            }
            flyout::FLYOUT_ACTION_MOUSE_ENTER => {
                self.stop_autoclose_timer();
            }
            flyout::FLYOUT_ACTION_MOUSE_LEAVE => {
                self.start_autoclose_timer();
            }
            _ => {}
        }
    }

    fn on_timer_tick(&mut self) {
        let now = Instant::now();
        let ended_before = self.timer.has_ended();
        let view = activities::timer::view(&mut self.timer, now);
        let changed = view != self.timer_view;
        self.timer_view = view;
        if !ended_before && self.timer.has_ended() {
            // Crossed zero on this tick: hold the banner and stop the 1 Hz
            // ticker. A click (or the hotkey) dismisses the banner.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_TICK_ID);
            }
            log::log("timer.ended");
        }
        if self.timer.has_ended() {
            self.queue
                .update(ActivityId::Timer, Priority::TimerEnding, true);
        } else if self.timer.is_running() {
            self.queue.update(ActivityId::Timer, Priority::Idle, true);
        }
        if changed {
            self.refresh_content();
        }
    }

    fn trigger_volume(&mut self, level: u32) {
        self.volume.trigger(level);
        self.queue.update(ActivityId::Volume, Priority::VolumeHud, true);
        unsafe {
            SetTimer(Some(self.hwnd), VOLUME_HUD_TIMER_ID, VOLUME_HUD_MS, None);
        }
        self.refresh_content();
    }

    /// Prototype previews are exclusive: the chosen activity is the only one
    /// active, so its compact state is always visible while previewing.
    fn preview_activity(&mut self, activity: ActivityId) {
        self.queue.update(ActivityId::Media, Priority::Media, false);
        self.queue.update(ActivityId::Call, Priority::Call, false);
        self.queue.update(ActivityId::Shelf, Priority::Shelf, false);
        self.queue.update(ActivityId::Timer, Priority::Idle, false);
        self.queue.update(ActivityId::Volume, Priority::VolumeHud, false);
        match activity {
            ActivityId::Media => {
                self.media_state = Some(activities::media::MediaUpdate::demo());
                self.queue.update(ActivityId::Media, Priority::Media, true);
            }
            ActivityId::Call => {
                self.call = activities::call::CallActivity::new_active();
                self.queue.update(ActivityId::Call, Priority::Call, true);
            }
            ActivityId::Shelf => {
                self.shelf.count = 3;
                self.shelf_view = Some(activities::shelf::ShelfView::demo());
                self.queue.update(ActivityId::Shelf, Priority::Shelf, true);
            }
            ActivityId::Timer => {
                self.timer = activities::timer::TimerActivity::default();
                self.timer.start_countdown(Duration::from_secs(760), Instant::now());
                self.timer_view = Some(activities::timer::TimerView::demo());
                self.queue.update(ActivityId::Timer, Priority::Idle, true);
            }
            ActivityId::Volume => {
                self.trigger_volume(42);
                return;
            }
            ActivityId::Idle => {}
        }
        self.refresh_content();
    }

    /// Applies a Tweaks menu choice: persist it, then rebuild the theme and
    /// redraw both the pill and (if open) the flyout.
    fn apply_tweak(&mut self, id: usize) {
        match id {
            1101 => { settings::set_dword(w!("FillPercent"), 4); }
            1102 => { settings::set_dword(w!("FillPercent"), 6); }
            1103 => { settings::set_dword(w!("FillPercent"), 9); }
            1104 => { settings::set_dword(w!("FillPercent"), 12); }
            1105 => { settings::set_dword(w!("FillPercent"), 15); }
            1106 => { settings::set_dword(w!("FillPercent"), 20); }
            1111 => { settings::set_dword(w!("Radius"), 6); }
            1112 => { settings::set_dword(w!("Radius"), 8); }
            1113 => { settings::set_dword(w!("Radius"), 10); }
            1114 => { settings::set_dword(w!("Radius"), 12); }
            1115 => { settings::set_dword(w!("Radius"), 14); }
            1121 => { settings::set_dword(w!("TimerMinutes"), 1); self.timer_secs = 60; }
            1122 => { settings::set_dword(w!("TimerMinutes"), 5); self.timer_secs = 300; }
            1123 => { settings::set_dword(w!("TimerMinutes"), 10); self.timer_secs = 600; }
            1124 => { settings::set_dword(w!("TimerMinutes"), 25); self.timer_secs = 1500; }
            1130 => { settings::clear_value(w!("Accent")); }
            1131 => { settings::set_dword(w!("Accent"), 0x00FF_CD60); }
            1132 => { settings::set_dword(w!("Accent"), 0x0000_B9FF); }
            1133 => { settings::set_dword(w!("Accent"), 0x0023_11E8); }
            1134 => { settings::set_dword(w!("Accent"), 0x005F_CB6C); }
            1135 => { settings::set_dword(w!("Accent"), 0x00B8_6487); }
            _ => return,
        }
        self.theme = theme::Theme::current();
        if let Some(r) = self.renderer.as_mut() {
            r.set_theme(self.theme);
            if self.visible {
                let _ = unsafe { r.draw() };
            }
        }
        get_flyout().with(|f| {
            if let Ok(mut borrow) = f.try_borrow_mut() {
                if let Some(flyout) = borrow.as_mut() {
                    // set_theme redraws when the flyout is open.
                    flyout.set_theme(self.theme);
                }
            }
        });
    }

    fn show_context_menu(&mut self) {
        let mut pt = POINT::default();
        unsafe {
            let _ = WinGetCursorPos(&mut pt);
            let hmenu = CreatePopupMenu().unwrap();
            let _ = AppendMenuW(hmenu, MF_STRING, 1001, w!("Music (Deewana Kar Raha Hai)"));
            let _ = AppendMenuW(hmenu, MF_STRING, 1002, w!("Call (Priya)"));
            let _ = AppendMenuW(hmenu, MF_STRING, 1003, w!("File Shelf (3 files)"));
            let _ = AppendMenuW(hmenu, MF_STRING, 1004, w!("Timer (Study session)"));
            let _ = AppendMenuW(hmenu, MF_STRING, 1005, w!("Volume HUD (42)"));
            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(hmenu, MF_STRING, 1006, w!("Simulate File Drag"));
            let _ = AppendMenuW(hmenu, MF_STRING, 1007, w!("Show Prototype Panel"));
            let _ = AppendMenuW(hmenu, MF_STRING, 1008, w!("Collapse Island"));
            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());

            // Tweaks: persisted in HKCU\Software\TaskbarIsland and applied live.
            let s = settings::Settings::current();
            let tweaks = CreatePopupMenu().unwrap();
            for (id, pct) in [(1101, 4u32), (1102, 6), (1103, 9), (1104, 12), (1105, 15), (1106, 20)] {
                let flags = if s.fill_percent == pct { MF_STRING | MF_CHECKED } else { MF_STRING };
                let label = HSTRING::from(format!("Fill {pct}%"));
                let _ = AppendMenuW(tweaks, flags, id, &label);
            }
            let _ = AppendMenuW(tweaks, MF_SEPARATOR, 0, PCWSTR::null());
            for (id, radius) in [(1111, 6u32), (1112, 8), (1113, 10), (1114, 12), (1115, 14)] {
                let flags = if s.radius_dip == radius { MF_STRING | MF_CHECKED } else { MF_STRING };
                let label = HSTRING::from(format!("Radius {radius}"));
                let _ = AppendMenuW(tweaks, flags, id, &label);
            }
            let _ = AppendMenuW(tweaks, MF_SEPARATOR, 0, PCWSTR::null());
            for (id, minutes) in [(1121, 1u32), (1122, 5), (1123, 10), (1124, 25)] {
                let flags = if s.timer_minutes == minutes { MF_STRING | MF_CHECKED } else { MF_STRING };
                let label = HSTRING::from(format!("Timer {minutes} min"));
                let _ = AppendMenuW(tweaks, flags, id, &label);
            }
            let _ = AppendMenuW(tweaks, MF_SEPARATOR, 0, PCWSTR::null());
            {
                let flags = if s.accent_abgr.is_none() { MF_STRING | MF_CHECKED } else { MF_STRING };
                let _ = AppendMenuW(tweaks, flags, 1130, w!("Accent: System"));
            }
            for (id, abgr, name) in [
                (1131u32, 0x00FF_CD60u32, "Accent: Blue"),
                (1132, 0x0000_B9FF, "Accent: Amber"),
                (1133, 0x0023_11E8, "Accent: Red"),
                (1134, 0x005F_CB6C, "Accent: Green"),
                (1135, 0x00B8_6487, "Accent: Purple"),
            ] {
                let flags = if s.accent_abgr == Some(abgr) { MF_STRING | MF_CHECKED } else { MF_STRING };
                let label = HSTRING::from(name);
                let _ = AppendMenuW(tweaks, flags, id as usize, &label);
            }
            let _ = AppendMenuW(hmenu, MF_POPUP, tweaks.0 as usize, w!("Tweaks"));

            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(hmenu, MF_STRING, 1009, w!("Exit"));

            let _ = TrackPopupMenu(hmenu, TPM_RIGHTBUTTON, pt.x, pt.y, Some(0), self.hwnd, None);
            let _ = DestroyMenu(hmenu);
        }
    }

    fn on_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            m if m == G_TASKBAR_CREATED_MSG.load(Ordering::Relaxed) => {
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
                if let Some(r) = self.renderer.as_mut() {
                    r.set_theme(self.theme);
                    let _ = unsafe { r.draw() };
                }
                get_flyout().with(|f| {
                    if let Ok(mut borrow) = f.try_borrow_mut() {
                        if let Some(flyout) = borrow.as_mut() {
                            flyout.set_theme(self.theme);
                        }
                    }
                });
                Some(LRESULT(0))
            }
            WM_APP_RECHECK => {
                self.apply();
                Some(LRESULT(0))
            }
            WM_APP_MEDIA => {
                let locked = self.media_snapshot.lock().expect("mutex").clone();
                let media = locked.filter(|update| !update.title.is_empty());
                self.media_state = media;
                self.queue.update(ActivityId::Media, Priority::Media, self.media_state.is_some());
                self.refresh_content();
                Some(LRESULT(0))
            }
            activities::shelf::WM_APP_SHELF => {
                match wparam.0 as u32 {
                    activities::shelf::SHELF_DROP => {
                        let paths = std::mem::take(&mut *self.shelf_slot.lock().expect("mutex"));
                        self.shelf_paths = paths;
                        self.shelf.count = self.shelf_paths.len();
                        self.shelf_view = activities::shelf::view(&self.shelf_paths);
                        self.queue.update(self.shelf.id(), self.shelf.priority(), self.shelf.active());
                        self.set_drop_hover(false);
                        self.refresh_content();
                    }
                    activities::shelf::SHELF_HOVER_ENTER => self.set_drop_hover(true),
                    activities::shelf::SHELF_HOVER_LEAVE => self.set_drop_hover(false),
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                if !self.tracking {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: self.hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = unsafe { TrackMouseEvent(&mut tme) };
                    self.tracking = true;
                }
                self.set_hover(true);
                self.stop_autoclose_timer();
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                self.set_hover(false);
                self.set_pressed(false);
                // Auto-close 700 ms after pointer leaves pill + flyout
                self.start_autoclose_timer();
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => {
                self.set_pressed(true);
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                self.set_pressed(false);
                if self.timer.has_ended() {
                    // Ended banner: click dismisses (spec: "click to dismiss").
                    self.timer.reset();
                    self.timer_view = None;
                    self.queue.update(ActivityId::Timer, Priority::Idle, false);
                    log::log("timer.dismiss");
                    self.refresh_content();
                } else {
                    // Clicking the pill toggles the flyout above taskbar
                    self.toggle_flyout();
                }
                Some(LRESULT(0))
            }
            WM_RBUTTONUP => {
                self.show_context_menu();
                Some(LRESULT(0))
            }
            WM_COMMAND => {
                let id = wparam.0 & 0xFFFF;
                match id {
                    1001 => self.preview_activity(ActivityId::Media),
                    1002 => self.preview_activity(ActivityId::Call),
                    1003 => self.preview_activity(ActivityId::Shelf),
                    1004 => self.preview_activity(ActivityId::Timer),
                    1005 => self.preview_activity(ActivityId::Volume),
                    1101..=1135 => self.apply_tweak(id),
                    1006 => {
                        // Simulate file drag
                        self.set_drop_hover(!self.drop_hover);
                    }
                    1007 => {
                        // Show prototype window
                        prototype::show_prototype_window(self.hwnd);
                    }
                    1008 => {
                        // Collapse
                        self.collapse_flyout();
                    }
                    1009 => {
                        unsafe { PostQuitMessage(0) };
                    }
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_APP_FLYOUT_ACTION => {
                self.on_flyout_action(wparam.0, lparam.0 as usize);
                Some(LRESULT(0))
            }
            prototype::WM_APP_PROTOTYPE_CMD => {
                let cmd = wparam.0;
                match cmd {
                    prototype::CMD_PREVIEW_MUSIC => self.preview_activity(ActivityId::Media),
                    prototype::CMD_PREVIEW_CALL => self.preview_activity(ActivityId::Call),
                    prototype::CMD_PREVIEW_SHELF => self.preview_activity(ActivityId::Shelf),
                    prototype::CMD_PREVIEW_TIMER => self.preview_activity(ActivityId::Timer),
                    prototype::CMD_PREVIEW_VOLUME => self.preview_activity(ActivityId::Volume),
                    prototype::CMD_SIMULATE_DRAG => {
                        self.set_drop_hover(!self.drop_hover);
                    }
                    prototype::CMD_COLLAPSE => {
                        self.collapse_flyout();
                    }
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_HOTKEY => {
                match wparam.0 as i32 {
                    HOTKEY_MUSIC => self.preview_activity(ActivityId::Media),
                    HOTKEY_CALL => self.preview_activity(ActivityId::Call),
                    HOTKEY_SHELF => self.preview_activity(ActivityId::Shelf),
                    HOTKEY_TIMER_VIEW => self.preview_activity(ActivityId::Timer),
                    HOTKEY_VOLUME => self.preview_activity(ActivityId::Volume),
                    HOTKEY_COLLAPSE => {
                        self.collapse_flyout();
                    }
                    HOTKEY_TIMER => {
                        if self.timer.has_ended() {
                            // Ended banner: the hotkey dismisses it.
                            self.timer.reset();
                            self.timer_view = None;
                            self.queue.update(ActivityId::Timer, Priority::Idle, false);
                            log::log("timer.dismiss (hotkey)");
                            self.refresh_content();
                        } else if self.timer.is_running() {
                            unsafe {
                                let _ = KillTimer(Some(self.hwnd), TIMER_TICK_ID);
                            }
                            self.timer.reset();
                            self.timer_view = None;
                            self.queue.update(ActivityId::Timer, Priority::Idle, false);
                            log::log("timer.cancel");
                            self.refresh_content();
                        } else {
                            self.timer.start_countdown(Duration::from_secs(self.timer_secs), Instant::now());
                            unsafe {
                                SetTimer(Some(self.hwnd), TIMER_TICK_ID, TIMER_TICK_MS, None);
                            }
                            log::log(&format!("timer.start: {}s", self.timer_secs));
                            self.on_timer_tick();
                        }
                    }
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == AUTOCLOSE_TIMER_ID => {
                self.stop_autoclose_timer();
                // Check if mouse is currently over pill or flyout
                let mut pt = POINT::default();
                unsafe {
                    let _ = WinGetCursorPos(&mut pt);
                }
                let in_pill = pt.x >= self.pill.left && pt.x <= self.pill.right
                    && pt.y >= self.pill.top && pt.y <= self.pill.bottom;
                // Also check if mouse is inside the flyout window
                let in_flyout = get_flyout().with(|f| {
                    if let Ok(borrow) = f.try_borrow() {
                        if let Some(flyout) = borrow.as_ref() {
                            if flyout.is_open && !flyout.is_closing {
                                let mut fr = RECT::default();
                                return unsafe {
                                    windows::Win32::UI::WindowsAndMessaging::GetWindowRect(
                                        flyout.hwnd,
                                        &mut fr,
                                    )
                                }
                                .is_ok()
                                    && pt.x >= fr.left
                                    && pt.x <= fr.right
                                    && pt.y >= fr.top
                                    && pt.y <= fr.bottom;
                            }
                        }
                    }
                    false
                });
                if !in_pill && !in_flyout {
                    self.collapse_flyout();
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == HOVER_ANIM_TIMER_ID => {
                let mut still = false;
                if let Some(renderer) = self.renderer.as_mut() {
                    let (redraw, animating) = renderer.tick_hover(Instant::now());
                    still = animating;
                    if redraw && self.visible {
                        let _ = unsafe { renderer.draw() };
                    }
                }
                if !still {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), HOVER_ANIM_TIMER_ID);
                    }
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == TIMER_TICK_ID => {
                self.on_timer_tick();
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == VOLUME_HUD_TIMER_ID => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), VOLUME_HUD_TIMER_ID);
                }
                self.volume.dismiss();
                self.queue.update(ActivityId::Volume, Priority::VolumeHud, false);
                self.refresh_content();
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => return DefWindowProcW(hwnd, msg, wparam, lparam),
        WM_NCDESTROY => {
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

    let handled = APP
        .try_with(|a| match a.try_borrow_mut() {
            Ok(mut app) => app.as_mut().and_then(|app| app.on_message(msg, wparam, lparam)),
            Err(_) => None,
        })
        .unwrap_or(None);
    match handled {
        Some(result) => result,
        None => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

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
