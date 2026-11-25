//! TaskbarIsland entry point.

#![windows_subsystem = "windows"]

mod activities;
mod anchor;
mod anim;
#[allow(dead_code)]
mod clipboard;
mod coexist;
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
mod win;
mod window;

use std::sync::atomic::Ordering;

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CoInitializeEx, CoInitializeSecurity, COINIT_APARTMENTTHREADED, EOAC_NONE,
    RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
};
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
    // v0.11 §1: stroke the shared card grid over the open card so any card
    // that paints outside `design::card_content_rect()` is visible at a glance.
    if args.iter().any(|arg| arg == "--debug-layout") {
        render::DEBUG_LAYOUT.store(true, Ordering::Relaxed);
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
    // §2 `--simulate-edge top|bottom|left|right`: pretend the bar sits on
    // that edge (same monitor, same thickness) so all four layouts — and the
    // pause/notice on a side edge — can be exercised on a machine whose real
    // taskbar never moves. A launch flag: the running island is left alone.
    if let Some(index) = args.iter().position(|arg| arg == "--simulate-edge") {
        let Some(edge) = args
            .get(index + 1)
            .and_then(|value| anchor::Edge::parse(value))
        else {
            eprintln!("--simulate-edge needs top|bottom|left|right");
            return;
        };
        anchor::set_simulated_edge(Some(edge));
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
    // §4: COM security is process-wide and first-come-first-served, so it has
    // to be claimed before anything else touches COM — the media worker, OLE
    // drag-drop, the call sources' UIA and the WMI watcher all inherit it.
    // Without this call WMI refuses every read with ACCESS_DENIED (see the
    // note at the top of `coexist.rs`), which would leave the coexistence
    // watcher blind to a tool started later. The UI thread is STA, which is
    // exactly what its later `OleInitialize` expects. A failure is logged and
    // swallowed: the island still runs, only late tool detection degrades.
    match unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) } {
        hr if hr.is_ok() => {
            if let Err(error) = unsafe {
                CoInitializeSecurity(
                    None,
                    -1, // RPC_C_AUTHN_DEFAULT: register the default services
                    None,
                    None,
                    RPC_C_AUTHN_LEVEL_DEFAULT,
                    RPC_C_IMP_LEVEL_IMPERSONATE,
                    None,
                    EOAC_NONE,
                    None,
                )
            } {
                log::log(&format!(
                    "com.security: {error} (0x{:08X})",
                    error.code().0 as u32
                ));
            }
        }
        hr => log::log(&format!("com.init: 0x{:08X}", hr.0 as u32)),
    }
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
    // §4: find any taskbar customizer already running *before* the first
    // embed decision, so a machine that boots TranslucentTB never asks
    // Explorer to parent the pill in the first place.
    coexist::scan();
    // Probe the local conversion engines in the background. The capability
    // matrix is ready before the first Convert card opens and startup never
    // blocks on FFmpeg's DLLs.
    engines::start_background_probe();
    // §8: scan the taskbar's free region once behind a 400 ms budget so the
    // very first frame already sits in the right slot. The watchers take over
    // from `window::create`. Placement is Auto-only (v0.11 owner feedback).
    anchor::scan_initial(std::time::Duration::from_millis(400));
    // `Anchor::compute` works in physical pixels and needs the display's real
    // DPI. Hard-coding 96 made the whole surface 20 % small at 125 % scaling.
    let dpi = anchor::dpi_for_window(taskbar);
    let Some(anchor) = anchor::Anchor::compute(taskbar, dpi, settings.slot_offset_dip) else {
        eprintln!("unable to compute taskbar anchor");
        return;
    };
    // Auto-hide: the bar may be *hidden* when the island starts (sign-in
    // with the bar tucked away, a relaunch while it is hidden) — starting
    // anyway is the difference between "the island is missing" and "it
    // rides out with the first reveal" (the taskbar-move hook re-anchors
    // the moment the bar shows). Fullscreen/presentation still gates
    // startup, as before.
    if !crate::anchor::is_allowed() {
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

    fn root() -> &'static str {
        env!("CARGO_MANIFEST_DIR")
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

    /// `[package.metadata] product-name = "..."` - the shipping product name.
    fn cargo_product_name() -> String {
        let content = read(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        let mut in_metadata = false;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_metadata = line == "[package.metadata]";
                continue;
            }
            if in_metadata {
                if let Some(rest) = line.strip_prefix("product-name") {
                    let value = rest
                        .trim_start()
                        .strip_prefix('=')
                        .map(str::trim_start)
                        .and_then(|r| r.strip_prefix('"'))
                        .and_then(|r| r.strip_suffix('"'));
                    if let Some(name) = value {
                        if !name.is_empty() {
                            return name.to_string();
                        }
                    }
                }
            }
        }
        panic!("product-name not found in Cargo.toml [package.metadata]");
    }

    #[test]
    fn version_consistency() {
        let version = cargo_version();
        assert!(
            version.starts_with("0."),
            "before 1.0.0 the major version stays 0 (got {version})"
        );
        assert!(
            version.split('.').count() == 3,
            "version must be x.y.z for Inno Setup's IsNewerVersion (got {version})"
        );
        // build.rs substitutes these into the VERSIONINFO resource.
        let rc = read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.rc"));
        assert!(
            rc.contains("@VERSION@"),
            "app.rc must be a @VERSION@ template"
        );
        assert!(
            rc.contains("@VERSION_CSV@"),
            "app.rc must template FILEVERSION/PRODUCTVERSION"
        );
        assert!(
            rc.contains("@PRODUCT_NAME@"),
            "app.rc must template the product name"
        );
        assert!(
            !rc.contains(&version),
            "app.rc carries a literal version; it must be generated from Cargo.toml"
        );
        assert!(
            !rc.contains(&cargo_product_name()),
            "app.rc carries a literal product name; it must be generated from Cargo.toml"
        );
    }

    #[test]
    fn installers_take_identity_from_the_build_script() {
        for iss in ["installer/installer.iss", "installer/converters.iss"] {
            let content = read(&format!("{}/{iss}", root()));
            assert!(
                content.contains("#ifndef MyAppVersion"),
                "{iss} must require /DMyAppVersion"
            );
            assert!(
                content.contains("#ifndef MyAppName"),
                "{iss} must require /DMyAppName"
            );
            assert!(
                !content.contains("#define MyAppVersion"),
                "{iss} must not hardcode a version"
            );
            assert!(
                !content.contains("#define MyAppName"),
                "{iss} must not hardcode the product name"
            );
        }
        // build.ps1 is what reads Cargo.toml and passes both defines.
        let ps1 = read(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/build.ps1"));
        assert!(
            ps1.contains("/DMyAppVersion=$MyAppVersion"),
            "build.ps1 must pass the version it read from Cargo.toml"
        );
        assert!(
            ps1.contains("/DMyAppName=$MyAppName"),
            "build.ps1 must pass the product name it read from Cargo.toml"
        );
    }

    #[test]
    fn release_workflow_checks_the_tag_against_cargo_toml() {
        let yml = read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.github/workflows/release.yml"
        ));
        assert!(
            yml.contains("GITHUB_REF_NAME"),
            "release.yml must read the tag it was triggered by"
        );
        assert!(
            yml.contains("Cargo.toml"),
            "release.yml must compare the tag with Cargo.toml"
        );
        assert!(
            yml.contains("tools/build.ps1"),
            "release.yml must build through tools/build.ps1"
        );
        assert!(
            yml.contains("secrets.GITHUB_TOKEN"),
            "release.yml must use the built-in token, never a stored secret"
        );
    }
}
