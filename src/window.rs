//! Native island window lifecycle and DirectComposition surface.

use std::mem::{size_of, ManuallyDrop};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{w, Error, Interface, Result};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1DeviceContext, ID2D1Image, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGISurface, IDXGISwapChain1, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::System::Ole::{IDropTarget, OleInitialize};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VK_ESCAPE, VK_T,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW, PostMessageW,
    PostQuitMessage, RegisterClassExW, SetTimer, SetWindowPos, ShowWindow, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, HTTRANSPARENT, HWND_TOPMOST, IDC_ARROW, MSG, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_SHOWNOACTIVATE, WM_DESTROY, WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONUP,
    WM_NCHITTEST, WM_PAINT, WM_SETTINGCHANGE, WM_THEMECHANGED, WM_TIMER, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::activities::call::{self, AudioCommand, CallRuntime};
use crate::activities::media::{self, MediaCommand, MediaSnapshot};
use crate::activities::shelf::{
    self, ShelfStore, SHELF_DROP, SHELF_HOVER_ENTER, SHELF_HOVER_LEAVE, WM_APP_SHELF,
};
use crate::activities::timer::Countdown;
use crate::activities::volume::{self, VolumeHud, WM_APP_VOLUME};
use crate::design::{self, ActivityKind, RectD};
use crate::gpu;
use crate::render::{HitMap, Painter, RenderInput};
use crate::theme::Theme;
use crate::view::{
    ActivityView, Body, Element, FlyoutView, IconId, Leading, MusicBody, PillKind, PillView,
    Subtitle, TileView, Trailing, ViewModel,
};

const CLASS_NAME: windows::core::PCWSTR = w!("TaskbarIsland");
const TIMER_HOTKEY_ID: i32 = 7;
const TIMER_FALLBACK_HOTKEY_ID: i32 = 8;

struct Surface {
    swapchain: IDXGISwapChain1,
    context: ID2D1DeviceContext,
    pill: RectD,
    hit_rect: RectD,
    flyout: RectD,
    flyout_screen: RectD,
    flyout_open: bool,
    hit_map: HitMap,
    painter: Painter,
    view: ViewModel,
    theme: Theme,
    volume: VolumeHud,
    media_rx: Receiver<MediaSnapshot>,
    media_tx: Sender<MediaCommand>,
    media_snapshot: Option<MediaSnapshot>,
    call_runtime: Arc<Mutex<CallRuntime>>,
    audio_tx: Sender<AudioCommand>,
    shelf_store: ShelfStore,
    shelf_slot: Arc<Mutex<Vec<String>>>,
    _shelf_target: IDropTarget,
    shelf_hover: bool,
    timer: Countdown,
}

thread_local! {
    static SURFACE: std::cell::RefCell<Option<Surface>> = const { std::cell::RefCell::new(None) };
}

pub fn register(instance: HINSTANCE) -> Result<()> {
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW)? };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // SAFETY: class is fully initialized and valid for this call.
    unsafe { RegisterClassExW(&class) };
    Ok(())
}

