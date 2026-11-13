//! Source A: UI Automation toast listener — the primary call source (§3).
//!
//! A dedicated MTA thread registers `UIA_Window_WindowOpenedEventId` and
//! `UIA_Window_WindowClosedEventId` handlers once on the UIA root element
//! and then pumps messages. When a window opens we read its elements (one
//! bounded walk behind the 500 ms budget), classify the text as a Phone Link
//! call toast, and emit a signal. When a window that *was* a call toast
//! closes we emit `Ended` — a dismissed or expired ring stops showing.
//!
//! Nothing here is polled: every read is the direct consequence of a UIA
//! event.

use std::collections::HashMap;
use std::sync::Mutex;

use windows::core::implement;
use windows::Win32::UI::Accessibility::{
    IUIAutomationCacheRequest, IUIAutomationElement, IUIAutomationEventHandler,
    IUIAutomationEventHandler_Impl, TreeScope_Children, UIA_Window_WindowClosedEventId,
    UIA_Window_WindowOpenedEventId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, IsWindow, TranslateMessage, MSG,
};

use super::uiakit::{self, DumpNode, UiElement};
use super::{CallSource, Emit, Signal, SignalKind, SourceCtx, SourceId};

/// Window classes Windows uses for toasts (§3 Source A).
pub const TOAST_CLASSES: &[&str] = &["Windows.UI.Core.CoreWindow", "XamlExplorerHostIslandWindow"];

/// Above these bounds the window is not a toast popup but the app itself
/// (opening Phone Link's Calls tab must not look like a ring). Toasts are a
/// few hundred DIP wide/tall; the Phone Link window is far larger. Values
/// are physical pixels, so they are generous on purpose.
const MAX_TOAST_WIDTH: i32 = 900;
const MAX_TOAST_HEIGHT: i32 = 700;

/// Toast-shaped window check: one of the known classes (or any class with
/// "toast" in it) and a popup-sized bounding box.
pub fn looks_like_toast(class: &str, width: i32, height: i32) -> bool {
    if width <= 0 || height <= 0 || width > MAX_TOAST_WIDTH || height > MAX_TOAST_HEIGHT {
        return false;
    }
    let lower = class.to_ascii_lowercase();
    TOAST_CLASSES
        .iter()
        .any(|known| class.eq_ignore_ascii_case(known))
        || lower.contains("toast")
}

/// What a toast says about a call. `caller` is the first non-chrome text
/// line; `None` means the toast carried no usable identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastCall {
    Incoming {
        caller: Option<String>,
    },
    Dialing {
        caller: Option<String>,
    },
    /// Missed / declined / ended toast: closes a ringing call if one is up.
    Ended {
        caller: Option<String>,
    },
}

/// Button labels that only appear on a ringing call.
const ANSWER_BUTTONS: &[&str] = &["answer", "accept", "decline", "ignore", "reject"];

/// Phrases, lowercased, that mean the call is already over.
const ENDED_WORDS: &[&str] = &[
    "missed call",
    "call declined",
    "declined the call",
    "call ended",
    "no answer",
    "call disconnected",
    "voice call ended",
];

/// Phrases that mean the phone is ringing.
const INCOMING_WORDS: &[&str] = &[
    "incoming call",
    "incoming video call",
    "is calling",
    "incoming voice call",
];

/// Phrases that mean we are placing the call.
const DIALING_WORDS: &[&str] = &["dialing", "outgoing call", "calling"];

/// Labels that are chrome rather than a caller: buttons, states and the
/// generic call words themselves.
const CHROME: &[&str] = &[
    "answer",
    "accept",
    "decline",
    "ignore",
    "reject",
    "dismiss",
    "snooze",
    "reply",
    "open",
    "view",
    "settings",
    "mute",
    "unmute",
    "speaker",
    "keypad",
    "hold",
    "add call",
    "end call",
    "hang up",
    "call",
    "calling",
    "phone call",
    "voice call",
    "incoming",
    "outgoing",
    "incoming call",
    "outgoing call",
    "incoming video call",
    "missed call",
    "video call",
    "on a call",
    "in a call",
    "dialing",
    "answered",
    "connected",
    "new voicemail",
    "phone link",
    "your phone",
];

