//! UIA actions for the Call card (spec §3 Actions).
//!
//! Windows exposes no API that controls another app's call, so Answer,
//! Decline and End are real `InvokePattern` presses on the button the
//! provider actually shows — in the call toast (Source A's window) or in
//! the Phone Link window (Source C's). Nothing is faked: the outcome that
//! comes back to the UI is what really happened, and `NotFound` becomes the
//! honest "Open Phone Link to do this" message.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetParent, GetWindowRect, GetWindowThreadProcessId,
    IsWindowVisible, PostMessageW,
};

use super::phonelink::{self, PHONE_PROCESSES};
use super::toast::looks_like_toast;
use super::uiakit::{self, InvokeError};
use crate::activities::call::{ActionOutcome, CALL_WAKE_ACTION, CALL_WAKE_HOLD, WM_APP_CALL};

/// One button press the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallAction {
    Answer,
    Decline,
    End,
    /// Phone Link's Hold button (live call only).
    Hold,
    /// Phone Link's Resume button - what Hold reads while the call is held.
    Resume,
    /// Hand the call back to the phone (Phone Link's own transfer control,
    /// v0.9 §4's "Phone" row in the call audio sheet).
    Transfer,
}

impl CallAction {
    /// Provider button labels to look for, exact match on `Name`.
    pub fn labels(self) -> &'static [&'static str] {
        match self {
            Self::Answer => &["Answer", "Accept"],
            Self::Decline => &["Decline", "Ignore", "Reject"],
            Self::End => phonelink::END_BUTTONS,
            Self::Hold => &["Hold"],
            Self::Resume => &["Resume"],
            // Phone Link words this control differently between builds, so
            // every label seen in the wild is tried before giving up.
            Self::Transfer => &["Phone", "Transfer to phone", "Transfer call"],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Decline => "decline",
            Self::End => "end",
            Self::Hold => "hold",
            Self::Resume => "resume",
            Self::Transfer => "transfer",
        }
    }
}

/// Run `action` on a worker thread (never the UI thread) and report back
/// with `WM_APP_CALL`, wparam [`CALL_WAKE_ACTION`], lparam the
/// [`ActionOutcome`] code.
pub fn invoke_async(island: isize, action: CallAction) {
    let spawned = std::thread::Builder::new()
        .name(format!("taskbar-island-call-{}", action.as_str()))
        .spawn(move || {
            let outcome = perform(action);
            crate::log::log(&format!(
                "call.action {} -> {}",
                action.as_str(),
                match outcome {
                    ActionOutcome::Invoked => "invoked",
                    ActionOutcome::NotFound => "not found",
                    ActionOutcome::Failed => "failed",
                }
            ));
            // SAFETY: posting to our own island window.
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(island as *mut _)),
                    WM_APP_CALL,
                    WPARAM(CALL_WAKE_ACTION),
                    LPARAM(outcome.code()),
                );
            }
        });
    if let Err(error) = spawned {
        crate::log::log(&format!("call.action spawn failed: {error}"));
    }
}

/// One bounded attempt: find the window that can do it and invoke there.
fn perform(action: CallAction) -> ActionOutcome {
    let windows = candidate_windows(action);
    if windows.is_empty() {
        return ActionOutcome::NotFound;
    }
    let labels: Vec<String> = action
        .labels()
        .iter()
        .map(|label| (*label).to_string())
        .collect();
    // The whole search + invoke shares one 500 ms budget, so a hung UIA
    // provider costs this worker thread only.
    uiakit::with_timeout(
        "call.action",
        move || {
            uiakit::init_mta();
            for hwnd in windows {
                let Some(root) = uiakit::root_from_hwnd(hwnd) else {
                    continue;
                };
                match uiakit::invoke_here(&root, &labels) {
                    Ok(()) => return ActionOutcome::Invoked,
                    // This window has no such button: try the next one.
                    Err(InvokeError::NotFound) => continue,
                    Err(InvokeError::Failed(_)) | Err(InvokeError::Unavailable) => {
                        return ActionOutcome::Failed
                    }
                }
            }
            ActionOutcome::NotFound
        },
        ActionOutcome::Failed,
    )
}

/// Windows that plausibly hold the button, best candidate first.
fn candidate_windows(action: CallAction) -> Vec<isize> {
    let mut top_level: Vec<isize> = Vec::new();
    // SAFETY: the callback only pushes handles into the vector we own.
    unsafe {
        let _ = EnumWindows(
            Some(collect_top_level),
            LPARAM(&mut top_level as *mut Vec<isize> as isize),
        );
    }
    top_level
        .into_iter()
        .filter(|hwnd| accepts(*hwnd, action))
        .collect()
}

