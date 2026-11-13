//! Source C: Phone Link window watcher (spec §3) — the authority for a live
//! call and the only source that sees outgoing calls.
//!
//! The watcher adopts the top-level window of `PhoneExperienceHost.exe` /
//! `YourPhone.exe` (process check, cached per pid) and re-reads it from
//! `SetWinEventHook` callbacks: foreground/show finds the window, name
//! changes (the in-call timer retypes every second) re-read it, and
//! hide/destroy takes it away. There is no timer and no polling: every walk
//! is the direct consequence of an event, coalesced to one walk per 500 ms.
//!
//! `End call` / `Hang up` or a bare `m:ss` timer on screen means Active; a
//! dial pad with a number in it means Dialing. When the tracked window shows
//! no call any more (or disappears) this source reports `Ended`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetParent, GetWindowThreadProcessId, IsIconic, EVENT_OBJECT_DESTROY,
    EVENT_OBJECT_HIDE, EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_FOREGROUND,
    WINEVENT_OUTOFCONTEXT,
};

use super::uiakit::{self, DumpNode};
use super::{CallSource, Emit, Signal, SignalKind, SourceCtx, SourceId};
use crate::activities::call_state::SAME_CALL_WINDOW;

/// Processes that host the Phone Link UI.
/// Processes that own the Phone Link top-level window (also used by the
/// action worker to find the window it invokes buttons in).
pub const PHONE_PROCESSES: &[&str] = &["phoneexperiencehost.exe", "yourphone.exe", "phonelink.exe"];

/// Name-change bursts (layout, hover, list refresh) collapse into one walk.
const READ_COALESCE: Duration = Duration::from_millis(500);

/// Buttons that only exist in the in-call UI.
/// Labels of the button that ends a call (spec §3 Source C). Also the
/// invoke candidates the action worker looks for.
pub const END_BUTTONS: &[&str] = &["end call", "hang up", "end", "stop call", "leave call"];
/// Present in the in-call UI too; logged as supporting evidence.
const MUTE_BUTTONS: &[&str] = &["mute", "unmute", "mute microphone", "unmute microphone"];
/// Buttons of the dial pad.
const DIAL_BUTTONS: &[&str] = &["call", "dial", "place call"];

/// App chrome that is never the caller (window titles and the main UI).
const WINDOW_CHROME: &[&str] = &[
    "phone link",
    "your phone",
    "calls",
    "recent",
    "contacts",
    "keypad",
    "messages",
    "photos",
    "apps",
    "settings",
    "search",
    "back",
    "new call",
    "add a note",
    "mute",
    "unmute",
    "speaker",
    "volume",
    "hold",
    "transfer",
    "end call",
    "hang up",
    "accept",
    "decline",
    "answer",
    "dialing",
    "calling",
    "incoming call",
];

/// What the Phone Link window shows right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowCall {
    /// In-call UI: `End call` / `Hang up`, or a bare running timer.
    Active { caller: Option<String> },
    /// Dial pad with a number typed in: an outgoing call is being placed.
    Dialing { caller: Option<String> },
}

/// Classify the Phone Link window's elements.
///
/// A bare `m:ss`/`mm:ss` timer counts as a call only together with an end or
/// mute button, or on its own when it is a standalone timer text — a clock
/// with `AM`/`PM` never matches [`is_timer`].
pub fn classify_window(nodes: &[DumpNode], app_names: &[String]) -> Option<WindowCall> {
    if nodes.is_empty() {
        return None;
    }
    let button = |names: &[&str]| {
        nodes.iter().any(|node| {
            node.ctrl == "Button" && names.contains(&node.name.trim().to_lowercase().as_str())
        })
    };
    let timer = nodes.iter().any(|node| is_timer(&node.name));
    let mute = button(MUTE_BUTTONS);
    if button(END_BUTTONS) || timer && mute || timer && !has_main_chrome(nodes) {
        return Some(WindowCall::Active {
            caller: caller_in_window(nodes, app_names),
        });
    }
    let number = nodes
        .iter()
        .filter(|node| node.ctrl == "Edit" || node.ctrl == "Text")
        .map(|node| node.name.chars().filter(|c| c.is_ascii_digit()).count())
        .max()
        .unwrap_or(0);
    if number >= 5 && button(DIAL_BUTTONS) {
        return Some(WindowCall::Dialing {
            caller: caller_in_window(nodes, app_names),
        });
    }
    None
}

