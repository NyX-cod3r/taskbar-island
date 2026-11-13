//! Call sources feeding one state machine (spec §3).
//!
//! Windows exposes no API for another app's call state, so several
//! independent [`CallSource`] modules observe what the system does show —
//! toasts, notifications, the Phone Link window, the notification database —
//! and emit [`Signal`]s into one inbox. The UI thread folds them into a
//! single [`CallMachine`] (a [`CallState`] plus identity rules) and never
//! talks to a source directly.

pub mod action;
pub mod diag;
pub mod notify;
pub mod phonelink;
pub mod toast;
pub mod uiakit;
pub mod wpn;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use windows::Win32::UI::WindowsAndMessaging::WM_APP;

use crate::activities::call_state::{CallEvent, CallIdentity, CallState, SAME_CALL_WINDOW};

/// `--dump-uia`, set once by `main` before the island window exists.
pub static DUMP_UIA: AtomicBool = AtomicBool::new(false);

/// `--simulate-call` that arrived before an island existed: the [`SimKind`]
/// code to fold in as soon as a Surface exists ([`NO_SIMULATION`] = none).
pub static PENDING_SIMULATE: AtomicUsize = AtomicUsize::new(NO_SIMULATION);

/// Sentinel for [`PENDING_SIMULATE`].
pub const NO_SIMULATION: usize = usize::MAX;

/// `--simulate-call` posts this to a running island to inject a fake event
/// (spec §3). Fixed `WM_APP` value: it is only ever sent between two
/// processes of this app.
pub const WM_APP_SIMULATE: u32 = WM_APP + 40;

/// Which fake event `--simulate-call` injects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimKind {
    Incoming,
    Dialing,
    Active,
    End,
}

impl SimKind {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "incoming" => Some(Self::Incoming),
            "dialing" | "dial" => Some(Self::Dialing),
            "active" => Some(Self::Active),
            "end" | "ended" => Some(Self::End),
            _ => None,
        }
    }

    pub fn code(self) -> usize {
        match self {
            Self::Incoming => 0,
            Self::Dialing => 1,
            Self::Active => 2,
            Self::End => 3,
        }
    }

    pub fn from_code(code: usize) -> Option<Self> {
        match code {
            0 => Some(Self::Incoming),
            1 => Some(Self::Dialing),
            2 => Some(Self::Active),
            3 => Some(Self::End),
            _ => None,
        }
    }

    /// The signal it injects. The caller text says plainly that it is fake.
    pub fn signal(self) -> Signal {
        let kind = match self {
            Self::Incoming => SignalKind::Incoming,
            Self::Dialing => SignalKind::Dialing,
            Self::Active => SignalKind::Active,
            Self::End => SignalKind::Ended,
        };
        Signal::new(SourceId::Simulated, kind, Some("Simulated call".into()))
    }
}

/// Which observer produced a signal. Part of the same-call identity rule
/// (spec §3: caller text *and* source must match within 5 s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceId {
    /// Source A: UI Automation toast listener.
    Toast,
    /// Source B: UserNotificationListener.
    Notify,
    /// Source C: Phone Link window watcher.
    Window,
    /// Source D: notification database (off by default).
    WpnDb,
    /// `--simulate-call` injection.
    Simulated,
}

impl SourceId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Toast => "toast",
            Self::Notify => "notify",
            Self::Window => "window",
            Self::WpnDb => "wpn-db",
            Self::Simulated => "simulated",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    /// A ringing call was observed.
    Incoming,
    /// An outgoing call started.
    Dialing,
    /// An in-call UI is present (or our Answer landed): the call is live on
    /// this PC.
    Active,
    /// The call went away.
    Ended,
}

/// One observation from one source, stamped when it was produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    pub source: SourceId,
    pub kind: SignalKind,
    /// Caller text as the source read it (never persisted).
    pub caller: Option<String>,
    pub at: Instant,
}

impl Signal {
    pub fn new(source: SourceId, kind: SignalKind, caller: Option<String>) -> Self {
        Self {
            source,
            kind,
            caller,
            at: Instant::now(),
        }
    }
}

/// Everything a source needs to run: whether to dump UIA trees, the optional
/// verbose diagnostics sink (`--diag-call` / `--dump-uia`), and the names
/// that mean "Phone Link" on this system.
#[derive(Clone)]
pub struct SourceCtx {
    /// `--dump-uia`: write the UIA tree of every new toast and of the Phone
    /// Link window into the diag log.
    pub dump_uia: bool,
    /// Verbose diagnostics sink; `None` in normal runs.
    pub diag: Option<crate::activities::callsrc::diag::Diag>,
    /// App display names that mean "Phone Link" on this system, resolved at
    /// runtime from the package (spec §3 Source A: localized names).
    pub phonelink_names: Vec<String>,
}