/// SAFETY: run with the address of a live `Vec<isize>`.
unsafe extern "system" fn collect_top_level(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    // SAFETY: caller contract above.
    let out = &mut *(lparam.0 as *mut Vec<isize>);
    out.push(hwnd.0 as isize);
    windows::core::BOOL(1)
}

fn accepts(hwnd: isize, action: CallAction) -> bool {
    if hwnd == 0 {
        return false;
    }
    let handle = HWND(hwnd as *mut _);
    match action {
        // Answer/Decline only exist on a ringing toast.
        CallAction::Answer | CallAction::Decline => {
            // SAFETY: handle is alive (it came from EnumWindows).
            if !unsafe { IsWindowVisible(handle) }.as_bool() {
                return false;
            }
            let mut class = [0u16; 256];
            // SAFETY: writable buffer of the stated length.
            let len = unsafe { GetClassNameW(handle, &mut class) };
            let class = if len > 0 {
                String::from_utf16_lossy(&class[..len as usize])
            } else {
                String::new()
            };
            let rect = {
                let mut rect = windows::Win32::Foundation::RECT::default();
                // SAFETY: handle is alive (it came from EnumWindows) and
                // `rect` is a writable out parameter.
                unsafe { GetWindowRect(handle, &mut rect) }
                    .map(|_| (rect.right - rect.left, rect.bottom - rect.top))
                    .unwrap_or((0, 0))
            };
            looks_like_toast(&class, rect.0, rect.1)
        }
        // End/Hold/Resume/Transfer all live in the Phone Link window (a
        // minimized one still counts).
        CallAction::End | CallAction::Hold | CallAction::Resume | CallAction::Transfer => {
            is_phone_window(hwnd)
        }
    }
}

/// Top-level window of a Phone Link process.
fn is_phone_window(hwnd: isize) -> bool {
    let handle = HWND(hwnd as *mut _);
    // Top level: no parent.
    // SAFETY: handle validity is the caller's contract.
    if unsafe { GetParent(handle) }.is_ok() {
        return false;
    }
    let mut pid = 0u32;
    // SAFETY: optional out pointer, written only when non-null.
    unsafe { GetWindowThreadProcessId(handle, Some(&mut pid)) };
    phonelink::process_name(pid).is_some_and(|name| {
        PHONE_PROCESSES
            .iter()
            .any(|known| name.eq_ignore_ascii_case(known))
    })
}

/// Probe Phone Link's own Hold/Resume button and report it with
/// `WM_APP_CALL` / [`CALL_WAKE_HOLD`]: `lparam` bit 0 = Hold exists for this
/// call, bit 1 = it currently reads "Resume" (the call is on hold). The card
/// only ever claims what the walk actually found; the matched button name is
/// logged so a `--dump-uia` capture can cross-check it. Never the UI thread.
pub fn probe_hold_async(island: isize) {
    let spawned = std::thread::Builder::new()
        .name("taskbar-island-call-hold-probe".into())
        .spawn(move || {
            let (available, held, matched) = probe_hold();
            crate::log::log(&format!(
                "call.hold probe: {}",
                matched.as_deref().unwrap_or("no Hold/Resume button")
            ));
            // SAFETY: posting to our own island window.
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(island as *mut _)),
                    WM_APP_CALL,
                    WPARAM(CALL_WAKE_HOLD),
                    LPARAM(i32::from(available) as isize | (i32::from(held) as isize) << 1),
                );
            }
        });
    if let Err(error) = spawned {
        crate::log::log(&format!("call.hold probe spawn failed: {error}"));
    }
}

/// One bounded walk over the Phone Link windows: a button named `Resume`
/// proves both availability and the held state, one named `Hold` proves
/// availability only, and neither proves nothing (the card keeps Hold
/// disabled). `(available, held, matched-name)`.
fn probe_hold() -> (bool, bool, Option<String>) {
    let windows = candidate_windows(CallAction::Hold);
    if windows.is_empty() {
        return (false, false, None);
    }
    uiakit::with_timeout(
        "call.hold-probe",
        move || {
            uiakit::init_mta();
            let Ok(automation) = uiakit::automation() else {
                return (false, false, None);
            };
            for hwnd in windows {
                let Some(root) = uiakit::root_from_hwnd(hwnd) else {
                    continue;
                };
                let names = uiakit::typed_names(&automation, &root);
                // Resume first: a window that shows both means the call is
                // held right now, and "Resume" is the stronger fact.
                if names.iter().any(|(_, name)| name == "Resume") {
                    return (true, true, Some("Resume".to_string()));
                }
                if names.iter().any(|(_, name)| name == "Hold") {
                    return (true, false, Some("Hold".to_string()));
                }
            }
            (false, false, None)
        },
        (false, false, None),
    )
}