/// Controls a call window would not show *without* an End/Mute button: with
/// any of them on screen, a bare timer is a list timestamp, not a call.
fn has_main_chrome(nodes: &[DumpNode]) -> bool {
    const MARKERS: &[&str] = &["messages", "photos", "apps", "settings", "contacts"];
    nodes.iter().any(|node| {
        node.ctrl == "Button"
            || node.ctrl == "ListItem"
            || node.ctrl == "TabItem"
            || MARKERS.contains(&node.name.trim().to_lowercase().as_str())
    })
}

/// A standalone `m:ss` / `mm:ss` duration — the in-call timer. Clocks with
/// a suffix (`12:34 PM`) and dates never match.
pub fn is_timer(text: &str) -> bool {
    let text = text.trim();
    let Some((minutes, seconds)) = text.split_once(':') else {
        return false;
    };
    !minutes.is_empty()
        && minutes.len() <= 2
        && seconds.len() == 2
        && minutes.chars().all(|c| c.is_ascii_digit())
        && seconds.chars().all(|c| c.is_ascii_digit())
}

/// First text line that is not chrome: not a button, not the window title,
/// not the app name, not a timer, not a wall of text.
fn caller_in_window(nodes: &[DumpNode], app_names: &[String]) -> Option<String> {
    for node in nodes {
        let name = node.name.trim();
        if name.is_empty()
            || name.chars().count() > 60
            || node.ctrl == "Button"
            || node.ctrl == "Image"
            || node.ctrl == "Hyperlink"
            || node.ctrl == "Window"
        {
            continue;
        }
        let lower = name.trim_end_matches(['.', '!', '…']).trim().to_lowercase();
        if WINDOW_CHROME.contains(&lower.as_str()) || is_timer(name) {
            continue;
        }
        if app_names.iter().any(|app| app.eq_ignore_ascii_case(name)) {
            continue;
        }
        if lower.ends_with(" am") || lower.ends_with(" pm") {
            continue;
        }
        return Some(name.to_string());
    }
    None
}

/// Source C itself.
pub struct PhoneLinkWatcher;

impl CallSource for PhoneLinkWatcher {
    fn id(&self) -> SourceId {
        SourceId::Window
    }

    fn label(&self) -> &'static str {
        "C: Phone Link window watcher"
    }

    fn run(self: Box<Self>, ctx: SourceCtx, emit: Emit) {
        run_watcher(ctx, emit);
    }
}

fn run_watcher(ctx: SourceCtx, emit: Emit) {
    uiakit::init_mta();
    let mut watcher = Watcher::new(ctx, emit);
    watcher.scan();
    let diag = watcher.ctx.diag.clone();
    // The OUTOFCONTEXT hook delivers on this thread's queue, so the watcher
    // lives in thread-local storage for the lifetime of the message loop.
    WATCHER.with(|slot| *slot.borrow_mut() = Some(watcher));
    let _hooks = install_hooks();
    if let Some(diag) = &diag {
        diag.log("source C ready: WinEvent hook (foreground/show/name/hide/destroy)");
    }
    loop {
        let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
        // SAFETY: our own thread's queue; 0 = WM_QUIT, <0 = error.
        let got =
            unsafe { windows::Win32::UI::WindowsAndMessaging::GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        // SAFETY: a normal dispatch of a message we just read.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
            windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
        }
    }
}

thread_local! {
    static WATCHER: RefCell<Option<Watcher>> = const { RefCell::new(None) };
}