/// What every source implements. A source owns its thread(s), is event
/// driven, and reports only what it observed — never what it wishes.
pub trait CallSource: Send {
    fn id(&self) -> SourceId;
    /// Human label for diagnostics.
    fn label(&self) -> &'static str;
    /// Run until the process exits (or the source is permanently unable to
    /// start, in which case it logs and returns).
    fn run(self: Box<Self>, ctx: SourceCtx, emit: Emit);
}

/// Delivery handle shared by every source: a channel into the UI thread (or
/// the `--diag-call` runner) plus a best-effort wake-up post.
#[derive(Clone)]
pub struct Emit {
    tx: Sender<Signal>,
    hwnd: isize,
}

impl Emit {
    pub fn new(tx: Sender<Signal>, hwnd: isize) -> Self {
        Self { tx, hwnd }
    }

    pub fn send(&self, signal: Signal) {
        if self.tx.send(signal).is_err() {
            return;
        }
        if self.hwnd != 0 {
            // SAFETY: posting to our own window; the receiver drains the
            // channel on the other side.
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(windows::Win32::Foundation::HWND(self.hwnd as *mut _)),
                    crate::activities::call::WM_APP_CALL,
                    windows::Win32::Foundation::WPARAM(crate::activities::call::CALL_WAKE_SIGNALS),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        }
    }
}

/// Whether two stamps are within [`SAME_CALL_WINDOW`] of each other in
/// either direction (never panics on ordering).
fn close_in_time(a: Instant, b: Instant) -> bool {
    a.checked_duration_since(b)
        .or_else(|| b.checked_duration_since(a))
        .is_some_and(|delta| delta < SAME_CALL_WINDOW)
}

/// The hub: folds source signals into the one state machine, applying the
/// identity rules (spec §3):
///
/// * the same call = same caller text **and** same source within 5 s;
/// * `Ended` from the toast/notification sources ends an Incoming/Dialing
///   call, but never an Active one within the answer grace — the toast
///   closes the moment the call is answered, and the Phone Link window
///   (Source C) is the authority for ending a live call;
/// * repeats of the same observation are idempotent.
#[derive(Default)]
pub struct CallMachine {
    pub state: CallState,
    /// Last (source, caller, at) for the same-call rule.
    last: Option<(SourceId, Option<String>, Instant)>,
}

/// After our own Answer, a toast close is expected and must not end the
/// call; the window source stays authoritative.
const ACTIVE_END_GRACE: Duration = Duration::from_secs(3);

impl CallMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one signal in. Returns whether the state changed (the UI then
    /// re-resolves the pill and card).
    pub fn on_signal(&mut self, signal: &Signal, now: Instant) -> bool {
        let same_call = self.last.as_ref().is_some_and(|(source, caller, at)| {
            *source == signal.source && *caller == signal.caller && close_in_time(*at, signal.at)
        });
        if same_call && signal.kind != SignalKind::Ended {
            // Same call, same source, within 5 s: a repeat observation.
            return false;
        }
        if signal.kind == SignalKind::Ended
            && self.state.phase == crate::activities::call_state::CallPhase::Active
            && signal.source != SourceId::Window
            && self
                .state
                .active_since
                .is_some_and(|since| now.duration_since(since) < ACTIVE_END_GRACE)
        {
            // The toast we were ringing on just closed because the call
            // connected; Source C reports the real end.
            return false;
        }
        self.last = match signal.kind {
            SignalKind::Ended => None,
            _ => Some((signal.source, signal.caller.clone(), signal.at)),
        };
        let before = self.state.clone();
        let identity = CallIdentity {
            name: signal.caller.clone(),
            number: None,
        };
        match signal.kind {
            SignalKind::Incoming => self.state.apply(CallEvent::Incoming(identity), now),
            SignalKind::Dialing => self.state.apply(CallEvent::Dialing(identity), now),
            SignalKind::Active => {
                // A source seeing the in-call UI: connect if we were
                // ringing/dialing, or adopt the call outright when the card
                // opened after it started.
                match self.state.phase {
                    crate::activities::call_state::CallPhase::Incoming
                    | crate::activities::call_state::CallPhase::Dialing => {
                        self.state.apply(CallEvent::Answered, now);
                    }
                    crate::activities::call_state::CallPhase::None => {
                        self.state.apply(CallEvent::Incoming(identity), now);
                        self.state.apply(CallEvent::Answered, now);
                    }
                    _ => {}
                }
            }
            SignalKind::Ended => self.state.apply(CallEvent::Ended, now),
        }
        self.state != before
    }

    /// Our own action landed (Answer via UIA Invoke, or the simulator):
    /// transition exactly as a provider confirmation would.
    pub fn action_landed(&mut self, now: Instant) -> bool {
        let before = self.state.clone();
        self.state.apply(CallEvent::Answered, now);
        self.state != before
    }

    /// The one-shot clear is due: Ended + 3 s.
    pub fn clear_due(&self, now: Instant) -> bool {
        self.state.clear_due(now)
    }

    pub fn clear(&mut self, now: Instant) -> bool {
        let before = self.state.clone();
        self.state.apply(CallEvent::Clear, now);
        self.state != before
    }
}

