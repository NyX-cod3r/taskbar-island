//! User tweaks persisted under `HKCU\Software\TaskbarIsland`.
//!
//! The design spec calls for a Tweaks panel (fill 4–20%, radius, accent) and a
//! configurable timer length. The pill is a taskbar child window, so a full
//! settings window would be heavier than the feature; instead the same values
//! are stored in the registry and exposed through the pill's right-click menu.
//! Values are clamped on read, so a hand-edited registry can never break the
//! pill's geometry.

use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
    REG_VALUE_TYPE,
};

pub const APP_KEY: PCWSTR = w!("Software\\TaskbarIsland");
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("TaskbarIsland");

pub const FILL_MIN: u32 = 4;
pub const FILL_MAX: u32 = 20;
pub const RADIUS_MIN: u32 = 4;
pub const RADIUS_MAX: u32 = 16;
pub const TIMER_MIN_MINUTES: u32 = 1;
pub const TIMER_MAX_MINUTES: u32 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Compact fill opacity in percent (4–20).
    pub fill_percent: u32,
    /// Pill corner radius in DIP (4–16).
    pub radius_dip: u32,
    /// Override accent as 0x00BBGGRR (COLORREF), or None for the system accent.
    pub accent_abgr: Option<u32>,
    /// Default countdown length for Ctrl+Alt+T, in minutes.
    pub timer_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fill_percent: 9,
            radius_dip: 10,
            accent_abgr: None,
            timer_minutes: 5,
        }
    }
}

impl Settings {
    pub fn current() -> Self {
        let d = Self::default();
        Self {
            fill_percent: read_dword(APP_KEY, w!("FillPercent"))
                .unwrap_or(d.fill_percent)
                .clamp(FILL_MIN, FILL_MAX),
            radius_dip: read_dword(APP_KEY, w!("Radius"))
                .unwrap_or(d.radius_dip)
                .clamp(RADIUS_MIN, RADIUS_MAX),
            accent_abgr: read_dword(APP_KEY, w!("Accent")).map(|v| v & 0x00FF_FFFF),
            timer_minutes: read_dword(APP_KEY, w!("TimerMinutes"))
                .unwrap_or(d.timer_minutes)
                .clamp(TIMER_MIN_MINUTES, TIMER_MAX_MINUTES),
        }
    }

    /// Converts a COLORREF (0x00BBGGRR) to linear-ish RGBA floats.
    pub fn accent_rgb(abgr: u32) -> (f32, f32, f32, f32) {
        let r = (abgr & 0xFF) as f32 / 255.0;
        let g = ((abgr >> 8) & 0xFF) as f32 / 255.0;
        let b = ((abgr >> 16) & 0xFF) as f32 / 255.0;
        (r, g, b, 1.0)
    }
}

/// Persists a DWORD under the app key. Best-effort: tweaks are a convenience,
/// never a startup requirement.
pub fn set_dword(name: PCWSTR, value: u32) -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            APP_KEY,
            Some(0),
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .is_err()
        {
            return false;
        }
        let bytes = value.to_le_bytes();
        let ok = RegSetValueExW(key, name, Some(0), REG_DWORD, Some(&bytes)).is_ok();
        let _ = RegCloseKey(key);
        ok
    }
}

/// Removes an override (used by "Accent: System").
pub fn clear_value(name: PCWSTR) -> bool {
    clear_registry_value(APP_KEY, name)
}

/// Returns whether Windows will launch the installed application at sign-in.
pub fn autostart_enabled() -> bool {
    read_string(RUN_KEY, RUN_VALUE).is_some()
}

/// Enables or disables launch at sign-in for the current user.
pub fn set_autostart(enabled: bool) -> bool {
    if !enabled {
        return clear_registry_value(RUN_KEY, RUN_VALUE);
    }

    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut command = exe.as_os_str().to_string_lossy().into_owned();
    command.insert(0, '"');
    command.push('"');
    let mut data: Vec<u16> = command.encode_utf16().collect();
    data.push(0);
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2) };

    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            Some(0),
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .is_err()
        {
            return false;
        }
        let ok = RegSetValueExW(key, RUN_VALUE, Some(0), REG_SZ, Some(bytes)).is_ok();
        let _ = RegCloseKey(key);
        ok
    }
}

fn read_dword(path: PCWSTR, value: PCWSTR) -> Option<u32> {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, path, Some(0), KEY_READ, &mut key).is_err() {
            return None;
        }
        let mut kind = REG_VALUE_TYPE::default();
        let mut data = [0u8; 4];
        let mut size = data.len() as u32;
        let result = RegQueryValueExW(
            key,
            value,
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if result.is_err() || size != 4 || kind != REG_DWORD {
            return None;
        }
        Some(u32::from_le_bytes(data))
    }
}

fn read_string(path: PCWSTR, value: PCWSTR) -> Option<String> {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, path, Some(0), KEY_READ, &mut key).is_err() {
            return None;
        }
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        if RegQueryValueExW(key, value, None, Some(&mut kind), None, Some(&mut size)).is_err()
            || kind != REG_SZ
            || size < 2
        {
            let _ = RegCloseKey(key);
            return None;
        }
        let mut data = vec![0u8; size as usize];
        let result = RegQueryValueExW(
            key,
            value,
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if result.is_err() || kind != REG_SZ {
            return None;
        }
        let words = std::slice::from_raw_parts(data.as_ptr() as *const u16, size as usize / 2);
        String::from_utf16(words)
            .ok()
            .map(|value| value.trim_end_matches('\0').to_owned())
    }
}

fn clear_registry_value(path: PCWSTR, name: PCWSTR) -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, path, Some(0), KEY_SET_VALUE, &mut key).is_err() {
            return true;
        }
        let result = RegDeleteValueW(key, name);
        let _ = RegCloseKey(key);
        result.is_ok()
    }
}
