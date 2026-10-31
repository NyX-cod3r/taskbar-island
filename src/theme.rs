//! Windows personalization state used by the renderer.

use windows::core::{w, Result};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub light: bool,
}

impl Theme {
    pub fn current() -> Self {
        Self {
            light: read_dword(
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("SystemUsesLightTheme"),
            )
            .unwrap_or(0)
                != 0,
        }
    }

    pub fn fill(self) -> (f32, f32, f32, f32) {
        if self.light {
            (0.0, 0.0, 0.0, 0.05)
        } else {
            (1.0, 1.0, 1.0, 0.08)
        }
    }

    pub fn border(self) -> (f32, f32, f32, f32) {
        if self.light {
            (0.0, 0.0, 0.0, 0.06)
        } else {
            (1.0, 1.0, 1.0, 0.10)
        }
    }
}

fn read_dword(path: windows::core::PCWSTR, value: windows::core::PCWSTR) -> Result<u32> {
    let mut key = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path, Some(0), KEY_READ, &mut key) }.ok()?;
    let mut kind = REG_VALUE_TYPE::default();
    let mut data = [0u8; 4];
    let mut size = data.len() as u32;
    let result = unsafe {
        RegQueryValueExW(
            key,
            value,
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut size),
        )
    };
    let _ = unsafe { RegCloseKey(key) };
    result.ok()?;
    if size != 4 {
        return Err(windows::core::Error::new(
            windows::core::HRESULT(0x80070057u32 as i32),
            "unexpected registry value size",
        ));
    }
    Ok(u32::from_le_bytes(data))
}
