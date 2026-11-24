//! Notification-area presence for the island.
//!
//! The island itself is a passive overlay, so the tray icon is where the
//! process exposes its only chrome: the startup toggle and a way to quit.
//! It is a native `Shell_NotifyIconW` icon; no extra dependency or window.

use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetCursorPos, LoadIconW, PostMessageW, RegisterClassExW, RegisterWindowMessageW,
    SetForegroundWindow, TrackPopupMenu, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    SW_SHOWNORMAL, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_CONTEXTMENU,
    WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
};

use crate::settings;

const CLASS_NAME: windows::core::PCWSTR = w!("TaskbarIslandTray");
const ICON_RESOURCE: u16 = 1;
const TRAY_ID: u32 = 1;
/// Classic (pre-version-4) callback: `lparam` carries the mouse message.
const WM_APP_TRAY: u32 = WM_APP + 40;
const ID_AUTOSTART: usize = 1;
const ID_EXIT: usize = 2;
const ID_MANAGE_ENGINES: usize = 3;
/// §3: grayed informational row while the island is paused — never a
/// command, so it falls through the menu match untouched.
const ID_PAUSED: usize = 4;
const TIP: &str = "TaskbarIsland — right-click for options";
/// The converters pack keeps a copy of itself beside the app so "Manage
/// engines..." is always one command away (installer\converters.iss).
pub(crate) const CONVERTERS_COPY_NAME: &str = "TaskbarIsland-Converters-Setup.exe";
/// v0.8 and earlier shipped the engines inside the single setup, so an
/// upgraded install may still carry that copy. Prefer the pack, fall back to
/// it, and never delete it: an existing self-update would break otherwise.
pub(crate) const LEGACY_SETUP_COPY_NAME: &str = "TaskbarIsland-Setup.exe";

static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
static ISLAND_HWND: AtomicIsize = AtomicIsize::new(0);
/// `TaskbarCreated` is a registered message, so it is only known at runtime.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
/// §3: the island is paused on a left/right taskbar — the menu says so.
static PAUSED: AtomicBool = AtomicBool::new(false);

/// §3: called on every pause/resume transition so the tray menu can show the
/// paused row. The island itself is the source of truth; this is a mirror.
pub(crate) fn set_paused(paused: bool) {
    PAUSED.store(paused, Ordering::Relaxed);
}

/// True when `message` is the runtime-registered `TaskbarCreated` broadcast
/// (Explorer restarted and a new taskbar exists). Registered message ids are
/// only known after `install` has queried the system, so a stored id of 0
/// means "not registered yet" and can never match a real message.
pub fn is_taskbar_created(message: u32) -> bool {
    let id = TASKBAR_CREATED.load(Ordering::Relaxed);
    id != 0 && message == id
}

/// The icon compiled into the executable by `build.rs`.
pub fn load_app_icon(
    instance: HINSTANCE,
) -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    // SAFETY: resource 1 is the ICON compiled by build.rs; MAKEINTRESOURCE is
    // a pointer-sized integer, not a string.
    unsafe {
        LoadIconW(
            Some(instance),
            PCWSTR::from_raw(ICON_RESOURCE as usize as *const u16),
        )
        .ok()
    }
}

pub fn install(instance: HINSTANCE, island: HWND) -> Result<()> {
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(tray_proc),
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // SAFETY: the class is fully initialized; a duplicate registration simply
    // returns zero and the existing class stays usable.
    unsafe {
        let _ = RegisterClassExW(&class);
    }
    // A hidden, non-activating-free window is needed so the tray menu can take
    // the foreground and dismiss on an outside click. It is never shown and
    // carries WS_EX_TOOLWINDOW so it never appears in the task switcher.
    // SAFETY: all handles belong to this process.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!("TaskbarIsland Tray"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    // SAFETY: Querying/registering a global message name is safe here.
    let created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    TASKBAR_CREATED.store(created, Ordering::Relaxed);
    TRAY_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
    ISLAND_HWND.store(island.0 as isize, Ordering::Relaxed);
    add_icon(hwnd)
}

/// Remove the icon and its host window. Called from the island's WM_DESTROY.
pub fn shutdown() {
    let raw = TRAY_HWND.swap(0, Ordering::Relaxed);
    if raw != 0 {
        // SAFETY: the handle was stored by `install` and is destroyed once.
        unsafe {
            let _ = DestroyWindow(HWND(raw as *mut core::ffi::c_void));
        }
    }
}

fn add_icon(hwnd: HWND) -> Result<()> {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_APP_TRAY,
        ..Default::default()
    };
    if let Some(icon) = load_app_icon(unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?.into()
    }) {
        data.hIcon = icon;
    }
    copy_wide(&mut data.szTip, TIP);
    // SAFETY: data is fully initialized and lives for the call.
    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
    if added.as_bool() {
        Ok(())
    } else {
        Err(windows::core::Error::from_thread())
    }
}

