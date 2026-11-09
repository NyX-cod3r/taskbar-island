//! Native island window lifecycle and DirectComposition surface.

use std::collections::HashMap;
use std::mem::{size_of, ManuallyDrop};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{w, Error, IUnknown, Interface, Result};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1DeviceContext, ID2D1Image, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::DirectComposition::{IDCompositionTarget, IDCompositionVisual};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGISurface, IDXGISwapChain1, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, EndPaint,
    ScreenToClient, SetWindowRgn, PAINTSTRUCT, RGN_OR,
};
use windows::Win32::System::Ole::{IDropTarget, OleInitialize};
use windows::Win32::System::SystemServices::MK_LBUTTON;
use windows::Win32::UI::Accessibility::{UiaReturnRawElementProvider, UiaRootObjectId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, ReleaseCapture, SendInput, SetCapture, TrackMouseEvent,
    UnregisterHotKey, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_SHIFT, TME_LEAVE, TRACKMOUSEEVENT, VIRTUAL_KEY,
    VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LBUTTON, VK_RETURN, VK_SHIFT,
    VK_SPACE, VK_T, VK_TAB, VK_UP, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindow, GetWindowRect, KillTimer,
    LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassExW, SetTimer, SetWindowPos,
    SetWindowsHookExW, ShowWindow, TranslateMessage, UnhookWindowsHookEx, CS_DBLCLKS, CS_HREDRAW,
    CS_VREDRAW, GW_HWNDPREV, HHOOK, HTTRANSPARENT, HWND_TOPMOST, IDC_ARROW, KBDLLHOOKSTRUCT, MSG,
    MSLLHOOKSTRUCT, SM_CXDRAG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP,
    WM_CREATE, WM_DESTROY, WM_ERASEBKGND, WM_GETOBJECT, WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCHITTEST, WM_PAINT,
    WM_RBUTTONDOWN, WM_SETTINGCHANGE, WM_THEMECHANGED, WM_TIMER, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::activities::call::{self, AudioCommand, CallRuntime};
use crate::activities::media::{self, MediaCommand, MediaSnapshot, WM_APP_MEDIA};
use crate::activities::shelf::{
    self, DragOutcome, ShelfStore, SHELF_DRAG_AT, SHELF_DROP, SHELF_DROP_CTRL, SHELF_HOVER_CONVERT,
    SHELF_HOVER_ENTER, SHELF_HOVER_LEAVE, WM_APP_SHELF,
};
use crate::activities::timer::Countdown;
use crate::anim::{cubic_bezier, Tween};
use crate::clipboard::{ClipboardRuntime, WM_APP_CLIPBOARD};
use crate::convert::options::{self, SourceKind, Tab};
use crate::convert::{self, jobs, shell, Probe, ProbeKind};
use crate::design::{self, ActivityKind, RectD};
use crate::engines::{
    self, Action, CancelToken, CapabilityTable, EngineId, EnginePaths, FileKind, Progress, Stage,
};
use crate::gpu;
use crate::render::{HitMap, Painter, ProgressVisual, RenderInput, WaveVisual};
use crate::theme::Theme;
use crate::thumb::{ThumbWorker, WM_APP_THUMB};
use crate::tooltip::{self, TipRequest, TooltipWindow};
use crate::view::{
    ActivityView, AlbumArt, Body, ConvertBody, ConvertOptionView, ConvertState, ConvertTabView,
    DoneView, Element, FlyoutView, IconId, Leading, MusicBody, PillKind, PillView, Subtitle,
    TileView, Trailing, ViewModel, WheelAction, WorkingView,
};

const CLASS_NAME: windows::core::PCWSTR = w!("TaskbarIsland");
const TIMER_HOTKEY_ID: i32 = 7;
const TIMER_FALLBACK_HOTKEY_ID: i32 = 8;
/// Hover/press fill tween. A separate timer so the 1 Hz countdown timer does
/// not force 16 ms wake-ups while the pointer is idle.
const HOVER_TIMER_ID: usize = 2;
/// Flyout open/close frames and the one-shot pointer-leave auto-close.
const FLYOUT_TIMER_ID: usize = 3;
const AUTO_CLOSE_TIMER_ID: usize = 4;
/// Low-frequency z-order guard. The shell's taskbar is topmost too and
/// Explorer re-raises it (e.g. when translucency is toggled), so the island
/// re-inserts itself at the top of the topmost band while it is running.
const ZORDER_TIMER_ID: usize = 5;
const ZORDER_INTERVAL_MS: u32 = 2000;
/// Posted by the low-level mouse hook when a click lands outside the island.
const WM_APP_FLYOUT_OUTSIDE: u32 = WM_APP + 30;
/// Posted by the scoped low-level keyboard hook while the card is open.
const WM_APP_RAIL_KEY: u32 = WM_APP + 31;
/// Frames for the rail indicator slide and content crossfade.
const RAIL_TIMER_ID: usize = 6;
/// One-shot tooltip delay, shared by every hover/focus target.
const TOOLTIP_TIMER_ID: usize = 9;
/// One-shot grace period before a missing media session clears the snapshot.
const MEDIA_CLEAR_TIMER_ID: usize = 10;
/// Smooths the convert progress bar and runs the done countdown.
const CONVERT_TIMER_ID: usize = 11;
/// Watches for a Ctrl+left-button drag starting anywhere on the desktop. The
/// drag data never reaches us before the pointer does, so the gesture is
/// polled and the expanded Convert card opens early with a waiting face.
const DRAG_WATCH_TIMER_ID: usize = 12;
const DRAG_WATCH_INTERVAL_MS: u32 = 120;
/// Posted by the conversion worker after each progress tick and at the end.
const WM_APP_CONVERT: u32 = WM_APP + 32;
/// Hourly retention sweep for the clipboard history.
const CLIP_SWEEP_TIMER_ID: usize = 13;
/// One-shot end of the "+N" clipboard pill pop.
const CLIP_POP_TIMER_ID: usize = 14;
/// One-shot auto-cancel of the two-step Clear confirm.
const CLIP_CONFIRM_TIMER_ID: usize = 15;
/// One-shot 450 ms delay before a history row shows its hover preview (spec 7).
const CLIP_PREVIEW_TIMER_ID: usize = 16;
/// `WM_MOUSELEAVE` ships under `UI::Controls` in windows-rs 0.62; the value is
/// stable Win32.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// Set only while the low-level mouse hook is armed, so the hook procedure can
/// post the outside-click message without touching the surface from a hook.
static HOOK_HWND: AtomicIsize = AtomicIsize::new(0);

/// Set only while the rail keyboard hook is armed.
static HOOK_KEYS_HWND: AtomicIsize = AtomicIsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlyoutPhase {
    Closed,
    Opening,
    Open,
    Closing,
}

/// Card content crossfade when the rail switches activity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContentPhase {
    Idle,
    Out,
    In,
}

/// Messages the worker sends back. The worker never touches `Surface`; it only
/// posts `WM_APP_CONVERT`, and the UI thread drains this channel.
enum ConvertMsg {
    Progress {
        done: usize,
        total: usize,
    },
    /// Live stage/fraction from the engine running the current file.
    Engine(Progress),
    Finished(jobs::Outcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConvertPhase {
    Choosing,
    Working {
        done: usize,
        total: usize,
        cancelling: bool,
    },
    Done,
    Failed,
}

/// Probing is synchronous: only headers are read, never pixel data, and the
/// list is capped by `convert::MAX_INPUTS`. Conversion itself runs on a
/// worker; this struct is the UI-side view of it.
struct ConvertSession {
    probes: Vec<Probe>,
    /// Indices into `probes` the converter can accept.
    convertible: Vec<usize>,
    /// Header file; the first convertible item, or the first probe when the
    /// whole batch is unsupported.
    primary: usize,
    /// Tile under the pointer; drives the tip and nothing else.
    hovered: Option<Action>,
    /// Active tab in the options strip.
    tab: Tab,
    phase: ConvertPhase,
    action: Option<Action>,
    started: Instant,
    /// Eased 0..=100 shown in the header.
    percent: f32,
    /// Engine-reported fraction within the current file, when it has one.
    fraction: Option<f32>,
    /// Reading/converting/saving; drives the three stage dots.
    stage: Stage,
    rx: Option<Receiver<ConvertMsg>>,
    cancel: CancelToken,
    outcome: Option<jobs::Outcome>,
    done_deadline: Option<Instant>,
    done_paused_at: Option<Instant>,
    failure: Option<(String, String)>,
    /// A Ctrl+drag was detected but the file has not reached the island yet;
    /// the card shows a waiting face with no options.
    waiting: bool,
}

impl ConvertSession {
    fn new(probes: Vec<Probe>, now: Instant) -> Option<Self> {
        if probes.is_empty() {
            return None;
        }
        let convertible: Vec<usize> = probes
            .iter()
            .enumerate()
            .filter(|(_, probe)| probe.kind.convertible())
            .map(|(index, _)| index)
            .collect();
        let primary = convertible.first().copied().unwrap_or(0);
        Some(Self {
            probes,
            convertible,
            primary,
            hovered: None,
            tab: Tab::ConvertTo,
            phase: ConvertPhase::Choosing,
            action: None,
            started: now,
            percent: 0.0,
            fraction: None,
            stage: Stage::Reading,
            rx: None,
            cancel: CancelToken::new(),
            outcome: None,
            done_deadline: None,
            done_paused_at: None,
            failure: None,
            waiting: false,
        })
    }

    /// The empty Convert card shown from the moment a Ctrl+drag is detected.
    /// It has no file, so it offers no options until the drag reaches the
    /// island and `start_convert` replaces it.
    fn waiting(now: Instant) -> Self {
        Self {
            probes: Vec::new(),
            convertible: Vec::new(),
            primary: 0,
            hovered: None,
            tab: Tab::ConvertTo,
            phase: ConvertPhase::Choosing,
            action: None,
            started: now,
            percent: 0.0,
            fraction: None,
            stage: Stage::Reading,
            rx: None,
            cancel: CancelToken::new(),
            outcome: None,
            done_deadline: None,
            done_paused_at: None,
            failure: None,
            waiting: true,
        }
    }

    fn from_paths(paths: &[String], now: Instant) -> Option<Self> {
        Self::new(convert::probe_all(paths), now)
    }

    fn pins_card_open(&self) -> bool {
        matches!(
            self.phase,
            ConvertPhase::Working { .. } | ConvertPhase::Done
        )
    }

    fn cancel(&self) {
        self.cancel.cancel();
    }

    fn finish(&mut self, outcome: jobs::Outcome, now: Instant) {
        let failed = outcome.created.is_empty();
        if failed {
            let (title, detail) = if outcome.cancelled {
                (
                    "Conversion cancelled".to_string(),
                    "No more files were converted.".to_string(),
                )
            } else {
                (
                    "Couldn't convert".to_string(),
                    outcome
                        .failed
                        .first()
                        .map(|failure| failure.reason.clone())
                        .unwrap_or_else(|| "This file couldn't be converted.".to_string()),
                )
            };
            self.failure = Some((title, detail));
            self.phase = ConvertPhase::Failed;
        } else {
            self.percent = 100.0;
            self.phase = ConvertPhase::Done;
            self.done_deadline =
                Some(now + Duration::from_millis(design::CONVERT_COUNTDOWN_MS as u64));
        }
        self.rx = None;
        self.outcome = Some(outcome);
    }

    fn notice(&self) -> Option<(&'static str, String)> {
        if !self.convertible.is_empty() {
            return None;
        }
        let probe = self.probes.first()?;
        let title = match &probe.kind {
            ProbeKind::CloudPending => "Not downloaded yet",
            ProbeKind::TooLarge { .. } => "File is too large",
            ProbeKind::Folder => "That's a folder",
            ProbeKind::Missing => "File not found",
            _ => "Unsupported file",
        };
        Some((
            title,
            probe
                .kind
                .notice()
                .unwrap_or("This file can't be converted yet.")
                .to_owned(),
        ))
    }
}

/// One decoded (or failed) thumbnail. `pending` keeps the UI from
/// queueing the same path on every repaint while the worker is busy; `max`
/// is the box it was asked for (`thumb::MAX_THUMB` for rows and tiles,
/// `thumb::PREVIEW_PX` for the hover preview), so a result only ever fills
/// the entry that requested it.
struct ThumbEntry {
    key: u64,
    max: u32,
    art: Option<Arc<AlbumArt>>,
    pending: bool,
}

/// Queue one file for decoding at `max`, remembering a cached failure so a
/// hostile or broken file is never decoded in a loop. Shared by the shelf
/// rows, the clipboard rows and the hover preview, which differ only in the
/// box they ask for.
fn queue_thumb(
    worker: &ThumbWorker,
    cache: &mut HashMap<String, ThumbEntry>,
    path: &str,
    max: u32,
) {
    let key = crate::thumb::key_for(path);
    match cache.get_mut(path) {
        // Same file at the same size: decoded, failing, or still in flight.
        Some(entry) if entry.key == key && entry.max == max => {}
        Some(entry) => {
            entry.key = key;
            entry.max = max;
            entry.art = None;
            entry.pending = true;
            worker.request(path, max);
        }
        None => {
            cache.insert(
                path.to_owned(),
                ThumbEntry {
                    key,
                    max,
                    art: None,
                    pending: true,
                },
            );
            worker.request(path, max);
        }
    }
}

/// Move one worker result into the entry that asked for it. `max` is part of
/// the match, so a 128 px row thumbnail can never land in the 384 px preview
/// cache (or the reverse) when both hold the same path.
fn fill_thumb(cache: &mut HashMap<String, ThumbEntry>, result: &crate::thumb::ThumbArt) -> bool {
    let mut changed = false;
    for entry in cache.values_mut() {
        if entry.pending && entry.key == result.key && entry.max == result.max {
            entry.art = result.art.clone();
            entry.pending = false;
            changed = true;
        }
    }
    changed
}

struct Surface {
    hwnd: HWND,
    scale: f32,
    /// Physical screen origin of the composition window (for hook hit tests).
    origin: (i32, i32),
    swapchain: IDXGISwapChain1,
    context: ID2D1DeviceContext,
    // Kept alive for the window lifetime; the visual owns the swapchain.
    _comp_target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    pill: RectD,
    flyout: RectD,
    flyout_open: bool,
    /// Activity the open flyout belongs to; closes it when the winner changes.
    flyout_owner: Option<ActivityKind>,
    flyout_phase: FlyoutPhase,
    flyout_started: Instant,
    flyout_alpha: f32,
    flyout_scale: f32,
    flyout_dy: f32,
    flyout_leave_at: Option<Instant>,
    mouse_hook: Option<HHOOK>,
    keyboard_hook: Option<HHOOK>,
    /// Rail selection, which runs ahead of the fading content during a swap.
    rail_current: Option<ActivityKind>,
    rail_from: Option<ActivityKind>,
    rail_t: f32,
    rail_tween: Option<Tween>,
    /// The separate layered popup shared by every tip in the card.
    tip: TooltipWindow,
    /// Element whose tip is pending or visible.
    tip_target: Option<Element>,
    /// Moment the current target became hovered/focused.
    tip_since: Instant,
    /// When a visible tip was last dismissed; a neighbour inside 300 ms
    /// reappears instantly instead of waiting out the 450 ms delay.
    tip_hidden_at: Option<Instant>,
    /// Keyboard-focused rail button (0..4).
    rail_focus: Option<usize>,
    content_phase: ContentPhase,
    content_started: Instant,
    content_alpha: f32,
    content_dy: f32,
    pending_kind: Option<ActivityKind>,
    hit_map: HitMap,
    painter: Painter,
    view: ViewModel,
    theme: Theme,
    media_rx: Receiver<Option<MediaSnapshot>>,
    media_tx: Sender<MediaCommand>,
    media_snapshot: Option<MediaSnapshot>,
    /// First moment the worker reported no session while a snapshot was live.
    media_missing_at: Option<Instant>,
    /// Last transport command we sent; a track change can outlive the normal
    /// grace period while the player loads the next item.
    media_command_at: Option<Instant>,
    call_runtime: Arc<Mutex<CallRuntime>>,
    audio_tx: Sender<AudioCommand>,
    shelf_store: ShelfStore,
    shelf_slot: Arc<Mutex<Vec<String>>>,
    _shelf_target: IDropTarget,
    /// Shared with the drop target: whether the open convert card can take
    /// the dragged batch at all (mixed kinds or no installed engine say no).
    convert_accepts: Arc<AtomicBool>,
    shelf_hover: bool,
    /// The open convert card was spring-loaded by a Ctrl+drag. Leaving the
    /// island, or releasing Ctrl, retracts it again; auto-close stays off
    /// while the drag is in flight.
    drag_convert: bool,
    /// Anchor of a possible Ctrl+left-button drag seen by the gesture watcher:
    /// when it was first sampled and the screen point it started from.
    drag_watch: Option<(Instant, i32, i32)>,
    /// The open convert card is the empty waiting face from that gesture.
    drag_waiting: bool,
    /// Active convert card. `None` means the shelf card's normal file view.
    convert: Option<ConvertSession>,
    /// Engine matrix the Convert card filters its options against, refreshed
    /// every time a conversion card opens.
    caps: CapabilityTable,
    /// Executable locations matching that matrix; cloned per conversion so the
    /// worker never holds the engine lock.
    engine_paths: EnginePaths,
    /// Tile indices gathered with Ctrl+click; a drag from any of them takes all.
    shelf_selected: Vec<usize>,
    /// Press origin of the tile under the button, used to detect a drag-out.
    shelf_press: Option<(Element, f32, f32)>,
    /// True from drag start until OLE's modal loop returns; click activation is
    /// suppressed meanwhile because the drop ends with a button-up.
    shelf_dragging: bool,
    /// Grid scroll offset in DIP; clamped against the virtual content height.
    shelf_scroll: f32,
    /// The soft cap dropped an older entry; the count tooltip explains it.
    shelf_overflow: bool,
    /// Grab offset inside the scrollbar thumb while it is dragged.
    shelf_scroll_drag: Option<f32>,
    /// Rubber-band rectangle in card DIP coordinates.
    shelf_rubber: Option<(f32, f32, f32, f32)>,
    /// Selection when the rubber-band started, for Ctrl+drag addition.
    shelf_rubber_base: Vec<usize>,
    /// Shift+click extends from this tile.
    shelf_shift_anchor: Option<usize>,
    /// Off-thread thumbnail worker and its path-keyed cache.
    thumb_worker: ThumbWorker,
    thumbs: HashMap<String, ThumbEntry>,
    /// 384 px decodes for the clipboard hover preview (spec 7): a separate
    /// cache from `thumbs` so one file can hold both sizes at once.
    clip_preview_art: HashMap<String, ThumbEntry>,
    /// Traveling wave phase for the music/timer pill and the music card.
    wave_phase: f32,
    /// Timestamp of the previous wave tick, for a frame-rate-independent phase.
    wave_tick_at: Instant,
    timer: Countdown,
    /// Manual activity chosen with the wheel; `None` follows priority.
    pin: Option<ActivityKind>,
    /// Automatic winner at the previous refresh, used to release a pin when
    /// another activity takes over or the pinned one ends.
    last_auto: Option<ActivityKind>,
    /// Accumulated wheel delta; precision touchpads send small steps.
    wheel_accum: i32,
    /// Element under the pointer, or `None` in the click-through halo.
    hover: Option<Element>,
    pressed: Option<Element>,
    hover_mix: f32,
    hover_tween: Option<Tween>,
    mouse_tracking: bool,
    /// User nudge from settings; kept so the anchor can be recomputed when
    /// the tray geometry changes after startup.
    slot_offset_dip: i32,
    /// Clipboard capture, history and hourly retention sweep.
    clip: Option<ClipboardRuntime>,
    clip_filter: crate::view::ClipFilter,
    clip_scroll: f32,
    /// Grab offset inside the clipboard scrollbar thumb while it is dragged.
    clip_scroll_drag: Option<f32>,
    clip_active: Option<usize>,
    /// Armed two-step Clear: when it was armed, so the confirm can auto-cancel
    /// after `design::CLIP_CLEAR_CONFIRM_MS`.
    clip_clear_at: Option<Instant>,
    /// Window that was in front when a copy-back-and-paste was asked for. The
    /// Ctrl+V is sent there once the card has closed, and only while it is
    /// still the foreground window (spec 3.2).
    pending_paste: Option<HWND>,
    /// End of the transient "+N" pill state after a new copy.
    clip_pop_until: Option<Instant>,
    clip_pop_count: u32,
    /// Store id of the row whose hover-preview overlay is on screen (spec 7).
    clip_preview: Option<u64>,
    /// `(row id, armed at)` while the 450 ms one-shot runs; cleared the moment
    /// the pointer leaves the row, the list scrolls or the card closes.
    clip_preview_arm: Option<(u64, Instant)>,
}

thread_local! {
    static SURFACE: std::cell::RefCell<Option<Surface>> = const { std::cell::RefCell::new(None) };
}

/// Run `f` with the active surface unless a nested message arrived while the
/// surface is already borrowed. Windows can dispatch messages synchronously
/// from inside Win32/DXGI calls (`SetWindowPos`, `SetWindowRgn`, `Present`), so
/// a plain `borrow_mut` panics there. Nested messages fall back to the default
/// procedure instead.
fn with_surface<R>(f: impl FnOnce(&mut Surface) -> R) -> Option<R> {
    SURFACE.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        slot.as_mut().map(f)
    })
}

fn with_surface_ref<R>(f: impl FnOnce(&Surface) -> R) -> Option<R> {
    SURFACE.with(|cell| cell.try_borrow().ok().and_then(|slot| slot.as_ref().map(f)))
}

pub fn register(instance: HINSTANCE) -> Result<()> {
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW)? };
    let icon = crate::tray::load_app_icon(instance);
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
        lpfnWndProc: Some(wnd_proc),
        hInstance: instance,
        hIcon: icon.unwrap_or_default(),
        hIconSm: icon.unwrap_or_default(),
        hCursor: cursor,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // SAFETY: class is fully initialized and valid for this call.
    let _atom = unsafe { RegisterClassExW(&class) };
    Ok(())
}

