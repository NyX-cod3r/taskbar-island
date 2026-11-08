//! TaskbarIsland entry point.

#![windows_subsystem = "windows"]

mod activities;
mod anchor;
mod anim;
mod design;
mod gpu;
mod icons;
mod json;
mod log;
mod render;
mod settings;
mod state;
mod theme;
mod tokens;
mod view;
mod window;

use windows::core::w;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

fn main() {
    // Anchor geometry is measured in physical screen coordinates. Without
    // per-monitor awareness Windows virtualizes the popup bounds at scaled
    // displays, moving the pill away from the taskbar hit area.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
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
    let Some(anchor) = anchor::Anchor::compute(taskbar, 96, settings.slot_offset_dip) else {
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
    if let Err(error) = window::create(instance, anchor.window, anchor.pill, anchor.flyout) {
        eprintln!("unable to create island window: {error}");
        return;
    }
    if let Err(error) = window::message_loop() {
        eprintln!("message loop failed: {error}");
    }
}
