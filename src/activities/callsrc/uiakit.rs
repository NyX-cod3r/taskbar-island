//! Shared UI Automation helpers for the call sources (spec §3).
//!
//! Rules:
//! * every UIA walk runs behind a 500 ms timeout and never on the UI
//!   thread — a hung provider can cost one source thread, never the pill;
//! * dumps are plain text lines (`ctrl=Button "Answer" class=...`) so
//!   recorded trees double as unit-test fixtures;
//! * actions report honestly: `invoke_button` distinguishes "no such
//!   button" from "Invoke failed" and never invents success.

use std::sync::mpsc;
use std::time::Duration;

use windows::core::{Interface, Result as WResult};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    TreeScope_Children, TreeScope_Descendants, TreeScope_Subtree, UIA_ButtonControlTypeId,
    UIA_EditControlTypeId, UIA_HyperlinkControlTypeId, UIA_ImageControlTypeId, UIA_InvokePatternId,
    UIA_ListItemControlTypeId, UIA_PaneControlTypeId, UIA_TextControlTypeId,
    UIA_WindowControlTypeId,
};

/// Every UIA read is wrapped in this budget (spec §3 Source A).
pub const UIA_TIMEOUT: Duration = Duration::from_millis(500);

/// A UIA element carried across the 500 ms helper boundary. The UIA core
/// marks its client objects apartment-agile, so using one from the helper
/// thread is legal; the newtype exists only to state that in `Send`.
pub struct UiElement(pub IUIAutomationElement);

// SAFETY: see above — the object behind the pointer is agile, and both
// threads only read from it.
unsafe impl Send for UiElement {}

impl UiElement {
    /// Borrow the interface inside a helper closure. Calling this method
    /// makes the closure capture the whole wrapper (Rust 2021 captures
    /// precise paths, so touching `.0` directly would bypass the `Send` impl
    /// above and fail to compile).
    pub fn with<T>(&self, work: impl FnOnce(&IUIAutomationElement) -> T) -> T {
        work(&self.0)
    }
}

/// Cap for tree dumps so one enormous window cannot fill the log.
const DUMP_LIMIT: usize = 400;

/// Run `work` on a helper thread, waiting at most [`UIA_TIMEOUT`].
/// On timeout the helper is abandoned (its result is dropped) and
/// `fallback` is returned; the timeout is logged.
pub fn with_timeout<T: Send + 'static>(
    what: &str,
    work: impl FnOnce() -> T + Send + 'static,
    fallback: T,
) -> T {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("taskbar-island-uia".into())
        .spawn(move || {
            let _ = tx.send(work());
        });
    if spawned.is_err() {
        return fallback;
    }
    match rx.recv_timeout(UIA_TIMEOUT) {
        Ok(value) => value,
        Err(_) => {
            crate::log::log(&format!("call.uia: {what} timed out after 500 ms"));
            fallback
        }
    }
}

/// A fresh UI Automation client for the calling thread (must be COM
/// initialized; every source thread calls `CoInitializeEx(MTA)` first).
pub fn automation() -> WResult<IUIAutomation> {
    // SAFETY: class id and interface pair match the UIA core server.
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
}

/// Initialize the calling thread as an MTA worker. Idempotent per thread.
pub fn init_mta() {
    // SAFETY: COINIT_MULTITHREADED on a worker thread; RPC_E_CHANGED_MODE is
    // harmless here (the thread never hosted STA).
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

/// Human control-type label for dumps and fixtures.
pub fn control_type_name(id: i32) -> &'static str {
    match id {
        x if x == UIA_WindowControlTypeId.0 => "Window",
        x if x == UIA_TextControlTypeId.0 => "Text",
        x if x == UIA_ButtonControlTypeId.0 => "Button",
        x if x == UIA_EditControlTypeId.0 => "Edit",
        x if x == UIA_ImageControlTypeId.0 => "Image",
        x if x == UIA_ListItemControlTypeId.0 => "ListItem",
        x if x == UIA_PaneControlTypeId.0 => "Pane",
        x if x == UIA_HyperlinkControlTypeId.0 => "Hyperlink",
        _ => "Other",
    }
}

/// Render the subtree as fixture-friendly lines:
/// `ctrl=Button "Answer" class=Windows.UI.Core.CoreWindow pid=1234`.
/// Truncated at [`DUMP_LIMIT`] elements.
pub fn tree_dump(automation: &IUIAutomation, root: &IUIAutomationElement) -> String {
    let mut out = String::new();
    dump_into(automation, root, 0, &mut out);
    out
}