/// Foreground/show (discovery), name change (the timer retyped), and
/// hide/destroy (the window went away).
fn install_hooks() -> Vec<HWINEVENTHOOK> {
    const EVENTS: &[u32] = &[
        EVENT_SYSTEM_FOREGROUND,
        EVENT_OBJECT_SHOW,
        EVENT_OBJECT_NAMECHANGE,
        EVENT_OBJECT_HIDE,
        EVENT_OBJECT_DESTROY,
    ];
    EVENTS
        .iter()
        .filter_map(|event| {
            // SAFETY: hook_proc outlives the thread; OUTOFCONTEXT means the
            // system only queues it to this thread.
            let hook = unsafe {
                SetWinEventHook(
                    *event,
                    *event,
                    None,
                    Some(hook_proc),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.0.is_null() {
                WATCHER.with(|slot| {
                    if let Some(watcher) = slot.borrow().as_ref() {
                        watcher.note(&format!("phonelink: hook {event} failed"));
                    }
                });
                None
            } else {
                Some(hook)
            }
        })
        .collect()
}

/// SAFETY: called by the system on this thread only (OUTOFCONTEXT).
unsafe extern "system" fn hook_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    idobject: i32,
    _idchild: i32,
    _ideventthread: u32,
    _dwmseventtime: u32,
) {
    use windows::Win32::UI::WindowsAndMessaging::OBJID_WINDOW;
    // Window-level events, plus any object's name change (the timer text).
    if idobject != OBJID_WINDOW.0 && event != EVENT_OBJECT_NAMECHANGE {
        return;
    }
    WATCHER.with(|slot| {
        // Re-entering (a walk inside a walk) is a no-op rather than a panic.
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return;
        };
        if let Some(watcher) = slot.as_mut() {
            watcher.on_event(event, hwnd.0 as isize);
        }
    });
}

/// One tracked Phone Link window plus what this source last reported.
struct Watcher {
    ctx: SourceCtx,
    emit: Emit,
    /// The top-level Phone Link window we are reading.
    tracked: Option<isize>,
    /// pid -> is a Phone Link process (resolved once per pid).
    phone_pids: HashMap<u32, bool>,
    /// What we last told the machine: only an observation of ours may be
    /// ended by this source (Source A's toast never owns a live call).
    reported: Option<SignalKind>,
    /// Caller of the last observation, for the `Ended` signal.
    last_caller: Option<String>,
    last_read: Option<Instant>,
    last_emit: Option<Instant>,
}

impl Watcher {
    fn new(ctx: SourceCtx, emit: Emit) -> Self {
        Self {
            ctx,
            emit,
            tracked: None,
            phone_pids: HashMap::new(),
            reported: None,
            last_caller: None,
            last_read: None,
            last_emit: None,
        }
    }

    fn note(&self, line: &str) {
        if let Some(diag) = &self.ctx.diag {
            diag.log(line);
        }
    }

    /// Adopt an already-running Phone Link window at startup.
    fn scan(&mut self) {
        if self.tracked.is_some() {
            return;
        }
        let mut top_level: Vec<isize> = Vec::new();
        // SAFETY: the callback only pushes handles into the vector we own.
        unsafe {
            let _ = EnumWindows(
                Some(collect_top_level),
                LPARAM(&mut top_level as *mut Vec<isize> as isize),
            );
        }
        for hwnd in top_level {
            if self.is_phone_window(hwnd) {
                self.note(&format!("phonelink: startup window {hwnd:#x}"));
                self.tracked = Some(hwnd);
                self.read();
                return;
            }
        }
    }

    fn on_event(&mut self, event: u32, hwnd: isize) {
        if hwnd == 0 {
            return;
        }
        match event {
            EVENT_SYSTEM_FOREGROUND | EVENT_OBJECT_SHOW => {
                if self.tracked == Some(hwnd) {
                    self.read();
                } else if self.reported.is_none() && self.is_phone_window(hwnd) {
                    // A different (or the first) Phone Link window showed.
                    self.note(&format!("phonelink: adopt window {hwnd:#x}"));
                    self.tracked = Some(hwnd);
                    self.last_read = None;
                    self.read();
                }
            }
            EVENT_OBJECT_NAMECHANGE => {
                if self.tracked == Some(hwnd) {
                    self.read();
                }
            }
            EVENT_OBJECT_HIDE | EVENT_OBJECT_DESTROY => {
                if self.tracked != Some(hwnd) {
                    return;
                }
                // A minimized window is hidden but still the same window.
                if event == EVENT_OBJECT_HIDE && unsafe { IsIconic(HWND(hwnd as *mut _)) }.as_bool()
                {
                    return;
                }
                self.note(&format!("phonelink: window {hwnd:#x} went away"));
                self.tracked = None;
                self.last_read = None;
                if let Some(previous) = self.reported.take() {
                    if previous != SignalKind::Ended {
                        self.note("phonelink: authority gone while a call was shown -> Ended");
                        let caller = self.last_caller.take();
                        self.emit
                            .send(Signal::new(SourceId::Window, SignalKind::Ended, caller));
                        self.last_emit = Some(Instant::now());
                    }
                }
                // Another Phone Link window may still exist.
                self.scan();
            }
            _ => {}
        }
    }

