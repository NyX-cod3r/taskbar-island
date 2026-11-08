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
mod tooltip;
mod tray;
mod uia;
mod view;
mod window;

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

fn main() {
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
    let dpi = unsafe { anchor::dpi_for_window(taskbar) };
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
