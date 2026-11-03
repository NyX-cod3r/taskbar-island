//! settings.rs — JSON settings, autostart registry and data paths.

use std::path::PathBuf;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ,
};

use crate::design::ActivityKind;
use crate::json::{self, Json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enabled {
    pub music: bool,
    pub call: bool,
    pub shelf: bool,
    pub timer: bool,
    pub volume: bool,
}

impl Default for Enabled {
    fn default() -> Self {
        Self {
            music: true,
            call: true,
            shelf: true,
            timer: true,
            volume: true,
        }
    }
}

impl Enabled {
    pub fn allows(self, kind: ActivityKind) -> bool {
        match kind {
            ActivityKind::Music => self.music,
            ActivityKind::Call => self.call,
            ActivityKind::Shelf => self.shelf,
            ActivityKind::Timer => self.timer,
            ActivityKind::Volume => self.volume,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub slot_offset_dip: i32,
    pub run_at_startup: bool,
    pub call_app_allowlist: Vec<String>,
    pub enabled: Enabled,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            slot_offset_dip: 0,
            run_at_startup: false,
            call_app_allowlist: vec![
                "Teams".into(),
                "Zoom".into(),
                "WhatsApp".into(),
                "Discord".into(),
                "Skype".into(),
                "Slack".into(),
            ],
            enabled: Enabled::default(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let path = settings_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let Ok(json) = json::parse(&text) else {
            return Self::default();
        };
        let mut settings = Self::default();
        if let Some(v) = json.get("slotOffsetDip").and_then(Json::as_i32) {
            settings.slot_offset_dip = v.clamp(-240, 240);
        }
        if let Some(v) = json.get("runAtStartup").and_then(Json::as_bool) {
            settings.run_at_startup = v;
        }
        if let Some(list) = json.get("callAppAllowlist").and_then(Json::as_array) {
            let values: Vec<String> = list
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            if !values.is_empty() {
                settings.call_app_allowlist = values;
            }
        }
        if let Some(enabled) = json.get("enabled") {
            settings.enabled = Enabled {
                music: enabled
                    .get("music")
                    .and_then(Json::as_bool)
                    .unwrap_or(settings.enabled.music),
                call: enabled
                    .get("call")
                    .and_then(Json::as_bool)
                    .unwrap_or(settings.enabled.call),
                shelf: enabled
                    .get("shelf")
                    .and_then(Json::as_bool)
                    .unwrap_or(settings.enabled.shelf),
                timer: enabled
                    .get("timer")
                    .and_then(Json::as_bool)
                    .unwrap_or(settings.enabled.timer),
                volume: enabled
                    .get("volume")
                    .and_then(Json::as_bool)
                    .unwrap_or(settings.enabled.volume),
            };
        }
        settings
    }

    pub fn save(&self) {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let enabled = Json::Obj(vec![
            ("music".into(), Json::Bool(self.enabled.music)),
            ("call".into(), Json::Bool(self.enabled.call)),
            ("shelf".into(), Json::Bool(self.enabled.shelf)),
            ("timer".into(), Json::Bool(self.enabled.timer)),
            ("volume".into(), Json::Bool(self.enabled.volume)),
        ]);
        let doc = Json::Obj(vec![
            (
                "slotOffsetDip".into(),
                Json::Num(self.slot_offset_dip as f64),
            ),
            ("runAtStartup".into(), Json::Bool(self.run_at_startup)),
            (
                "callAppAllowlist".into(),
                Json::Arr(
                    self.call_app_allowlist
                        .iter()
                        .map(|s| Json::Str(s.clone()))
                        .collect(),
                ),
            ),
            ("enabled".into(), enabled),
        ]);
        let _ = std::fs::write(path, json::to_string_pretty(&doc));
    }
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TaskbarIsland")
}

pub fn appdata_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TaskbarIsland")
}

pub fn settings_path() -> PathBuf {
    appdata_dir().join("settings.json")
}

pub fn shelf_path() -> PathBuf {
    data_dir().join("shelf.json")
}

pub fn log_path() -> PathBuf {
    data_dir().join("log.txt")
}

pub fn autostart_enabled() -> bool {
    read_run_value().is_some()
}

pub fn set_autostart(enabled: bool) -> bool {
    let mut key = HKEY::default();
    // SAFETY: key handles are always closed before returning.
    if unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            Some(0),
            KEY_READ | KEY_SET_VALUE,
            &mut key,
        )
    }
    .is_err()
    {
        return false;
    }
    let result = if enabled {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => {
                let _ = unsafe { RegCloseKey(key) };
                return false;
            }
        };
        let command = format!("\"{}\"", exe.display());
        let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: wide is a NUL-terminated UTF-16 string with the byte length we pass.
        unsafe {
            RegSetValueExW(
                key,
                w!("TaskbarIsland"),
                Some(0),
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    wide.as_ptr() as *const u8,
                    wide.len() * 2,
                )),
            )
        }
        .is_ok()
    } else {
        // SAFETY: key is valid; deleting an absent value is harmless.
        let status = unsafe { RegDeleteValueW(key, w!("TaskbarIsland")) };
        status.is_ok() || status == ERROR_FILE_NOT_FOUND
    };
    // SAFETY: key was opened above.
    let _ = unsafe { RegCloseKey(key) };
    result
}

fn read_run_value() -> Option<String> {
    let mut key = HKEY::default();
    // SAFETY: key handles are closed below on every path.
    if unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            Some(0),
            KEY_READ,
            &mut key,
        )
    }
    .is_err()
    {
        return None;
    }
    let mut size = 0u32;
    // SAFETY: size-only query with no buffer.
    let first =
        unsafe { RegQueryValueExW(key, w!("TaskbarIsland"), None, None, None, Some(&mut size)) };
    if first.is_err() || size == 0 {
        let _ = unsafe { RegCloseKey(key) };
        return None;
    }
    let mut data = vec![0u8; size as usize];
    // SAFETY: data is sized to the value length returned above.
    let result = unsafe {
        RegQueryValueExW(
            key,
            w!("TaskbarIsland"),
            None,
            None,
            Some(data.as_mut_ptr()),
            Some(&mut size),
        )
    };
    let _ = unsafe { RegCloseKey(key) };
    if result.is_err() {
        return None;
    }
    let wide: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|c| *c != 0)
        .collect();
    Some(String::from_utf16_lossy(&wide))
}

#[allow(dead_code)]
pub fn module_path() -> Option<PathBuf> {
    let mut buffer = [0u16; 1024];
    // SAFETY: buffer is a fixed, writable UTF-16 buffer.
    let len = unsafe { GetModuleFileNameW(None, &mut buffer) } as usize;
    (len > 0).then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..len])))
}

#[allow(dead_code)]
fn _pcwstr_marker(_: PCWSTR) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_gates_activities() {
        let mut e = Enabled::default();
        e.music = false;
        assert!(!e.allows(ActivityKind::Music));
        assert!(e.allows(ActivityKind::Call));
    }

    #[test]
    fn defaults_are_sane() {
        let s = Settings::default();
        assert_eq!(s.slot_offset_dip, 0);
        assert!(s.enabled.volume);
        assert!(s.call_app_allowlist.iter().any(|v| v == "Teams"));
    }
}
