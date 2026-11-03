//! theme.rs — dark/light mode, accent color and reduced-motion from Windows.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use crate::design::{self, Color};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub accent: Color,
    pub has_accent: bool,
    pub reduced_motion: bool,
    pub fill_percent: f32,
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
        let accent_raw = read_dword(w!("Software\\TaskbarIsland"), w!("Accent"))
            .or_else(|| {
                read_dword(
                    w!("Software\\Microsoft\\Windows\\DWM"),
                    w!("AccentColor"),
                )
            });
        let has_accent = accent_raw.is_some();
        let fill_percent = read_dword(
            w!("Software\\TaskbarIsland"),
            w!("FillPercent"),
        )
        .unwrap_or(9)
        .clamp(4, 20) as f32;
        let radius_dip = read_dword(w!("Software\\TaskbarIsland"), w!("Radius"))
            .unwrap_or(10)
            .clamp(4, 16) as f32;
        // DWM stores 0xAABBGGRR.
        let accent = accent_raw.map(accent_color).unwrap_or(design::SHELF_BLUE);

        Self {
            dark: !light,
            accent,
            has_accent,
            reduced_motion: !animations_enabled(),
            fill_percent,
            radius_dip,
        }
    }

    pub fn pill_fill(self) -> Color {
        if self.dark {
            design::white(self.fill_percent / 100.0)
        } else {
            design::black(design::LIGHT_PILL_FILL_A)
        }
    }

    pub fn pill_fill_hover(self) -> Color {
        if self.dark {
            design::white(((self.fill_percent + 6.0) / 100.0).min(1.0))
        } else {
            design::black(design::LIGHT_PILL_FILL_HOVER_A)
        }
    }

    pub fn pill_fill_press(self) -> Color {
        if self.dark {
            design::white((self.fill_percent / 100.0).min(1.0))
        } else {
            design::black(0.03)
        }
    }

    pub fn pill_border_rest(self) -> Color {
        if self.dark {
            design::white(design::PILL_BORDER_REST_A)
        } else {
            design::black(design::LIGHT_PILL_BORDER_A)
        }
    }

    pub fn fly_fill(self) -> Color {
        let c = if self.dark {
            design::FLY_FILL
        } else {
            design::LIGHT_FLY_FILL
        };
        c.with_alpha(design::FLY_FILL_A)
    }

    pub fn fly_border(self) -> Color {
        if self.dark {
            design::white(design::FLY_BORDER_A)
        } else {
            design::black(design::LIGHT_FLY_BORDER_A)
        }
    }

    pub fn text_primary(self) -> Color {
        if self.dark {
            design::white(design::TEXT_PRIMARY_A)
        } else {
            design::LIGHT_TEXT
        }
    }

    pub fn text_pill_label(self) -> Color {
        if self.dark {
            design::white(design::TEXT_PILL_LABEL_A)
        } else {
            design::LIGHT_TEXT
        }
    }

    pub fn text_secondary(self) -> Color {
        if self.dark {
            design::white(design::TEXT_SECONDARY_A)
        } else {
            design::black(design::LIGHT_TEXT_SECONDARY_A)
        }
    }

    pub fn text_tertiary(self) -> Color {
        if self.dark {
            design::white(design::TEXT_TERTIARY_A)
        } else {
            design::black(0.50)
        }
    }

    pub fn surface_button(self) -> Color {
        if self.dark {
            design::white(design::SURFACE_BUTTON_A)
        } else {
            design::black(design::LIGHT_SURFACE_BUTTON_A)
        }
    }

    pub fn surface_button_hover(self) -> Color {
        if self.dark {
            design::white(design::SURFACE_BUTTON_HOVER_A)
        } else {
            design::black(0.10)
        }
    }

    pub fn surface_button_press(self) -> Color {
        if self.dark {
            design::white(design::SURFACE_BUTTON_PRESS_A)
        } else {
            design::black(0.03)
        }
    }

    pub fn track(self) -> Color {
        if self.dark {
            design::white(design::TRACK_A)
        } else {
            design::black(design::LIGHT_TRACK_A)
        }
    }

    pub fn timer_track(self) -> Color {
        if self.dark {
            design::white(design::TIMER_TRACK_A)
        } else {
            design::black(0.10)
        }
    }

    pub fn shadow_alpha(self, base: f32) -> f32 {
        if self.dark {
            base
        } else {
            base * 0.56
        }
    }

    pub fn shelf_color(self) -> Color {
        if self.has_accent {
            self.accent
        } else {
            design::SHELF_BLUE
        }
    }

    pub fn pill_radius(self) -> f32 {
        self.radius_dip
    }
}

fn accent_color(value: u32) -> Color {
    // DWM stores COLORREF values as 0x00BBGGRR.
    Color {
        r: (value & 0xFF) as f32 / 255.0,
        g: ((value >> 8) & 0xFF) as f32 / 255.0,
        b: ((value >> 16) & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

pub fn animations_enabled() -> bool {
    let mut enabled = windows::core::BOOL(1);
    // SAFETY: SPI_GETCLIENTAREAANIMATION copies a BOOL into the provided pointer.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    ok.is_ok() && enabled.as_bool()
}

fn read_dword(path: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut key = HKEY::default();
    // SAFETY: registry handles are closed below on every path.
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path, Some(0), KEY_READ, &mut key) }.is_err() {
        return None;
    }
    let mut kind = REG_VALUE_TYPE::default();
    let mut data = [0u8; 4];
    let mut size = data.len() as u32;
    // SAFETY: data points to four writable bytes and size is its length.
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
    // SAFETY: `key` was opened above and is no longer used.
    let _ = unsafe { RegCloseKey(key) };
    if result.is_err() {
        return None;
    }
    (size == 4).then(|| u32::from_le_bytes(data))
}

/// Unused parameter marker that keeps the signature stable for future per-monitor themes.
#[allow(dead_code)]
pub fn _hwnd_marker(_hwnd: HWND) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_color_decodes_windows_colorref_order() {
        assert_eq!(
            accent_color(0x0030_2010),
            Color {
                r: 0x10 as f32 / 255.0,
                g: 0x20 as f32 / 255.0,
                b: 0x30 as f32 / 255.0,
                a: 1.0,
            }
        );
    }
}
