//! TaskbarIsland entry point.

#![windows_subsystem = "windows"]

mod activities;
mod anchor;
mod anim;
#[allow(dead_code)]
mod clipboard;
mod convert;
mod design;
// The shelf/convert phases consume the rest of this API; Phase 1 lands the
// capability matrix and router ahead of the UI.
#[allow(dead_code)]
mod engines;
mod gpu;
mod icons;
mod json;
mod log;
mod offline;
mod render;
mod settings;
mod state;
mod theme;
mod thumb;
// Token drift validation: the JSON document + parser are consumed by the
// design-tokens test (and tooling), not by the hot path (`design` owns that).
#[allow(dead_code)]
mod tokens;
mod tooltip;
mod tray;
mod uia;
mod view;
mod window;

use std::sync::atomic::Ordering;

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};

use activities::callsrc;

fn main() {
    // Spec §3 diagnostics are parsed first so they run without a taskbar,
    // without the single-instance mutex and without creating a window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(seconds) = diag_call_seconds(&args) {
        // Source D stays opt-in exactly as it is in the running app.
        let use_wpn_db = settings::Settings::load().call_use_wpn_db;
        let dump_uia = args.iter().any(|arg| arg == "--dump-uia");
        match callsrc::run_diag(seconds, dump_uia, use_wpn_db) {
            Ok(path) => println!("--diag-call log: {}", path.display()),
            Err(error) => eprintln!("--diag-call failed: {error}"),
        }
        return;
    }
    if args.iter().any(|arg| arg == "--dump-uia") {
        callsrc::DUMP_UIA.store(true, Ordering::Relaxed);
    }
    if let Some(index) = args.iter().position(|arg| arg == "--simulate-call") {
        let Some(kind) = args
            .get(index + 1)
            .and_then(|value| callsrc::SimKind::parse(value))
        else {
            eprintln!("--simulate-call needs incoming|dialing|active|end");
            return;
        };
        // A running island takes it now; otherwise the code is parked for
        // the island this launch is about to create.
        if !deliver_simulation(kind) {
            callsrc::PENDING_SIMULATE.store(kind.code(), Ordering::Relaxed);
        }
    }
    // Anchor geometry is measured in physical screen coordinates. Without
    // per-monitor awareness Windows virtualizes the popup bounds at scaled
    // displays, moving the pill away from the taskbar hit area.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // Groups the Start menu shortcut, tray icon and process under one
        // shell identity instead of the generic "taskbar-island.exe".
        let _ = SetCurrentProcessExplicitAppUserModelID(w!("TaskbarIsland.TaskbarIsland"));
    }
    theme::enable_native_dark_menus();
    // Held for the life of the process. The installer has the same name in
    // `AppMutex`, which lets it detect and close a running island before an
    // upgrade. A second launch exits before touching any window.
    let _instance_mutex = unsafe { CreateMutexW(None, false, w!("TaskbarIsland.SingleInstance")) };
    if _instance_mutex.is_ok() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }
    let instance = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => module.into(),
        Err(error) => {
            eprintln!("unable to get module handle: {error}");
            return;
        }
    };

    let Some(taskbar) = anchor::taskbar_window() else {
        eprintln!("Windows taskbar is not available");
        return;
    };
    // Both debug and release builds register this class. Refuse to create a
    // second surface when an older instance is already running.
    if unsafe { FindWindowW(w!("TaskbarIsland"), windows::core::PCWSTR::null()) }
        .map(|hwnd| !hwnd.0.is_null())
        .unwrap_or(false)
    {
        return;
    }
    let settings = settings::Settings::load();
    // Probe the local conversion engines in the background. The capability
    // matrix is ready before the first Convert card opens and startup never
    // blocks on FFmpeg's DLLs.
    engines::start_background_probe();
    // `Anchor::compute` works in physical pixels and needs the display's real
    // DPI. Hard-coding 96 made the whole surface 20 % small at 125 % scaling.
    let dpi = anchor::dpi_for_window(taskbar);
    let Some(anchor) = anchor::Anchor::compute(taskbar, dpi, settings.slot_offset_dip) else {
        eprintln!("unable to compute taskbar anchor");
        return;
    };
    if !anchor.visible {
        return;
    }

    if let Err(error) = window::register(instance) {
        eprintln!("unable to register island window: {error}");
        return;
    }
    if let Err(error) = window::create(instance, &anchor, settings.slot_offset_dip) {
        eprintln!("unable to create island window: {error}");
        return;
    }
    if let Err(error) = window::message_loop() {
        eprintln!("message loop failed: {error}");
    }
}

// ---------------------------------------------------------------------------
// Spec §3 diagnostics.

/// `--diag-call [seconds]` (default 300 = the five minutes §3 asks for).
fn diag_call_seconds(args: &[String]) -> Option<u64> {
    let index = args.iter().position(|arg| arg == "--diag-call")?;
    Some(
        args.get(index + 1)
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|seconds| *seconds > 0)
            .unwrap_or(300),
    )
}

/// Hand a `--simulate-call` event to the island that is already running.
/// Returns false when there is none, so the caller can park it instead.
fn deliver_simulation(kind: callsrc::SimKind) -> bool {
    let Ok(hwnd) = (unsafe { FindWindowW(w!("TaskbarIsland"), windows::core::PCWSTR::null()) })
    else {
        return false;
    };
    if hwnd.0.is_null() {
        return false;
    }
    // SAFETY: posting to this app's own island window; the receiver drains
    // the injected signal on its next paint.
    unsafe {
        let _ = PostMessageW(
            Some(hwnd),
            callsrc::WM_APP_SIMULATE,
            WPARAM(kind.code()),
            LPARAM(0),
        );
    }
    true
}

// ---------------------------------------------------------------------------
// Release gate: the product identity is declared in exactly one place
// (Cargo.toml: `version` + `[package.metadata] product-name`) and every
// artifact that shows it must be *generated* from there - never carry its own
// copy. CI (.github/workflows/release.yml) additionally fails a tag that
// disagrees with Cargo.toml.

#[cfg(test)]
mod version_tests {
    use std::fs;

    fn read(path: &str) -> String {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"))
    }

    /// `Cargo.toml` is the single source of truth for the package version.
    fn cargo_version() -> String {
        let content = read(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("version = ") {
                return rest.trim().trim_matches('"').to_string();
            }
        }
        panic!("version not found in Cargo.toml");
    }

    /// `installer/installer.iss` → `#define MyAppVersion`.
    fn installer_version() -> String {
        let content = read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/installer/installer.iss"
        ));
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("#define MyAppVersion ") {
                return rest.trim().trim_matches('"').to_string();
            }
        }
        panic!("MyAppVersion not found in installer.iss");
    }

    /// `assets/app.rc` → `VALUE "FileVersion", "..."`.
    fn resource_version() -> String {
        let content = read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.rc"));
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix(r#"VALUE "FileVersion", "#) {
                return rest.trim().trim_matches('"').to_string();
            }
        }
        panic!("FileVersion not found in app.rc");
    }

    #[test]
    fn version_consistency() {
        let expected = "0.8.0";
        assert_eq!(cargo_version(), expected, "Cargo.toml version");
        assert_eq!(installer_version(), expected, "installer AppVersion");
        assert_eq!(resource_version(), expected, "resource FileVersion");
        assert_eq!(cargo_version(), installer_version());
        assert_eq!(cargo_version(), resource_version());
    }
}