/// Classify a toast's element text. `app_names` are the Phone Link display
/// names resolved at runtime (plus the `Phone Link` / `Your Phone`
/// fallbacks); with no names at all the classifier falls back to the call
/// words alone rather than reporting nothing.
pub fn classify_toast(nodes: &[DumpNode], app_names: &[String]) -> Option<ToastCall> {
    let texts: Vec<&str> = nodes
        .iter()
        .map(|node| node.name.trim())
        .filter(|text| !text.is_empty())
        .collect();
    if texts.is_empty() {
        return None;
    }
    let lower: Vec<String> = texts.iter().map(|text| text.to_lowercase()).collect();
    let contains = |needle: &str| lower.iter().any(|text| text.contains(needle));

    // App gate: this must be a Phone Link toast (spec §3 Source A).
    let app_hit = app_names.is_empty()
        || app_names.iter().any(|app| {
            let app = app.to_lowercase();
            !app.is_empty() && lower.iter().any(|text| text.contains(&app))
        });
    if !app_hit {
        return None;
    }

    let buttons = nodes.iter().any(|node| {
        node.ctrl == "Button" && ANSWER_BUTTONS.contains(&node.name.trim().to_lowercase().as_str())
    });

    let kind = if ENDED_WORDS.iter().any(|word| contains(word)) {
        ToastCall::Ended {
            caller: caller_of(nodes, app_names),
        }
    } else if buttons || INCOMING_WORDS.iter().any(|word| contains(word)) {
        ToastCall::Incoming {
            caller: caller_of(nodes, app_names),
        }
    } else if DIALING_WORDS.iter().any(|word| contains(word)) {
        ToastCall::Dialing {
            caller: caller_of(nodes, app_names),
        }
    } else {
        return None;
    };
    Some(kind)
}

/// The first text line that is not chrome: not a button, not the window
/// title (which is the app name), not the app name, not a call phrase, not a
/// clock, not a wall of text.
fn caller_of(nodes: &[DumpNode], app_names: &[String]) -> Option<String> {
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
        // "Calling..." and "Incoming call." are chrome too.
        let lower = name.trim_end_matches(['.', '!', '…']).trim().to_lowercase();
        if CHROME.contains(&lower.as_str()) {
            continue;
        }
        if app_names.iter().any(|app| app.eq_ignore_ascii_case(name)) {
            continue;
        }
        if is_clock(name) {
            continue;
        }
        return Some(name.to_string());
    }
    None
}

/// A bare `mm:ss` / `m:ss` duration (the call timer) is never a caller.
fn is_clock(text: &str) -> bool {
    let digits = text.chars().filter(|c| c.is_ascii_digit()).count();
    (1..=5).contains(&digits)
        && text.len() <= 6
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || c == ':' || c == '.')
}

/// Source A itself.
pub struct ToastSource;

impl CallSource for ToastSource {
    fn id(&self) -> SourceId {
        SourceId::Toast
    }

    fn label(&self) -> &'static str {
        "A: UIA toast listener"
    }

    fn run(self: Box<Self>, ctx: SourceCtx, emit: Emit) {
        run(ctx, emit);
    }
}

/// Element snapshot taken when a window opens.
#[derive(Default)]
struct Snapshot {
    hwnd: isize,
    class: String,
    width: i32,
    height: i32,
    nodes: Vec<DumpNode>,
}

/// What a window that reported a call was, so its close can end the call.
#[derive(Clone)]
struct TrackedToast {
    caller: Option<String>,
    kind: SignalKind,
}

