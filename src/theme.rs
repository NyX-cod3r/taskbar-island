//! Windows personalization state used by the renderer.

use crate::settings;
use windows::core::{w, Result};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE,
};

#[allow(dead_code)]
pub const PILL_RADIUS_DIP: f32 = 10.0;
pub const FLYOUT_RADIUS_DIP: f32 = 8.0;
#[allow(dead_code)]
pub const BUTTON_RADIUS_DIP: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub light: bool,
    pub accent: (f32, f32, f32, f32),
    /// Compact fill opacity from the Tweaks setting (4–20%).
    pub fill_percent: f32,
    /// Pill corner radius in DIP from the Tweaks setting.
    pub radius_dip: f32,
}

impl Theme {
    pub fn current() -> Self {
        let light = read_dword(
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
        )
        .unwrap_or(0)
            != 0;

        // Accent: Tweaks override first, then the DWM accent, then #60CDFF.
        let tweaks = settings::Settings::current();
        let accent = tweaks
            .accent_abgr
            .map(settings::Settings::accent_rgb)
            .or_else(|| {
                read_dword(w!("Software\\Microsoft\\Windows\\DWM"), w!("AccentColor"))
                    .ok()
                    .map(settings::Settings::accent_rgb)
            })
            .unwrap_or((0x60 as f32 / 255.0, 0xCD as f32 / 255.0, 1.0, 1.0));

        Self {
            light,
            accent,
            fill_percent: tweaks.fill_percent as f32,
            radius_dip: tweaks.radius_dip as f32,
        }
    }

    /// Compact fill: white at the Tweaks percentage in dark mode; light mode
    /// keeps the spec's 5% black so contrast stays correct on a light taskbar.
    pub fn fill(self) -> (f32, f32, f32, f32) {
        if self.light {
            (0.0, 0.0, 0.0, 0.05)
        } else {
            (1.0, 1.0, 1.0, self.fill_percent / 100.0)
        }
    }

    /// Compact border: 1 px, one point above the fill.
    pub fn border(self) -> (f32, f32, f32, f32) {
        if self.light {
            (0.0, 0.0, 0.0, 0.08)
        } else {
            (1.0, 1.0, 1.0, ((self.fill_percent + 1.0) / 100.0).min(1.0))
        }
    }

    /// Hover fill: fill + 6 points (spec: 9% → 15%).
    pub fn hover_fill(self) -> (f32, f32, f32, f32) {
        if self.light {
            (0.0, 0.0, 0.0, 0.09)
        } else {
            (1.0, 1.0, 1.0, ((self.fill_percent + 6.0) / 100.0).min(1.0))
        }
    }

    /// Hover border: hover fill + 5 points (spec: 15% → 20%).
    pub fn hover_border(self) -> (f32, f32, f32, f32) {
        if self.light {
            (0.0, 0.0, 0.0, 0.15)
        } else {
            (1.0, 1.0, 1.0, ((self.fill_percent + 11.0) / 100.0).min(1.0))
        }
    }

    /// Primary text: white 100%.
    pub fn text(self) -> (f32, f32, f32, f32) {
        if self.light {
            (0.0, 0.0, 0.0, 1.0)
        } else {
            (1.0, 1.0, 1.0, 1.0)
        }
    }

    /// Secondary text: white 70%.
    pub fn secondary_text(self) -> (f32, f32, f32, f32) {
        if self.light {
            (0.0, 0.0, 0.0, 0.70)
        } else {
            (1.0, 1.0, 1.0, 0.70)
        }
    }

    /// Flyout fill: rgb(40, 40, 40) at 95%.
    pub fn flyout_fill(self) -> (f32, f32, f32, f32) {
        (40.0 / 255.0, 40.0 / 255.0, 40.0 / 255.0, 0.95)
    }

    /// Flyout border: 1 px, white 9%.
    pub fn flyout_border(self) -> (f32, f32, f32, f32) {
        (1.0, 1.0, 1.0, 0.09)
    }

    /// System accent color (#60CDFF default).
    pub fn accent_color(self) -> (f32, f32, f32, f32) {
        self.accent
    }

    /// Status color for call active: #6CCB5F.
    pub fn call_color() -> (f32, f32, f32, f32) {
        (0x6C as f32 / 255.0, 0xCB as f32 / 255.0, 0x5F as f32 / 255.0, 1.0)
    }

    /// Status color for End button: #C42B1C.
    pub fn end_color() -> (f32, f32, f32, f32) {
        (0xC4 as f32 / 255.0, 0x2B as f32 / 255.0, 0x1C as f32 / 255.0, 1.0)
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