fn dump_into(
    automation: &IUIAutomation,
    element: &IUIAutomationElement,
    depth: usize,
    out: &mut String,
) {
    if out.lines().count() >= DUMP_LIMIT {
        return;
    }
    let name = unsafe { element.CurrentName() }
        .map(|name| name.to_string())
        .unwrap_or_default();
    let name = name.replace(['\n', '\r'], " ");
    let class = unsafe { element.CurrentClassName() }
        .map(|class| class.to_string())
        .unwrap_or_default();
    let class = class.replace(['\n', '\r'], " ");
    let ctrl = unsafe { element.CurrentControlType() }
        .map(|ctrl| control_type_name(ctrl.0))
        .unwrap_or("Other");
    let pid = unsafe { element.CurrentProcessId() }.unwrap_or(0);
    out.push_str(&format!(
        "{indent}ctrl={ctrl} \"{name}\" class={class} pid={pid}\n",
        indent = "  ".repeat(depth),
    ));
    let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else {
        return;
    };
    let Ok(children) = (unsafe { element.FindAll(TreeScope_Children, &condition) }) else {
        return;
    };
    let Ok(count) = (unsafe { children.Length() }) else {
        return;
    };
    for index in 0..count.min(DUMP_LIMIT as i32) {
        let Ok(child) = (unsafe { children.GetElement(index) }) else {
            continue;
        };
        // Depth-first with a flat budget: grandchildren are appended at the
        // next depth until the cap is hit.
        dump_into(automation, &child, depth + 1, out);
    }
}

/// Why an action could not be delivered. `NotFound` is the honest
/// "Open Phone Link to do this" case (spec §3 Actions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeError {
    /// No matching button exists in the window right now.
    NotFound,
    /// A button was found but Invoke failed (the action did not happen).
    Failed(String),
    /// The window itself could not be read (UIA unavailable).
    Unavailable,
}

/// Invoke on the *current* thread. Call it from inside [`with_timeout`] (or
/// any other helper that has already called [`init_mta`]); it does not spawn
/// a nested helper of its own.
pub fn invoke_here(
    root: &IUIAutomationElement,
    candidates: &[String],
) -> std::result::Result<(), InvokeError> {
    let Ok(automation) = automation() else {
        return Err(InvokeError::Unavailable);
    };
    let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else {
        return Err(InvokeError::Unavailable);
    };
    let Ok(items) = (unsafe { root.FindAll(TreeScope_Descendants, &condition) }) else {
        return Err(InvokeError::Unavailable);
    };
    let Ok(count) = (unsafe { items.Length() }) else {
        return Err(InvokeError::Unavailable);
    };
    let mut found: Option<IUIAutomationElement> = None;
    for index in 0..count.min(DUMP_LIMIT as i32) {
        let Ok(element) = (unsafe { items.GetElement(index) }) else {
            continue;
        };
        let Ok(name) = (unsafe { element.CurrentName() }) else {
            continue;
        };
        if candidates
            .iter()
            .any(|candidate| candidate == &name.to_string())
        {
            found = Some(element);
            break;
        }
    }
    let Some(target) = found else {
        return Err(InvokeError::NotFound);
    };
    let pattern = unsafe { target.GetCurrentPattern(UIA_InvokePatternId) }
        .map_err(|error| InvokeError::Failed(format!("pattern: {error}")))?;
    let invoke = pattern
        .cast::<IUIAutomationInvokePattern>()
        .map_err(|error| InvokeError::Failed(format!("cast: {error}")))?;
    unsafe { invoke.Invoke() }.map_err(|error| InvokeError::Failed(format!("invoke: {error}")))
}

/// One classified element: the control-type label and the Name string. The
/// same shape comes out of a live UIA walk and out of a recorded fixture, so
/// [`parse_dump`] round-trips exactly what [`tree_dump`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpNode {
    pub ctrl: String,
    pub name: String,
}

/// Parse recorded dump lines back into nodes:
/// `  ctrl=Button "Answer" class=... pid=1234` -> `Button / Answer`.
/// Blank lines and anything that does not start with `ctrl=` are skipped, so
/// a whole `--dump-uia` block (including its `--- header ---` lines) can be
/// handed in as a fixture. Test-only: production walks the live tree.
#[cfg(test)]
pub fn parse_dump(text: &str) -> Vec<DumpNode> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("ctrl=") else {
            continue;
        };
        let Some((ctrl, rest)) = rest.split_once(' ') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            continue;
        };
        out.push(DumpNode {
            ctrl: ctrl.to_string(),
            name: rest[..end].to_string(),
        });
    }
    out
}