    /// Whether this is a top-level Phone Link window (cached per pid).
    fn is_phone_window(&mut self, hwnd: isize) -> bool {
        if hwnd == 0 {
            return false;
        }
        // Top level: no parent.
        // SAFETY: handle validity is the caller's contract.
        if unsafe { GetParent(HWND(hwnd as *mut _)) }.is_ok() {
            return false;
        }
        let mut pid = 0u32;
        // SAFETY: optional out pointer, written only when non-null.
        unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid)) };
        if pid == 0 {
            return false;
        }
        if let Some(known) = self.phone_pids.get(&pid) {
            return *known;
        }
        let is_phone = process_name(pid)
            .map(|name| {
                PHONE_PROCESSES
                    .iter()
                    .any(|known| name.eq_ignore_ascii_case(known))
            })
            .unwrap_or(false);
        self.phone_pids.insert(pid, is_phone);
        is_phone
    }

    /// One bounded walk of the tracked window; emits what it shows.
    fn read(&mut self) {
        let Some(hwnd) = self.tracked else {
            return;
        };
        if let Some(last) = self.last_read {
            if last.elapsed() < READ_COALESCE {
                return;
            }
        }
        self.last_read = Some(Instant::now());
        let view = read_window(hwnd);
        if view.hwnd == 0 {
            // Nothing could be read: keep the state we have rather than
            // inventing an end.
            return;
        }
        if self.ctx.dump_uia {
            if let Some(diag) = &self.ctx.diag {
                if let Some(tree) = uiakit::dump_window(hwnd) {
                    diag.dump_uia(&format!("phone link window {hwnd:#x}"), &tree);
                }
            }
        }
        match classify_window(&view.nodes, &self.ctx.phonelink_names) {
            Some(WindowCall::Active { caller }) => self.report(SignalKind::Active, caller),
            Some(WindowCall::Dialing { caller }) => self.report(SignalKind::Dialing, caller),
            None => {
                if let Some(previous) = self.reported.take() {
                    if previous != SignalKind::Ended {
                        self.note("phonelink: no call on screen -> Ended");
                        let caller = self.last_caller.take();
                        self.emit
                            .send(Signal::new(SourceId::Window, SignalKind::Ended, caller));
                        self.last_emit = Some(Instant::now());
                    }
                }
            }
        }
    }

    /// Emit an observation, repeating it only when the hub's 5 s same-call
    /// window has passed (so a machine cleared by a stray toast recovers).
    fn report(&mut self, kind: SignalKind, caller: Option<String>) {
        let stale = self
            .last_emit
            .is_none_or(|at| at.elapsed() >= SAME_CALL_WINDOW);
        if self.reported == Some(kind) && !stale {
            self.last_caller = caller;
            return;
        }
        self.reported = Some(kind);
        self.last_emit = Some(Instant::now());
        self.last_caller = caller.clone();
        self.note(&format!("phonelink: {kind:?} caller={caller:?}"));
        self.emit.send(Signal::new(SourceId::Window, kind, caller));
    }
}

/// `EnumWindows` callback: collect top-level handles only.
unsafe extern "system" fn collect_top_level(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: run_watcher passes the address of a live Vec.
    let out = &mut *(lparam.0 as *mut Vec<isize>);
    out.push(hwnd.0 as isize);
    BOOL(1)
}

/// Process image file name, or `None` when it cannot be read.
pub fn process_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // SAFETY: limited query access is enough for the image name.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 512];
    let mut size = buf.len() as u32;
    // SAFETY: the buffer is valid for `size` wchars; the size is in/out.
    let read = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
    };
    // SAFETY: the handle came from OpenProcess and nobody else closes it.
    let _ = unsafe { CloseHandle(handle) };
    read.ok()?;
    let full = String::from_utf16_lossy(&buf[..size as usize]);
    Some(full.rsplit(['\\', '/']).next()?.to_string())
}

/// The tracked window's identity and elements, behind the 500 ms budget.
/// `hwnd == 0` means "could not be read".
#[derive(Default)]
struct WindowView {
    hwnd: isize,
    nodes: Vec<DumpNode>,
}