pub fn create(instance: HINSTANCE, rect: RECT, pill: RectD, flyout: RectD) -> Result<HWND> {
    // SAFETY: the class was registered above and all handles belong to this process.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            CLASS_NAME,
            w!("Taskbar Island"),
            WS_POPUP,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    // SAFETY: hwnd was created above; placement does not activate it.
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )?;
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    let (media_tx, media_command_rx) = mpsc::channel();
    let (media_snapshot_tx, media_rx) = mpsc::channel();
    media::subscribe(media_snapshot_tx, media_command_rx, media_tx.clone())?;
    let call_runtime = call::runtime();
    call::subscribe(
        hwnd,
        crate::settings::Settings::load().call_app_allowlist,
        call_runtime.clone(),
    )?;
    let audio_tx = call::subscribe_audio(hwnd, call_runtime.clone())?;
    // SAFETY: the UI thread owns the island window and remains initialized
    // until the window is destroyed.
    if unsafe { OleInitialize(None).is_err() } {
        return Err(Error::from_hresult(windows::Win32::Foundation::E_FAIL));
    }
    let shelf_slot = Arc::new(Mutex::new(Vec::new()));
    // SAFETY: OLE is initialized on this UI thread and the target is retained
    // by Surface for the window lifetime.
    let shelf_target = unsafe { shelf::register_drop_target(hwnd, shelf_slot.clone())? };
    SURFACE.with(|surface| {
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        let local_pill = RectD::new(
            pill.x - rect.left as f32,
            pill.y - rect.top as f32,
            pill.w,
            pill.h,
        );
        let local_flyout = RectD::new(
            flyout.x - rect.left as f32,
            flyout.y - rect.top as f32,
            flyout.w,
            flyout.h,
        );
        crate::log::log(&format!(
            "window.geometry: host={}x{} rect=({},{}), pill=({:.1},{:.1},{:.1},{:.1}) local=({:.1},{:.1})",
            width,
            height,
            rect.left,
            rect.top,
            pill.x,
            pill.y,
            pill.w,
            pill.h,
            local_pill.x,
            local_pill.y
        ));
        *surface.borrow_mut() = Some(Surface::new(
            hwnd,
            width,
            height,
            local_pill,
            pill,
            local_flyout,
            flyout,
            media_rx,
            media_tx,
            call_runtime,
            audio_tx,
            shelf_slot,
            shelf_target,
        )?);
        if let Some(surface) = surface.borrow_mut().as_mut() {
            if !surface.shelf_store.is_empty() {
                surface.apply_shelf();
            }
            surface.draw()?;
            surface.update_window_region(hwnd);
            surface.draw_visible_fallback(hwnd);
        }
        // Reassert the composition window after its first frame. Explorer can
        // otherwise place a newly-created non-activating popup below the
        // taskbar during startup.
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
        }
        Ok::<(), windows::core::Error>(())
    })?;
    volume::subscribe(hwnd)?;
    unsafe {
        SetTimer(Some(hwnd), 1, 100, None);
        if RegisterHotKey(
            Some(hwnd),
            TIMER_HOTKEY_ID,
            MOD_ALT | MOD_CONTROL,
            VK_T.0 as u32,
        )
        .is_err()
        {
            if RegisterHotKey(
                Some(hwnd),
                TIMER_FALLBACK_HOTKEY_ID,
                MOD_ALT | MOD_CONTROL | MOD_SHIFT,
                VK_T.0 as u32,
            )
            .is_err()
            {
                crate::log::log("window.timer: unable to register Ctrl+Alt+T or Ctrl+Alt+Shift+T");
            } else {
                crate::log::log(
                    "window.timer: Ctrl+Alt+T unavailable; using Ctrl+Alt+Shift+T fallback",
                );
            }
        }
    }
    Ok(hwnd)
}