fn remove_icon(hwnd: HWND) {
    let data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    // SAFETY: the same identity used by NIM_ADD is being deleted.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

fn notify(hwnd: HWND, title: &str, text: &str) {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_INFO,
        ..Default::default()
    };
    copy_wide(&mut data.szInfoTitle, title);
    copy_wide(&mut data.szInfo, text);
    // SAFETY: data is fully initialized and lives for the call.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

/// §8: the one-time "not enough room" balloon, raised by the island window
/// from `sync_anchor` while the free band is narrower than the pill.
pub(crate) fn notice(title: &str, text: &str) {
    crate::log::log(&format!("tray.notice: {title} — {text}"));
    let raw = TRAY_HWND.load(Ordering::Relaxed);
    if raw == 0 {
        return;
    }
    notify(HWND(raw as *mut core::ffi::c_void), title, text);
}

fn copy_wide(destination: &mut [u16], text: &str) {
    let units: Vec<u16> = text.encode_utf16().take(destination.len() - 1).collect();
    destination[..units.len()].copy_from_slice(&units);
    destination[units.len()] = 0;
}

fn show_menu(hwnd: HWND) {
    crate::log::log("tray.menu: open");
    // SAFETY: menu handles are created and destroyed in this function; the
    // calls run on the UI thread that owns the tray window.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        // §3: grayed state row — the island is paused on a side taskbar.
        if PAUSED.load(Ordering::Relaxed) {
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, ID_PAUSED, w!("Island paused"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        }
        let startup = settings::autostart_enabled();
        let startup_flags = if startup {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(menu, startup_flags, ID_AUTOSTART, w!("Start with Windows"));
        // v0.11 owner feedback: no `Island position` menu — placement is
        // Auto-only and the app decides where the pill sits.
        if manage_engines_setup().is_some() {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_MANAGE_ENGINES, w!("Manage engines..."));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("Exit TaskbarIsland"));
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        crate::log::log(&format!("tray.menu: cmd={}", command.0));
        match command.0 as usize {
            ID_AUTOSTART => toggle_startup(hwnd),
            ID_MANAGE_ENGINES => open_manage_engines(),
            ID_EXIT => exit_app(),
            _ => {}
        }
    }
}

fn toggle_startup(hwnd: HWND) {
    let enable = !settings::autostart_enabled();
    if settings::set_autostart(enable) {
        let mut settings = settings::Settings::load();
        settings.run_at_startup = enable;
        settings.save();
        notify(
            hwnd,
            "TaskbarIsland",
            if enable {
                "TaskbarIsland will start automatically when you sign in."
            } else {
                "TaskbarIsland will no longer start automatically."
            },
        );
    } else {
        crate::log::log("tray.autostart: unable to update the Run key");
        notify(
            hwnd,
            "TaskbarIsland",
            "The startup setting could not be changed.",
        );
    }
}

/// The installed pack copy lives beside the executable; dev builds simply do
/// not have one.
pub(crate) fn manage_engines_command(exe_dir: &Path) -> Option<PathBuf> {
    for name in [CONVERTERS_COPY_NAME, LEGACY_SETUP_COPY_NAME] {
        let setup = exe_dir.join(name);
        if setup.is_file() {
            return Some(setup);
        }
    }
    None
}

fn manage_engines_setup() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    manage_engines_command(exe.parent()?)
}

fn open_manage_engines() {
    use std::os::windows::ffi::OsStrExt;

    let Some(setup) = manage_engines_setup() else {
        return;
    };
    let wide: Vec<u16> = setup.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: the path is a NUL-terminated buffer that lives for the call;
    // the setup copy was installed by us next to the executable.
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

fn exit_app() {
    let island = ISLAND_HWND.load(Ordering::Relaxed);
    if island != 0 {
        // SAFETY: the island window is owned by this process and WM_CLOSE
        // destroys it, which tears the process down through WM_DESTROY.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(island as *mut core::ffi::c_void)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

extern "system" fn tray_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_APP_TRAY => {
            let event = lparam.0 as u32;
            if matches!(event, WM_LBUTTONUP | WM_RBUTTONUP | WM_CONTEXTMENU) {
                show_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            remove_icon(hwnd);
            LRESULT(0)
        }
        _ => {
            let created = TASKBAR_CREATED.load(Ordering::Relaxed);
            if created != 0 && message == created {
                // Explorer restarted and dropped every tray icon.
                if let Err(error) = add_icon(hwnd) {
                    crate::log::log_err("tray.readd", &error);
                }
                return LRESULT(0);
            }
            let _ = wparam;
            // SAFETY: unhandled messages go to the default procedure.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manage_engines_needs_the_installed_setup_copy() {
        let dir = std::env::temp_dir().join(format!("tbi-tray-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(manage_engines_command(&dir), None);

        // A pre-0.9 install only has the legacy single setup copy.
        let legacy = dir.join(LEGACY_SETUP_COPY_NAME);
        std::fs::write(&legacy, b"stub").unwrap();
        assert_eq!(manage_engines_command(&dir), Some(legacy));

        // With the converters pack present it wins over the legacy copy.
        let pack = dir.join(CONVERTERS_COPY_NAME);
        std::fs::write(&pack, b"stub").unwrap();
        assert_eq!(manage_engines_command(&dir), Some(pack));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