fn run(ctx: SourceCtx, emit: Emit) {
    uiakit::init_mta();
    let diag = ctx.diag.clone();
    let Ok(automation) = uiakit::automation() else {
        crate::log::log("call.toast: UIA client unavailable");
        return;
    };
    // SAFETY: both calls query the UIA root; failure is reported, not fatal.
    let Ok(root) = (unsafe { automation.GetRootElement() }) else {
        crate::log::log("call.toast: UIA root unavailable");
        return;
    };
    let handler: IUIAutomationEventHandler = ToastHandler {
        ctx,
        emit,
        tracked: Mutex::new(HashMap::new()),
    }
    .into();
    // SAFETY: registering on the desktop root, alive for this thread's life.
    unsafe {
        let no_cache = None::<&IUIAutomationCacheRequest>;
        if let Err(error) = automation.AddAutomationEventHandler(
            UIA_Window_WindowOpenedEventId,
            &root,
            TreeScope_Children,
            no_cache,
            &handler,
        ) {
            crate::log::log(&format!("call.toast: WindowOpened hook failed: {error}"));
            return;
        }
        if let Err(error) = automation.AddAutomationEventHandler(
            UIA_Window_WindowClosedEventId,
            &root,
            TreeScope_Children,
            no_cache,
            &handler,
        ) {
            crate::log::log(&format!("call.toast: WindowClosed hook failed: {error}"));
        }
    }
    if let Some(diag) = &diag {
        diag.log("source A ready: WindowOpened/WindowClosed on the UIA root");
    }
    // Pump this thread's queue; UIA delivers our handlers from here.
    loop {
        let mut msg = MSG::default();
        // SAFETY: our own thread's message queue; 0 = WM_QUIT, <0 = error.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        // SAFETY: a normal dispatch of a message we just read.
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[implement(IUIAutomationEventHandler)]
struct ToastHandler {
    ctx: SourceCtx,
    emit: Emit,
    /// hwnd -> what that window told us, so its close can end the call.
    tracked: Mutex<HashMap<isize, TrackedToast>>,
}

impl ToastHandler_Impl {
    fn window_opened(&self, sender: &IUIAutomationElement) {
        let snapshot = read_window(sender);
        if snapshot.hwnd == 0 {
            return;
        }
        if let Some(diag) = &self.ctx.diag {
            diag.log(&format!(
                "toast.open hwnd={:#x} class={} size={}x{} nodes={}",
                snapshot.hwnd,
                snapshot.class,
                snapshot.width,
                snapshot.height,
                snapshot.nodes.len()
            ));
        }
        if !looks_like_toast(&snapshot.class, snapshot.width, snapshot.height) {
            if let Some(diag) = &self.ctx.diag {
                diag.log("toast.open: not a toast-shaped window, ignored");
            }
            return;
        }
        if self.ctx.dump_uia {
            if let Some(diag) = &self.ctx.diag {
                if let Some(tree) = uiakit::dump_window(snapshot.hwnd) {
                    diag.dump_uia(&format!("toast window {:#x}", snapshot.hwnd), &tree);
                }
            }
        }
        let Some(call) = classify_toast(&snapshot.nodes, &self.ctx.phonelink_names) else {
            if let Some(diag) = &self.ctx.diag {
                diag.log("toast.open: no call content");
            }
            return;
        };
        let (kind, caller) = match call {
            ToastCall::Incoming { caller } => (SignalKind::Incoming, caller),
            ToastCall::Dialing { caller } => (SignalKind::Dialing, caller),
            ToastCall::Ended { caller } => (SignalKind::Ended, caller),
        };
        if let Some(diag) = &self.ctx.diag {
            diag.log(&format!(
                "toast.call {:?} caller={:?}",
                kind,
                caller.as_deref().map(mask_preview)
            ));
        }
        if kind != SignalKind::Ended {
            if let Ok(mut tracked) = self.tracked.lock() {
                tracked.insert(
                    snapshot.hwnd,
                    TrackedToast {
                        caller: caller.clone(),
                        kind,
                    },
                );
            }
        }
        self.emit.send(Signal::new(SourceId::Toast, kind, caller));
    }

    fn window_closed(&self, sender: &IUIAutomationElement) {
        // The element may already be dead; then fall back to sweeping the
        // windows that no longer exist.
        let hwnd = read_hwnd(sender);
        let mut ended: Vec<TrackedToast> = Vec::new();
        if let Ok(mut tracked) = self.tracked.lock() {
            if hwnd != 0 {
                if let Some(known) = tracked.remove(&hwnd) {
                    ended.push(known);
                }
            } else {
                let stale: Vec<isize> = tracked
                    .keys()
                    .copied()
                    .filter(|hwnd| {
                        !unsafe {
                            IsWindow(Some(windows::Win32::Foundation::HWND(*hwnd as *mut _)))
                        }
                        .as_bool()
                    })
                    .collect();
                for hwnd in stale {
                    if let Some(known) = tracked.remove(&hwnd) {
                        ended.push(known);
                    }
                }
            }
        }
        for known in ended {
            if let Some(diag) = &self.ctx.diag {
                diag.log(&format!(
                    "toast.close hwnd={:#x} ended {:?} caller={:?}",
                    hwnd,
                    known.kind,
                    known.caller.as_deref().map(mask_preview)
                ));
            }
            self.emit.send(Signal::new(
                SourceId::Toast,
                SignalKind::Ended,
                known.caller,
            ));
        }
    }
}

impl IUIAutomationEventHandler_Impl for ToastHandler_Impl {
    fn HandleAutomationEvent(
        &self,
        sender: windows::core::Ref<'_, IUIAutomationElement>,
        eventid: windows::Win32::UI::Accessibility::UIA_EVENT_ID,
    ) -> windows::core::Result<()> {
        let Some(sender) = sender.as_ref() else {
            return Ok(());
        };
        if eventid == UIA_Window_WindowOpenedEventId {
            self.window_opened(sender);
        } else if eventid == UIA_Window_WindowClosedEventId {
            self.window_closed(sender);
        }
        Ok(())
    }
}

/// Read the window's identity and elements on a helper thread behind the
/// 500 ms budget; on timeout we report "nothing observed".
fn read_window(sender: &IUIAutomationElement) -> Snapshot {
    let element = UiElement(sender.clone());
    uiakit::with_timeout(
        "toast.read",
        move || {
            uiakit::init_mta();
            element.with(|element| {
                // SAFETY: element is cloned (AddRef) and outlives the closure.
                let hwnd = unsafe { element.CurrentNativeWindowHandle() }
                    .map(|hwnd| hwnd.0 as isize)
                    .unwrap_or(0);
                let class = unsafe { element.CurrentClassName() }
                    .map(|class| class.to_string())
                    .unwrap_or_default();
                let (width, height) = unsafe { element.CurrentBoundingRectangle() }
                    .map(|rect| (rect.right - rect.left, rect.bottom - rect.top))
                    .unwrap_or((0, 0));
                let nodes = match uiakit::automation() {
                    Ok(automation) => uiakit::dump_nodes(&automation, element),
                    Err(_) => Vec::new(),
                };
                Snapshot {
                    hwnd,
                    class,
                    width,
                    height,
                    nodes,
                }
            })
        },
        Snapshot::default(),
    )
}

/// Handle of the closing window, best effort and bounded.
fn read_hwnd(sender: &IUIAutomationElement) -> isize {
    let element = UiElement(sender.clone());
    uiakit::with_timeout(
        "toast.close.hwnd",
        move || {
            uiakit::init_mta();
            element.with(|element| {
                // SAFETY: property read on a cloned element.
                unsafe { element.CurrentNativeWindowHandle() }
                    .map(|hwnd| hwnd.0 as isize)
                    .unwrap_or(0)
            })
        },
        0,
    )
}

/// Caller text for log lines only: long digit runs are masked.
fn mask_preview(text: &str) -> String {
    crate::activities::call_state::mask_number(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    fn nodes(list: &[(&str, &str)]) -> Vec<DumpNode> {
        list.iter()
            .map(|(ctrl, name)| DumpNode {
                ctrl: (*ctrl).to_string(),
                name: (*name).to_string(),
            })
            .collect()
    }

    /// Synthesized from the documented Phone Link toast layout (the machine
    /// has no live call to record) — see the §3 report.
    const INCOMING_FIXTURE: &str = r#"
ctrl=Window "Phone Link" class=Windows.UI.Core.CoreWindow pid=4242
  ctrl=Text "Incoming call" class=Windows.UI.Core.TextBlock pid=4242
  ctrl=Text "Priya Sharma" class=Windows.UI.Core.TextBlock pid=4242
  ctrl=Text "Mobile" class=Windows.UI.Core.TextBlock pid=4242
  ctrl=Button "Answer" class=Windows.UI.UIButton pid=4242
  ctrl=Button "Decline" class=Windows.UI.UIButton pid=4242
"#;

    /// Synthesized outgoing-call toast.
    const DIALING_FIXTURE: &str = r#"
ctrl=Window "Your Phone" class=Windows.UI.Core.CoreWindow pid=4242
  ctrl=Text "Calling..." class=Windows.UI.Core.TextBlock pid=4242
  ctrl=Text "+91 98765 43210" class=Windows.UI.Core.TextBlock pid=4242
  ctrl=Button "Cancel" class=Windows.UI.UIButton pid=4242
"#;

    /// Synthesized missed-call toast: ends a ring, never starts one.
    const MISSED_FIXTURE: &str = r#"
ctrl=Window "Phone Link" class=Windows.UI.Core.CoreWindow pid=4242
  ctrl=Text "Missed call" class=Windows.UI.Core.TextBlock pid=4242
  ctrl=Text "Bo Chan" class=Windows.UI.Core.TextBlock pid=4242
  ctrl=Text "2 minutes ago" class=Windows.UI.Core.TextBlock pid=4242
"#;

    /// Synthesized Phone Link main window: must not classify.
    const MAIN_WINDOW_FIXTURE: &str = r#"
ctrl=Window "Phone Link" class=ApplicationFrameWindow pid=7777
  ctrl=Text "Calls" class=TextBlock pid=7777
  ctrl=Text "Recent" class=TextBlock pid=7777
  ctrl=Text "Incoming" class=TextBlock pid=7777
  ctrl=Button "New call" class=Button pid=7777
"#;

    #[test]
    fn incoming_fixture_is_classified_with_the_first_text_as_caller() {
        let parsed = uiakit::parse_dump(INCOMING_FIXTURE);
        assert_eq!(parsed.len(), 6);
        let call = classify_toast(&parsed, &names(&["Phone Link"])).expect("incoming toast");
        assert_eq!(
            call,
            ToastCall::Incoming {
                caller: Some("Priya Sharma".into())
            }
        );
    }

    #[test]
    fn outgoing_fixture_is_dialing_and_keeps_the_number() {
        let call = classify_toast(
            &uiakit::parse_dump(DIALING_FIXTURE),
            &names(&["Phone Link", "Your Phone"]),
        )
        .expect("dialing toast");
        assert_eq!(
            call,
            ToastCall::Dialing {
                caller: Some("+91 98765 43210".into())
            }
        );
    }

    #[test]
    fn missed_call_fixture_reports_ended() {
        let call = classify_toast(&uiakit::parse_dump(MISSED_FIXTURE), &names(&["Phone Link"]))
            .expect("missed toast");
        assert_eq!(
            call,
            ToastCall::Ended {
                caller: Some("Bo Chan".into())
            }
        );
    }

    #[test]
    fn a_app_window_with_the_same_app_name_is_not_a_call() {
        assert!(classify_toast(
            &uiakit::parse_dump(MAIN_WINDOW_FIXTURE),
            &names(&["Phone Link"])
        )
        .is_none());
    }

    #[test]
    fn other_apps_toasts_are_ignored() {
        let nodes = nodes(&[
            ("Window", "Contoso Mail"),
            ("Text", "New message from Ada"),
            ("Button", "Dismiss"),
        ]);
        assert!(classify_toast(&nodes, &names(&["Phone Link"])).is_none());
        // Even with no app names resolved, a plain mail toast is not a call.
        assert!(classify_toast(&nodes, &[]).is_none());
    }

    #[test]
    fn without_app_names_a_call_toast_still_classifies() {
        let nodes = nodes(&[
            ("Window", "Phone Link"),
            ("Text", "Incoming call"),
            ("Button", "Answer"),
            ("Button", "Decline"),
        ]);
        assert_eq!(
            classify_toast(&nodes, &[]),
            Some(ToastCall::Incoming { caller: None })
        );
    }

    #[test]
    fn toast_shape_rejects_the_big_app_window() {
        assert!(looks_like_toast("Windows.UI.Core.CoreWindow", 560, 320));
        assert!(looks_like_toast("Windows.UI.Shell.ToastWindow", 560, 320));
        assert!(!looks_like_toast("Windows.UI.Core.CoreWindow", 1400, 900));
        assert!(!looks_like_toast("ApplicationFrameWindow", 560, 320));
        assert!(!looks_like_toast("Windows.UI.Core.CoreWindow", 0, 0));
    }

    #[test]
    fn the_call_timer_is_never_a_caller() {
        let nodes = nodes(&[
            ("Window", "Phone Link"),
            ("Text", "Incoming call"),
            ("Text", "0:07"),
            ("Text", "Ada Lovelace"),
            ("Button", "Answer"),
        ]);
        let call = classify_toast(&nodes, &names(&["Phone Link"])).expect("call toast");
        assert_eq!(
            call,
            ToastCall::Incoming {
                caller: Some("Ada Lovelace".into())
            },
            "the mm:ss timer line must be skipped"
        );
    }
}