/// Start every enabled source on its own thread, delivering into `tx`. The
/// localized Phone Link display names are resolved first on this bootstrap
/// thread (a WinRT call — never the UI thread, spec §3 Source A).
///
/// `hwnd == 0` runs headless (`--diag-call`): signals still arrive on the
/// channel but nothing is posted.
pub fn spawn_sources(
    hwnd: isize,
    dump_uia: bool,
    diag: Option<diag::Diag>,
    use_wpn_db: bool,
    tx: Sender<Signal>,
) {
    let spawned = std::thread::Builder::new()
        .name("taskbar-island-call-bootstrap".into())
        .spawn(move || {
            let names = crate::activities::call::phone_link_names();
            crate::log::log(&format!(
                "call.sources: starting (phonelink names: {names:?}, wpn-db: {use_wpn_db})"
            ));
            // `--dump-uia` on its own writes into a dedicated `uia-*.log`;
            // behind `--diag-call` the trees share the call log instead.
            let diag = match diag {
                Some(sink) => Some(sink),
                None if dump_uia => match diag::Diag::open(&diag::log_path("uia")) {
                    Ok(sink) => Some(sink),
                    Err(error) => {
                        crate::log::log(&format!("call.sources: no dump sink: {error}"));
                        None
                    }
                },
                None => None,
            };
            let ctx = SourceCtx {
                dump_uia,
                diag,
                phonelink_names: names,
            };
            let mut sources: Vec<Box<dyn CallSource>> = vec![
                Box::new(toast::ToastSource),
                Box::new(notify::NotifySource),
                Box::new(phonelink::PhoneLinkWatcher),
            ];
            if use_wpn_db {
                sources.push(Box::new(wpn::WpnDbSource));
            }
            for source in sources {
                let id = source.id();
                let ctx = ctx.clone();
                let emit = Emit::new(tx.clone(), hwnd);
                let spawned = std::thread::Builder::new()
                    .name(format!("taskbar-island-call-{}", id.as_str()))
                    .spawn(move || {
                        crate::log::log(&format!("call.source start: {}", source.label()));
                        source.run(ctx, emit);
                    });
                if let Err(error) = spawned {
                    crate::log::log(&format!(
                        "call.source {} failed to start: {error}",
                        id.as_str()
                    ));
                }
            }
        });
    if let Err(error) = spawned {
        crate::log::log(&format!("call.sources failed to start: {error}"));
    }
}