fn read_window(hwnd: isize) -> WindowView {
    uiakit::with_timeout(
        "phonelink.read",
        move || {
            uiakit::init_mta();
            let Ok(automation) = uiakit::automation() else {
                return WindowView::default();
            };
            // SAFETY: the handle came from a live window this thread read.
            let Ok(element) = (unsafe { automation.ElementFromHandle(HWND(hwnd as *mut _)) })
            else {
                return WindowView::default();
            };
            WindowView {
                hwnd,
                nodes: uiakit::dump_nodes(&automation, &element),
            }
        },
        WindowView::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(list: &[(&str, &str)]) -> Vec<DumpNode> {
        list.iter()
            .map(|(ctrl, name)| DumpNode {
                ctrl: (*ctrl).to_string(),
                name: (*name).to_string(),
            })
            .collect()
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    /// Synthesized from the documented Phone Link in-call layout (no live
    /// call exists on this machine) — see the §3 report.
    const IN_CALL_FIXTURE: &str = r#"
ctrl=Window "Phone Link" class=Windows.UI.Core.CoreWindow pid=9001
  ctrl=Text "Priya Sharma" class=TextBlock pid=9001
  ctrl=Text "0:07" class=TextBlock pid=9001
  ctrl=Button "Mute" class=Button pid=9001
  ctrl=Button "Speaker" class=Button pid=9001
  ctrl=Button "End call" class=Button pid=9001
"#;

    const MAIN_WINDOW_FIXTURE: &str = r#"
ctrl=Window "Phone Link" class=ApplicationFrameWindow pid=9002
  ctrl=Button "Calls" class=AppBarButton pid=9002
  ctrl=Button "Messages" class=AppBarButton pid=9002
  ctrl=Button "Photos" class=AppBarButton pid=9002
  ctrl=Text "Recent" class=TextBlock pid=9002
  ctrl=Text "Ada Lovelace" class=TextBlock pid=9002
  ctrl=Text "2 minutes ago" class=TextBlock pid=9002
"#;

    const DIAL_FIXTURE: &str = r#"
ctrl=Window "Phone Link" class=Windows.UI.Core.CoreWindow pid=9003
  ctrl=Text "Keypad" class=TextBlock pid=9003
  ctrl=Edit "98765 43210" class=TextBox pid=9003
  ctrl=Button "Call" class=Button pid=9003
"#;

    #[test]
    fn in_call_window_is_active_with_the_first_text_as_caller() {
        let call = classify_window(
            &uiakit::parse_dump(IN_CALL_FIXTURE),
            &names(&["Phone Link"]),
        )
        .expect("in-call UI");
        assert_eq!(
            call,
            WindowCall::Active {
                caller: Some("Priya Sharma".into())
            }
        );
    }

    #[test]
    fn the_main_window_is_not_a_call() {
        assert_eq!(
            classify_window(
                &uiakit::parse_dump(MAIN_WINDOW_FIXTURE),
                &names(&["Phone Link"])
            ),
            None
        );
    }

    #[test]
    fn a_bare_timer_among_main_window_controls_is_not_a_call() {
        let call = classify_window(
            &nodes(&[
                ("Window", "Phone Link"),
                ("Button", "Messages"),
                ("Text", "12:34"),
                ("Text", "Ada Lovelace"),
            ]),
            &names(&["Phone Link"]),
        );
        assert_eq!(call, None, "a message timestamp is not an active call");
    }

    #[test]
    fn a_dial_pad_with_a_number_is_dialing() {
        let call = classify_window(&uiakit::parse_dump(DIAL_FIXTURE), &names(&["Phone Link"]))
            .expect("dial pad");
        assert_eq!(
            call,
            WindowCall::Dialing {
                caller: Some("98765 43210".into())
            }
        );
    }

    #[test]
    fn timers_match_but_clocks_and_dates_do_not() {
        assert!(is_timer("0:07"));
        assert!(is_timer("12:34"));
        assert!(is_timer(" 1:05 "));
        assert!(!is_timer("12:34 PM"));
        assert!(!is_timer("123:45"));
        assert!(!is_timer("2 minutes ago"));
        assert!(!is_timer("no timer"));
        assert!(!is_timer("1:5"));
    }

    #[test]
    fn an_end_button_alone_means_active() {
        let call = classify_window(
            &nodes(&[
                ("Window", "Phone Link"),
                ("Text", "0:00"),
                ("Button", "End call"),
            ]),
            &names(&["Phone Link"]),
        )
        .expect("end button");
        assert_eq!(call, WindowCall::Active { caller: None });
    }
}
