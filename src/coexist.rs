//! §4: coexistence with the other taskbar customizers.
//!
//! TranslucentTB, Windhawk, ExplorerPatcher, StartAllBack, TaskbarX and
//! RoundedTB all take ownership of `Shell_TrayWnd` in some way. Embedding
//! the pill as a `WS_CHILD` of a bar one of them has re-styled is the one
//! configuration that has ever produced Event Viewer 1000/1002 (an unhandled
//! exception in the taskbar's own window procedure) plus a WER CrashDump, so
//! v0.12 refuses to embed while one is present and falls back to the
//! pre-0.11 *separate* pill instead (the card window draws it).
//!
//! Detection is event-driven end to end:
//!
//! * **at launch** one `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` pass
//!   matches running processes, then a `TH32CS_SNAPMODULE` pass over
//!   `explorer.exe` catches the two tools that are *not* processes of their
//!   own (ExplorerPatcher and StartAllBack live as DLLs inside Explorer);
//! * **after launch** a worker thread blocks in
//!   `IWbemServices::ExecNotificationQuery` on
//!   `SELECT * FROM __InstanceCreationEvent WITHIN 1 WHERE TargetInstance
//!   ISA 'Win32_Process'` and only wakes for a real process creation —
//!   no timer, no scan loop;
//! * **on `TaskbarCreated`** the whole scan runs again, because a restarted
//!   Explorer loads whatever is installed into it afresh.
//!
//! The scan is re-entrant and idempotent: `FOUND` only ever moves off zero
//! once, so a later tool starting never re-raises the one-time notice.
//!
//! Two WMI requirements cost a day to find and are load-bearing here, so
//! they are recorded rather than rediscovered:
//!
//! * **`CoInitializeSecurity` must run first, in `main`.** Without an
//!   explicit call COM falls back to implicit defaults that register no
//!   authentication service, and the WMI service then answers *every* read
//!   with `WBEM_E_ACCESS_DENIED` — including a plain `GetObject`, with no
//!   query involved. A `CoSetProxyBlanket` on our proxy cannot repair it.
//!   `main` therefore does `CoInitializeEx(STA)` + `CoInitializeSecurity`
//!   before any other COM work (the UI thread's later `OleInitialize` is
//!   happy with a thread that is already STA — verified).
//! * **the notification query needs `WBEM_FLAG_FORWARD_ONLY |
//!   WBEM_FLAG_RETURN_IMMEDIATELY` together.** Either bit alone, or the
//!   synchronous `0`, is rejected with `WBEM_E_INVALID_PARAMETER`
//!   (`0x80041008`). `ExecQuery` cannot be used as a substitute: it opens an
//!   event query happily and then fails the first `Next` with
//!   `WBEM_E_INVALID_QUERY`.

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::core::{w, IUnknown, Interface, BSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
    MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{
    IEnumWbemClassObject, IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator,
    WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// The island window learns about a tool started while it is already up.
/// `WM_APP + 56`; the next free id is `+57`.
pub const WM_APP_TOOL_DETECTED: u32 = WM_APP + 56;

/// Display names, indexed by `FOUND - 1` (0 = nothing detected). Kept as a
/// table of `&'static str` so the shared state stays a single `AtomicUsize`.
const LABELS: &[&str] = &[
    "TranslucentTB",
    "Windhawk",
    "TaskbarX",
    "RoundedTB",
    "StartAllBack",
    "ExplorerPatcher",
];

const L_TRANSLUCENTTB: usize = 0;
const L_WINDHAWK: usize = 1;
const L_TASKBARX: usize = 2;
const L_ROUNDEDTB: usize = 3;
const L_STARTALLBACK: usize = 4;
const L_EXPLORERPATCHER: usize = 5;

/// Executable names (lower case) that mean "a tool owns the taskbar".
const TOOLS: &[(&str, usize)] = &[
    ("translucenttb.exe", L_TRANSLUCENTTB),
    ("windhawk.exe", L_WINDHAWK),
    ("windhawk_svc.exe", L_WINDHAWK),
    ("taskbarx.exe", L_TASKBARX),
    ("taskbarxconfigurator.exe", L_TASKBARX),
    ("roundedtb.exe", L_ROUNDEDTB),
    ("startallbackcfg.exe", L_STARTALLBACK),
    ("startallbacktray.exe", L_STARTALLBACK),
];

/// Path fragments of the DLLs ExplorerPatcher and StartAllBack inject into
/// `explorer.exe`. Matched against a module's `szExePath`, lower-cased, so
/// the install location does not matter.
const INJECT_DIRS: &[(&str, usize)] = &[
    ("\\explorerpatcher\\", L_EXPLORERPATCHER),
    ("\\startallback\\", L_STARTALLBACK),
];

/// WQL: one event per *created* process, sampled by WMI at 1 s. Our side
/// blocks in `Next(WBEM_INFINITE)` until one arrives.
const PROCESS_QUERY: &str =
    "SELECT * FROM __InstanceCreationEvent WITHIN 1 WHERE TargetInstance ISA 'Win32_Process'";

/// 0 = nothing found, otherwise `LABELS` index + 1.
static FOUND: AtomicUsize = AtomicUsize::new(0);
/// The island window, so the worker can announce a late arrival.
static ISLAND: AtomicUsize = AtomicUsize::new(0);

/// True while a known taskbar tool is (or was) present this session.
pub fn active() -> bool {
    FOUND.load(Ordering::Relaxed) != 0
}

/// The tool that engaged safe mode, if any.
pub fn found() -> Option<&'static str> {
    let value = FOUND.load(Ordering::Relaxed);
    (value != 0).then(|| LABELS[value - 1])
}

/// Announce the island window to the watcher (and to `scan` re-runs).
pub fn watch(hwnd: HWND) {
    ISLAND.store(hwnd.0 as usize, Ordering::Relaxed);
    // Fire-and-forget like the media/call/clipboard workers: the process
    // exit ends it, and it spends its life blocked in `Next`.
    std::thread::spawn(process_watcher);
}

/// One synchronous pass over the running processes (and over Explorer's
/// modules). Called before the first embed decision and again on
/// `TaskbarCreated`. Safe to call repeatedly; only the first hit counts.
pub fn scan() {
    let Some(index) = find_tool() else {
        return;
    };
    crate::log::log(&format!("coexist.scan: {} is running", LABELS[index]));
    if engage(index) {
        announce();
    }
}

/// Record the first tool found. Returns true only on the 0 -> n transition,
/// which is what makes the tray notice a one-time event.
fn engage(index: usize) -> bool {
    FOUND
        .compare_exchange(0, index + 1, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

fn announce() {
    let raw = ISLAND.load(Ordering::Relaxed);
    if raw == 0 {
        return;
    }
    // SAFETY: the handle came from `watch` and belongs to this process.
    unsafe {
        let _ = PostMessageW(
            Some(HWND(raw as *mut core::ffi::c_void)),
            WM_APP_TOOL_DETECTED,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );
    }
}

// ---------------------------------------------------------------------------
// The scans.

fn find_tool() -> Option<usize> {
    let (tool, explorer) = scan_processes();
    tool.or_else(|| explorer.and_then(explorer_tool))
}

/// One `TH32CS_SNAPPROCESS` pass: the first known tool wins, and the pid of
/// `explorer.exe` is remembered for the module pass.
fn scan_processes() -> (Option<usize>, Option<u32>) {
    let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(handle) => Guard(handle),
        Err(error) => {
            crate::log::log_err("coexist.scan", &error);
            return (None, None);
        }
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = None;
    let mut explorer = None;
    // SAFETY: `entry` is a live, correctly sized buffer for the snapshot.
    let mut status = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while status.is_ok() {
        let exe = wide(&entry.szExeFile).to_lowercase();
        if found.is_none() {
            found = TOOLS.iter().find(|(name, _)| *name == exe).map(|(_, l)| *l);
        }
        if explorer.is_none() && exe == "explorer.exe" {
            explorer = Some(entry.th32ProcessID);
        }
        if found.is_some() && explorer.is_some() {
            break;
        }
        // SAFETY: same live buffer, next row of the same snapshot.
        status = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    (found, explorer)
}

/// The two in-process tools: `TH32CS_SNAPMODULE` over `explorer.exe` and a
/// path-fragment match, so an uninstalled tool cannot leave a false positive
/// the way a stale registry key would.
fn explorer_tool(pid: u32) -> Option<usize> {
    let snapshot =
        match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) } {
            Ok(handle) => Guard(handle),
            Err(error) => {
                // Explorer can run at a higher integrity level than us; that is
                // a documented limitation, not a failure to hide.
                crate::log::log(&format!("coexist.scan: module snapshot failed ({error})"));
                return None;
            }
        };
    let mut entry = MODULEENTRY32W {
        dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: `entry` is a live, correctly sized buffer for the snapshot.
    let mut status = unsafe { Module32FirstW(snapshot.0, &mut entry) };
    while status.is_ok() {
        let path = wide(&entry.szExePath).to_lowercase();
        if let Some(index) = INJECT_DIRS.iter().find(|(frag, _)| path.contains(frag)) {
            return Some(index.1);
        }
        // SAFETY: same live buffer, next row of the same snapshot.
        status = unsafe { Module32NextW(snapshot.0, &mut entry) };
    }
    None
}

fn wide(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// Closes a ToolHelp snapshot on every path out of a scan.
struct Guard(HANDLE);

impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateToolhelp32Snapshot above and is
        // closed exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// ---------------------------------------------------------------------------
// The watcher thread.

/// Block on WMI's process-creation event for the life of the process.
fn process_watcher() {
    // SAFETY: matched with the CoUninitialize below on this thread.
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        crate::log::log("coexist.watch: CoInitializeEx failed; no late detection");
        return;
    }
    match watch_loop() {
        Ok(()) => crate::log::log("coexist.watch: event stream ended"),
        Err(error) => crate::log::log_err("coexist.watch", &error),
    }
    // SAFETY: balances the successful CoInitializeEx above.
    unsafe { CoUninitialize() };
}

/// `WBEM_*` codes are opaque in a single line, so name the step that failed.
fn step<T>(name: &str, outcome: windows::core::Result<T>) -> windows::core::Result<T> {
    if let Err(error) = &outcome {
        crate::log::log(&format!("coexist.watch: {name} failed ({error})"));
    }
    outcome
}

fn watch_loop() -> windows::core::Result<()> {
    // SAFETY: the CLSID constant is the published WbemLocator class id and
    // the interface is the one that class exposes.
    let locator: IWbemLocator = step("CoCreateInstance", unsafe {
        CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)
    })?;
    let services: IWbemServices = step("ConnectServer", unsafe {
        locator.ConnectServer(
            &BSTR::from("root\\cimv2"),
            &BSTR::new(),
            &BSTR::new(),
            &BSTR::new(),
            0,
            &BSTR::new(),
            None,
        )
    })?;
    // No `CoSetProxyBlanket` here: process-wide `CoInitializeSecurity` (set
    // once at the top of `main`) is what the WMI service actually checks.
    // Without it *every* read fails with ACCESS_DENIED — even a plain
    // `GetObject` — and a proxy-level blanket cannot repair that, because the
    // implicit defaults register no authentication service at all.
    let enumerator: IEnumWbemClassObject = step("ExecNotificationQuery", unsafe {
        services.ExecNotificationQuery(
            &BSTR::from("WQL"),
            &BSTR::from(PROCESS_QUERY),
            // Both bits are required *together*: FORWARD_ONLY alone or
            // RETURN_IMMEDIATELY alone is rejected with
            // WBEM_E_INVALID_PARAMETER (0x80041008), as is the synchronous
            // 0. This is the pair every WMI event client uses.
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        )
    })?;
    crate::log::log("coexist.watch: listening for process creation");
    loop {
        let mut objects: [Option<IWbemClassObject>; 1] = [None];
        let mut returned = 0u32;
        // WBEM_INFINITE is (DWORD)-1: the thread sleeps here, it never spins.
        let status = unsafe { enumerator.Next(-1, &mut objects, &mut returned) };
        if status.is_err() {
            return Err(windows::core::Error::from_hresult(status));
        }
        let Some(event) = objects[0].take() else {
            continue;
        };
        if let Some(index) = created_tool(&event) {
            // Logged on every hit, not just the first: the list is eight
            // executables, so a line here means a taskbar tool really did
            // start — and it is the only proof the event stream is alive.
            crate::log::log(&format!("coexist.watch: {} started", LABELS[index]));
            if engage(index) {
                announce();
            }
        }
    }
}

/// Pull `TargetInstance.Name` out of a `__Instance*Event` object.
fn target_name(event: &IWbemClassObject) -> Option<String> {
    let mut target = VARIANT::default();
    // SAFETY: `event` is the object WMI just handed us; `target` is a live
    // out parameter that VARIANT's Drop clears.
    unsafe { event.Get(w!("TargetInstance"), 0, &mut target, None, None) }.ok()?;
    let unknown = IUnknown::try_from(&target).ok()?;
    let process: IWbemClassObject = unknown.cast().ok()?;
    let mut name = VARIANT::default();
    // SAFETY: as above, on the instance WMI embedded in the event.
    unsafe { process.Get(w!("Name"), 0, &mut name, None, None) }.ok()?;
    Some(BSTR::try_from(&name).ok()?.to_string().to_lowercase())
}

/// The tool a `__InstanceCreationEvent` names, if it is one of ours.
fn created_tool(event: &IWbemClassObject) -> Option<usize> {
    let exe = target_name(event)?;
    TOOLS.iter().find(|(n, _)| *n == exe).map(|(_, l)| *l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_maps_to_a_label() {
        for (exe, label) in TOOLS {
            assert!(*label < LABELS.len(), "{exe} points past LABELS");
            assert!(!LABELS[*label].is_empty());
        }
        for (fragment, label) in INJECT_DIRS {
            assert!(*label < LABELS.len(), "{fragment} points past LABELS");
            assert!(fragment.starts_with('\\') && fragment.ends_with('\\'));
        }
    }

    #[test]
    fn labels_cover_the_six_tools_of_the_spec() {
        for expected in [
            "TranslucentTB",
            "Windhawk",
            "ExplorerPatcher",
            "StartAllBack",
            "TaskbarX",
            "RoundedTB",
        ] {
            assert!(LABELS.contains(&expected), "{expected} is not detected");
        }
    }

    #[test]
    fn wide_trims_at_the_nul() {
        assert_eq!(wide(&[b'a' as u16, b'b' as u16, 0, b'c' as u16]), "ab");
        assert_eq!(wide(&[]), "");
    }

    #[test]
    fn find_tool_does_not_match_common_names() {
        // Guards the case-folding path: the comparison is lower-case on both
        // sides, so `TranslucentTB.exe` must not sneak past a mixed case.
        let exe = "TranslucentTB.exe".to_lowercase();
        assert!(TOOLS.iter().any(|(name, _)| *name == exe));
        assert!(!TOOLS.iter().any(|(name, _)| *name == "notepad.exe"));
    }
}