/// `--diag-call`: run every source for `seconds`, log each signal and the
/// state transition it caused, then a per-source summary. Returns the log
/// path so the caller can print it.
pub fn run_diag(seconds: u64, dump_uia: bool, use_wpn_db: bool) -> std::io::Result<PathBuf> {
    use crate::activities::call_state::mask_number;

    let path = diag::log_path("call");
    let sink = diag::Diag::open(&path)?;
    sink.log(&format!(
        "--diag-call: {seconds}s, dump-uia={dump_uia}, wpn-db={use_wpn_db}, identity={}",
        diag::package_identity()
    ));
    let (tx, rx) = mpsc::channel();
    spawn_sources(0, dump_uia, Some(sink.clone()), use_wpn_db, tx);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut machine = CallMachine::new();
    let counters = diag::Counters::default();
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining.min(Duration::from_secs(1))) {
            Ok(signal) => {
                counters.bump(signal.source.as_str());
                let caller = signal.caller.as_deref().map(mask_number);
                sink.log(&format!(
                    "signal {} {:?} caller={caller:?}",
                    signal.source.as_str(),
                    signal.kind
                ));
                let now = Instant::now();
                if machine.on_signal(&signal, now) {
                    sink.log(&format!(
                        "  state -> {:?} (caller={})",
                        machine.state.phase,
                        machine.state.caller().map(mask_number).unwrap_or_default()
                    ));
                }
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    sink.log(&format!("--diag-call done: {}", counters.summary()));
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(source: SourceId, kind: SignalKind, caller: &str) -> Signal {
        Signal {
            source,
            kind,
            caller: Some(caller.into()),
            at: Instant::now(),
        }
    }

    #[test]
    fn same_call_is_caller_plus_source_within_five_seconds() {
        let mut machine = CallMachine::new();
        let now = Instant::now();
        let first = sig(SourceId::Toast, SignalKind::Incoming, "Ada");
        assert!(machine.on_signal(&first, now));
        // Same caller + same source inside the window: one call, not two.
        let mut repeat = first.clone();
        repeat.at = now + Duration::from_secs(1);
        assert!(!machine.on_signal(&repeat, now + Duration::from_secs(1)));
        assert_eq!(
            machine.state.phase,
            crate::activities::call_state::CallPhase::Incoming
        );
        // A different source reporting the same caller is a different
        // observation and must pass through (it can carry Active).
        let window = sig(SourceId::Window, SignalKind::Active, "Ada");
        assert!(machine.on_signal(&window, now + Duration::from_secs(2)));
        assert_eq!(
            machine.state.phase,
            crate::activities::call_state::CallPhase::Active
        );
    }

    #[test]
    fn outside_the_five_second_window_the_same_caller_rings_again() {
        let mut machine = CallMachine::new();
        let now = Instant::now();
        let first = sig(SourceId::Toast, SignalKind::Incoming, "Ada");
        assert!(machine.on_signal(&first, now));
        machine.on_signal(
            &Signal {
                at: now + Duration::from_secs(6),
                ..first.clone()
            },
            now + Duration::from_secs(6),
        );
        // Ended in between; a later ring from the same source is new.
        machine.on_signal(
            &sig(SourceId::Toast, SignalKind::Ended, "Ada"),
            now + Duration::from_secs(6),
        );
        let mut later = first.clone();
        later.at = now + Duration::from_secs(7);
        assert!(machine.on_signal(&later, now + Duration::from_secs(7)));
        assert_eq!(
            machine.state.phase,
            crate::activities::call_state::CallPhase::Incoming
        );
    }

    #[test]
    fn toast_close_right_after_the_answer_does_not_end_an_active_call() {
        let mut machine = CallMachine::new();
        let now = Instant::now();
        machine.on_signal(&sig(SourceId::Toast, SignalKind::Incoming, "Ada"), now);
        assert!(machine.action_landed(now + Duration::from_secs(1)));
        // The toast closes the moment the call connects.
        let closed = Signal {
            source: SourceId::Toast,
            kind: SignalKind::Ended,
            caller: Some("Ada".into()),
            at: now + Duration::from_secs(2),
        };
        assert!(!machine.on_signal(&closed, now + Duration::from_secs(2)));
        assert_eq!(
            machine.state.phase,
            crate::activities::call_state::CallPhase::Active
        );
        // The window source is the authority for the real end.
        let ended = Signal {
            source: SourceId::Window,
            kind: SignalKind::Ended,
            caller: Some("Ada".into()),
            at: now + Duration::from_secs(30),
        };
        assert!(machine.on_signal(&ended, now + Duration::from_secs(30)));
        assert_eq!(
            machine.state.phase,
            crate::activities::call_state::CallPhase::Ended
        );
    }

    #[test]
    fn toast_close_ends_a_ring_that_was_never_answered() {
        let mut machine = CallMachine::new();
        let now = Instant::now();
        machine.on_signal(&sig(SourceId::Toast, SignalKind::Incoming, "Ada"), now);
        let closed = Signal {
            source: SourceId::Toast,
            kind: SignalKind::Ended,
            caller: Some("Ada".into()),
            at: now + Duration::from_secs(4),
        };
        assert!(machine.on_signal(&closed, now + Duration::from_secs(4)));
        assert_eq!(
            machine.state.phase,
            crate::activities::call_state::CallPhase::Ended
        );
        assert!(machine.clear_due(now + Duration::from_secs(8)));
        assert!(machine.clear(now + Duration::from_secs(8)));
        assert_eq!(
            machine.state.phase,
            crate::activities::call_state::CallPhase::None
        );
    }

    #[test]
    fn incoming_then_window_active_connects_the_call() {
        let mut machine = CallMachine::new();
        let now = Instant::now();
        machine.on_signal(&sig(SourceId::Toast, SignalKind::Incoming, "Ada"), now);
        // Source C sees the in-call UI after the user answers on the PC.
        machine.on_signal(
            &sig(SourceId::Window, SignalKind::Active, "Ada"),
            now + Duration::from_secs(3),
        );
        assert_eq!(
            machine.state.phase,
            crate::activities::call_state::CallPhase::Active
        );
        assert!(machine.state.active_since.is_some());
    }

    #[test]
    fn window_active_alone_adopts_an_ongoing_call() {
        let mut machine = CallMachine::new();
        let now = Instant::now();
        machine.on_signal(&sig(SourceId::Window, SignalKind::Active, "Bo"), now);
        assert_eq!(
            machine.state.phase,
            crate::activities::call_state::CallPhase::Active
        );
    }
}