/// Live descendants as [`DumpNode`]s (control type + name), bounded by the
/// 500 ms budget of [`typed_names`].
pub fn dump_nodes(automation: &IUIAutomation, root: &IUIAutomationElement) -> Vec<DumpNode> {
    typed_names(automation, root)
        .into_iter()
        .map(|(id, name)| DumpNode {
            ctrl: control_type_name(id).to_string(),
            name,
        })
        .collect()
}

/// Full nested tree of a top-level window for `--dump-uia`, behind the
/// 500 ms budget. `None` when the window could not be read.
pub fn dump_window(hwnd: isize) -> Option<String> {
    if hwnd == 0 {
        return None;
    }
    let tree = with_timeout(
        "dump_window",
        move || {
            init_mta();
            let Ok(automation) = automation() else {
                return String::new();
            };
            // SAFETY: the caller read this handle moments ago.
            let Ok(element) = (unsafe {
                automation.ElementFromHandle(windows::Win32::Foundation::HWND(hwnd as *mut _))
            }) else {
                return String::new();
            };
            tree_dump(&automation, &element)
        },
        String::new(),
    );
    if tree.is_empty() {
        None
    } else {
        Some(tree)
    }
}

/// Root UIA element of a window handle. Call it from inside
/// [`with_timeout`] with COM already initialized (the action worker does).
pub fn root_from_hwnd(hwnd: isize) -> Option<IUIAutomationElement> {
    let automation = automation().ok()?;
    // SAFETY: the caller read this handle moments ago and keeps it alive.
    unsafe { automation.ElementFromHandle(windows::Win32::Foundation::HWND(hwnd as *mut _)) }.ok()
}

/// Control type of the element, for classification (timer texts live under
/// Text controls, buttons under Button).
pub fn control_type(element: &IUIAutomationElement) -> i32 {
    unsafe { element.CurrentControlType() }
        .map(|ctrl| ctrl.0)
        .unwrap_or(0)
}

/// Bounded property read used by Source C: (control type, name) pairs of all
/// descendants, behind the 500 ms budget. Returned as raw fixture lines so
/// the classifier and the dump format stay identical.
pub fn typed_names(automation: &IUIAutomation, root: &IUIAutomationElement) -> Vec<(i32, String)> {
    let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else {
        return Vec::new();
    };
    // Subtree again: the window's own Name is the toast app title.
    let Ok(items) = (unsafe { root.FindAll(TreeScope_Subtree, &condition) }) else {
        return Vec::new();
    };
    let Ok(count) = (unsafe { items.Length() }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for index in 0..count.min(DUMP_LIMIT as i32) {
        let Ok(element) = (unsafe { items.GetElement(index) }) else {
            continue;
        };
        let Ok(name) = (unsafe { element.CurrentName() }) else {
            continue;
        };
        let name = name.to_string();
        let name = name.trim().to_string();
        if !name.is_empty() {
            out.push((control_type(&element), name));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_timeout_returns_the_value_when_fast() {
        let value = with_timeout("fast", || 7, 0);
        assert_eq!(value, 7);
    }

    #[test]
    fn with_timeout_falls_back_when_the_work_hangs() {
        let start = std::time::Instant::now();
        let value = with_timeout(
            "hang",
            || {
                std::thread::sleep(Duration::from_secs(60));
                7
            },
            0,
        );
        assert_eq!(value, 0);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "must not wait 60 s"
        );
    }

    #[test]
    fn control_type_labels_cover_the_fixture_types() {
        assert_eq!(control_type_name(UIA_ButtonControlTypeId.0), "Button");
        assert_eq!(control_type_name(UIA_TextControlTypeId.0), "Text");
        assert_eq!(control_type_name(UIA_WindowControlTypeId.0), "Window");
        assert_eq!(control_type_name(-1), "Other");
    }

    #[test]
    fn parse_dump_round_trips_a_recorded_tree() {
        let dump = "--- toast 0x1234 ---\n  ctrl=Window \"Phone Link\" class=Windows.UI.Core.CoreWindow pid=9\n    ctrl=Text \"Incoming call\" class=TextBlock pid=9\n    ctrl=Button \"Answer\" class=Button pid=9\n    ctrl=Button \"Decline\" class=Button pid=9\n--- end ---\n";
        let nodes = parse_dump(dump);
        assert_eq!(nodes.len(), 4);
        assert_eq!(nodes[0].ctrl, "Window");
        assert_eq!(nodes[0].name, "Phone Link");
        assert_eq!(nodes[1].name, "Incoming call");
        assert_eq!(nodes[3].name, "Decline");
        assert!(parse_dump("").is_empty());
        assert!(parse_dump("not a dump line").is_empty());
    }
}