pub fn create(
    instance: HINSTANCE,
    anchor: &crate::anchor::Anchor,
    slot_offset_dip: i32,
) -> Result<HWND> {
    let rect = anchor.window;
    // SAFETY: the class was registered above and all handles belong to this process.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP,
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
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    let settings = crate::settings::Settings::load();
    let (media_tx, media_command_rx) = mpsc::channel();
    let (media_snapshot_tx, media_rx) = mpsc::channel();
    media::subscribe(media_snapshot_tx, media_command_rx, media_tx.clone(), hwnd)?;
    let call_runtime = call::runtime();
    call::subscribe(
        hwnd,
        settings.call_app_allowlist.clone(),
        call_runtime.clone(),
    )?;
    let audio_tx = call::subscribe_audio(hwnd, call_runtime.clone())?;
    // SAFETY: the UI thread owns the island window and remains initialized
    // until the window is destroyed.
    if unsafe { OleInitialize(None).is_err() } {
        return Err(Error::from_hresult(windows::Win32::Foundation::E_FAIL));
    }
    let shelf_slot = Arc::new(Mutex::new(Vec::new()));
    let convert_accepts = Arc::new(AtomicBool::new(false));
    let clip = ClipboardRuntime::start(instance, hwnd, settings.clipboard_paused);
    // SAFETY: OLE is initialized on this UI thread and the target is retained
    // by Surface for the window lifetime.
    let shelf_target =
        unsafe { shelf::register_drop_target(hwnd, shelf_slot.clone(), convert_accepts.clone())? };
    SURFACE.with(|surface| {
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        let local_pill = anchor.local(anchor.pill);
        let local_flyout = anchor.local(anchor.flyout);
        *surface.borrow_mut() = Some(Surface::new(
            hwnd,
            width,
            height,
            anchor.scale,
            (rect.left, rect.top),
            local_pill,
            local_flyout,
            slot_offset_dip,
            media_rx,
            media_tx,
            call_runtime,
            audio_tx,
            shelf_slot,
            shelf_target,
            convert_accepts,
            clip,
        )?);
        Ok::<(), windows::core::Error>(())
    })?;
    with_surface(|surface| {
        if !surface.shelf_store.is_empty() {
            surface.apply_shelf();
        }
        surface.draw()
    })
    .transpose()?;
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
    // Keep the island above the topmost taskbar across appearance changes
    // (TranslucentTB toggles, Explorer restarts) without polling faster than
    // the taskbar can re-raise itself.
    // SAFETY: the timer id belongs to this window.
    unsafe {
        SetTimer(Some(hwnd), ZORDER_TIMER_ID, ZORDER_INTERVAL_MS, None);
        SetTimer(
            Some(hwnd),
            DRAG_WATCH_TIMER_ID,
            DRAG_WATCH_INTERVAL_MS,
            None,
        );
        SetTimer(Some(hwnd), CLIP_SWEEP_TIMER_ID, design::CLIP_SWEEP_MS, None);
    }
    if let Err(error) = crate::tray::install(instance, hwnd) {
        crate::log::log_err("tray.install", &error);
    }
    unsafe {
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
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Click-outside detection lives only while the flyout is open. The hook posts
/// a message; it never swallows the click itself.
/// Scoped rail keyboard navigation. The island is `WS_EX_NOACTIVATE`, so it
/// can never own focus; this hook forwards arrow/Enter/Space keys to the card
/// while it is open and the pointer or keyboard focus is on the rail. It never
/// swallows a key, so the foreground app keeps working normally.
unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && wparam.0 as u32 == WM_KEYDOWN {
        // SAFETY: for code >= 0 Win32 guarantees `lparam` is a KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let vk = info.vkCode;
        let interesting = [
            VK_UP, VK_DOWN, VK_HOME, VK_END, VK_RETURN, VK_SPACE, VK_TAB, VK_DELETE,
        ]
        .iter()
        .any(|key| key.0 as u32 == vk);
        let target = HOOK_KEYS_HWND.load(Ordering::Relaxed);
        if interesting && target != 0 {
            // SAFETY: the hook is armed only while that island window exists.
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(target as *mut core::ffi::c_void)),
                    WM_APP_RAIL_KEY,
                    WPARAM(vk as usize),
                    LPARAM(0),
                );
            }
        }
    }
    // SAFETY: pass-through chain; the hook never consumes the key.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn low_level_mouse_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && matches!(wparam.0 as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN) {
        // SAFETY: for code >= 0 Win32 guarantees `lparam` is an MSLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let inside = with_surface_ref(|surface| surface.screen_contains(info.pt)).unwrap_or(true);
        let target = HOOK_HWND.load(Ordering::Relaxed);
        if !inside && target != 0 {
            // SAFETY: the hook is armed only while that island window exists.
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(target as *mut core::ffi::c_void)),
                    WM_APP_FLYOUT_OUTSIDE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }
    // SAFETY: low-level hooks must always forward to the next hook.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Re-insert the island above the taskbar when the shell has raised itself
/// over the pill. Explorer is topmost as well and re-raises the taskbar on
/// appearance changes (translucency toggles, Explorer restarts), which would
/// otherwise leave the pill painted behind an opaque taskbar.
///
/// The z-order is inspected first so the guard never climbs over unrelated
/// topmost windows such as the Start menu or a system flyout.
fn reassert_topmost(hwnd: HWND) {
    let Some(taskbar) = crate::anchor::taskbar_window() else {
        return;
    };
    let mut above = false;
    let mut cursor = hwnd;
    // SAFETY: read-only walk up this process's z-order chain.
    unsafe {
        for _ in 0..256 {
            cursor = GetWindow(cursor, GW_HWNDPREV).unwrap_or_default();
            if cursor.0.is_null() {
                break;
            }
            if cursor == taskbar {
                above = true;
                break;
            }
        }
    }
    if !above {
        return;
    }
    // SAFETY: hwnd is this thread's island window.
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

/// Sample the taskbar's rendered tone around the island so the pill can pick
/// text and fill colors that contrast with the bar, not with the registry's
/// light/dark preference (which customizers can leave disagreeing).
fn sampled_taskbar_tone(hwnd: HWND) -> Option<bool> {
    let taskbar = crate::anchor::taskbar_window()?;
    let mut island = RECT::default();
    // SAFETY: `island` is a valid out pointer for this thread's window.
    unsafe { GetWindowRect(hwnd, &mut island).ok()? };
    crate::theme::sample_taskbar_light(taskbar, island)
}

/// Screen point packed into an LPARAM by the drop target.
fn unpack_point(lparam: LPARAM) -> (i32, i32) {
    let packed = lparam.0 as u64;
    ((packed as u32) as i32, ((packed >> 32) as u32) as i32)
}

/// Whether a key is held right now, even if another process has focus.
/// A drag from Explorer never gives the island input state, so async state
/// is the only way to see the Ctrl modifier.
fn key_down(key: i32) -> bool {
    // SAFETY: GetAsyncKeyState has no preconditions.
    (unsafe { GetAsyncKeyState(key) } as u16 & 0x8000) != 0
}

/// Ctrl+V as four keyboard events, for copy-back-and-paste (spec 3.2).
/// `SendInput` only queues them, so nothing here can block the UI thread.
fn send_ctrl_v() -> std::result::Result<(), String> {
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS::default()
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [
        key(VK_CONTROL, false),
        key(VK_V, false),
        key(VK_V, true),
        key(VK_CONTROL, true),
    ];
    // SAFETY: `inputs` is a well-formed array of keyboard events.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(format!("queued {sent} of {} key events", inputs.len()));
    }
    Ok(())
}

/// Whether a queued paste may still be sent. `target` is the window that was
/// in front when the row was copied, `foreground` is who is in front now; a
/// null target never counts, because with no foreground window there is
/// nothing to paste into (spec 3.2 and 5.7).
fn paste_is_due(target: HWND, foreground: HWND) -> bool {
    !target.0.is_null() && target == foreground
}

/// Recompute the anchor and slide the island when the tray's left edge has
/// moved. Adding or removing tray icons (or taskbar pins, which can add
/// them) shifts `TrayNotifyWnd` left and right; the pill must follow so it
/// always keeps the 12 DIP gap and never ends up under the hidden-menu
/// button. Only position changes are applied — the window's size is fixed
/// for its swapchain, so a taskbar height/DPI change is left to a restart.
fn sync_anchor(hwnd: HWND) {
    let Some(taskbar) = crate::anchor::taskbar_window() else {
        return;
    };
    let Some(slot) = with_surface_ref(|surface| surface.slot_offset_dip) else {
        return;
    };
    let dpi = crate::anchor::dpi_for_window(taskbar);
    let Some(anchor) = crate::anchor::Anchor::compute(taskbar, dpi, slot) else {
        return;
    };
    if !anchor.visible {
        return;
    }
    let mut current = RECT::default();
    // SAFETY: current is a valid out pointer.
    if unsafe { GetWindowRect(hwnd, &mut current) }.is_err() {
        return;
    }
    let new_w = anchor.window.right - anchor.window.left;
    let new_h = anchor.window.bottom - anchor.window.top;
    let cur_w = current.right - current.left;
    let cur_h = current.bottom - current.top;
    if (new_w, new_h) != (cur_w, cur_h) {
        // Size changes need a swapchain resize; not supported at runtime.
        return;
    }
    if current.left == anchor.window.left && current.top == anchor.window.top {
        return;
    }
    // Move outside any surface borrow: SetWindowPos synchronously dispatches
    // window messages back into wnd_proc.
    // SAFETY: hwnd is this thread's island window.
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            anchor.window.left,
            anchor.window.top,
            new_w,
            new_h,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOZORDER,
        );
    }
    with_surface(|surface| {
        surface.origin = (anchor.window.left, anchor.window.top);
        surface.pill = anchor.local(anchor.pill);
        surface.flyout = anchor.local(anchor.flyout);
        let _ = surface.draw();
    });
}