pub fn message_loop() -> Result<()> {
    let mut message = MSG::default();
    loop {
        // SAFETY: message is a valid writable structure for GetMessageW.
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 == -1 {
            return Err(windows::core::Error::from_thread());
        }
        if result.0 == 0 {
            return Ok(());
        }
        // SAFETY: message was populated by GetMessageW.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

extern "system" fn wnd_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_PAINT => {
            if let Err(error) =
                SURFACE.with(|surface| surface.borrow_mut().as_mut().map(Surface::draw).transpose())
            {
                crate::log::log_err("window.paint", &error);
            }
            SURFACE.with(|surface| {
                if let Some(surface) = surface.borrow().as_ref() {
                    surface.draw_visible_fallback(hwnd);
                }
            });
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let x = (lparam.0 as i16) as f32;
            let y = ((lparam.0 >> 16) as i16) as f32;
            let hit = SURFACE.with(|surface| {
                surface
                    .borrow()
                    .as_ref()
                    .map(|surface| {
                        surface.hit_rect.contains(x, y)
                            || (surface.flyout_open && surface.flyout_screen.contains(x, y))
                    })
                    .unwrap_or(false)
            });
            if hit {
                // Keep the pill available for future interaction without activating the window.
                LRESULT(1)
            } else {
                LRESULT(HTTRANSPARENT as isize)
            }
        }
        WM_LBUTTONUP => {
            SURFACE.with(|surface| {
                if let Some(surface) = surface.borrow_mut().as_mut() {
                    let x = (lparam.0 as i16) as f32;
                    let y = ((lparam.0 >> 16) as i16) as f32;
                    if surface.flyout_open {
                        if let Some(element) = surface.hit_map.hit(x, y) {
                            let command = match element {
                                Element::MusicPlay => Some(MediaCommand::TogglePlayPause),
                                Element::MusicPrev => Some(MediaCommand::Previous),
                                Element::MusicNext => Some(MediaCommand::Next),
                                Element::MusicShuffle => Some(MediaCommand::ToggleShuffle),
                                Element::MusicRepeat => Some(MediaCommand::CycleRepeat),
                                Element::CallMic => {
                                    let _ = surface.audio_tx.send(AudioCommand::ToggleMic);
                                    None
                                }
                                Element::CallOutput => {
                                    let _ = surface.audio_tx.send(AudioCommand::ToggleOutput);
                                    None
                                }
                                Element::TimerToggle => {
                                    surface.timer.toggle_pause(Instant::now());
                                    surface.apply_timer(Instant::now());
                                    None
                                }
                                Element::TimerAdd => {
                                    surface.timer.add_minute();
                                    surface.apply_timer(Instant::now());
                                    None
                                }
                                Element::TimerStop => {
                                    surface.timer.stop();
                                    surface.restore_non_timer();
                                    None
                                }
                                Element::ClearAll => {
                                    surface.shelf_store.clear();
                                    surface.apply_shelf();
                                    None
                                }
                                Element::ShelfTile(index) => {
                                    if let Some(path) = surface.shelf_store.paths().get(index) {
                                        if !shelf::open(path) {
                                            crate::log::log(&format!(
                                                "shelf.open failed: {}",
                                                path
                                            ));
                                        }
                                    }
                                    None
                                }
                                _ => None,
                            };
                            if let Some(command) = command {
                                let _ = surface.media_tx.send(command);
                                return;
                            }
                        }
                    }
                    if surface.pill.contains(x, y) {
                        surface.flyout_open = !surface.flyout_open;
                        unsafe {
                            let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_APP_VOLUME => {
            SURFACE.with(|surface| {
                if let Some(surface) = surface.borrow_mut().as_mut() {
                    let (level, muted) = volume::decode(wparam.0);
                    surface
                        .volume
                        .trigger(level, muted, std::time::Instant::now());
                    surface.view.overlay_pill = Some(surface.volume_pill());
                    if let Err(error) = surface.draw() {
                        crate::log::log_err("window.volume.paint", &error);
                    } else {
                        crate::log::log("window.volume.paint: presented");
                    }
                }
            });
            LRESULT(0)
        }
        call::WM_APP_CALL | call::WM_APP_CALL_AUDIO => {
            SURFACE.with(|surface| {
                if let Some(surface) = surface.borrow_mut().as_mut() {
                    surface.apply_call();
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_APP_SHELF => {
            SURFACE.with(|surface| {
                if let Some(surface) = surface.borrow_mut().as_mut() {
                    match wparam.0 as u32 {
                        SHELF_DROP => {
                            let paths = surface
                                .shelf_slot
                                .lock()
                                .map(|mut slot| std::mem::take(&mut *slot))
                                .unwrap_or_default();
                            if !paths.is_empty() {
                                surface.shelf_store.add(paths);
                                surface.shelf_hover = false;
                                surface.view.overlay_pill = None;
                                surface.apply_shelf();
                            }
                        }
                        SHELF_HOVER_ENTER => {
                            surface.shelf_hover = true;
                            surface.view.overlay_pill = Some(surface.shelf_drag_pill());
                        }
                        SHELF_HOVER_LEAVE => {
                            surface.shelf_hover = false;
                            surface.view.overlay_pill = None;
                        }
                        _ => {}
                    }
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_HOTKEY if matches!(wparam.0 as i32, TIMER_HOTKEY_ID | TIMER_FALLBACK_HOTKEY_ID) => {
            SURFACE.with(|surface| {
                if let Some(surface) = surface.borrow_mut().as_mut() {
                    surface.toggle_timer();
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u32 == VK_ESCAPE.0 as u32 => {
            SURFACE.with(|surface| {
                if let Some(surface) = surface.borrow_mut().as_mut() {
                    if surface.flyout_open {
                        surface.flyout_open = false;
                        unsafe {
                            let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER => {
            SURFACE.with(|surface| {
                if let Some(surface) = surface.borrow_mut().as_mut() {
                    let mut changed = false;
                    while let Ok(snapshot) = surface.media_rx.try_recv() {
                        surface.apply_media(snapshot);
                        changed = true;
                    }
                    let now = std::time::Instant::now();
                    if surface.timer.running() {
                        if surface.timer.tick(now) {
                            surface.apply_timer(now);
                            changed = true;
                        } else if surface.view.main.as_ref().map(|view| view.kind)
                            == Some(ActivityKind::Timer)
                        {
                            surface.apply_timer(now);
                            changed = true;
                        }
                    }
                    if surface.shelf_store.prune()
                        && surface.view.main.as_ref().map(|view| view.kind)
                            == Some(ActivityKind::Shelf)
                    {
                        surface.apply_shelf();
                        changed = true;
                    }
                    if surface.volume.expired(now) {
                        surface.volume.dismiss();
                        surface.view.overlay_pill = None;
                        changed = true;
                    }
                    if changed {
                        unsafe {
                            let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            SURFACE.with(|surface| {
                if let Some(surface) = surface.borrow_mut().as_mut() {
                    if surface.refresh_theme() {
                        unsafe {
                            let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: this window owns the registration.
            unsafe {
                UnregisterHotKey(Some(hwnd), TIMER_HOTKEY_ID);
                UnregisterHotKey(Some(hwnd), TIMER_FALLBACK_HOTKEY_ID);
            }
            // SAFETY: the drop target belongs to this UI thread and window.
            unsafe {
                shelf::revoke_drop_target(hwnd);
                windows::Win32::System::Ole::OleUninitialize();
            }
            // SAFETY: this window owns the process message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => {
            // SAFETY: unhandled messages are delegated to the system procedure.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

impl Surface {
    fn refresh_theme(&mut self) -> bool {
        let next = Theme::current();
        if next == self.theme {
            return false;
        }
        self.theme = next;
        true
    }

    fn new(
        hwnd: HWND,
        width: u32,
        height: u32,
        pill: RectD,
        hit_rect: RectD,
        flyout: RectD,
        flyout_screen: RectD,
        media_rx: Receiver<MediaSnapshot>,
        media_tx: Sender<MediaCommand>,
        call_runtime: Arc<Mutex<CallRuntime>>,
        audio_tx: Sender<AudioCommand>,
        shelf_slot: Arc<Mutex<Vec<String>>>,
        shelf_target: IDropTarget,
    ) -> Result<Self> {
        let stack = gpu::shared()?;
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width,
            Height: height,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        // SAFETY: COM interfaces and the swap-chain descriptor are valid.
        let (swapchain, context) = unsafe {
            let swapchain = stack
                .dxgi_factory
                .CreateSwapChainForHwnd(&stack.d3d, hwnd, &desc, None, None)?;
            let context = stack
                .d2d
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            (swapchain, context)
        };
        Ok(Self {
            swapchain,
            context,
            pill,
            hit_rect,
            flyout,
            flyout_screen,
            flyout_open: false,
            hit_map: HitMap::default(),
            painter: Painter::new(),
            view: ViewModel {
                main: Some(ViewModel::empty_shelf_view(Theme::current().shelf_color())),
                second: None,
                overlay_pill: None,
            },
            theme: Theme::current(),
            volume: VolumeHud::default(),
            media_rx,
            media_tx,
            media_snapshot: None,
            call_runtime,
            audio_tx,
            shelf_store: ShelfStore::load(),
            shelf_slot,
            _shelf_target: shelf_target,
            shelf_hover: false,
            timer: Countdown::default(),
        })
    }

    fn apply_shelf(&mut self) {
        if self
            .call_runtime
            .lock()
            .map(|runtime| runtime.detect.is_some())
            .unwrap_or(false)
            || self.media_snapshot.is_some()
            || self.timer.active()
        {
            return;
        }
        let files = crate::view::file_tiles(self.shelf_store.paths());
        let count = self.shelf_store.len();
        let accent = self.theme.shelf_color();
        self.view.main = Some(ActivityView {
            kind: ActivityKind::Shelf,
            accent,
            pill: PillView {
                icon: IconId::Folder,
                tile: TileView::colored(accent),
                label: "Shelf".into(),
                value: if count == 0 {
                    "Empty".into()
                } else {
                    format!("{count} file{}", if count == 1 { "" } else { "s" })
                },
                progress: None,
                kind: PillKind::Normal,
            },
            flyout: FlyoutView {
                leading: Leading::Glyph {
                    icon: IconId::Folder,
                    bg: accent,
                    fg: design::DARK_GLYPH,
                },
                title: "File shelf".into(),
                subtitle: Subtitle {
                    prefix: format!("{count} parked"),
                    accent: None,
                },
                trailing: Trailing::None,
                body: Body::Shelf(crate::view::ShelfBody { files }),
            },
        });
    }

    fn shelf_drag_pill(&self) -> PillView {
        PillView {
            icon: IconId::Folder,
            tile: TileView::colored(self.theme.shelf_color()),
            label: "Drop to shelve".into(),
            value: String::new(),
            progress: None,
            kind: PillKind::DragOver,
        }
    }

    fn toggle_timer(&mut self) {
        let now = Instant::now();
        if self.timer.active() {
            if self.timer.finished {
                self.timer.stop();
                self.restore_non_timer();
                crate::log::log("timer.dismiss (hotkey)");
            } else {
                self.timer.toggle_pause(now);
                self.apply_timer(now);
            }
            return;
        }
        let seconds = std::env::var("TBI_TIMER_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| (1..=3600).contains(value))
            .unwrap_or(5 * 60);
        self.timer = Countdown::start("Timer", Duration::from_secs(seconds), now);
        self.apply_timer(now);
        crate::log::log(&format!("timer.start: {seconds}s"));
    }

    fn restore_non_timer(&mut self) {
        if let Some(media) = self.media_snapshot.clone() {
            self.apply_media(media);
        } else if self.view.main.as_ref().map(|view| view.kind) == Some(ActivityKind::Timer) {
            self.view.main = None;
        }
    }

    fn apply_timer(&mut self, now: Instant) {
        if !self.timer.active() {
            return;
        }
        if !self.timer.finished
            && self
                .call_runtime
                .lock()
                .map(|runtime| runtime.detect.is_some())
                .unwrap_or(false)
        {
            return;
        }
        if !self.timer.finished
            && self.media_snapshot.is_some()
            && self.view.main.as_ref().map(|view| view.kind) == Some(ActivityKind::Music)
        {
            return;
        }
        let accent = design::TIMER_AMBER;
        let remaining = self.timer.remaining(now).as_secs_f64();
        let elapsed = self.timer.elapsed(now).as_secs_f64();
        self.view.main = Some(ActivityView {
            kind: ActivityKind::Timer,
            accent,
            pill: PillView {
                icon: IconId::Clock,
                tile: TileView::colored(accent),
                label: self.timer.name.clone(),
                value: if self.timer.finished {
                    "Done".into()
                } else {
                    crate::view::fmt_clock(remaining)
                },
                progress: Some(self.timer.remaining_fraction(now)),
                kind: if self.timer.finished {
                    PillKind::Finished
                } else {
                    PillKind::Normal
                },
            },
            flyout: FlyoutView {
                leading: Leading::Ring {
                    fraction: self.timer.fraction(now),
                },
                title: self.timer.name.clone(),
                subtitle: Subtitle {
                    prefix: if self.timer.finished {
                        "Timer done".into()
                    } else if self.timer.running() {
                        "Running".into()
                    } else {
                        "Paused".into()
                    },
                    accent: None,
                },
                trailing: Trailing::TimerTime(crate::view::fmt_clock(remaining)),
                body: Body::Timer(crate::view::TimerBody {
                    elapsed: crate::view::fmt_clock(elapsed),
                    total: crate::view::fmt_clock(self.timer.total.as_secs_f64()),
                    fraction: self.timer.fraction(now),
                    paused: !self.timer.running(),
                    finished: self.timer.finished,
                }),
            },
        });
    }

    fn apply_media(&mut self, snapshot: MediaSnapshot) {
        self.media_snapshot = Some(snapshot.clone());
        if self
            .call_runtime
            .lock()
            .map(|runtime| runtime.detect.is_some())
            .unwrap_or(false)
        {
            return;
        }
        let position = snapshot.effective_position(std::time::Instant::now());
        let fraction = snapshot.fraction(std::time::Instant::now());
        let accent = snapshot.accent;
        let controls = snapshot.controls;
        let pill = PillView {
            icon: IconId::Music,
            tile: TileView::colored(accent),
            label: snapshot.title.clone(),
            value: snapshot.artist.clone(),
            progress: Some(fraction),
            kind: PillKind::Normal,
        };
        let flyout = FlyoutView {
            leading: Leading::Art {
                art: snapshot.art.clone(),
                accent,
            },
            title: snapshot.title.clone(),
            subtitle: Subtitle {
                prefix: snapshot.artist.clone(),
                accent: Some(snapshot.source_app.clone()),
            },
            trailing: Trailing::None,
            body: Body::Music(MusicBody {
                elapsed: crate::view::fmt_clock(position),
                remaining: crate::view::fmt_remaining(snapshot.duration - position),
                fraction,
                playing: snapshot.playing,
                shuffle: snapshot.shuffle,
                repeat: snapshot.repeat,
                controls,
                live: snapshot.live,
            }),
        };
        self.view.main = Some(ActivityView {
            kind: ActivityKind::Music,
            accent,
            pill,
            flyout,
        });
        if self.volume.active {
            self.view.overlay_pill = Some(self.volume_pill());
        }
    }

    fn apply_call(&mut self) {
        let Ok(runtime) = self.call_runtime.lock() else {
            crate::log::log("window.call: runtime mutex poisoned");
            return;
        };
        let Some(detect) = runtime.detect.as_ref() else {
            let media = self.media_snapshot.clone();
            drop(runtime);
            if let Some(media) = media {
                self.apply_media(media);
            } else if self.view.main.as_ref().map(|view| view.kind) == Some(ActivityKind::Call) {
                self.view.main = None;
            }
            return;
        };
        let accent = design::CALL_GREEN;
        let now = std::time::Instant::now();
        let duration = detect.duration_at_fetch
            + now.saturating_duration_since(detect.started).as_secs_f64()
            - detect.duration_at_fetch;
        let pill = PillView {
            icon: IconId::Phone,
            tile: TileView::colored(accent),
            label: detect.app_name.clone(),
            value: crate::view::fmt_clock(duration),
            progress: None,
            kind: PillKind::Normal,
        };
        let flyout = FlyoutView {
            leading: Leading::Initial {
                ch: detect.app_name.chars().next().unwrap_or('C'),
                bg: accent,
                fg: design::DARK_GLYPH,
            },
            title: detect.app_name.clone(),
            subtitle: Subtitle {
                prefix: runtime.device.clone(),
                accent: Some("Active call".into()),
            },
            trailing: Trailing::None,
            body: Body::Call(crate::view::CallBody {
                device: runtime.device.clone(),
                app_name: detect.app_name.clone(),
                mic_muted: runtime.mic_muted,
                output_muted: runtime.output_muted,
            }),
        };
        self.view.main = Some(ActivityView {
            kind: ActivityKind::Call,
            accent,
            pill,
            flyout,
        });
    }

    fn volume_pill(&self) -> PillView {
        let level = self.volume.level as f32 / 100.0;
        PillView {
            icon: if self.volume.muted {
                IconId::SpeakerMuted
            } else {
                IconId::Speaker
            },
            tile: TileView::volume(),
            label: "Volume".into(),
            value: if self.volume.muted {
                "Muted".into()
            } else {
                format!("{}%", self.volume.level)
            },
            progress: Some(level),
            kind: PillKind::Volume {
                level,
                muted: self.volume.muted,
            },
        }
    }

    fn draw(&mut self) -> Result<()> {
        // SAFETY: the swapchain, device context and target are owned by this surface.
        unsafe {
            let back_buffer: ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
            let surface: IDXGISurface = back_buffer.cast()?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: ManuallyDrop::new(None),
            };
            let target = self
                .context
                .CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
            self.context.SetTarget(&target);
            self.context.BeginDraw();
            self.context.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }));
            let render_target = self.context.cast()?;
            let input = RenderInput {
                view: &self.view,
                theme: self.theme,
                scale: 1.0,
                pill: self.pill,
                bubble: None,
                flyout: self.flyout_open.then_some(self.flyout),
                flyout_alpha: self.flyout_open as u8 as f32,
                pill_alpha: 1.0,
                top_alpha: 1.0,
                body_alpha: 1.0,
                top_dy: 0.0,
                body_dy: 0.0,
                hover: None,
                pressed: None,
                focus: None,
                hover_mix: 0.0,
                sheen: None,
                flash_t: None,
                drag_bob: 0.0,
                progress: None,
                icon_swap: None,
            };
            self.hit_map = self.painter.draw(&render_target, &input)?;
            self.context.EndDraw(None, None)?;
            self.context.SetTarget(None::<&ID2D1Image>);
            self.swapchain.Present(1, Default::default()).ok()?;
        }
        Ok(())
    }

    fn update_window_region(&self, hwnd: HWND) {
        unsafe {
            let region = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(
                self.pill.x.round() as i32,
                self.pill.y.round() as i32,
                (self.pill.x + self.pill.w).round() as i32,
                (self.pill.y + self.pill.h).round() as i32,
                24,
                24,
            );
            let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(hwnd, Some(region), true);
        }
    }

    fn draw_visible_fallback(&self, hwnd: HWND) {
        unsafe {
            let dc = windows::Win32::Graphics::Gdi::GetDC(Some(hwnd));
            if dc.0.is_null() {
                return;
            }
            let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(COLORREF(0x00302d2b));
            let accent = windows::Win32::Graphics::Gdi::CreateSolidBrush(COLORREF(0x00d68b45));
            let left = self.pill.x.round() as i32;
            let top = self.pill.y.round() as i32;
            let right = (self.pill.x + self.pill.w).round() as i32;
            let bottom = (self.pill.y + self.pill.h).round() as i32;
            let _ = windows::Win32::Graphics::Gdi::SelectObject(dc, brush.into());
            let _ = windows::Win32::Graphics::Gdi::RoundRect(dc, left, top, right, bottom, 24, 24);
            let _ = windows::Win32::Graphics::Gdi::SelectObject(dc, accent.into());
            let _ = windows::Win32::Graphics::Gdi::Rectangle(
                dc,
                left + 6,
                top + 6,
                left + 12,
                bottom - 6,
            );
            let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush.into());
            let _ = windows::Win32::Graphics::Gdi::DeleteObject(accent.into());
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(Some(hwnd), dc);
        }
    }
}