extern "system" fn wnd_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_NCCREATE => LRESULT(1),
        WM_CREATE => LRESULT(0),
        WM_GETOBJECT => {
            if lparam.0 as i32 == UiaRootObjectId {
                let provider = with_surface_ref(|surface| surface.a11y_provider()).flatten();
                if let Some(provider) = provider {
                    // SAFETY: UIA asked for the root object of this window; the
                    // provider resolves every name against this frame's hits.
                    return unsafe { UiaReturnRawElementProvider(hwnd, wparam, lparam, &provider) };
                }
            }
            // SAFETY: falls through to the default handling for non-UIA ids.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            // SAFETY: BeginPaint/EndPaint bracket the validation of this update region.
            unsafe { BeginPaint(hwnd, &mut paint) };
            if let Some(Err(error)) = with_surface(Surface::draw) {
                crate::log::log_err("window.paint", &error);
            }
            // SAFETY: balanced with BeginPaint above.
            let _ = unsafe { EndPaint(hwnd, &paint) };
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_NCHITTEST => {
            let mut point = POINT {
                x: (lparam.0 as i16) as i32,
                y: ((lparam.0 >> 16) as i16) as i32,
            };
            // SAFETY: point is writable and hwnd is the island window.
            let _ = unsafe { ScreenToClient(hwnd, &mut point) };
            let hit = with_surface_ref(|surface| {
                let x = point.x as f32 / surface.scale;
                let y = point.y as f32 / surface.scale;
                (surface.view.pill().is_some() && surface.pill.contains(x, y))
                    || (surface.flyout_open && surface.flyout.contains(x, y))
            })
            .unwrap_or(false);
            if hit {
                // Keep the pill available for future interaction without activating the window.
                LRESULT(1)
            } else {
                LRESULT(HTTRANSPARENT as isize)
            }
        }
        WM_MOUSEMOVE => {
            let button_down = (wparam.0 & MK_LBUTTON.0 as usize) != 0;
            let drag_paths = with_surface(|surface| {
                surface.note_pointer_inside();
                if !surface.mouse_tracking {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    // SAFETY: `track` is fully initialized and targets this window.
                    unsafe {
                        let _ = TrackMouseEvent(&mut track);
                    }
                    surface.mouse_tracking = true;
                }
                let x = (lparam.0 as i16) as f32 / surface.scale;
                let y = ((lparam.0 >> 16) as i16) as f32 / surface.scale;
                let hit = surface.hit_at(x, y);
                let mut changed = surface.set_hover(hit, Instant::now());
                if let Some(grab) = surface.shelf_scroll_drag {
                    surface.hide_tip();
                    changed |= surface.drag_scrollbar(y, grab);
                }
                if let Some(grab) = surface.clip_scroll_drag {
                    surface.hide_tip();
                    changed |= surface.drag_clip_scrollbar(y, grab);
                }
                if surface.shelf_rubber.is_some() {
                    surface.hide_tip();
                    changed |= surface.update_rubber(x, y);
                }
                if changed {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
                surface.begin_shelf_drag(x, y, button_down)
            })
            .flatten();
            if let Some(paths) = drag_paths {
                // SAFETY: OLE took its own capture for the modal loop; letting
                // go first avoids fighting it. The surface borrow is released.
                unsafe {
                    let _ = ReleaseCapture();
                }
                let outcome = unsafe { shelf::start_drag(&paths) };
                with_surface(|surface| surface.finish_shelf_drag(outcome, &paths));
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
            if delta != 0 {
                // WM_MOUSEWHEEL carries screen coordinates.
                let px = (lparam.0 as i16) as i32;
                let py = ((lparam.0 >> 16) as i16) as i32;
                let switched = with_surface(|surface| {
                    let x = (px - surface.origin.0) as f32 / surface.scale;
                    let y = (py - surface.origin.1) as f32 / surface.scale;
                    surface.wheel(delta, x, y)
                })
                .unwrap_or(false);
                if switched {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_surface(|surface| {
                surface.mouse_tracking = false;
                surface.hide_tip();
                let was_pressed = surface.pressed.take().is_some();
                let changed = surface.set_hover(None, Instant::now());
                if !surface.shelf_dragging {
                    surface.schedule_auto_close(Instant::now());
                }
                if changed || was_pressed {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            with_surface(|surface| {
                surface.hide_tip();
                let x = (lparam.0 as i16) as f32 / surface.scale;
                let y = ((lparam.0 >> 16) as i16) as f32 / surface.scale;
                let hit = surface.hit_at(x, y);
                surface.pressed = hit;
                surface.shelf_press = match hit {
                    Some(element @ Element::ShelfTile(_)) => Some((element, x, y)),
                    _ => None,
                };
                match hit {
                    Some(Element::ShelfTile(index)) => {
                        surface.press_shelf_tile(index);
                    }
                    Some(Element::ShelfScrollbar) => {
                        surface.begin_scrollbar_drag(y);
                    }
                    Some(Element::ClipScrollbar) => {
                        surface.begin_clip_scrollbar_drag(y);
                    }
                    None if surface.flyout_open && surface.shelf_viewport().contains(x, y) => {
                        surface.begin_rubber(x, y);
                    }
                    _ => {}
                }
                let capture = surface.shelf_press.is_some()
                    || surface.shelf_scroll_drag.is_some()
                    || surface.clip_scroll_drag.is_some()
                    || surface.shelf_rubber.is_some();
                if capture {
                    // Capture so a fast drag still reaches the threshold
                    // even after the pointer leaves the tile.
                    // SAFETY: capture belongs to this thread's window.
                    unsafe {
                        SetCapture(hwnd);
                    }
                }
                surface.set_hover(hit, Instant::now());
                if hit.is_some() {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            with_surface(|surface| {
                surface.hide_tip();
                let x = (lparam.0 as i16) as f32 / surface.scale;
                let y = ((lparam.0 >> 16) as i16) as f32 / surface.scale;
                match surface.hit_at(x, y) {
                    Some(Element::ShelfTile(index)) => {
                        if let Some(path) = surface.shelf_store.paths().get(index) {
                            if !shelf::open(path) {
                                crate::log::log(&format!("shelf.open failed: {}", path));
                            }
                        }
                    }
                    // Double-click = copy back AND paste (spec 3.2). The
                    // card closes first; the Ctrl+V is queued for the
                    // window that is still in front when it has closed.
                    Some(Element::ClipRow(id)) => {
                        surface.copy_back(id, true);
                    }
                    _ => {}
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with_surface(|surface| {
                let x = (lparam.0 as i16) as f32 / surface.scale;
                let y = ((lparam.0 >> 16) as i16) as f32 / surface.scale;
                surface.shelf_press = None;
                surface.shelf_scroll_drag = None;
                surface.clip_scroll_drag = None;
                surface.end_rubber();
                if !surface.shelf_dragging {
                    // SAFETY: balances the SetCapture on a tile press.
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                }
                let pressed = surface.pressed.take();
                let hit = surface.hit_at(x, y);
                // A release only activates when it lands on the element that
                // was pressed. `pressed == None` keeps PostMessage-driven
                // tests working without a matching button-down. A drag-out
                // ends with a button-up too, but must not open the file.
                let activate = !surface.shelf_dragging
                    && hit.is_some()
                    && (pressed.is_none() || pressed == hit);
                surface.set_hover(hit, Instant::now());
                if activate {
                    match hit {
                        Some(Element::Pill) => {
                            if surface.view.flyout().is_some() {
                                let now = Instant::now();
                                if surface.flyout_open
                                    && surface.flyout_phase != FlyoutPhase::Closing
                                {
                                    surface.begin_close_flyout(now);
                                } else {
                                    surface.open_flyout(now);
                                }
                            }
                        }
                        Some(element) => {
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
                                    let now = Instant::now();
                                    if surface.timer.running() {
                                        surface.timer.toggle_pause(now);
                                    } else {
                                        if !surface.timer.active() {
                                            surface.timer.preset(5);
                                        }
                                        surface.timer.resume(now);
                                    }
                                    surface.apply_timer(now);
                                    surface.sync_tick_timer();
                                    None
                                }
                                Element::TimerPreset(minutes) => {
                                    surface.timer.preset(minutes);
                                    surface.apply_timer(Instant::now());
                                    surface.sync_tick_timer();
                                    None
                                }
                                Element::Rail(kind) => {
                                    surface.activate_rail(kind);
                                    None
                                }
                                Element::TimerStop => {
                                    surface.timer.stop();
                                    surface.refresh(Instant::now());
                                    surface.sync_tick_timer();
                                    surface.begin_close_flyout(Instant::now());
                                    None
                                }
                                Element::ShelfSelectAll => {
                                    surface.select_all_shelf();
                                    None
                                }
                                Element::ShelfClear => {
                                    surface.clear_shelf();
                                    None
                                }
                                Element::ShelfRemove(index) => {
                                    surface.remove_shelf_indices(&[index]);
                                    None
                                }
                                Element::ShelfTile(_) => None,
                                Element::ClipFilter(filter) => {
                                    surface.clip_filter = filter;
                                    surface.clip_scroll = 0.0;
                                    surface.clip_active = None;
                                    surface.show_clipboard();
                                    None
                                }
                                Element::ClipPause => {
                                    if let Some(clip) = surface.clip.as_mut() {
                                        let paused = !clip.is_paused();
                                        clip.set_paused(paused);
                                        let mut settings = crate::settings::Settings::load();
                                        settings.clipboard_paused = paused;
                                        settings.save();
                                    }
                                    surface.show_clipboard();
                                    None
                                }
                                Element::ClipClear => {
                                    // Two-step confirm (spec 3.3): the trash
                                    // swaps for a red ✓ and a ✕, and the
                                    // whole thing auto-cancels after 4 s.
                                    surface.arm_clip_clear(hwnd);
                                    surface.show_clipboard();
                                    None
                                }
                                Element::ClipClearConfirm => {
                                    surface.disarm_clip_clear();
                                    if let Some(clip) = surface.clip.as_mut() {
                                        clip.clear_unpinned();
                                    }
                                    surface.clip_active = None;
                                    surface.clip_scroll = 0.0;
                                    surface.show_clipboard();
                                    None
                                }
                                Element::ClipClearCancel => {
                                    surface.disarm_clip_clear();
                                    surface.show_clipboard();
                                    None
                                }
                                Element::ClipPin(id) => {
                                    if let Some(clip) = surface.clip.as_mut() {
                                        if let Some(item) = clip.store.get(id) {
                                            let _ = clip.set_pinned(id, !item.pinned);
                                        }
                                    }
                                    surface.show_clipboard();
                                    None
                                }
                                Element::ClipDelete(id) => {
                                    if let Some(clip) = surface.clip.as_mut() {
                                        clip.remove(id);
                                    }
                                    surface.clip_active = None;
                                    surface.show_clipboard();
                                    None
                                }
                                Element::ClipRow(id) => {
                                    // Clicking a row puts it back on the clipboard
                                    // (spec 3.2) and leaves it as the keyboard
                                    // focus, which is what Enter and Delete act on.
                                    surface.clip_active = surface.clip_row_index(id);
                                    surface.copy_back(id, false);
                                    surface.show_clipboard();
                                    None
                                }
                                Element::ClipCopy(id) => {
                                    surface.copy_back(id, false);
                                    None
                                }
                                Element::ClipScrollbar => None,
                                Element::ConvertButton => {
                                    let paths = surface.shelf_convert_paths();
                                    surface.start_convert(paths, Instant::now());
                                    None
                                }
                                Element::ConvertClose => {
                                    surface.end_convert();
                                    surface.begin_close_flyout(Instant::now());
                                    None
                                }
                                Element::ConvertOption(action) => {
                                    surface.choose_convert(action);
                                    None
                                }
                                Element::ConvertTab(tab) => {
                                    surface.select_convert_tab(tab);
                                    None
                                }
                                Element::ConvertCancel => {
                                    surface.cancel_conversion();
                                    None
                                }
                                Element::ConvertShow => {
                                    surface.reveal_output();
                                    None
                                }
                                Element::ConvertCopy => {
                                    surface.copy_output();
                                    None
                                }
                                Element::ConvertUndo => {
                                    surface.undo_conversion();
                                    None
                                }
                                _ => None,
                            };
                            if let Some(command) = command {
                                crate::log::log(&format!("media.command {command:?}"));
                                surface.note_media_command();
                                let _ = surface.media_tx.send(command);
                            }
                        }
                        None => {}
                    }
                }
                // A live surface can disappear while its flyout is open
                // (timer stopped, media ended). Never leave an invisible
                // hit rectangle behind.
                if surface.view.flyout().is_none() && surface.flyout_open {
                    surface.finish_close_flyout();
                }
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        call::WM_APP_CALL | call::WM_APP_CALL_AUDIO => {
            with_surface(|surface| {
                surface.apply_call();
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        WM_APP_SHELF => {
            with_surface(|surface| {
                match wparam.0 as u32 {
                    SHELF_DROP => {
                        surface.drag_convert = false;
                        surface.drag_waiting = false;
                        let paths = surface
                            .shelf_slot
                            .lock()
                            .map(|mut slot| std::mem::take(&mut *slot))
                            .unwrap_or_default();
                        if !paths.is_empty() {
                            surface.shelf_overflow = surface.shelf_store.add(paths);
                            surface.shelf_selected.clear();
                            surface.shelf_shift_anchor = None;
                            surface.shelf_rubber = None;
                            surface.shelf_scroll = 0.0;
                            surface.shelf_hover = false;
                            surface.view.overlay_pill = None;
                            surface.apply_shelf();
                        }
                    }
                    SHELF_DROP_CTRL => {
                        let (px, py) = unpack_point(lparam);
                        let sprung = surface.drag_convert;
                        surface.drag_waiting = false;
                        let paths = surface
                            .shelf_slot
                            .lock()
                            .map(|mut slot| std::mem::take(&mut *slot))
                            .unwrap_or_default();
                        if !paths.is_empty() {
                            if sprung && surface.convert.is_some() {
                                // The card is already up under the drag;
                                // the drop only decides the option.
                                surface.drag_convert = false;
                                if let Some(Element::ConvertOption(choice)) =
                                    surface.hit_pointer_at_screen(px, py)
                                {
                                    surface.choose_convert(choice);
                                }
                            } else {
                                surface.start_convert(paths, Instant::now());
                            }
                        }
                    }
                    SHELF_HOVER_ENTER => {
                        surface.shelf_hover = true;
                        surface.view.overlay_pill = Some(surface.shelf_drag_pill(false));
                        if surface.drag_convert || surface.drag_waiting {
                            surface.drag_convert = false;
                            surface.drag_waiting = false;
                            surface.retract_drag_convert();
                        }
                    }
                    SHELF_HOVER_CONVERT => {
                        surface.shelf_hover = true;
                        surface.view.overlay_pill = Some(surface.shelf_drag_pill(true));
                        // Spring-load the card as soon as Ctrl is over the
                        // island, so the drag can be released on a tile.
                        // A session that is already working or showing its
                        // result is left alone.
                        let replaceable = surface
                            .convert
                            .as_ref()
                            .map(|session| session.phase == ConvertPhase::Choosing)
                            .unwrap_or(true);
                        if replaceable && !surface.drag_convert {
                            let paths = surface
                                .shelf_slot
                                .lock()
                                .map(|slot| slot.clone())
                                .unwrap_or_default();
                            if !paths.is_empty() {
                                surface.start_convert(paths, Instant::now());
                                surface.drag_convert = true;
                                surface.drag_waiting = false;
                            }
                        }
                    }
                    SHELF_HOVER_LEAVE => {
                        surface.shelf_hover = false;
                        surface.view.overlay_pill = None;
                        surface.set_hover(None, Instant::now());
                        if surface.drag_convert || surface.drag_waiting {
                            surface.drag_convert = false;
                            surface.drag_waiting = false;
                            surface.retract_drag_convert();
                        }
                    }
                    SHELF_DRAG_AT if surface.drag_convert => {
                        let (px, py) = unpack_point(lparam);
                        let hover = surface
                            .hit_pointer_at_screen(px, py)
                            .filter(|element| matches!(element, Element::ConvertOption(_)));
                        surface.set_hover(hover, Instant::now());
                    }
                    _ => {}
                }
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        WM_APP_FLYOUT_OUTSIDE => {
            with_surface(|surface| {
                if surface.flyout_open {
                    surface.begin_close_flyout(Instant::now());
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_APP_RAIL_KEY => {
            with_surface(|surface| {
                surface.handle_rail_key(wparam.0 as u32);
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        WM_HOTKEY if matches!(wparam.0 as i32, TIMER_HOTKEY_ID | TIMER_FALLBACK_HOTKEY_ID) => {
            with_surface(|surface| {
                surface.toggle_timer();
                surface.sync_tick_timer();
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u32 == VK_ESCAPE.0 as u32 => {
            with_surface(|surface| {
                surface.hide_tip();
                if surface.flyout_open {
                    surface.begin_close_flyout(Instant::now());
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_APP_MEDIA => {
            with_surface(|surface| {
                let mut changed = false;
                while let Ok(snapshot) = surface.media_rx.try_recv() {
                    match snapshot {
                        Some(snapshot) => surface.apply_media(snapshot),
                        None => surface.media_gone(),
                    }
                    changed = true;
                }
                if changed {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_APP_CONVERT => {
            with_surface(|surface| {
                surface.apply_convert_messages();
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        WM_APP_CLIPBOARD => {
            with_surface(|surface| {
                surface.apply_clipboard(Instant::now());
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == CONVERT_TIMER_ID => {
            with_surface(|surface| {
                if surface.advance_convert(std::time::Instant::now()) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == ZORDER_TIMER_ID => {
            reassert_topmost(hwnd);
            sync_anchor(hwnd);
            // The bar can restyle without a WM_SETTINGCHANGE (customizers,
            // Explorer restarts); re-sample so the pill follows its tone.
            with_surface(|surface| {
                if surface.refresh_theme() {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == HOVER_TIMER_ID => {
            with_surface(|surface| {
                if surface.advance_hover(std::time::Instant::now()) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == FLYOUT_TIMER_ID => {
            with_surface(|surface| {
                if surface.advance_flyout(Instant::now()) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == RAIL_TIMER_ID => {
            with_surface(|surface| {
                if surface.advance_rail(Instant::now()) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TOOLTIP_TIMER_ID => {
            with_surface(|surface| {
                surface.tip_due(Instant::now());
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == CLIP_SWEEP_TIMER_ID => {
            with_surface(|surface| {
                surface.sweep_clipboard();
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == CLIP_POP_TIMER_ID => {
            with_surface(|surface| {
                surface.end_clip_pop();
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == CLIP_CONFIRM_TIMER_ID => {
            with_surface(|surface| {
                surface.expire_clip_clear();
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == CLIP_PREVIEW_TIMER_ID => {
            with_surface(|surface| {
                if surface.clip_preview_due(Instant::now()) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_APP_THUMB => {
            with_surface(|surface| {
                if surface.drain_thumbs() {
                    // Rebuild the open card so the new bitmap lands in the
                    // body, then repaint (same pattern as the state mutators).
                    let kind = surface.view.main.as_ref().map(|view| view.kind);
                    match kind {
                        Some(ActivityKind::Shelf) => surface.show_shelf(),
                        Some(ActivityKind::Clipboard) => surface.show_clipboard(),
                        _ => {}
                    }
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == AUTO_CLOSE_TIMER_ID => {
            with_surface(|surface| {
                let now = Instant::now();
                let due = surface
                    .flyout_leave_at
                    .map(|left| {
                        now.saturating_duration_since(left).as_millis()
                            >= design::AUTO_CLOSE_MS as u128
                    })
                    .unwrap_or(false);
                // One-shot: kill before re-arming so a slow frame cannot
                // leave two expiries queued.
                unsafe {
                    let _ = KillTimer(Some(surface.hwnd), AUTO_CLOSE_TIMER_ID);
                }
                if due {
                    // Work in flight, the finished card, and a card
                    // spring-loaded by an active drag own their lifetime.
                    let pinned = surface.drag_convert
                        || surface.drag_waiting
                        || surface
                            .convert
                            .as_ref()
                            .map(|session| session.pins_card_open())
                            .unwrap_or(false);
                    if !pinned {
                        surface.begin_close_flyout(now);
                        unsafe {
                            let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                        }
                    }
                } else if let Some(left) = surface.flyout_leave_at {
                    let elapsed = now.saturating_duration_since(left).as_millis() as u32;
                    let remaining = design::AUTO_CLOSE_MS.saturating_sub(elapsed).max(1);
                    unsafe {
                        SetTimer(Some(surface.hwnd), AUTO_CLOSE_TIMER_ID, remaining, None);
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == MEDIA_CLEAR_TIMER_ID => {
            with_surface(|surface| {
                surface.expire_media();
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == DRAG_WATCH_TIMER_ID => {
            with_surface(|surface| {
                if surface.watch_ctrl_drag(Instant::now()) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER => {
            with_surface(|surface| {
                let mut changed = false;
                let now = std::time::Instant::now();
                if surface.timer.running()
                    && (surface.timer.tick(now)
                        || surface.view.main.as_ref().map(|view| view.kind)
                            == Some(ActivityKind::Timer))
                {
                    surface.apply_timer(now);
                    changed = true;
                }
                if surface.wave_animating() {
                    surface.advance_wave(now);
                    changed = true;
                }
                surface.sync_tick_timer();
                if changed {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            // A taskbar appearance change (e.g. toggling translucency) is a
            // likely moment for Explorer to raise the taskbar above us.
            reassert_topmost(hwnd);
            sync_anchor(hwnd);
            crate::theme::flush_native_menus();
            with_surface(|surface| {
                if surface.refresh_theme() {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            // Unhook outside-click detection and rail keys if the window dies
            // with the flyout still open.
            with_surface(|surface| {
                surface.remove_mouse_hook();
                surface.remove_keyboard_hook();
            });
            // SAFETY: this window owns the registration.
            unsafe {
                let _ = KillTimer(Some(hwnd), 1);
                let _ = KillTimer(Some(hwnd), HOVER_TIMER_ID);
                let _ = KillTimer(Some(hwnd), FLYOUT_TIMER_ID);
                let _ = KillTimer(Some(hwnd), AUTO_CLOSE_TIMER_ID);
                let _ = KillTimer(Some(hwnd), ZORDER_TIMER_ID);
                let _ = KillTimer(Some(hwnd), DRAG_WATCH_TIMER_ID);
                let _ = KillTimer(Some(hwnd), RAIL_TIMER_ID);
                let _ = KillTimer(Some(hwnd), TOOLTIP_TIMER_ID);
                let _ = KillTimer(Some(hwnd), MEDIA_CLEAR_TIMER_ID);
                let _ = KillTimer(Some(hwnd), CLIP_SWEEP_TIMER_ID);
                let _ = KillTimer(Some(hwnd), CLIP_POP_TIMER_ID);
                let _ = KillTimer(Some(hwnd), CLIP_CONFIRM_TIMER_ID);
                let _ = KillTimer(Some(hwnd), CLIP_PREVIEW_TIMER_ID);
                let _ = UnregisterHotKey(Some(hwnd), TIMER_HOTKEY_ID);
                let _ = UnregisterHotKey(Some(hwnd), TIMER_FALLBACK_HOTKEY_ID);
            }
            // SAFETY: the drop target belongs to this UI thread and window.
            unsafe {
                shelf::revoke_drop_target(hwnd);
                windows::Win32::System::Ole::OleUninitialize();
            }
            crate::tray::shutdown();
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
        let next = Theme::current().with_taskbar_tone(sampled_taskbar_tone(self.hwnd));
        if next == self.theme {
            return false;
        }
        self.theme = next;
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        hwnd: HWND,
        width: u32,
        height: u32,
        scale: f32,
        origin: (i32, i32),
        pill: RectD,
        flyout: RectD,
        slot_offset_dip: i32,
        media_rx: Receiver<Option<MediaSnapshot>>,
        media_tx: Sender<MediaCommand>,
        call_runtime: Arc<Mutex<CallRuntime>>,
        audio_tx: Sender<AudioCommand>,
        shelf_slot: Arc<Mutex<Vec<String>>>,
        shelf_target: IDropTarget,
        convert_accepts: Arc<AtomicBool>,
        clip: Option<ClipboardRuntime>,
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
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            ..Default::default()
        };
        // SAFETY: COM interfaces and the swap-chain descriptor are valid.
        let (swapchain, context, comp_target, visual) = unsafe {
            let device: IUnknown = stack.d3d.cast()?;
            let swapchain = stack
                .dxgi_factory
                .CreateSwapChainForComposition(&device, &desc, None)?;
            let context = stack
                .d2d
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let comp_target = stack.comp.CreateTargetForHwnd(hwnd, true)?;
            let visual = stack.comp.CreateVisual()?;
            visual.SetContent(&swapchain.cast::<IUnknown>()?)?;
            comp_target.SetRoot(&visual)?;
            stack.comp.Commit()?;
            (swapchain, context, comp_target, visual)
        };
        let engine_snapshot = engines::snapshot();
        Ok(Self {
            hwnd,
            scale,
            origin,
            swapchain,
            context,
            _comp_target: comp_target,
            _visual: visual,
            pill,
            flyout,
            flyout_open: false,
            flyout_owner: None,
            flyout_phase: FlyoutPhase::Closed,
            flyout_started: Instant::now(),
            flyout_alpha: 0.0,
            flyout_scale: 1.0,
            flyout_dy: 0.0,
            flyout_leave_at: None,
            mouse_hook: None,
            keyboard_hook: None,
            rail_current: None,
            rail_from: None,
            rail_t: 1.0,
            rail_tween: None,
            tip: TooltipWindow::new(),
            tip_target: None,
            tip_since: Instant::now(),
            tip_hidden_at: None,
            rail_focus: None,
            content_phase: ContentPhase::Idle,
            content_started: Instant::now(),
            content_alpha: 1.0,
            content_dy: 0.0,
            pending_kind: None,
            hit_map: HitMap::default(),
            painter: Painter::new(),
            view: ViewModel {
                main: Some(ViewModel::idle(Theme::current().shelf_color())),
                ..ViewModel::empty()
            },
            theme: Theme::current().with_taskbar_tone(sampled_taskbar_tone(hwnd)),
            media_rx,
            media_tx,
            media_snapshot: None,
            media_missing_at: None,
            media_command_at: None,
            call_runtime,
            audio_tx,
            shelf_store: ShelfStore::load(),
            shelf_slot,
            _shelf_target: shelf_target,
            convert_accepts,
            shelf_hover: false,
            drag_convert: false,
            drag_watch: None,
            drag_waiting: false,
            convert: None,
            caps: engine_snapshot.capabilities,
            engine_paths: engine_snapshot.paths,
            shelf_selected: Vec::new(),
            shelf_press: None,
            shelf_dragging: false,
            shelf_scroll: 0.0,
            shelf_overflow: false,
            shelf_scroll_drag: None,
            shelf_rubber: None,
            shelf_rubber_base: Vec::new(),
            shelf_shift_anchor: None,
            thumb_worker: ThumbWorker::start(hwnd),
            thumbs: HashMap::new(),
            clip_preview_art: HashMap::new(),
            wave_phase: 0.0,
            wave_tick_at: Instant::now(),
            timer: Countdown::default(),
            pin: None,
            last_auto: None,
            wheel_accum: 0,
            hover: None,
            pressed: None,
            hover_mix: 0.0,
            hover_tween: None,
            mouse_tracking: false,
            slot_offset_dip,
            clip,
            clip_filter: crate::view::ClipFilter::All,
            clip_scroll: 0.0,
            clip_scroll_drag: None,
            clip_active: None,
            clip_clear_at: None,
            pending_paste: None,
            clip_pop_until: None,
            clip_pop_count: 0,
            clip_preview: None,
            clip_preview_arm: None,
        })
    }
    fn apply_shelf(&mut self) {
        self.shelf_store.prune();
        self.shelf_selected.retain(|&i| i < self.shelf_store.len());
        let scroll = crate::view::shelf_scroll(self.shelf_store.len(), self.shelf_scroll);
        self.shelf_scroll = scroll.scroll;
        self.thumbs.retain(|path, _| {
            self.shelf_store
                .paths()
                .iter()
                .any(|p| p.eq_ignore_ascii_case(path))
        });
        self.hide_tip();
        self.refresh(Instant::now());
    }

    /// Explorer-style tile press: plain selects one, Ctrl toggles, Shift takes
    /// the range from the previous anchor. Modifier state comes from the async
    /// keyboard because the island never owns focus.
    fn press_shelf_tile(&mut self, index: usize) {
        let ctrl = key_down(VK_CONTROL.0 as i32);
        let shift = key_down(VK_SHIFT.0 as i32);
        if shift {
            let anchor = self.shelf_shift_anchor.unwrap_or(index);
            let (lo, hi) = if anchor <= index {
                (anchor, index)
            } else {
                (index, anchor)
            };
            if !ctrl {
                self.shelf_selected.clear();
            }
            for i in lo..=hi {
                if !self.shelf_selected.contains(&i) {
                    self.shelf_selected.push(i);
                }
            }
        } else if ctrl {
            if let Some(pos) = self.shelf_selected.iter().position(|&i| i == index) {
                self.shelf_selected.remove(pos);
            } else {
                self.shelf_selected.push(index);
            }
            self.shelf_shift_anchor = Some(index);
        } else {
            self.shelf_selected.clear();
            self.shelf_selected.push(index);
            self.shelf_shift_anchor = Some(index);
        }
        self.show_shelf();
    }

    fn begin_rubber(&mut self, x: f32, y: f32) {
        let ctrl = key_down(VK_CONTROL.0 as i32);
        self.shelf_rubber_base = if ctrl {
            self.shelf_selected.clone()
        } else {
            self.shelf_selected.clear();
            Vec::new()
        };
        self.shelf_rubber = Some((x, y, 0.0, 0.0));
        self.shelf_shift_anchor = None;
        self.show_shelf();
    }

    /// Extends the rubber-band to the pointer. Returns whether it changed.
    fn update_rubber(&mut self, x: f32, y: f32) -> bool {
        let Some((x0, y0, _, _)) = self.shelf_rubber else {
            return false;
        };
        let band = RectD::new(x0.min(x), y0.min(y), (x - x0).abs(), (y - y0).abs());
        self.shelf_rubber = Some((band.x, band.y, band.w, band.h));
        let viewport = self.shelf_viewport();
        let scroll = crate::view::shelf_scroll(self.shelf_store.len(), self.shelf_scroll);
        let cols = design::SHELF_COLS;
        let pitch_x = design::SHELF_TILE_W + design::SHELF_TILE_GAP;
        let mut selected = self.shelf_rubber_base.clone();
        for row in scroll.first_row..scroll.first_row + scroll.rows {
            for col in 0..cols {
                let index = row * cols + col;
                if index >= self.shelf_store.len() {
                    break;
                }
                let tile = RectD::new(
                    viewport.x + col as f32 * pitch_x,
                    viewport.y + row as f32 * design::SHELF_ROW_H - scroll.scroll,
                    design::SHELF_TILE_W,
                    design::SHELF_TILE_H,
                );
                let intersects = tile.x < band.right()
                    && tile.right() > band.x
                    && tile.y < band.bottom()
                    && tile.bottom() > band.y;
                if intersects && !selected.contains(&index) {
                    selected.push(index);
                }
            }
        }
        selected.sort_unstable();
        self.shelf_selected = selected;
        self.show_shelf();
        true
    }

    fn end_rubber(&mut self) {
        if self.shelf_rubber.take().is_some() {
            self.show_shelf();
        }
        self.shelf_rubber_base.clear();
    }

    fn select_all_shelf(&mut self) {
        self.shelf_selected = (0..self.shelf_store.len()).collect();
        self.shelf_shift_anchor = None;
        self.show_shelf();
    }

    /// Clear removes only the selected references when there is a selection,
    /// otherwise every reference. The files on disk are never touched.
    fn clear_shelf(&mut self) {
        self.shelf_store.remove_or_clear(&self.shelf_selected);
        self.shelf_selected.clear();
        self.shelf_shift_anchor = None;
        self.shelf_rubber = None;
        self.shelf_overflow = false;
        self.shelf_scroll = 0.0;
        self.apply_shelf();
    }

    fn remove_shelf_indices(&mut self, indices: &[usize]) {
        if indices.is_empty() {
            return;
        }
        self.shelf_store.remove_indices(indices);
        self.shelf_selected.clear();
        self.shelf_shift_anchor = None;
        self.apply_shelf();
    }

    /// Paths the Shelf "Convert" chip acts on: the Ctrl+click selection, or
    /// the only file on the shelf.
    fn shelf_convert_paths(&self) -> Vec<String> {
        if !self.shelf_selected.is_empty() {
            return self
                .shelf_selected
                .iter()
                .filter_map(|index| self.shelf_store.paths().get(*index).cloned())
                .collect();
        }
        if self.shelf_store.len() == 1 {
            return self.shelf_store.paths().to_vec();
        }
        Vec::new()
    }

    /// Opens the converter for a batch. Probing only reads headers; the card
    /// appears even for unsupported files so the reason is visible.
    fn start_convert(&mut self, paths: Vec<String>, now: Instant) {
        if paths.is_empty() {
            return;
        }
        // Picking up an engine installed after launch costs one fingerprint
        // check per executable, not a full re-probe.
        engines::refresh_shared();
        let snapshot = engines::snapshot();
        self.caps = snapshot.capabilities;
        self.engine_paths = snapshot.paths;
        let next = ConvertSession::from_paths(&paths, now);
        let Some(next) = next else {
            return;
        };
        if let Some(previous) = &self.convert {
            previous.cancel();
        }
        self.convert = Some(next);
        self.shelf_hover = false;
        self.view.overlay_pill = None;
        self.pin = Some(ActivityKind::Shelf);
        self.show_shelf();
        // A card belonging to another activity cannot host the converter, and
        // a card that is closing must animate back in, so reopen it for the
        // Shelf (this also runs the open tween again).
        self.ensure_convert_flyout(now);
    }

    fn ensure_convert_flyout(&mut self, now: Instant) {
        if !self.flyout_open
            || self.flyout_owner != Some(ActivityKind::Shelf)
            || self.flyout_phase == FlyoutPhase::Closing
        {
            self.open_flyout(now);
            self.advance_flyout(now);
        }
    }

    /// Poll for a Ctrl+left-button drag starting anywhere on the desktop.
    /// OLE only reports the drag once the pointer is over the island, so the
    /// gesture itself is sampled; the card then opens early with a waiting
    /// face and turns into the real grid when the drag arrives.
    /// Returns whether a repaint is needed.
    fn watch_ctrl_drag(&mut self, now: Instant) -> bool {
        let pressed = key_down(VK_CONTROL.0 as i32) && key_down(VK_LBUTTON.0 as i32);
        if !pressed || self.shelf_dragging {
            self.drag_watch = None;
            // A drop over the island is OLE's to finish; only the card that
            // opened before the drag arrived is retracted here.
            if self.drag_waiting && !pressed {
                self.drag_waiting = false;
                self.retract_drag_convert();
                return true;
            }
            return false;
        }
        let mut pt = POINT::default();
        // SAFETY: GetCursorPos only writes the caller's point.
        if unsafe { GetCursorPos(&mut pt) }.is_err() {
            return false;
        }
        let Some((started, ax, ay)) = self.drag_watch else {
            self.drag_watch = Some((now, pt.x, pt.y));
            return false;
        };
        // Once the drag owns the card, do not replace it with the waiting
        // face: the OLE enter already turned it into the real options.
        if self.drag_waiting
            || self.drag_convert
            || now.saturating_duration_since(started)
                < Duration::from_millis(DRAG_WATCH_INTERVAL_MS as u64)
        {
            return false;
        }
        if (pt.x - ax).pow(2) + (pt.y - ay).pow(2) < 64 {
            return false;
        }
        // A press that began on the island is its own UI, not a file drag.
        if self.screen_contains(POINT { x: ax, y: ay }) {
            return false;
        }
        self.drag_watch = None;
        self.open_drag_waiting(now);
        true
    }

    fn open_drag_waiting(&mut self, now: Instant) {
        if self.drag_waiting || self.shelf_dragging {
            return;
        }
        // Never interrupt another activity's card or a conversion in flight.
        if self.flyout_open && self.flyout_owner != Some(ActivityKind::Shelf) {
            return;
        }
        if self
            .convert
            .as_ref()
            .is_some_and(|session| session.phase != ConvertPhase::Choosing)
        {
            return;
        }
        if let Some(previous) = &self.convert {
            previous.cancel();
        }
        self.convert = Some(ConvertSession::waiting(now));
        self.drag_waiting = true;
        self.drag_convert = false;
        self.shelf_hover = false;
        self.view.overlay_pill = None;
        self.pin = Some(ActivityKind::Shelf);
        self.show_shelf();
        self.ensure_convert_flyout(now);
    }

    fn end_convert(&mut self) {
        if let Some(session) = &self.convert {
            session.cancel();
        }
        self.convert_accepts.store(false, Ordering::Relaxed);
        if self.convert.take().is_some() {
            self.stop_convert_timer();
            self.refresh(Instant::now());
        }
    }

    /// A tile on the Convert page was clicked.
    fn choose_convert(&mut self, action: Action) {
        let Some(session) = self.convert.as_ref() else {
            return;
        };
        if session.phase != ConvertPhase::Choosing || session.convertible.is_empty() {
            return;
        }
        let Some(primary) = session.probes.get(session.primary) else {
            return;
        };
        if !options::action_available(file_kind(primary), action, &self.caps) {
            return;
        }
        self.begin_conversion(action);
    }

    /// Switch the options strip to another tab.
    fn select_convert_tab(&mut self, tab: Tab) {
        let Some(session) = self.convert.as_mut() else {
            return;
        };
        if session.phase != ConvertPhase::Choosing || session.tab == tab {
            return;
        }
        session.tab = tab;
        session.hovered = None;
        self.show_convert();
    }

    fn begin_conversion(&mut self, action: Action) {
        let now = Instant::now();
        let hwnd_raw = self.hwnd.0 as isize;
        let Some(session) = self.convert.as_mut() else {
            return;
        };
        let inputs: Vec<String> = session
            .convertible
            .iter()
            .filter_map(|index| session.probes.get(*index).map(|probe| probe.path.clone()))
            .collect();
        if inputs.is_empty() {
            return;
        }
        let total = inputs.len();
        let (tx, rx) = mpsc::channel();
        session.phase = ConvertPhase::Working {
            done: 0,
            total,
            cancelling: false,
        };
        session.action = Some(action);
        session.started = now;
        session.percent = 0.0;
        session.fraction = None;
        session.stage = Stage::Reading;
        session.rx = Some(rx);
        session.cancel = CancelToken::new();
        session.outcome = None;
        session.failure = None;
        session.done_deadline = None;
        session.done_paused_at = None;
        let cancel = session.cancel.clone();
        let caps = self.caps.clone();
        let paths = self.engine_paths.clone();
        // OCR jobs need a language; the router has already checked that one is
        // installed, so the first installed language is the choice.
        let language = matches!(action, Action::ImageOcr(_))
            .then(|| caps.get(EngineId::Tesseract).ocr_languages.first().cloned())
            .flatten();

        std::thread::spawn(move || {
            let outcome = jobs::run_batch(
                &inputs,
                action,
                language.as_deref(),
                &caps,
                &paths,
                &cancel,
                &mut |done, total, _name| {
                    let _ = tx.send(ConvertMsg::Progress { done, total });
                    post_convert(hwnd_raw);
                },
                &mut |_done, _total, progress| {
                    let _ = tx.send(ConvertMsg::Engine(progress));
                    post_convert(hwnd_raw);
                },
            );
            let _ = tx.send(ConvertMsg::Finished(outcome));
            post_convert(hwnd_raw);
        });

        // SAFETY: the timer id belongs to this window.
        unsafe {
            SetTimer(Some(self.hwnd), CONVERT_TIMER_ID, 33, None);
        }
        self.show_convert();
    }

    fn stop_convert_timer(&self) {
        // SAFETY: the timer id belongs to this window.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), CONVERT_TIMER_ID);
        }
    }

    fn sync_convert_timer(&self) {
        let interval = self
            .convert
            .as_ref()
            .and_then(|session| match session.phase {
                ConvertPhase::Working { .. } => Some(33),
                ConvertPhase::Done => Some(100),
                _ => None,
            });
        // SAFETY: the timer id belongs to this window.
        unsafe {
            match interval {
                Some(ms) => {
                    SetTimer(Some(self.hwnd), CONVERT_TIMER_ID, ms, None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), CONVERT_TIMER_ID);
                }
            }
        }
    }

    /// Drains worker messages. Called on the UI thread only.
    fn apply_convert_messages(&mut self) {
        let now = Instant::now();
        let mut finished = false;
        if let Some(session) = self.convert.as_mut() {
            let mut messages = Vec::new();
            if let Some(rx) = &session.rx {
                while let Ok(message) = rx.try_recv() {
                    messages.push(message);
                }
            }
            for message in messages {
                match message {
                    ConvertMsg::Progress { done, total } => {
                        if let ConvertPhase::Working {
                            done: current,
                            total: expected,
                            ..
                        } = &mut session.phase
                        {
                            *current = done;
                            *expected = total;
                        }
                        session.fraction = None;
                        session.stage = Stage::Reading;
                    }
                    ConvertMsg::Engine(progress) => {
                        session.fraction = progress.fraction;
                        session.stage = progress.stage;
                    }
                    ConvertMsg::Finished(outcome) => {
                        session.finish(outcome, now);
                        finished = true;
                    }
                }
            }
        }
        if finished {
            self.show_convert();
            self.sync_convert_timer();
        }
    }

    /// One tick of the convert card: eases the percentage and runs the done
    /// countdown. Returns whether a repaint is needed.
    fn advance_convert(&mut self, now: Instant) -> bool {
        let inside = self.flyout_leave_at.is_none();
        let mut repaint = false;
        let mut close = false;
        if let Some(session) = self.convert.as_mut() {
            match session.phase {
                ConvertPhase::Working { done, total, .. } => {
                    let total = total.max(1) as f32;
                    let elapsed = now.saturating_duration_since(session.started).as_secs_f32();
                    let estimate = ((done as f32 + (elapsed / (1.4 * total)).min(1.0)) / total
                        * 90.0)
                        .min(90.0);
                    let measured = session
                        .fraction
                        .map(|fraction| (done as f32 + fraction.clamp(0.0, 1.0)) / total * 100.0);
                    let target = measured.unwrap_or(estimate).min(99.0);
                    if session.percent < target {
                        session.percent = (session.percent + 1.1).min(target);
                    }
                    repaint = true;
                }
                ConvertPhase::Done => {
                    let Some(mut deadline) = session.done_deadline else {
                        return false;
                    };
                    if inside {
                        if session.done_paused_at.is_none() {
                            session.done_paused_at = Some(now);
                        }
                    } else if let Some(paused) = session.done_paused_at.take() {
                        deadline += now.saturating_duration_since(paused);
                        session.done_deadline = Some(deadline);
                    }
                    let reference = session.done_paused_at.unwrap_or(now);
                    if deadline <= reference && !inside {
                        close = true;
                    } else {
                        repaint = true;
                    }
                }
                _ => {}
            }
        }
        if close {
            self.begin_close_flyout(now);
            return true;
        }
        if repaint {
            // The painter reads the snapshot stored in `view`, so the model has
            // to be rebuilt on every tick or the card would stay frozen.
            self.show_convert();
        }
        repaint
    }

    fn cancel_conversion(&mut self) {
        let mut changed = false;
        if let Some(session) = self.convert.as_mut() {
            if matches!(session.phase, ConvertPhase::Working { .. }) {
                session.cancel.cancel();
                if let ConvertPhase::Working { cancelling, .. } = &mut session.phase {
                    *cancelling = true;
                }
                changed = true;
            }
        }
        if changed {
            self.show_convert();
        }
    }

    fn reveal_output(&mut self) {
        let Some(path) = output_paths(&self.convert).into_iter().next() else {
            return;
        };
        if let Err(error) = shell::reveal(&path) {
            crate::log::log(&format!("convert.reveal failed: {error}"));
        }
    }

    fn copy_output(&mut self) {
        let paths = output_paths(&self.convert);
        if let Err(error) = shell::copy_paths(self.hwnd, &paths) {
            crate::log::log(&format!("convert.copy failed: {error}"));
        }
    }

    fn undo_conversion(&mut self) {
        let paths = output_paths(&self.convert);
        if paths.is_empty() {
            return;
        }
        match shell::recycle(&paths) {
            Ok(()) => {
                if let Some(session) = self.convert.as_mut() {
                    // These files are gone; keeping them in the outcome would
                    // make a second Undo target paths that no longer exist.
                    session.outcome = None;
                    session.done_deadline = None;
                    session.done_paused_at = None;
                    session.failure = Some((
                        "Moved to the Recycle Bin".into(),
                        "The new file was removed. The original is untouched.".into(),
                    ));
                    session.phase = ConvertPhase::Failed;
                }
                self.show_convert();
                self.sync_convert_timer();
            }
            Err(error) => crate::log::log(&format!("convert.undo failed: {error}")),
        }
    }

    fn show_convert(&mut self) {
        // Keep the active tab inside the set this file can actually offer.
        if self
            .convert
            .as_ref()
            .is_some_and(|session| !session.waiting)
        {
            let caps = self.caps.clone();
            if let Some(session) = self.convert.as_mut() {
                if let Some(primary) = session.probes.get(session.primary) {
                    let kind = file_kind(primary);
                    let sources = source_kinds(session);
                    let tabs = options::tabs(kind, &caps, &sources);
                    if !tabs.is_empty() && !tabs.contains(&session.tab) {
                        session.tab = tabs[0];
                    }
                }
            }
        }
        let Some(session) = &self.convert else {
            return;
        };
        if session.waiting {
            // The batch is not probed yet, so the drop must be allowed to
            // land; `start_convert` replaces this face with the real options.
            self.convert_accepts.store(true, Ordering::Relaxed);
            let accent = self.theme.shelf_color();
            self.view.main = Some(ActivityView {
                kind: ActivityKind::Shelf,
                accent,
                pill: PillView {
                    icon: IconId::Convert,
                    tile: TileView::colored(accent),
                    label: "Convert".into(),
                    value: String::new(),
                    progress: None,
                    kind: PillKind::Normal,
                    art: None,
                },
                flyout: FlyoutView {
                    kind: ActivityKind::Shelf,
                    leading: Leading::Glyph {
                        icon: IconId::Convert,
                        bg: accent,
                        fg: design::DARK_GLYPH,
                    },
                    title: "Convert".into(),
                    subtitle: Subtitle::default(),
                    status: String::new(),
                    status_accent: false,
                    trailing: Trailing::Close,
                    body: Body::Convert(ConvertBody {
                        title: "Drop to convert".into(),
                        badge: String::new(),
                        badge_color: accent,
                        icon: IconId::Convert,
                        thumb: None,
                        stacked: false,
                        tabs: Vec::new(),
                        active_tab: Tab::ConvertTo,
                        options: Vec::new(),
                        drag_target: None,
                        hint: String::new(),
                        state: ConvertState::Waiting,
                    }),
                },
            });
            return;
        }
        let count = session.probes.len();
        let Some(primary) = session.probes.get(session.primary) else {
            return;
        };
        let mut title = if count == 1 {
            primary.name.clone()
        } else {
            format!("{count} files")
        };
        // While a Ctrl+drag hovers a tile, the header previews the very name
        // the drop will write.
        if self.drag_convert {
            if let Some(action) = session.hovered {
                if let Some(format) = engines::output::target_for(action, primary.format()) {
                    title = format!(
                        "\u{2192} {}",
                        jobs::preview_name(&primary.path, action, format)
                    );
                }
            }
        }

        // Everything the card may offer lives in the options table; the card
        // only maps the visible rows into view data. A mixed batch shares no
        // job, so the card says so instead of offering a type-specific grid.
        let kind = file_kind(primary);
        let mixed = mixed_kinds(&session.probes, &session.convertible);
        let sources = source_kinds(session);
        let all_tabs: Vec<ConvertTabView> = if mixed {
            Vec::new()
        } else {
            options::tabs(kind, &self.caps, &sources)
                .into_iter()
                .map(|tab| ConvertTabView {
                    tab,
                    label: tab.label().to_string(),
                })
                .collect()
        };
        let active_tab = all_tabs
            .iter()
            .find(|tab| tab.tab == session.tab)
            .or_else(|| all_tabs.first())
            .map(|tab| tab.tab)
            .unwrap_or(Tab::ConvertTo);
        // §5: the strip is pointless with a single tab (Spreadsheet,
        // Presentation and Several files ship one; a missing engine can
        // collapse any other card too). The grid then takes the strip's
        // space — the active tab still comes from the full list above.
        let tabs: Vec<ConvertTabView> =
            if mixed || options::tab_strip_hidden(kind, &self.caps, &sources) {
                Vec::new()
            } else {
                all_tabs
            };
        let grid_options: Vec<ConvertOptionView> = if mixed {
            Vec::new()
        } else {
            options::options(kind, active_tab, &self.caps, &sources)
                .into_iter()
                .map(|def| ConvertOptionView {
                    action: def.action,
                    format: def.format.map(str::to_string),
                    icon: if def.format.is_some() {
                        None
                    } else {
                        Some(def.icon)
                    },
                    label: def.label.to_string(),
                })
                .collect()
        };

        let state = match session.phase {
            ConvertPhase::Working { .. } => {
                let action = session
                    .action
                    .unwrap_or(Action::ImageConvert(engines::ImageFormat::Png));
                let target = engines::output::target_for(action, primary.format());
                let cancelling = matches!(
                    session.phase,
                    ConvertPhase::Working {
                        cancelling: true,
                        ..
                    }
                );
                let target_badge = target
                    .map(|format| format.label().to_string())
                    .unwrap_or_default();
                let target_color = target
                    .map(jobs::badge_color)
                    .unwrap_or_else(|| self.theme.shelf_color());
                let stage = session.stage.index();
                ConvertState::Working(WorkingView {
                    percent: session.percent.round().clamp(0.0, 100.0) as u8,
                    source_badge: primary.badge().to_string(),
                    source_color: primary.badge_color(),
                    target_badge,
                    target_color,
                    stage,
                    cancelling,
                })
            }
            ConvertPhase::Done => {
                let Some(outcome) = session.outcome.as_ref() else {
                    return;
                };
                let Some(first) = outcome.created.first() else {
                    return;
                };
                let single = outcome.created.len() == 1;
                let saved = outcome.saved_percent();
                let input_bytes = if single {
                    first.input_bytes
                } else {
                    outcome.input_bytes()
                };
                let bytes = if single {
                    first.output_bytes
                } else {
                    outcome.output_bytes()
                };
                let grew = saved.is_none() && input_bytes > 0 && bytes > input_bytes;
                let name = if single {
                    if first.output_name.is_empty() {
                        if first.text.is_some() {
                            "Copied to the clipboard".to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        first.output_name.clone()
                    }
                } else {
                    format!("{} new files", outcome.created.len())
                };
                let grow_percent = if grew {
                    (((bytes - input_bytes) as f64 / input_bytes as f64) * 100.0).round() as u32
                } else {
                    0
                };
                let meta = if first.output_name.is_empty() {
                    String::new()
                } else if let Some(percent) = saved {
                    format!(
                        "{} \u{00b7} \u{2212}{percent}% smaller",
                        convert::size_label(bytes)
                    )
                } else if grew {
                    format!(
                        "{} \u{00b7} +{grow_percent}% larger",
                        convert::size_label(bytes)
                    )
                } else {
                    convert::size_label(bytes)
                };
                let has_file = outcome.created.iter().any(|c| !c.output_name.is_empty());
                let remaining = done_remaining(session, Instant::now());
                let countdown = (remaining.as_secs_f32()
                    / (design::CONVERT_COUNTDOWN_MS as f32 / 1000.0))
                    .clamp(0.0, 1.0);
                ConvertState::Done(DoneView {
                    name,
                    meta,
                    meta_accent: saved.is_some(),
                    meta_grew: grew,
                    has_file,
                    countdown,
                })
            }
            ConvertPhase::Failed => {
                let (title, detail) = session.failure.clone().unwrap_or_else(|| {
                    (
                        "Couldn't convert".into(),
                        "This file couldn't be converted.".into(),
                    )
                });
                ConvertState::Failed { title, detail }
            }
            ConvertPhase::Choosing if mixed => ConvertState::Notice {
                title: "Different file types".into(),
                detail: "Convert files of one type at a time.".into(),
            },
            ConvertPhase::Choosing => match session.notice() {
                Some((title, detail)) => ConvertState::Notice {
                    title: title.into(),
                    detail,
                },
                None => ConvertState::Grid,
            },
        };
        let is_grid = matches!(state, ConvertState::Grid);
        let drag_target = if self.drag_convert && is_grid {
            session.hovered
        } else {
            None
        };
        self.convert_accepts
            .store(is_grid && !grid_options.is_empty(), Ordering::Relaxed);
        let hint = match state {
            ConvertState::Failed { .. } => "The original file is never changed.".to_string(),
            _ => String::new(),
        };

        let accent = self.theme.shelf_color();
        self.view.main = Some(ActivityView {
            kind: ActivityKind::Shelf,
            accent,
            pill: PillView {
                icon: IconId::Folder,
                tile: TileView::colored(accent),
                label: "File shelf".into(),
                value: format!("{}", self.shelf_store.len()),
                progress: None,
                kind: PillKind::Normal,
                art: None,
            },
            flyout: FlyoutView {
                kind: ActivityKind::Shelf,
                leading: Leading::Glyph {
                    icon: IconId::Folder,
                    bg: accent,
                    fg: design::DARK_GLYPH,
                },
                title: "Convert".into(),
                subtitle: Subtitle::default(),
                status: String::new(),
                status_accent: false,
                trailing: if session.pins_card_open() {
                    Trailing::None
                } else {
                    Trailing::Close
                },
                body: Body::Convert(ConvertBody {
                    title,
                    badge: primary.badge().into(),
                    badge_color: primary.badge_color(),
                    icon: primary.icon(),
                    thumb: None,
                    stacked: count > 1,
                    tabs: if is_grid { tabs } else { Vec::new() },
                    active_tab,
                    options: if is_grid { grid_options } else { Vec::new() },
                    drag_target,
                    hint,
                    state,
                }),
            },
        });
    }

    /// Crosses the system drag threshold with a shelf tile pressed: take the
    /// selected paths (or just the pressed tile) and hand them to OLE.
    fn begin_shelf_drag(&mut self, x: f32, y: f32, button_down: bool) -> Option<Vec<String>> {
        let (element, px, py) = self.shelf_press?;
        if !button_down {
            self.shelf_press = None;
            return None;
        }
        let threshold = (unsafe { GetSystemMetrics(SM_CXDRAG) } as f32 / self.scale).max(3.0);
        if (x - px).powi(2) + (y - py).powi(2) < threshold * threshold {
            return None;
        }
        self.shelf_press = None;
        self.pressed = None;
        let Element::ShelfTile(index) = element else {
            return None;
        };
        let indices: Vec<usize> = if self.shelf_selected.contains(&index) {
            self.shelf_selected.clone()
        } else {
            vec![index]
        };
        let paths: Vec<String> = indices
            .into_iter()
            .filter_map(|i| self.shelf_store.paths().get(i).cloned())
            .collect();
        if paths.is_empty() {
            return None;
        }
        self.hide_tip();
        self.shelf_dragging = true;
        Some(paths)
    }

    fn finish_shelf_drag(&mut self, outcome: DragOutcome, paths: &[String]) {
        self.shelf_dragging = false;
        self.pressed = None;
        match outcome {
            DragOutcome::Move => {
                self.shelf_store.remove_paths(paths);
                self.shelf_selected.clear();
            }
            DragOutcome::Copy | DragOutcome::Cancel => {}
        }
        self.apply_shelf();
        // The pointer is on the drop target now, so give the card its normal
        // auto-close countdown.
        self.schedule_auto_close(Instant::now());
    }

    fn call_live(&self) -> bool {
        self.call_runtime
            .lock()
            .map(|runtime| runtime.detect.is_some())
            .unwrap_or(false)
    }

    /// Priority winner while the wheel switcher sits on Auto. A fresh clipboard
    /// copy pops the pill for two seconds, then the pill returns to its idle
    /// look; the Clipboard activity never outranks live media or a timer.
    fn auto_winner(&self, call_live: bool, now: Instant) -> Option<ActivityKind> {
        if call_live {
            Some(ActivityKind::Call)
        } else if self.timer.active() && self.timer.finished {
            Some(ActivityKind::Timer)
        } else if self.media_snapshot.is_some() {
            Some(ActivityKind::Music)
        } else if self.timer.active() {
            Some(ActivityKind::Timer)
        } else if self.clip_pop_until.is_some_and(|until| until > now) {
            Some(ActivityKind::Clipboard)
        } else if !self.shelf_store.is_empty() {
            Some(ActivityKind::Shelf)
        } else {
            None
        }
    }

    /// Re-resolve the pill after any state change. A manual pin survives until
    /// another activity takes over (an incoming call, a new winner) or the
    /// pinned activity ends.
    fn refresh(&mut self, now: Instant) {
        if self.convert.is_some() {
            // The converter owns the card until it is closed or rails away.
            self.show_shelf();
            return;
        }
        let call_live = self.call_live();
        let auto = self.auto_winner(call_live, now);
        if let Some(pin) = self.pin {
            if (call_live && pin != ActivityKind::Call)
                || (auto != self.last_auto && auto != Some(pin))
            {
                self.pin = None;
            }
        }
        self.last_auto = auto;
        self.show(self.pin.or(auto), now);
    }

    fn show(&mut self, kind: Option<ActivityKind>, now: Instant) {
        match kind {
            Some(ActivityKind::Music) => self.show_music(),
            Some(ActivityKind::Call) => self.show_call(),
            Some(ActivityKind::Timer) => self.show_timer(now),
            Some(ActivityKind::Shelf) => self.show_shelf(),
            Some(ActivityKind::Clipboard) => self.show_clipboard(),
            _ => self.view.main = Some(ViewModel::idle(self.theme.shelf_color())),
        }
    }

    /// Wheel over the pill cycles Auto → Music → Call → Shelf → Timer →
    /// Clipboard. An open card belongs to the previous activity, so it closes
    /// immediately.
    const SHOW_ORDER: [Option<ActivityKind>; 6] = [
        None,
        Some(ActivityKind::Music),
        Some(ActivityKind::Call),
        Some(ActivityKind::Shelf),
        Some(ActivityKind::Timer),
        Some(ActivityKind::Clipboard),
    ];

    /// A card under the pointer owns the wheel. Inside the shelf grid the
    /// notches scroll files; anywhere else on an open card they do nothing, so
    /// the activity never changes underneath the user. The wheel only cycles
    /// activities while the card is closed.
    fn wheel(&mut self, delta: i32, x: f32, y: f32) -> bool {
        let shelf_card = matches!(
            self.view.main.as_ref().map(|view| view.kind),
            Some(ActivityKind::Shelf)
        ) && self.convert.is_none();
        let clip_card = matches!(
            self.view.main.as_ref().map(|view| view.kind),
            Some(ActivityKind::Clipboard)
        );
        let over_grid = shelf_card
            && self.flyout_open
            && self.flyout.contains(x, y)
            && self.shelf_viewport().contains(x, y);
        let over_clip_list = clip_card
            && self.flyout_open
            && self.flyout.contains(x, y)
            && self.clip_viewport().contains(x, y);
        let max_scroll = if clip_card {
            crate::view::clip_scroll(&self.clip_strides(), self.clip_scroll).max_scroll
        } else {
            crate::view::shelf_scroll(self.shelf_store.len(), self.shelf_scroll).max_scroll
        };
        match crate::view::wheel_action(
            self.flyout_open,
            shelf_card || clip_card,
            over_grid || over_clip_list,
            max_scroll > 0.0,
        ) {
            WheelAction::ScrollList => {
                self.hide_tip();
                let steps = (delta as f32 / 120.0).clamp(-3.0, 3.0);
                if clip_card {
                    self.clip_scroll = (self.clip_scroll - steps * design::CLIP_SCROLL_STEP)
                        .clamp(0.0, max_scroll);
                    self.show_clipboard();
                } else {
                    let next = self.shelf_scroll - steps * design::SHELF_SCROLL_STEP;
                    self.set_shelf_scroll(next);
                }
                true
            }
            WheelAction::Cycle => {
                self.wheel_accum += delta;
                let mut changed = false;
                while self.wheel_accum >= 120 {
                    self.wheel_accum -= 120;
                    self.cycle_activity(-1);
                    changed = true;
                }
                while self.wheel_accum <= -120 {
                    self.wheel_accum += 120;
                    self.cycle_activity(1);
                    changed = true;
                }
                changed
            }
            WheelAction::Ignore => {
                self.wheel_accum = 0;
                false
            }
        }
    }

    fn cycle_activity(&mut self, step: isize) {
        let idx = Self::SHOW_ORDER
            .iter()
            .position(|pin| *pin == self.pin)
            .unwrap_or(0);
        let next = (idx as isize + step).rem_euclid(Self::SHOW_ORDER.len() as isize) as usize;
        self.pin = Self::SHOW_ORDER[next];
        crate::log::log(&format!("activity.show: {:?}", self.pin));
        if self.flyout_open {
            self.begin_close_flyout(Instant::now());
        }
        self.refresh(Instant::now());
        self.sync_tick_timer();
    }

    /// Grid viewport in card DIP coordinates. Must match `draw_shelf_card`.
    fn shelf_viewport(&self) -> RectD {
        let content = crate::view::content_rect(self.flyout);
        RectD::new(
            content.x,
            content.y + design::SHELF_TOOLBAR_H + design::SHELF_TOOLBAR_GAP,
            content.w - design::SHELF_SCROLL_W - design::SHELF_SCROLL_GAP,
            design::SHELF_VIEW_H,
        )
    }

    /// Visible scrollbar track in card DIP coordinates.
    fn shelf_track(&self) -> RectD {
        let viewport = self.shelf_viewport();
        let content = crate::view::content_rect(self.flyout);
        RectD::new(
            content.x + content.w - design::SHELF_SCROLL_W,
            viewport.y,
            design::SHELF_SCROLL_W,
            viewport.h,
        )
    }

    fn shelf_thumb(&self, track: RectD, content_h: f32, scroll: f32, max_scroll: f32) -> RectD {
        let thumb_h = (track.h * track.h / content_h.max(1.0))
            .max(design::SHELF_SCROLL_MIN)
            .min(track.h);
        let t = if max_scroll > 0.0 {
            (scroll / max_scroll).clamp(0.0, 1.0)
        } else {
            0.0
        };
        RectD::new(track.x, track.y + (track.h - thumb_h) * t, track.w, thumb_h)
    }

    /// Clamps and applies a scroll offset. Returns whether the view changed.
    fn set_shelf_scroll(&mut self, scroll: f32) -> bool {
        let next = crate::view::shelf_scroll(self.shelf_store.len(), scroll).scroll;
        if (next - self.shelf_scroll).abs() < 0.01 {
            return false;
        }
        self.shelf_scroll = next;
        self.hide_tip();
        self.show_shelf();
        true
    }

    /// Press on the scrollbar: grabbing the thumb keeps the pointer's offset
    /// inside it; pressing the empty track centres the thumb on the pointer.
    fn begin_scrollbar_drag(&mut self, y: f32) {
        let view = crate::view::shelf_scroll(self.shelf_store.len(), self.shelf_scroll);
        if view.max_scroll <= 0.0 {
            return;
        }
        let track = self.shelf_track();
        let thumb = self.shelf_thumb(track, view.content_h, view.scroll, view.max_scroll);
        let grab = if y >= thumb.y && y <= thumb.bottom() {
            y - thumb.y
        } else {
            thumb.h * 0.5
        };
        self.shelf_scroll_drag = Some(grab);
        self.drag_scrollbar(y, grab);
    }

    /// Maps the pointer back to a scroll offset while the thumb is held.
    fn drag_scrollbar(&mut self, y: f32, grab: f32) -> bool {
        let view = crate::view::shelf_scroll(self.shelf_store.len(), self.shelf_scroll);
        if view.max_scroll <= 0.0 {
            return false;
        }
        let track = self.shelf_track();
        let thumb = self.shelf_thumb(track, view.content_h, view.scroll, view.max_scroll);
        let travel = (track.h - thumb.h).max(1.0);
        let t = ((y - grab - track.y) / travel).clamp(0.0, 1.0);
        self.set_shelf_scroll(t * view.max_scroll)
    }

    /// Rows the history list currently shows, after the active filter. The
    /// wheel and the scrollbar drag both scroll this same set.
    fn clip_len(&self) -> usize {
        self.clip.as_ref().map_or(0, |clip| {
            clip.store
                .items()
                .iter()
                .filter(|item| self.clip_filter.matches(item))
                .count()
        })
    }

    /// History list viewport in window DIP coordinates. Must match the list
    /// rect in `draw_clipboard_card`; `view::clipboard_viewport` takes the
    /// flyout origin so the two coordinate spaces cannot drift apart.
    fn clip_viewport(&self) -> RectD {
        crate::view::clipboard_viewport(self.flyout)
    }

    /// Visible scrollbar track for the history list, beside the list it scrolls.
    fn clip_track(&self) -> RectD {
        let list = self.clip_viewport();
        RectD::new(
            list.right() + design::CLIP_SCROLL_GAP,
            list.y,
            design::CLIP_SCROLL_W,
            list.h,
        )
    }

    /// Thumb geometry, matching the one `draw_clipboard_card` paints from the
    /// same four numbers.
    fn clip_thumb(&self, track: RectD, content_h: f32, scroll: f32, max_scroll: f32) -> RectD {
        let thumb_h =
            (track.h * track.h / content_h.max(1.0)).clamp(design::CLIP_SCROLL_MIN, track.h);
        let t = if max_scroll > 0.0 {
            (scroll / max_scroll).clamp(0.0, 1.0)
        } else {
            0.0
        };
        RectD::new(track.x, track.y + (track.h - thumb_h) * t, track.w, thumb_h)
    }

    /// Clamps and applies the history scroll offset. Returns whether the view
    /// changed.
    fn set_clip_scroll(&mut self, scroll: f32) -> bool {
        let next = crate::view::clip_scroll(&self.clip_strides(), scroll).scroll;
        if (next - self.clip_scroll).abs() < 0.01 {
            return false;
        }
        self.clip_scroll = next;
        self.hide_tip();
        self.show_clipboard();
        true
    }

    /// Press on the history scrollbar: grabbing the thumb keeps the pointer's
    /// offset inside it; pressing the empty track centres the thumb on it.
    fn begin_clip_scrollbar_drag(&mut self, y: f32) {
        let view = crate::view::clip_scroll(&self.clip_strides(), self.clip_scroll);
        if view.max_scroll <= 0.0 {
            return;
        }
        let track = self.clip_track();
        let thumb = self.clip_thumb(track, view.content_h, view.scroll, view.max_scroll);
        let grab = if y >= thumb.y && y <= thumb.bottom() {
            y - thumb.y
        } else {
            thumb.h * 0.5
        };
        self.clip_scroll_drag = Some(grab);
        self.drag_clip_scrollbar(y, grab);
    }

    /// Maps the pointer back to a history scroll offset while the thumb is held.
    fn drag_clip_scrollbar(&mut self, y: f32, grab: f32) -> bool {
        let view = crate::view::clip_scroll(&self.clip_strides(), self.clip_scroll);
        if view.max_scroll <= 0.0 {
            return false;
        }
        let track = self.clip_track();
        let thumb = self.clip_thumb(track, view.content_h, view.scroll, view.max_scroll);
        let travel = (track.h - thumb.h).max(1.0);
        let t = ((y - grab - track.y) / travel).clamp(0.0, 1.0);
        self.set_clip_scroll(t * view.max_scroll)
    }

    /// Queue thumbnails for the visible image tiles only. Cached failures are
    /// kept so a hostile or broken file is never decoded in a loop.
    fn ensure_thumbs(&mut self) {
        let view = crate::view::shelf_scroll(self.shelf_store.len(), self.shelf_scroll);
        let cols = design::SHELF_COLS;
        let mut visible: Vec<String> = Vec::new();
        for row in view.first_row..view.first_row + view.rows {
            for col in 0..cols {
                let index = row * cols + col;
                let Some(path) = self.shelf_store.paths().get(index) else {
                    break;
                };
                if crate::view::is_image(path) {
                    visible.push(path.clone());
                }
            }
        }
        self.queue_thumbs(visible);
    }

    /// Queue thumbnails for the visible clipboard rows only: the stored image
    /// blob thumbnail (generated at capture, spec 7) or a single image file
    /// through the same worker. The cache is bounded by the 30-item history.
    fn ensure_clip_thumbs(&mut self) {
        let Some(clip) = self.clip.as_ref() else {
            return;
        };
        let filter = self.clip_filter;
        let items = clip.store.items();
        let filtered: Vec<_> = items.iter().filter(|item| filter.matches(item)).collect();
        let strides: Vec<f32> = filtered
            .iter()
            .map(|item| crate::view::clip_row_kind(item).stride())
            .collect();
        let view = crate::view::clip_scroll(&strides, self.clip_scroll);
        let mut visible: Vec<String> = Vec::new();
        for index in view.first_row..view.first_row + view.rows {
            let Some(item) = filtered.get(index) else {
                break;
            };
            match crate::view::clip_row_kind(item) {
                crate::view::ClipRowKind::Image { .. } => visible.push(
                    crate::clipboard::store::thumb_path(&item.digest)
                        .to_string_lossy()
                        .into_owned(),
                ),
                crate::view::ClipRowKind::Files
                    if item.paths.len() == 1
                        && item.paths.first().is_some_and(|p| crate::view::is_image(p)) =>
                {
                    visible.push(item.paths[0].clone());
                }
                _ => {}
            }
        }
        self.queue_thumbs(visible);
    }

    /// Shared by the shelf and clipboard queues: one request per path, with
    /// cached failures remembered so nothing decodes in a loop.
    fn queue_thumbs(&mut self, visible: Vec<String>) {
        for path in visible {
            queue_thumb(
                &self.thumb_worker,
                &mut self.thumbs,
                &path,
                crate::thumb::MAX_THUMB,
            );
        }
    }

    /// Queue the 384 px decode behind the hover preview (spec 7). The cache
    /// holds only the file the preview is about: previews are transient, so a
    /// long session never piles up decoded pictures.
    fn ensure_clip_preview(&mut self) {
        let Some(id) = self
            .clip_preview
            .or(self.clip_preview_arm.map(|(id, _)| id))
        else {
            self.clip_preview_art.clear();
            return;
        };
        let path = self
            .clip
            .as_ref()
            .and_then(|clip| clip.store.get(id))
            .and_then(crate::view::clip_preview_path);
        let Some(path) = path else {
            self.clip_preview_art.clear();
            return;
        };
        self.clip_preview_art.retain(|known, _| *known == path);
        queue_thumb(
            &self.thumb_worker,
            &mut self.clip_preview_art,
            &path,
            crate::thumb::PREVIEW_PX,
        );
    }

    /// Drops finished thumbnails into the cache. Returns whether a repaint is
    /// needed while the shelf or clipboard card is on screen.
    fn drain_thumbs(&mut self) -> bool {
        let results = self.thumb_worker.drain();
        if results.is_empty() {
            return false;
        }
        let mut changed = false;
        for result in &results {
            changed |= fill_thumb(&mut self.thumbs, result);
            changed |= fill_thumb(&mut self.clip_preview_art, result);
        }
        changed
            && self.flyout_open
            && matches!(
                self.view.main.as_ref().map(|view| view.kind),
                Some(ActivityKind::Shelf | ActivityKind::Clipboard)
            )
    }

    fn show_shelf(&mut self) {
        if self.convert.is_some() {
            self.show_convert();
            return;
        }
        let scroll = crate::view::shelf_scroll(self.shelf_store.len(), self.shelf_scroll);
        self.shelf_scroll = scroll.scroll;
        self.ensure_thumbs();
        let thumbs = &self.thumbs;
        let files = crate::view::shelf_tiles(
            self.shelf_store.paths(),
            &self.shelf_selected,
            scroll.scroll,
            &|path| thumbs.get(path).and_then(|entry| entry.art.clone()),
        );
        let count = self.shelf_store.len();
        let total = crate::view::fmt_size(crate::view::total_size(self.shelf_store.paths()));
        let accent = self.theme.shelf_color();
        let selected = self.shelf_selected.len();
        let convertible = self
            .shelf_convert_paths()
            .iter()
            .filter(|path| crate::view::is_convertible(path))
            .count();
        self.view.main = Some(ActivityView {
            kind: ActivityKind::Shelf,
            accent,
            pill: PillView {
                icon: IconId::Folder,
                tile: TileView::colored(accent),
                label: "File shelf".into(),
                value: format!("{count}"),
                progress: None,
                kind: PillKind::Normal,
                art: None,
            },
            flyout: FlyoutView {
                kind: ActivityKind::Shelf,
                leading: Leading::Glyph {
                    icon: IconId::Folder,
                    bg: accent,
                    fg: design::DARK_GLYPH,
                },
                title: "Shelf".into(),
                subtitle: Subtitle {
                    prefix: format!(
                        "{count} item{} \u{00b7} {total}",
                        if count == 1 { "" } else { "s" }
                    ),
                    accent: None,
                },
                status: String::new(),
                status_accent: false,
                trailing: Trailing::None,
                body: Body::Shelf(crate::view::ShelfBody {
                    files,
                    total: count,
                    selected,
                    convertible,
                    scroll: scroll.scroll,
                    max_scroll: scroll.max_scroll,
                    content_h: scroll.content_h,
                    overflow: self.shelf_overflow,
                    converting: false,
                    rubber: self.shelf_rubber,
                }),
            },
        });
    }

    /// Phase 2 shell: live history counts drive the pill; the list UI lands
    /// in the next phase. Opening the card runs the retention sweep too.
    fn show_clipboard(&mut self) {
        let accent = design::RAIL_CLIP;
        if let Some(clip) = self.clip.as_mut() {
            clip.sweep(crate::clipboard::now_secs());
        }
        let count = self.clip.as_ref().map_or(0, |clip| clip.store.len());
        let pinned = self
            .clip
            .as_ref()
            .map_or(0, |clip| clip.store.pinned_count());
        let paused = self.clip.as_ref().is_some_and(ClipboardRuntime::is_paused);
        let pop = self
            .clip_pop_until
            .filter(|until| *until > Instant::now())
            .map(|_| self.clip_pop_count.max(1));
        let filter = self.clip_filter;
        let now = crate::clipboard::now_secs();
        // Queue thumbnail work for the visible rows first, then resolve what
        // the worker already produced (spec 7 thumbnails; never decoded here).
        self.ensure_clip_thumbs();
        // The overlay belongs to a row that is still under the pointer: if the
        // hover moved on (or the row was evicted), drop it rather than freeze a
        // stale preview over the list.
        if self
            .clip_preview
            .is_some_and(|id| self.hover != Some(Element::ClipRow(id)))
        {
            self.dismiss_clip_preview();
        }
        // Start (or restart) the 384 px decode so the picture is usually ready
        // the moment the 450 ms delay lands (spec 7).
        self.ensure_clip_preview();
        let thumbs = &self.thumbs;
        let (rows, scroll) = self.clip.as_ref().map_or_else(
            || (Vec::new(), crate::view::clip_scroll(&[], 0.0)),
            |clip| {
                let scroll = crate::view::clip_scroll(&self.clip_strides(), self.clip_scroll);
                let rows = crate::view::clip_rows(
                    clip.store.items(),
                    filter,
                    scroll.scroll,
                    now,
                    &|path| thumbs.get(path).and_then(|entry| entry.art.clone()),
                );
                (rows, scroll)
            },
        );
        let preview = self.clip_preview.and_then(|id| {
            let item = self.clip.as_ref()?.store.get(id)?;
            let row = rows.iter().find(|row| row.id == id)?;
            let path = crate::view::clip_preview_path(item)?;
            Some(crate::view::ClipPreviewView {
                id,
                art: self
                    .clip_preview_art
                    .get(&path)
                    .and_then(|entry| entry.art.clone()),
                body: crate::view::clip_preview_body(item),
                source: row.source.clone(),
                image: row.image,
                mono: row.mono,
            })
        });
        let body = crate::view::ClipboardBody {
            total: count,
            pinned,
            paused,
            filter,
            count_text: crate::view::clip_count_text(count, pinned, filter),
            rows,
            scroll: scroll.scroll,
            max_scroll: scroll.max_scroll,
            content_h: scroll.content_h,
            active_row: self.clip_active,
            clear_confirm: self.clip_clear_at.map(|_| count.saturating_sub(pinned)),
            preview,
        };
        self.view.main = Some(ActivityView {
            kind: ActivityKind::Clipboard,
            accent,
            pill: crate::view::clipboard_pill(count, pop, paused),
            flyout: FlyoutView {
                kind: ActivityKind::Clipboard,
                leading: Leading::Glyph {
                    icon: IconId::Clipboard,
                    bg: accent,
                    fg: design::DARK_GLYPH,
                },
                title: "Clipboard".into(),
                subtitle: Subtitle::default(),
                status: String::new(),
                status_accent: false,
                trailing: Trailing::None,
                body: Body::Clipboard(body),
            },
        });
    }

    /// Drain worker results on the UI thread. Storage, retention and eviction
    /// already ran on the workers; this only mutates the store model and pops
    /// the pill.
    fn apply_clipboard(&mut self, now: Instant) {
        let mut stored = 0u32;
        {
            let Some(clip) = self.clip.as_mut() else {
                return;
            };
            let wall = crate::clipboard::now_secs();
            for result in clip.drain() {
                if clip.apply(result.item, wall).stored {
                    stored += 1;
                }
            }
        }
        if stored > 0 {
            self.clip_pop_until = Some(now + Duration::from_millis(design::CLIP_POP_MS as u64));
            self.clip_pop_count = stored;
            // SAFETY: the one-shot pop timer belongs to this window.
            unsafe {
                SetTimer(
                    Some(self.hwnd),
                    CLIP_POP_TIMER_ID,
                    design::CLIP_POP_MS,
                    None,
                );
            }
        }
        self.refresh(now);
    }

    fn sweep_clipboard(&mut self) {
        let removed = self
            .clip
            .as_mut()
            .map_or(0, |clip| clip.sweep(crate::clipboard::now_secs()));
        if removed > 0 {
            self.refresh(Instant::now());
        }
    }

    fn end_clip_pop(&mut self) {
        // SAFETY: the one-shot pop timer fired.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), CLIP_POP_TIMER_ID);
        }
        self.clip_pop_until = None;
        self.clip_pop_count = 0;
        self.refresh(Instant::now());
    }

    /// Arms the two-step Clear (spec 3.3): the toolbar swaps the trash for a
    /// red ✓ and a ✕, and the confirm auto-cancels after four seconds.
    fn arm_clip_clear(&mut self, hwnd: HWND) {
        if self.clip_clear_at.is_some() {
            return;
        }
        self.clip_clear_at = Some(Instant::now());
        // SAFETY: the one-shot confirm timer belongs to this window.
        unsafe {
            SetTimer(
                Some(hwnd),
                CLIP_CONFIRM_TIMER_ID,
                design::CLIP_CLEAR_CONFIRM_MS,
                None,
            );
        }
    }

    fn disarm_clip_clear(&mut self) {
        if self.clip_clear_at.take().is_none() {
            return;
        }
        // SAFETY: the timer id belongs to this window; KillTimer tolerates an
        // id that was never armed.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), CLIP_CONFIRM_TIMER_ID);
        }
    }

    /// The four seconds ran out: drop back to the plain trash button. The card
    /// may have moved to another activity in the meantime, in which case there
    /// is no clipboard view to rebuild.
    fn expire_clip_clear(&mut self) {
        if self.clip_clear_at.is_none() {
            return;
        }
        self.disarm_clip_clear();
        if self.view.main.as_ref().map(|view| view.kind) == Some(ActivityKind::Clipboard) {
            self.show_clipboard();
        }
    }

    /// Puts history row `id` back on the system clipboard (spec 3.2). With
    /// `paste`, the card closes first and a Ctrl+V is queued for the window that
    /// was in front at this moment.
    fn copy_back(&mut self, id: u64, paste: bool) -> bool {
        let copied = self.clip.as_ref().is_some_and(|clip| {
            let Some(item) = clip.store.get(id) else {
                return false;
            };
            match clip.copy_back(item, self.hwnd) {
                Ok(()) => true,
                Err(error) => {
                    crate::log::log(&format!("clip.copy_back failed: {error}"));
                    false
                }
            }
        });
        if !copied {
            return false;
        }
        if paste {
            // SAFETY: read-only query of the foreground window; the island is
            // WS_EX_NOACTIVATE, so this is still the user's own window.
            let target = unsafe { GetForegroundWindow() };
            self.pending_paste = Some(target);
            self.begin_close_flyout(Instant::now());
        }
        true
    }

    /// Sends the queued Ctrl+V once the card has closed, and only if the window
    /// the row was copied for is still in front — otherwise the keystroke would
    /// land in whatever the user switched to, so it is dropped and the copy
    /// alone stands (spec 3.2, 5.7).
    fn flush_pending_paste(&mut self) {
        let Some(target) = self.pending_paste.take() else {
            return;
        };
        // SAFETY: read-only query.
        let foreground = unsafe { GetForegroundWindow() };
        if !paste_is_due(target, foreground) {
            crate::log::log("clip.paste skipped: the target window lost focus");
            return;
        }
        if let Err(error) = send_ctrl_v() {
            crate::log::log(&format!("clip.paste failed: {error}"));
        }
    }

    fn show_music(&mut self) {
        let Some(snapshot) = self.media_snapshot.clone() else {
            self.view.main = Some(self.placeholder(ActivityKind::Music));
            return;
        };
        let now = Instant::now();
        let position = snapshot.effective_position(now);
        let fraction = snapshot.fraction(now);
        let accent = snapshot.accent;
        let controls = snapshot.controls;
        let pill = PillView {
            icon: IconId::Music,
            tile: TileView::colored(accent),
            label: snapshot.title.clone(),
            value: snapshot.artist.clone(),
            progress: Some(fraction),
            kind: PillKind::Normal,
            art: snapshot.art.clone(),
        };
        let flyout = FlyoutView {
            kind: ActivityKind::Music,
            leading: Leading::Art {
                art: snapshot.art.clone(),
                accent,
            },
            title: snapshot.title.clone(),
            subtitle: Subtitle {
                prefix: snapshot.artist.clone(),
                accent: Some(snapshot.source_app.clone()),
            },
            status: String::new(),
            status_accent: false,
            trailing: Trailing::None,
            body: Body::Music(MusicBody {
                title: snapshot.title.clone(),
                artist: snapshot.artist.clone(),
                source: snapshot.source_app.clone(),
                art: snapshot.art.clone(),
                elapsed: crate::view::fmt_clock(position),
                remaining: crate::view::fmt_remaining(snapshot.duration - position),
                fraction,
                playing: snapshot.playing,
                shuffle: snapshot.shuffle,
                repeat: snapshot.repeat,
                controls,
            }),
        };
        self.view.main = Some(ActivityView {
            kind: ActivityKind::Music,
            accent,
            pill,
            flyout,
        });
    }

    fn show_call(&mut self) {
        let Ok(runtime) = self.call_runtime.lock() else {
            crate::log::log("window.call: runtime mutex poisoned");
            self.view.main = Some(self.placeholder(ActivityKind::Call));
            return;
        };
        let Some(detect) = runtime.detect.as_ref() else {
            drop(runtime);
            self.view.main = Some(self.placeholder(ActivityKind::Call));
            return;
        };
        let accent = design::CALL_GREEN;
        let now = Instant::now();
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
            art: None,
        };
        let flyout = FlyoutView {
            kind: ActivityKind::Call,
            leading: Leading::Glyph {
                icon: IconId::Phone,
                bg: accent,
                fg: design::DARK_GLYPH,
            },
            title: "Call".into(),
            subtitle: Subtitle::default(),
            status: "Active call".into(),
            status_accent: true,
            trailing: Trailing::None,
            body: Body::Call(crate::view::CallBody {
                device: runtime.device.clone(),
                app_name: detect.app_name.clone(),
                elapsed: crate::view::fmt_clock(duration),
                live: true,
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

    fn show_timer(&mut self, now: Instant) {
        if !self.timer.active() {
            self.view.main = Some(self.placeholder(ActivityKind::Timer));
            return;
        }
        let accent = design::TIMER_AMBER;
        let remaining = self.timer.remaining(now).as_secs_f64();
        let status = if self.timer.finished {
            "Done"
        } else if self.timer.running() {
            "Running"
        } else if self.timer.armed() {
            "Ready"
        } else {
            "Paused"
        };
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
                art: None,
            },
            flyout: FlyoutView {
                kind: ActivityKind::Timer,
                leading: Leading::Glyph {
                    icon: IconId::Clock,
                    bg: accent,
                    fg: design::DARK_GLYPH,
                },
                title: "Timer".into(),
                subtitle: Subtitle::default(),
                status: status.into(),
                status_accent: true,
                trailing: Trailing::None,
                body: Body::Timer(crate::view::TimerBody {
                    remaining: crate::view::fmt_clock(remaining),
                    fraction: self.timer.fraction(now),
                    running: self.timer.running(),
                    armed: self.timer.armed(),
                    finished: self.timer.finished,
                }),
            },
        });
    }

    /// Honest empty states for an activity pinned before it has live content.
    fn placeholder(&self, kind: ActivityKind) -> ActivityView {
        match kind {
            ActivityKind::Music => ActivityView {
                kind,
                accent: design::MUSIC_DEFAULT,
                pill: PillView {
                    icon: IconId::Music,
                    tile: TileView::colored(design::MUSIC_DEFAULT),
                    label: "Music".into(),
                    value: "Nothing playing".into(),
                    progress: None,
                    kind: PillKind::Normal,
                    art: None,
                },
                flyout: FlyoutView {
                    kind: ActivityKind::Music,
                    leading: Leading::Art {
                        art: None,
                        accent: design::MUSIC_DEFAULT,
                    },
                    title: "Nothing playing right now".into(),
                    subtitle: Subtitle::default(),
                    status: String::new(),
                    status_accent: false,
                    trailing: Trailing::None,
                    body: Body::Music(MusicBody {
                        title: "Nothing playing right now".into(),
                        artist: String::new(),
                        source: String::new(),
                        art: None,
                        elapsed: "--:--".into(),
                        remaining: "--:--".into(),
                        fraction: 0.0,
                        playing: false,
                        shuffle: false,
                        repeat: crate::view::RepeatMode::None,
                        controls: crate::view::MediaControls::default(),
                    }),
                },
            },
            ActivityKind::Call => ActivityView {
                kind,
                accent: design::CALL_GREEN,
                pill: PillView {
                    icon: IconId::Phone,
                    tile: TileView::colored(design::CALL_GREEN),
                    label: "Call".into(),
                    value: "No call".into(),
                    progress: None,
                    kind: PillKind::Normal,
                    art: None,
                },
                flyout: FlyoutView {
                    kind: ActivityKind::Call,
                    leading: Leading::Glyph {
                        icon: IconId::Phone,
                        bg: design::CALL_GREEN,
                        fg: design::DARK_GLYPH,
                    },
                    title: "Call".into(),
                    subtitle: Subtitle::default(),
                    status: "No call".into(),
                    status_accent: true,
                    trailing: Trailing::None,
                    body: Body::Call(crate::view::CallBody {
                        device: "No active call".into(),
                        app_name: "call".into(),
                        elapsed: String::new(),
                        live: false,
                        mic_muted: false,
                        output_muted: false,
                    }),
                },
            },
            ActivityKind::Timer => ActivityView {
                kind,
                accent: design::TIMER_AMBER,
                pill: PillView {
                    icon: IconId::Clock,
                    tile: TileView::colored(design::TIMER_AMBER),
                    label: "Timer".into(),
                    value: "No timer".into(),
                    progress: None,
                    kind: PillKind::Normal,
                    art: None,
                },
                flyout: FlyoutView {
                    kind: ActivityKind::Timer,
                    leading: Leading::Glyph {
                        icon: IconId::Clock,
                        bg: design::TIMER_AMBER,
                        fg: design::DARK_GLYPH,
                    },
                    title: "Timer".into(),
                    subtitle: Subtitle::default(),
                    status: "No timer".into(),
                    status_accent: true,
                    trailing: Trailing::None,
                    body: Body::Timer(crate::view::TimerBody {
                        remaining: "00:00".into(),
                        fraction: 0.0,
                        running: false,
                        armed: false,
                        finished: false,
                    }),
                },
            },
            _ => ViewModel::idle(self.theme.shelf_color()),
        }
    }

    fn shelf_drag_pill(&self, convert: bool) -> PillView {
        PillView {
            icon: if convert {
                IconId::Convert
            } else {
                IconId::Folder
            },
            tile: TileView::colored(self.theme.shelf_color()),
            label: if convert {
                "Convert on drop".into()
            } else {
                "Drop to shelve".into()
            },
            value: String::new(),
            progress: None,
            kind: PillKind::DragOver,
            art: None,
        }
    }

    fn toggle_timer(&mut self) {
        let now = Instant::now();
        if self.timer.active() {
            if self.timer.finished {
                self.timer.stop();
                self.refresh(now);
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

    /// True while the on-screen pill (or its card) carries a travelling wave:
    /// music that is playing, or a countdown that is running.
    fn wave_animating(&self) -> bool {
        match self.view.main.as_ref().map(|view| view.kind) {
            Some(ActivityKind::Music) => self
                .media_snapshot
                .as_ref()
                .map(|snapshot| snapshot.playing && snapshot.duration > 0.0)
                .unwrap_or(false),
            Some(ActivityKind::Timer) => self.timer.running(),
            _ => false,
        }
    }

    /// Rebuild the active view from its extrapolated position and travel the
    /// wave; `now` keeps the countdown and media clock exact between events.
    fn advance_wave(&mut self, now: Instant) {
        let dt = now
            .saturating_duration_since(self.wave_tick_at)
            .as_secs_f32()
            .min(0.5);
        self.wave_tick_at = now;
        // One wavelength every 1.2 s; the eye reads it as a running wave.
        self.wave_phase =
            (self.wave_phase + dt * design::WAVE_LEN / 1.2).rem_euclid(design::WAVE_LEN);
        self.refresh(now);
    }

    fn sync_tick_timer(&self) {
        let interval = if self.wave_animating() {
            // The pill itself now animates, so a resting pill needs the same
            // smooth cadence as an open card.
            80
        } else if self.timer.running() {
            1000
        } else {
            0
        };
        // SAFETY: the timer id belongs to this window; KillTimer tolerates a
        // timer that was never created.
        unsafe {
            if interval == 0 {
                let _ = KillTimer(Some(self.hwnd), 1);
            } else {
                SetTimer(Some(self.hwnd), 1, interval, None);
            }
        }
    }

    /// Element under a window-local DIP point. The flyout wins where it
    /// overlaps the pill; outside both surfaces the halo is click-through.
    /// While the card is opening/closing its drawn position is transformed, so
    /// elements only accept clicks once the tween has landed.
    fn hit_at(&self, x: f32, y: f32) -> Option<Element> {
        if self.flyout_open && self.flyout_phase == FlyoutPhase::Open && self.flyout.contains(x, y)
        {
            let hit = self.hit_map.hit(x, y);
            if self.content_phase != ContentPhase::Idle {
                // Content is mid-swap; its hit rects belong to the frame that
                // is fading out. Only the rail stays interactive.
                return hit.filter(|element| matches!(element, Element::Rail(_)));
            }
            return hit;
        }
        if self.view.pill().is_some() && self.pill.contains(x, y) {
            return Some(Element::Pill);
        }
        None
    }

    /// Element under a physical screen point, even while the card is still
    /// opening. The drop target reports screen pixels; a drag can arrive
    /// faster than the open tween, and a drop must never be lost to that.
    fn hit_pointer_at_screen(&self, px: i32, py: i32) -> Option<Element> {
        let x = (px - self.origin.0) as f32 / self.scale;
        let y = (py - self.origin.1) as f32 / self.scale;
        if self.flyout_open
            && self.flyout_phase != FlyoutPhase::Closed
            && self.flyout.contains(x, y)
        {
            return self.hit_map.hit(x, y);
        }
        if self.view.pill().is_some() && self.pill.contains(x, y) {
            return Some(Element::Pill);
        }
        None
    }

    /// Put back a card that only existed because a Ctrl+drag was passing over
    /// the island. A drag that already chose an option keeps the card.
    fn retract_drag_convert(&mut self) {
        let choosing = self
            .convert
            .as_ref()
            .is_some_and(|session| session.phase == ConvertPhase::Choosing);
        if choosing {
            self.end_convert();
            self.begin_close_flyout(Instant::now());
        }
    }

    /// Update the hovered element and start the 160 ms fill crossfade when it
    /// changes. Returns whether a repaint is needed.
    fn set_hover(&mut self, hover: Option<Element>, now: Instant) -> bool {
        if self.hover == hover {
            return false;
        }
        self.hover = hover;
        let target = if matches!(hover, Some(Element::Pill)) {
            1.0
        } else {
            0.0
        };
        self.hover_tween = Some(Tween::new(
            now,
            design::HOVER_MS,
            self.hover_mix,
            target,
            design::EASE_CONTENT,
        ));
        self.sync_hover_timer();
        self.schedule_tip(now);
        // The row preview follows the pointer with the same rule as the tip:
        // a new hover target restarts the 450 ms (spec 7).
        self.sync_clip_preview(now);
        if self.convert.is_some() {
            let action = match hover {
                Some(Element::ConvertOption(action)) => Some(action),
                _ => None,
            };
            let changed = match self.convert.as_mut() {
                Some(session) if session.hovered != action => {
                    session.hovered = action;
                    true
                }
                _ => false,
            };
            if changed {
                self.show_convert();
            }
        }
        true
    }

    /// Advance the hover crossfade. Returns whether the frame changed.
    fn advance_hover(&mut self, now: Instant) -> bool {
        let Some(tween) = self.hover_tween else {
            return false;
        };
        self.hover_mix = tween.value(now);
        if tween.done(now) {
            self.hover_mix = tween.to;
            self.hover_tween = None;
            self.sync_hover_timer();
        }
        true
    }

    /// 16 ms only while the hover fill is moving; idle keeps the timer off.
    fn sync_hover_timer(&self) {
        // SAFETY: the timer id belongs to this window.
        unsafe {
            if self.hover_tween.is_some() {
                SetTimer(Some(self.hwnd), HOVER_TIMER_ID, 16, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), HOVER_TIMER_ID);
            }
        }
    }

    /// True when a physical screen point lands on an interactive surface.
    /// The low-level hook uses this to ignore clicks on the island itself.
    fn screen_contains(&self, pt: POINT) -> bool {
        let x = (pt.x - self.origin.0) as f32 / self.scale;
        let y = (pt.y - self.origin.1) as f32 / self.scale;
        (self.view.pill().is_some() && self.pill.contains(x, y))
            || (self.flyout_open && self.flyout.contains(x, y))
    }

    fn install_mouse_hook(&mut self) {
        if self.mouse_hook.is_some() {
            return;
        }
        HOOK_HWND.store(self.hwnd.0 as isize, Ordering::Relaxed);
        // SAFETY: WH_MOUSE_LL accepts a null module handle and thread id 0 for
        // a process-wide hook; the callback is a static function.
        match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), None, 0) } {
            Ok(hook) => self.mouse_hook = Some(hook),
            Err(error) => crate::log::log_err("window.flyout.hook", &error),
        }
    }

    fn remove_mouse_hook(&mut self) {
        if let Some(hook) = self.mouse_hook.take() {
            // SAFETY: the handle came from SetWindowsHookExW above.
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
        HOOK_HWND.store(0, Ordering::Relaxed);
    }

    fn install_keyboard_hook(&mut self) {
        if self.keyboard_hook.is_some() {
            return;
        }
        HOOK_KEYS_HWND.store(self.hwnd.0 as isize, Ordering::Relaxed);
        // SAFETY: WH_KEYBOARD_LL accepts a null module handle and thread id 0.
        match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), None, 0) } {
            Ok(hook) => self.keyboard_hook = Some(hook),
            Err(error) => crate::log::log_err("window.rail.hook", &error),
        }
    }

    fn remove_keyboard_hook(&mut self) {
        if let Some(hook) = self.keyboard_hook.take() {
            // SAFETY: the handle came from SetWindowsHookExW above.
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
        HOOK_KEYS_HWND.store(0, Ordering::Relaxed);
    }

    // ------------------------------------------------------------ rail

    /// Commit the pending rail selection: the pill and card now follow the
    /// chosen activity without closing the card.
    fn commit_rail(&mut self, now: Instant) {
        let Some(kind) = self.pending_kind.take() else {
            return;
        };
        self.pin = Some(kind);
        self.refresh(now);
        let actual = self.view.main.as_ref().map(|view| view.kind);
        self.flyout_owner = actual;
        if actual != Some(kind) {
            // The pin was released at once (e.g. an incoming call); follow.
            self.rail_current = actual;
        }
        self.sync_tick_timer();
    }

    /// Rail button click / Enter. The indicator starts immediately; content
    /// fades out, swaps, then fades in 4 DIP below and settles up.
    fn activate_rail(&mut self, kind: ActivityKind) {
        if !self.flyout_open || self.flyout_phase != FlyoutPhase::Open {
            return;
        }
        if self.content_phase != ContentPhase::Idle {
            return;
        }
        let current = self
            .rail_current
            .or_else(|| self.view.main.as_ref().map(|view| view.kind));
        if current == Some(kind) {
            return;
        }
        if let Some(session) = &self.convert {
            session.cancel();
        }
        self.convert = None;
        self.convert_accepts.store(false, Ordering::Relaxed);
        self.stop_convert_timer();
        self.hide_tip();
        let now = Instant::now();
        self.rail_from = current;
        self.rail_current = Some(kind);
        self.pending_kind = Some(kind);
        if self.theme.reduced_motion {
            self.rail_t = 1.0;
            self.rail_tween = None;
            self.commit_rail(now);
            self.content_phase = ContentPhase::Idle;
            self.content_alpha = 1.0;
            self.content_dy = 0.0;
        } else {
            self.rail_t = 0.0;
            self.rail_tween = Some(Tween::new(
                now,
                design::RAIL_MOVE_MS,
                0.0,
                1.0,
                design::EASE_RAIL,
            ));
            self.content_phase = ContentPhase::Out;
            self.content_started = now;
        }
        self.sync_rail_timer();
    }

    /// One frame of the rail indicator slide and the content crossfade.
    fn advance_rail(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if let Some(tween) = self.rail_tween {
            self.rail_t = tween.value(now);
            if tween.done(now) {
                self.rail_t = tween.to;
                self.rail_tween = None;
            }
            changed = true;
        }
        match self.content_phase {
            ContentPhase::Out => {
                let elapsed = now
                    .saturating_duration_since(self.content_started)
                    .as_secs_f32()
                    * 1000.0;
                let t = (elapsed / design::CONTENT_OUT_MS as f32).clamp(0.0, 1.0);
                self.content_alpha = 1.0 - cubic_bezier(design::EASE_CONTENT, t);
                if t >= 1.0 {
                    self.commit_rail(now);
                    self.content_phase = ContentPhase::In;
                    self.content_started = now;
                    self.content_alpha = 0.0;
                    self.content_dy = design::CONTENT_SETTLE;
                }
                changed = true;
            }
            ContentPhase::In => {
                let elapsed = now
                    .saturating_duration_since(self.content_started)
                    .as_secs_f32()
                    * 1000.0;
                let t = (elapsed / design::CONTENT_IN_MS as f32).clamp(0.0, 1.0);
                if t >= 1.0 {
                    self.content_phase = ContentPhase::Idle;
                    self.content_alpha = 1.0;
                    self.content_dy = 0.0;
                } else {
                    let eased = cubic_bezier(design::EASE_CONTENT, t);
                    self.content_alpha = eased;
                    self.content_dy = (1.0 - eased) * design::CONTENT_SETTLE;
                }
                changed = true;
            }
            ContentPhase::Idle => {}
        }
        self.sync_rail_timer();
        changed
    }

    fn sync_rail_timer(&self) {
        // SAFETY: the timer id belongs to this window.
        unsafe {
            if self.rail_tween.is_some() || self.content_phase != ContentPhase::Idle {
                SetTimer(Some(self.hwnd), RAIL_TIMER_ID, design::FRAME_MS, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), RAIL_TIMER_ID);
            }
        }
    }

    /// The element whose tip should appear: the hovered control, or the rail
    /// button that owns keyboard focus. Only an open, settled card shows tips.
    /// A *hovered* history row is deliberately absent: its richer 450 ms
    /// preview overlay (spec 7) replaces the tooltip. Keyboard focus on a row
    /// still carries the row's own tip (spec 3.2).
    fn tip_candidate(&self) -> Option<Element> {
        if !self.flyout_open
            || self.flyout_phase != FlyoutPhase::Open
            || self.content_phase != ContentPhase::Idle
        {
            return None;
        }
        let hover = match self.hover {
            Some(
                element @ (Element::Rail(_)
                | Element::ShelfTile(_)
                | Element::ShelfRemove(_)
                | Element::ShelfSelectAll
                | Element::ShelfClear
                | Element::ShelfCount
                | Element::ConvertButton
                | Element::ConvertClose
                | Element::ConvertCancel
                | Element::ConvertShow
                | Element::ConvertCopy
                | Element::ConvertUndo
                | Element::ConvertStage(_)
                | Element::ConvertTab(_)
                | Element::ConvertOption(_)
                | Element::MusicPlay
                | Element::MusicPrev
                | Element::MusicNext
                | Element::MusicShuffle
                | Element::MusicRepeat
                | Element::CallMic
                | Element::CallOutput
                | Element::ClipFilter(_)
                | Element::ClipPause
                | Element::ClipClear
                | Element::ClipClearConfirm
                | Element::ClipClearCancel
                | Element::ClipPin(_)
                | Element::ClipCopy(_)
                | Element::ClipDelete(_)),
            ) => Some(element),
            _ => None,
        };
        if hover.is_some() {
            return hover;
        }
        if self.hover.is_none() {
            if let Some(index) = self.rail_focus {
                return Some(Element::Rail(
                    design::RAIL_ORDER[index.min(design::RAIL_ORDER.len() - 1)],
                ));
            }
            // Keyboard focus on a history row carries the row's own tip
            // (spec 3.2: hover and keyboard focus behave the same).
            if self.view.main.as_ref().map(|view| view.kind) == Some(ActivityKind::Clipboard) {
                if let Some(id) = self.clip_active.and_then(|index| self.clip_row_id(index)) {
                    return Some(Element::ClipRow(id));
                }
            }
            return None;
        }
        None
    }

    /// Title plus optional muted detail. The title is also the accessibility
    /// name, so icon-only controls stay self-describing.
    fn tip_text(&self, element: Element) -> Option<(String, Option<String>)> {
        let text = |title: &str| Some((title.to_string(), None));
        match element {
            Element::Rail(ActivityKind::Music) => text("Music"),
            Element::Rail(ActivityKind::Timer) => text("Timer"),
            Element::Rail(ActivityKind::Call) => text("Calls"),
            Element::Rail(ActivityKind::Shelf) => text("File shelf"),
            Element::Rail(ActivityKind::Clipboard) => text("Clipboard"),
            Element::ClipFilter(filter) => text(filter.label()),
            Element::ClipPause => Some((
                if self.clip.as_ref().is_some_and(ClipboardRuntime::is_paused) {
                    "Resume capture"
                } else {
                    "Pause capture"
                }
                .to_string(),
                None,
            )),
            Element::ClipClear => Some((
                "Clear history".to_string(),
                Some("Pinned items stay".to_string()),
            )),
            Element::ClipClearConfirm => {
                let n = self.clip.as_ref().map_or(0, |clip| {
                    clip.store.len().saturating_sub(clip.store.pinned_count())
                });
                Some((
                    format!("Clear {n} items"),
                    Some("Pinned items stay".to_string()),
                ))
            }
            Element::ClipClearCancel => text("Cancel"),
            Element::ClipPin(id) => self
                .clip
                .as_ref()?
                .store
                .get(id)
                .map(|item| (if item.pinned { "Unpin" } else { "Pin" }.to_string(), None)),
            Element::ClipCopy(id) => {
                let item = self.clip.as_ref()?.store.get(id)?;
                // A dead file row keeps Copy hoverable only so this can explain
                // why it does nothing (spec 4).
                let dead = item.kind == crate::clipboard::store::ClipKind::Files
                    && item
                        .paths
                        .iter()
                        .all(|path| !std::path::Path::new(path).exists());
                if dead {
                    Some(("File moved or deleted".to_string(), None))
                } else {
                    text("Copy")
                }
            }
            Element::ClipDelete(_) => text("Delete"),
            Element::ClipRow(id) => self
                .clip
                .as_ref()?
                .store
                .get(id)
                .map(|item| (crate::view::clip_tip(item), None)),
            Element::ShelfSelectAll => text("Select all"),
            Element::ShelfClear => Some((
                if self.shelf_selected.is_empty() {
                    "Clear shelf".to_string()
                } else {
                    "Remove selected".to_string()
                },
                Some("Your files stay where they are".to_string()),
            )),
            Element::ShelfCount => {
                let count = self.shelf_store.len();
                let title = if count == 1 {
                    "1 item".to_string()
                } else {
                    format!("{count} items")
                };
                let detail = self.shelf_overflow.then(|| {
                    format!(
                        "Older files were dropped at the {} item limit.",
                        design::SHELF_SOFT_CAP
                    )
                });
                Some((title, detail))
            }
            Element::ShelfTile(index) => {
                let path = self.shelf_store.paths().get(index)?;
                let name = crate::thumb::display_name(path);
                let detail = match std::fs::metadata(path) {
                    Ok(meta) if meta.is_dir() => Some("Folder".to_string()),
                    Ok(meta) => Some(crate::view::fmt_size(meta.len())),
                    Err(_) => None,
                };
                Some((name, detail))
            }
            Element::ShelfRemove(_) => Some((
                "Remove from shelf".to_string(),
                Some("Your file stays where it is".to_string()),
            )),
            Element::ConvertButton => text("Convert"),
            Element::ConvertClose => text("Close"),
            Element::ConvertCancel => text("Cancel"),
            Element::ConvertShow => text("Show in folder"),
            Element::ConvertCopy => Some((
                "Copy file".to_string(),
                Some("Puts the file on the clipboard".to_string()),
            )),
            Element::ConvertUndo => Some((
                "Undo".to_string(),
                Some("Moves the new file to the Recycle Bin".to_string()),
            )),
            Element::ConvertStage(0) => text("Reading"),
            Element::ConvertStage(1) => text("Converting"),
            Element::ConvertStage(_) => text("Saving"),
            Element::ConvertTab(tab) => text(tab.label()),
            Element::ConvertOption(action) => {
                let title = options::label_for(action)?;
                let hint = self
                    .convert
                    .as_ref()
                    .and_then(|session| session.probes.get(session.primary))
                    .and_then(|primary| {
                        options::engine_hint(file_kind(primary), action, &self.caps)
                    });
                Some((title.to_string(), hint))
            }
            Element::MusicPlay => text(
                if self
                    .media_snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.playing)
                    .unwrap_or(false)
                {
                    "Pause"
                } else {
                    "Play"
                },
            ),
            Element::MusicPrev => text("Previous track"),
            Element::MusicNext => text("Next track"),
            Element::MusicShuffle => text("Shuffle"),
            Element::MusicRepeat => text(match self.media_snapshot.as_ref().map(|s| s.repeat) {
                Some(crate::view::RepeatMode::List) => "Repeat list",
                Some(crate::view::RepeatMode::Track) => "Repeat track",
                _ => "Repeat off",
            }),
            Element::CallMic => text(if self.call_mic_muted() {
                "Unmute microphone"
            } else {
                "Mute microphone"
            }),
            Element::CallOutput => text(if self.call_output_muted() {
                "Unmute speaker"
            } else {
                "Mute speaker"
            }),
            _ => None,
        }
    }

    fn call_mic_muted(&self) -> bool {
        self.call_runtime
            .lock()
            .map(|runtime| runtime.mic_muted)
            .unwrap_or(false)
    }

    fn call_output_muted(&self) -> bool {
        self.call_runtime
            .lock()
            .map(|runtime| runtime.output_muted)
            .unwrap_or(false)
    }

    /// Anchor rectangle in physical screen pixels.
    fn tip_anchor(&self, element: Element) -> Option<RECT> {
        let target = self.hit_map.target(element)?;
        let r = target.rect;
        Some(RECT {
            left: self.origin.0 + (r.x * self.scale).round() as i32,
            top: self.origin.1 + (r.y * self.scale).round() as i32,
            right: self.origin.0 + (r.right() * self.scale).round() as i32,
            bottom: self.origin.1 + (r.bottom() * self.scale).round() as i32,
        })
    }

    /// (above, align_right) for the popup relative to the anchored control.
    fn tip_placement(&self, element: Element) -> (bool, bool) {
        let Some(target) = self.hit_map.target(element) else {
            return (false, false);
        };
        let mut right = target.rect.right() > self.flyout.right() - 24.0;
        let mut above = false;
        if let Element::ShelfTile(index) | Element::ShelfRemove(index) = element {
            let scroll = crate::view::shelf_scroll(self.shelf_store.len(), self.shelf_scroll);
            let row = index / design::SHELF_COLS;
            let last_visible =
                ((scroll.scroll + design::SHELF_VIEW_H) / design::SHELF_ROW_H).floor() as usize;
            above = row >= last_visible;
            right = index % design::SHELF_COLS == design::SHELF_COLS - 1;
        }
        (above, right)
    }

    /// Retarget the tooltip after a hover/focus change and arm its one-shot
    /// delay. Neighbouring controls skip the delay when a tip was just shown.
    fn schedule_tip(&mut self, now: Instant) {
        let candidate = self.tip_candidate();
        if candidate == self.tip_target {
            return;
        }
        self.hide_tip();
        self.tip_target = candidate;
        self.tip_since = now;
        // SAFETY: the timer id belongs to this window.
        unsafe {
            match candidate {
                Some(_) => {
                    let delay = tooltip::delay_ms(self.tip_hidden_at, now).max(1);
                    SetTimer(Some(self.hwnd), TOOLTIP_TIMER_ID, delay, None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), TOOLTIP_TIMER_ID);
                }
            }
        }
    }

    /// Dismiss the popup immediately (click, scroll, drag, card close, Esc).
    /// The hover-preview overlay is a popup too: it goes with the tip so no
    /// stale picture is left over the list.
    fn hide_tip(&mut self) {
        let now = Instant::now();
        self.dismiss_clip_preview();
        if self.tip.is_visible() {
            self.tip.hide();
            self.tip_hidden_at = Some(now);
        }
        self.tip_target = None;
        self.tip_since = now;
        // SAFETY: the timer id belongs to this window.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TOOLTIP_TIMER_ID);
        }
    }

    /// One-shot timer expiry: show the tip if the target stayed put for the
    /// full delay; otherwise re-arm for the remainder.
    fn tip_due(&mut self, now: Instant) {
        // SAFETY: one-shot timer; killing an already-fired id is harmless.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TOOLTIP_TIMER_ID);
        }
        let candidate = self.tip_candidate();
        if candidate != self.tip_target {
            self.tip_target = None;
            self.schedule_tip(now);
            return;
        }
        let Some(element) = candidate else {
            return;
        };
        let delay = tooltip::delay_ms(self.tip_hidden_at, now);
        let waited = now.saturating_duration_since(self.tip_since).as_millis() as u32;
        if waited < delay {
            // SAFETY: the timer id belongs to this window.
            unsafe {
                SetTimer(
                    Some(self.hwnd),
                    TOOLTIP_TIMER_ID,
                    delay.saturating_sub(waited).max(1),
                    None,
                );
            }
            return;
        }
        let Some((title, detail)) = self.tip_text(element) else {
            return;
        };
        let Some(anchor) = self.tip_anchor(element) else {
            return;
        };
        let (above, align_right) = self.tip_placement(element);
        let request = TipRequest {
            title: &title,
            detail: detail.as_deref(),
            anchor,
            prefer_above: above,
            align_right,
        };
        self.tip.show(&request, self.scale);
    }

    /// Arm the 450 ms one-shot for the row under the pointer (spec 7). Moving
    /// onto another row restarts it, so the overlay only ever appears over a
    /// steady hover; the 384 px decode starts here to earn the delay as head
    /// start.
    fn sync_clip_preview(&mut self, now: Instant) {
        let row = match self.hover {
            Some(Element::ClipRow(id)) => Some(id),
            _ => None,
        };
        if self.clip_preview_arm.map(|(id, _)| id) == row {
            return;
        }
        self.dismiss_clip_preview();
        if let Some(id) = row {
            self.clip_preview_arm = Some((id, now));
            self.ensure_clip_preview();
            // SAFETY: the timer id belongs to this window.
            unsafe {
                SetTimer(
                    Some(self.hwnd),
                    CLIP_PREVIEW_TIMER_ID,
                    design::CLIP_PREVIEW_MS,
                    None,
                );
            }
        }
    }

    /// Drop the overlay, its one-shot and the decoded picture: the pointer
    /// left the row, the list scrolled, the card closed or a click landed.
    fn dismiss_clip_preview(&mut self) {
        self.clip_preview = None;
        self.clip_preview_arm = None;
        self.clip_preview_art.clear();
        // SAFETY: one-shot timer; killing an id that never armed is harmless.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), CLIP_PREVIEW_TIMER_ID);
        }
    }

    /// One-shot expiry: show the overlay if the same row is still under the
    /// pointer for the whole delay, otherwise re-arm for the remainder.
    /// Returns whether an overlay appeared (so the caller can repaint).
    fn clip_preview_due(&mut self, now: Instant) -> bool {
        // SAFETY: one-shot timer; killing an already-fired id is harmless.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), CLIP_PREVIEW_TIMER_ID);
        }
        let Some((id, since)) = self.clip_preview_arm else {
            return false;
        };
        if self.hover != Some(Element::ClipRow(id)) {
            self.dismiss_clip_preview();
            return false;
        }
        let waited = now.saturating_duration_since(since).as_millis() as u64;
        if !crate::view::clip_preview_ready(waited) {
            // SAFETY: the timer id belongs to this window.
            unsafe {
                SetTimer(
                    Some(self.hwnd),
                    CLIP_PREVIEW_TIMER_ID,
                    (u64::from(design::CLIP_PREVIEW_MS) - waited).max(1) as u32,
                    None,
                );
            }
            return false;
        }
        self.clip_preview = Some(id);
        self.refresh(now);
        true
    }

    /// Accessibility snapshot for the current frame: one node per control that
    /// also has a tooltip, with the tooltip title as the UIA name.
    fn a11y_nodes(&self) -> Vec<crate::uia::A11yNode> {
        let mut seen: Vec<Element> = Vec::new();
        let mut nodes = Vec::new();
        for target in &self.hit_map.targets {
            if seen.contains(&target.element) {
                continue;
            }
            let Some((title, detail)) = self.tip_text(target.element) else {
                continue;
            };
            seen.push(target.element);
            let r = target.rect;
            let rect = RECT {
                left: self.origin.0 + (r.x * self.scale).round() as i32,
                top: self.origin.1 + (r.y * self.scale).round() as i32,
                right: self.origin.0 + (r.right() * self.scale).round() as i32,
                bottom: self.origin.1 + (r.bottom() * self.scale).round() as i32,
            };
            let focus = match (target.element, self.rail_focus) {
                (Element::Rail(kind), Some(index)) => design::RAIL_ORDER.get(index) == Some(&kind),
                _ => false,
            };
            nodes.push(crate::uia::A11yNode {
                name: title,
                help: detail.unwrap_or_default(),
                rect,
                enabled: self.element_enabled(target.element),
                focus,
            });
        }
        nodes
    }

    fn element_enabled(&self, element: Element) -> bool {
        match element {
            Element::ShelfSelectAll => {
                !self.shelf_store.is_empty() && self.shelf_selected.len() < self.shelf_store.len()
            }
            Element::ShelfClear => !self.shelf_store.is_empty(),
            Element::ConvertButton => self
                .shelf_convert_paths()
                .iter()
                .any(|path| crate::view::is_convertible(path)),
            _ => true,
        }
    }

    fn a11y_provider(
        &self,
    ) -> Option<windows::Win32::UI::Accessibility::IRawElementProviderSimple> {
        let mut window = RECT::default();
        // SAFETY: the window belongs to this process.
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut window);
        }
        Some(crate::uia::provider(self.hwnd, window, self.a11y_nodes()))
    }

    fn rail_engaged(&self) -> bool {
        self.rail_focus.is_some() || matches!(self.hover, Some(Element::Rail(_)))
    }

    /// Per-item stride of the filtered history list (34 DIP rows, 50 DIP for
    /// images): the input to every `clip_scroll` call so scrolling and row
    /// layout agree.
    fn clip_strides(&self) -> Vec<f32> {
        self.clip.as_ref().map_or_else(Vec::new, |clip| {
            crate::view::clip_strides(clip.store.items(), self.clip_filter)
        })
    }

    /// The filtered history row with store id `id`, as an index into the list.
    fn clip_row_index(&self, id: u64) -> Option<usize> {
        self.clip
            .as_ref()?
            .store
            .items()
            .iter()
            .filter(|item| self.clip_filter.matches(item))
            .position(|item| item.id == id)
    }

    /// Store id of the focused history row.
    fn clip_row_id(&self, index: usize) -> Option<u64> {
        self.clip
            .as_ref()?
            .store
            .items()
            .iter()
            .filter(|item| self.clip_filter.matches(item))
            .nth(index)
            .map(|item| item.id)
    }

    /// Scrolls the list just enough to keep `index` in view.
    fn reveal_clip_row(&mut self, index: usize) {
        // Mixed 34/50 DIP rows: the offsets come from the stride list.
        let strides = self.clip_strides();
        let top: f32 = strides.iter().take(index).sum();
        let bottom = top + strides.get(index).copied().unwrap_or(design::CLIP_ROW_H);
        let viewport = self.clip_viewport();
        if top < self.clip_scroll {
            self.set_clip_scroll(top);
        } else if bottom > self.clip_scroll + viewport.h {
            self.set_clip_scroll(bottom - viewport.h);
        }
    }

    /// Delete removes the row the keyboard focus is on; a single delete needs
    /// no confirmation (spec 3.2). Returns whether a row was removed.
    fn delete_focused_clip_row(&mut self) -> bool {
        let Some(index) = self.clip_active.filter(|_| {
            self.view.main.as_ref().map(|view| view.kind) == Some(ActivityKind::Clipboard)
        }) else {
            return false;
        };
        let Some(id) = self.clip_row_id(index) else {
            return false;
        };
        if let Some(clip) = self.clip.as_mut() {
            clip.remove(id);
        }
        let len = self.clip_len();
        self.clip_active = (len > 0).then(|| index.min(len - 1));
        self.show_clipboard();
        true
    }

    /// History-list keys while the rail is not engaged (spec 3.2): the arrows
    /// move the row focus, Enter copies it back and Shift+Enter also pastes it
    /// into the window that was in front.
    fn handle_clip_row_key(&mut self, vk: u32) {
        if self.convert.is_some()
            || self.view.main.as_ref().map(|view| view.kind) != Some(ActivityKind::Clipboard)
        {
            return;
        }
        let len = self.clip_len();
        if len == 0 {
            return;
        }
        let up = vk == VK_UP.0 as u32;
        let down = vk == VK_DOWN.0 as u32;
        let home = vk == VK_HOME.0 as u32;
        let end = vk == VK_END.0 as u32;
        let enter = vk == VK_RETURN.0 as u32;
        if !(up || down || home || end || enter) {
            return;
        }
        let focused = self.clip_active.filter(|index| *index < len);
        if enter {
            // The island is WS_EX_NOACTIVATE, so the user's own window still
            // owns the foreground: a paste lands there once the card closes.
            let paste = key_down(VK_SHIFT.0 as i32);
            if let Some(id) = self.clip_row_id(focused.unwrap_or(0)) {
                self.copy_back(id, paste);
            }
            return;
        }
        let next = if up {
            Some(match focused {
                Some(index) => index.saturating_sub(1),
                None => len - 1,
            })
        } else if down {
            Some(match focused {
                Some(index) => (index + 1).min(len - 1),
                None => 0,
            })
        } else if home {
            Some(0)
        } else if end {
            Some(len - 1)
        } else {
            None
        };
        let Some(index) = next else {
            return;
        };
        self.clip_active = Some(index);
        self.show_clipboard();
        self.reveal_clip_row(index);
        self.schedule_tip(Instant::now());
    }

    /// Arrow/Home/End move the rail focus; Enter/Space activate it. Delete
    /// removes the hovered or selected shelf files, or the focused history row.
    /// When the rail is not engaged the same keys drive the history list.
    fn handle_rail_key(&mut self, vk: u32) {
        if !self.flyout_open || self.flyout_phase != FlyoutPhase::Open {
            return;
        }
        if vk == VK_DELETE.0 as u32 {
            let shelf_card = self.convert.is_none()
                && self.view.main.as_ref().map(|view| view.kind) == Some(ActivityKind::Shelf);
            if shelf_card {
                let indices = if !self.shelf_selected.is_empty() {
                    self.shelf_selected.clone()
                } else {
                    match self.hover {
                        Some(Element::ShelfTile(index)) => vec![index],
                        _ => Vec::new(),
                    }
                };
                self.remove_shelf_indices(&indices);
                return;
            }
            self.delete_focused_clip_row();
            return;
        }
        if !self.rail_engaged() {
            self.handle_clip_row_key(vk);
            return;
        }
        let current = self
            .rail_focus
            .or_else(|| match self.hover {
                Some(Element::Rail(kind)) => Some(design::rail_index(kind)),
                _ => None,
            })
            .or_else(|| self.rail_current.map(design::rail_index))
            .unwrap_or(0)
            .min(design::RAIL_ORDER.len() - 1);
        let next = if vk == VK_UP.0 as u32 {
            Some((current + design::RAIL_ORDER.len() - 1) % design::RAIL_ORDER.len())
        } else if vk == VK_DOWN.0 as u32 || vk == VK_TAB.0 as u32 {
            Some((current + 1) % design::RAIL_ORDER.len())
        } else if vk == VK_HOME.0 as u32 {
            Some(0)
        } else if vk == VK_END.0 as u32 {
            Some(design::RAIL_ORDER.len() - 1)
        } else {
            None
        };
        if let Some(index) = next {
            self.rail_focus = Some(index);
            self.schedule_tip(Instant::now());
        } else if vk == VK_RETURN.0 as u32 || vk == VK_SPACE.0 as u32 {
            self.activate_rail(design::RAIL_ORDER[current]);
        }
    }

    fn open_flyout(&mut self, now: Instant) {
        if self.view.flyout().is_none() {
            return;
        }
        self.flyout_open = true;
        self.flyout_owner = self.view.main.as_ref().map(|view| view.kind);
        self.flyout_phase = FlyoutPhase::Opening;
        self.flyout_started = now;
        self.flyout_alpha = 0.0;
        self.flyout_scale = design::FLYOUT_SCALE;
        self.flyout_dy = design::FLYOUT_RISE;
        let kind = self.view.main.as_ref().map(|view| view.kind);
        self.rail_current = kind;
        self.rail_from = None;
        self.rail_t = 1.0;
        self.rail_tween = None;
        self.hide_tip();
        self.rail_focus = None;
        self.content_phase = ContentPhase::Idle;
        self.content_alpha = 1.0;
        self.content_dy = 0.0;
        self.pending_kind = None;
        // A paste queued for a close that never happened must not fire on some
        // later close.
        self.pending_paste = None;
        self.clear_auto_close();
        self.install_mouse_hook();
        self.install_keyboard_hook();
        self.sync_flyout_timer();
        self.sync_tick_timer();
    }

    fn begin_close_flyout(&mut self, now: Instant) {
        if !self.flyout_open || self.flyout_phase == FlyoutPhase::Closing {
            return;
        }
        self.flyout_phase = FlyoutPhase::Closing;
        self.flyout_started = now;
        self.hide_tip();
        self.clear_auto_close();
        self.sync_flyout_timer();
    }

    /// Drop the card without animating (its activity vanished) and disarm the
    /// outside-click hook so idle never pays for it.
    fn finish_close_flyout(&mut self) {
        self.flyout_open = false;
        self.flyout_owner = None;
        self.flyout_phase = FlyoutPhase::Closed;
        self.flyout_alpha = 0.0;
        self.flyout_scale = 1.0;
        self.flyout_dy = 0.0;
        self.clear_auto_close();
        self.remove_mouse_hook();
        self.remove_keyboard_hook();
        self.rail_current = None;
        self.rail_from = None;
        self.rail_t = 1.0;
        self.rail_tween = None;
        self.hide_tip();
        self.rail_focus = None;
        self.content_phase = ContentPhase::Idle;
        self.content_alpha = 1.0;
        self.content_dy = 0.0;
        self.pending_kind = None;
        self.shelf_scroll_drag = None;
        self.clip_scroll_drag = None;
        self.shelf_rubber = None;
        // The card is gone: an armed Clear is no longer actionable, and a
        // queued paste finally gets its chance (spec 3.2).
        self.disarm_clip_clear();
        self.flush_pending_paste();
        if let Some(session) = &self.convert {
            session.cancel();
        }
        self.convert = None;
        self.convert_accepts.store(false, Ordering::Relaxed);
        self.stop_convert_timer();
        // The closed card's snapshot still holds the abandoned Convert page;
        // rebuild it so reopening starts on the shelf again.
        if self
            .view
            .main
            .as_ref()
            .is_some_and(|view| view.kind == ActivityKind::Shelf)
        {
            self.show_shelf();
        }
        // SAFETY: the timer ids belong to this window.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), FLYOUT_TIMER_ID);
            let _ = KillTimer(Some(self.hwnd), RAIL_TIMER_ID);
            let _ = KillTimer(Some(self.hwnd), TOOLTIP_TIMER_ID);
        }
        self.sync_tick_timer();
    }

    /// One frame of the open/close tween. Returns whether a repaint is needed,
    /// including the final closing frame that clears the card.
    fn advance_flyout(&mut self, now: Instant) -> bool {
        let elapsed = now
            .saturating_duration_since(self.flyout_started)
            .as_secs_f32()
            * 1000.0;
        match self.flyout_phase {
            FlyoutPhase::Opening => {
                let fade_t = (elapsed / design::FLYOUT_OPEN_FADE_MS as f32).clamp(0.0, 1.0);
                let move_t = (elapsed / design::FLYOUT_OPEN_MOVE_MS as f32).clamp(0.0, 1.0);
                let fade = cubic_bezier(design::EASE_FLYOUT, fade_t);
                let moved = cubic_bezier(design::EASE_FLYOUT, move_t);
                self.flyout_alpha = fade;
                self.flyout_scale = design::FLYOUT_SCALE + (1.0 - design::FLYOUT_SCALE) * moved;
                self.flyout_dy = (1.0 - moved) * design::FLYOUT_RISE;
                if elapsed >= design::FLYOUT_OPEN_MOVE_MS as f32 {
                    self.flyout_alpha = 1.0;
                    self.flyout_scale = 1.0;
                    self.flyout_dy = 0.0;
                    self.flyout_phase = FlyoutPhase::Open;
                    self.sync_flyout_timer();
                }
                true
            }
            FlyoutPhase::Closing => {
                let t = (elapsed / design::FLYOUT_CLOSE_MS as f32).clamp(0.0, 1.0);
                self.flyout_alpha = 1.0 - t;
                self.flyout_scale = 1.0 - (1.0 - design::FLYOUT_SCALE) * t;
                self.flyout_dy = design::FLYOUT_RISE * t;
                if t >= 1.0 {
                    self.finish_close_flyout();
                }
                true
            }
            FlyoutPhase::Closed | FlyoutPhase::Open => false,
        }
    }

    fn sync_flyout_timer(&self) {
        // SAFETY: the timer id belongs to this window.
        unsafe {
            if matches!(
                self.flyout_phase,
                FlyoutPhase::Opening | FlyoutPhase::Closing
            ) {
                SetTimer(Some(self.hwnd), FLYOUT_TIMER_ID, design::FRAME_MS, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), FLYOUT_TIMER_ID);
            }
        }
    }

    fn note_pointer_inside(&mut self) {
        if self.flyout_leave_at.take().is_some() {
            // SAFETY: the timer id belongs to this window.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), AUTO_CLOSE_TIMER_ID);
            }
        }
    }

    fn schedule_auto_close(&mut self, now: Instant) {
        if !self.flyout_open || self.flyout_phase == FlyoutPhase::Closing {
            return;
        }
        // The spring-loaded card lives as long as the drag does; the OLE
        // message loop can starve our mouse tracking, so leave it alone.
        if self.drag_convert || self.drag_waiting {
            return;
        }
        if self.flyout_leave_at.is_some() {
            return;
        }
        self.flyout_leave_at = Some(now);
        // SAFETY: the timer id belongs to this window.
        unsafe {
            SetTimer(
                Some(self.hwnd),
                AUTO_CLOSE_TIMER_ID,
                design::AUTO_CLOSE_MS,
                None,
            );
        }
    }

    fn clear_auto_close(&mut self) {
        self.flyout_leave_at = None;
        // SAFETY: the timer id belongs to this window.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), AUTO_CLOSE_TIMER_ID);
        }
    }

    /// The media session disappeared or has no title. Drop the stale snapshot;
    /// `refresh` falls back to the next live activity or the idle pill.
    fn clear_media(&mut self) {
        self.media_snapshot = None;
        self.media_missing_at = None;
        self.media_command_at = None;
        self.refresh(Instant::now());
        self.sync_tick_timer();
    }

    /// A missing session is normally a gap while the player swaps tracks, not
    /// a real stop. Hold the last snapshot briefly and clear only if nothing
    /// replaces it, so the card stays open and the pill never falls back to
    /// Shelf for a single frame.
    fn media_gone(&mut self) {
        if self.media_snapshot.is_none() || self.media_missing_at.is_some() {
            return;
        }
        self.media_missing_at = Some(Instant::now());
        crate::log::log("media.gone (holding snapshot)");
        self.arm_media_clear();
    }

    /// Arm the one-shot clear. A transport click buys a longer hold because
    /// the player may take seconds to load what comes next.
    fn arm_media_clear(&mut self) {
        let now = Instant::now();
        let deadline = match self.media_command_at {
            Some(at)
                if now.saturating_duration_since(at).as_millis()
                    < design::MEDIA_COMMAND_HOLD_MS as u128 =>
            {
                at + std::time::Duration::from_millis(design::MEDIA_COMMAND_HOLD_MS as u64)
            }
            _ => now + std::time::Duration::from_millis(design::MEDIA_CLEAR_MS as u64),
        };
        let wait = deadline.saturating_duration_since(now).as_millis().max(1) as u32;
        // SAFETY: this window owns the one-shot timer id; re-arming with the
        // same id replaces the previous expiry.
        unsafe {
            SetTimer(Some(self.hwnd), MEDIA_CLEAR_TIMER_ID, wait, None);
        }
    }

    /// Remember that the user just drove the player, and stretch a pending
    /// clear so a slow track load cannot drop the card.
    fn note_media_command(&mut self) {
        self.media_command_at = Some(Instant::now());
        if self.media_missing_at.is_some() {
            self.arm_media_clear();
        }
    }

    fn expire_media(&mut self) {
        // SAFETY: one-shot timer; killing an already-fired id is harmless.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), MEDIA_CLEAR_TIMER_ID);
        }
        if self.media_missing_at.is_some() {
            crate::log::log("media.gone expired -> clear");
            self.clear_media();
        }
    }

    fn apply_timer(&mut self, now: Instant) {
        self.refresh(now);
    }

    fn apply_media(&mut self, snapshot: MediaSnapshot) {
        self.media_snapshot = Some(snapshot);
        if let Some(missing) = self.media_missing_at.take() {
            crate::log::log(&format!(
                "media.back after {}ms",
                missing.elapsed().as_millis()
            ));
            // The command that opened the gap has completed.
            self.media_command_at = None;
            // SAFETY: cancel the pending clear now that a session is visible.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), MEDIA_CLEAR_TIMER_ID);
            }
        }
        self.refresh(Instant::now());
        self.sync_tick_timer();
    }

    fn apply_call(&mut self) {
        self.refresh(Instant::now());
    }

    fn draw(&mut self) -> Result<()> {
        // The activity behind an open card can vanish between messages. Drop
        // the card immediately in that case; there is nothing to animate.
        if self.flyout_open && self.view.flyout().is_none() {
            self.finish_close_flyout();
        } else if self.flyout_open
            && self.flyout_owner != self.view.main.as_ref().map(|view| view.kind)
        {
            // A different activity won while the card was open; don't leave
            // stale content (or an idle fallback) on screen.
            self.begin_close_flyout(Instant::now());
        }
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
            // A composition swapchain must clear to fully transparent black:
            // every pixel the island does not paint has to let the taskbar
            // material show through unchanged.
            self.context.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
            let render_target = self.context.cast()?;
            let input = RenderInput {
                view: &self.view,
                theme: self.theme,
                scale: self.scale,
                pill: self.pill,
                bubble: None,
                flyout: (self.flyout_open && self.view.flyout().is_some()).then_some(self.flyout),
                flyout_alpha: self.flyout_alpha,
                flyout_scale: self.flyout_scale,
                flyout_dy: self.flyout_dy,
                pill_alpha: 1.0,
                top_alpha: self.flyout_alpha * self.content_alpha,
                body_alpha: self.flyout_alpha * self.content_alpha,
                top_dy: self.content_dy,
                body_dy: self.content_dy,
                hover: self.hover,
                pressed: self.pressed.map(|element| (element, 1.0)),
                focus: self
                    .rail_focus
                    .map(|index| Element::Rail(design::RAIL_ORDER[index])),
                hover_mix: self.hover_mix,
                sheen: None,
                flash_t: None,
                drag_bob: 0.0,
                progress: match self.view.main.as_ref() {
                    Some(view) if view.kind == ActivityKind::Music => match &view.flyout.body {
                        Body::Music(music) => Some(ProgressVisual {
                            fraction: music.fraction,
                            phase: self.wave_phase,
                            amplitude: if music.playing {
                                design::WAVE_AMP
                            } else {
                                design::WAVE_AMP * design::WAVE_FLAT
                            },
                        }),
                        _ => None,
                    },
                    _ => None,
                },
                pill_wave: match self.view.main.as_ref().map(|view| view.kind) {
                    Some(ActivityKind::Music) => Some(WaveVisual {
                        phase: self.wave_phase,
                        amplitude: if self
                            .media_snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.playing)
                            .unwrap_or(false)
                        {
                            design::PILL_WAVE_AMP
                        } else {
                            design::PILL_WAVE_AMP * design::WAVE_FLAT
                        },
                    }),
                    Some(ActivityKind::Timer) => Some(WaveVisual {
                        phase: self.wave_phase,
                        amplitude: if self.timer.running() {
                            design::PILL_WAVE_AMP
                        } else {
                            design::PILL_WAVE_AMP * design::WAVE_FLAT
                        },
                    }),
                    _ => None,
                },
                icon_swap: None,
                rail_current: self.rail_current,
                rail_from: self.rail_from,
                rail_t: self.rail_t,
                drag_over: self.shelf_hover,
            };
            match self.painter.draw(&render_target, &input) {
                Ok(hits) => self.hit_map = hits,
                Err(error) => {
                    let _ = self.context.EndDraw(None, None);
                    self.context.SetTarget(None::<&ID2D1Image>);
                    return Err(error);
                }
            }
            let end = self.context.EndDraw(None, None);
            self.context.SetTarget(None::<&ID2D1Image>);
            end?;
            // Present without waiting for vblank; the app is event driven and
            // composition will show the newest surface.
            self.swapchain.Present(0, Default::default()).ok()?;
        }
        self.update_window_region();
        Ok(())
    }

    /// Restrict the HWND's hit-test and OLE-drop area to the visible rounded
    /// surfaces. The flyout's halo is included so its soft shadow is not
    /// clipped; `WM_NCHITTEST` still reports the halo as click-through.
    fn update_window_region(&self) {
        let scale = self.scale;
        // SAFETY: every GDI region is owned by this function; on success
        // `SetWindowRgn` takes ownership of `region`.
        unsafe {
            let region = CreateRectRgn(0, 0, 0, 0);
            if self.view.pill().is_some() {
                let r = self.physical(self.pill);
                let radius = (self.theme.pill_radius() * 2.0 * scale).round() as i32;
                let pill =
                    CreateRoundRectRgn(r.left, r.top, r.right + 1, r.bottom + 1, radius, radius);
                CombineRgn(Some(region), Some(region), Some(pill), RGN_OR);
                let _ = DeleteObject(pill.into());
            }
            if self.flyout_open && self.view.flyout().is_some() {
                let r = self.physical(self.flyout);
                let halo = (design::SHADOW_HALO * scale).round() as i32;
                let radius = (design::R_SURFACE * 2.0 * scale).round() as i32 + halo * 2;
                let fly = CreateRoundRectRgn(
                    r.left - halo,
                    r.top - halo,
                    r.right + halo + 1,
                    r.bottom + halo + 1,
                    radius,
                    radius,
                );
                CombineRgn(Some(region), Some(region), Some(fly), RGN_OR);
                let _ = DeleteObject(fly.into());
            }
            let _ = SetWindowRgn(self.hwnd, Some(region), true);
        }
    }

    fn physical(&self, r: RectD) -> RECT {
        RECT {
            left: (r.x * self.scale).round() as i32,
            top: (r.y * self.scale).round() as i32,
            right: ((r.x + r.w) * self.scale).round() as i32,
            bottom: ((r.y + r.h) * self.scale).round() as i32,
        }
    }
}

/// The 4 x 2 Convert grid: up to four container targets, then the four
/// actions. Targets are accent text; actions carry a glyph. "Strip EXIF" is
/// dimmed when the header file has no metadata to remove.
/// Posts the worker's wake-up to the UI thread. Only an `isize` crosses the
/// thread boundary, so the worker never holds a non-Send `HWND`.
fn post_convert(hwnd: isize) {
    // SAFETY: the window outlives the worker; if it closed, PostMessage fails
    // harmlessly and the message is dropped.
    unsafe {
        let _ = PostMessageW(
            Some(HWND(hwnd as *mut core::ffi::c_void)),
            WM_APP_CONVERT,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

fn output_paths(session: &Option<ConvertSession>) -> Vec<PathBuf> {
    session
        .as_ref()
        .and_then(|session| session.outcome.as_ref())
        .map(|outcome| {
            outcome
                .created
                .iter()
                .map(|created| created.output.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn done_remaining(session: &ConvertSession, now: Instant) -> Duration {
    let Some(deadline) = session.done_deadline else {
        return Duration::ZERO;
    };
    deadline.saturating_duration_since(session.done_paused_at.unwrap_or(now))
}

/// Coarse source class the option table is keyed by.
fn file_kind(probe: &Probe) -> FileKind {
    match probe.kind {
        ProbeKind::Image(_) => FileKind::Image,
        ProbeKind::Video(_) => FileKind::Video,
        ProbeKind::Audio(_) => FileKind::Audio,
        ProbeKind::Document(_) => FileKind::Document,
        ProbeKind::Pdf => FileKind::Pdf,
        ProbeKind::Sheet(_) => FileKind::Spreadsheet,
        ProbeKind::Present(_) => FileKind::Presentation,
        // A ZIP archive opens the Document card (§2.9).
        ProbeKind::Zip => FileKind::Document,
        _ => FileKind::Other,
    }
}

/// A batch spanning more than one class shares no job, so the card offers no
/// grid at all.
fn mixed_kinds(probes: &[Probe], convertible: &[usize]) -> bool {
    let mut kinds = convertible
        .iter()
        .filter_map(|index| probes.get(*index))
        .filter(|probe| probe.kind.convertible())
        .map(file_kind);
    let Some(first) = kinds.next() else {
        return false;
    };
    kinds.any(|other| other != first)
}

/// Exact containers in the batch, so a tile never offers the file's own
/// format back. Heterogeneous batches hide every format they already contain.
fn source_kinds(session: &ConvertSession) -> Vec<SourceKind> {
    session
        .convertible
        .iter()
        .filter_map(|index| session.probes.get(*index))
        .filter_map(|probe| SourceKind::from_source(probe.format()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::detect::{ImageInfo, ProbeKind};

    fn probe(kind: ProbeKind) -> Probe {
        Probe {
            path: String::new(),
            name: String::new(),
            bytes: 0,
            kind,
        }
    }

    fn image(format: crate::convert::detect::SourceFormat) -> Probe {
        probe(ProbeKind::Image(ImageInfo {
            format,
            width: 8,
            height: 8,
            has_alpha: false,
            has_exif: false,
        }))
    }

    #[test]
    fn one_class_batch_is_not_mixed() {
        let probes = vec![
            image(crate::convert::detect::SourceFormat::Jpeg),
            image(crate::convert::detect::SourceFormat::Png),
            probe(ProbeKind::Video(crate::convert::detect::SourceFormat::Mp4)),
        ];
        assert!(!mixed_kinds(&probes, &[0, 1]));
        assert!(!mixed_kinds(&probes, &[]));
    }

    #[test]
    fn different_classes_share_no_grid() {
        let probes = vec![
            image(crate::convert::detect::SourceFormat::Jpeg),
            probe(ProbeKind::Video(crate::convert::detect::SourceFormat::Mp4)),
            probe(ProbeKind::Missing),
        ];
        assert!(mixed_kinds(&probes, &[0, 1]));
        // The unsupported probe is not convertible, so it cannot mix the set.
        assert!(!mixed_kinds(&probes, &[0, 2]));
    }

    /// Copy-back paste only fires for the window that was in front when the row
    /// was copied; if the user switched away the keystroke is dropped (5.7).
    #[test]
    fn paste_only_fires_for_the_window_that_was_copied_for() {
        let first = HWND(std::ptr::dangling_mut());
        let second = HWND(2usize as *mut _);
        assert!(paste_is_due(first, first));
        assert!(!paste_is_due(first, second));
        // No foreground window at all: never paste into the void.
        let none = HWND(std::ptr::null_mut());
        assert!(!paste_is_due(none, none));
    }
}
