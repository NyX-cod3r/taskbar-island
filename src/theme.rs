//! theme.rs — dark/light mode, accent color and reduced-motion from Windows.

use windows::core::{w, PCSTR, PCWSTR};
use windows::Win32::Foundation::{FARPROC, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetDC, GetPixel, GetSysColor, ReleaseDC, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW,
    COLOR_WINDOWTEXT,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE,
};
use windows::Win32::UI::Accessibility::HIGHCONTRASTW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SPI_GETHIGHCONTRAST,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use crate::design::{self, Color};

/// Opt the process into the shell's light/dark popup menus. The two entry
/// points are stable but undocumented uxtheme ordinals; if they are missing
/// the tray menu simply keeps the default appearance.
pub fn enable_native_dark_menus() {
    // SAFETY: `uxtheme_proc` returns a null option for a missing export, and
    // the signatures match the shell's own (non-exported) declarations.
    unsafe {
        if let Some(proc) = uxtheme_proc(135) {
            let set_preferred_app_mode: unsafe extern "system" fn(i32) -> i32 =
                std::mem::transmute(proc);
            let _ = set_preferred_app_mode(1); // PreferredAppMode::AllowDark
        }
    }
}

/// Repaint native menus that already exist after a light/dark switch.
pub fn flush_native_menus() {
    // SAFETY: see `enable_native_dark_menus`.
    unsafe {
        if let Some(proc) = uxtheme_proc(136) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(proc);
            flush();
        }
    }
}

/// `uxtheme` exports these by ordinal only.
unsafe fn uxtheme_proc(ordinal: u16) -> FARPROC {
    let Ok(module) = LoadLibraryW(w!("uxtheme.dll")) else {
        return None;
    };
    GetProcAddress(module, PCSTR::from_raw(ordinal as usize as *const u8))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// Whether the taskbar under the pill is actually light. Windows normally
    /// follows `SystemUsesLightTheme`, but shell replacements and some
    /// customizers keep the bar dark regardless, so the pill samples it.
    pub taskbar_light: bool,
    pub accent: Color,
    pub has_accent: bool,
    pub reduced_motion: bool,
    pub high_contrast: bool,
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
            .or_else(|| read_dword(w!("Software\\Microsoft\\Windows\\DWM"), w!("AccentColor")));
        let has_accent = accent_raw.is_some();
        let fill_percent = read_dword(w!("Software\\TaskbarIsland"), w!("FillPercent"))
            .unwrap_or(9)
            .clamp(4, 20) as f32;
        let radius_dip = read_dword(w!("Software\\TaskbarIsland"), w!("Radius"))
            .unwrap_or(3)
            .clamp(4, 16) as f32;
        // DWM stores 0xAABBGGRR.
        let accent = accent_raw.map(accent_color).unwrap_or(design::SHELF_BLUE);

        Self {
            dark: !light,
            taskbar_light: light,
            accent,
            has_accent,
            reduced_motion: !animations_enabled(),
            high_contrast: high_contrast(),
            fill_percent,
            radius_dip,
        }
    }

    /// Fold in the sampled taskbar tone. The registry key can disagree with
    /// what is on screen, so the sampled value wins when it is available.
    pub fn with_taskbar_tone(mut self, sampled: Option<bool>) -> Self {
        if let Some(light) = sampled {
            self.taskbar_light = light;
        }
        self
    }

    pub fn pill_fill(self) -> Color {
        if self.taskbar_light {
            design::black(design::LIGHT_PILL_FILL_A)
        } else {
            design::white(self.fill_percent / 100.0)
        }
    }

    pub fn pill_fill_hover(self) -> Color {
        if self.taskbar_light {
            design::black(design::LIGHT_PILL_FILL_HOVER_A)
        } else {
            design::white(((self.fill_percent + 6.0) / 100.0).min(1.0))
        }
    }

    pub fn pill_fill_press(self) -> Color {
        if self.taskbar_light {
            design::black(0.03)
        } else {
            design::white((self.fill_percent / 100.0).min(1.0))
        }
    }

    pub fn pill_border_rest(self) -> Color {
        if self.taskbar_light {
            design::black(design::LIGHT_PILL_BORDER_A)
        } else {
            design::white(design::PILL_BORDER_REST_A)
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
        if self.taskbar_light {
            design::LIGHT_TEXT
        } else {
            design::white(design::TEXT_PILL_LABEL_A)
        }
    }

    /// Accent for the pill's value text, kept legible against the bar behind
    /// it. Accents that already have enough contrast pass through untouched;
    /// a dark accent (or a pale one on a light bar) is nudged toward white
    /// (black) instead of vanishing.
    pub fn pill_value(self, accent: Color) -> Color {
        let luma = 0.2126 * accent.r + 0.7152 * accent.g + 0.0722 * accent.b;
        let (target, pole) = if self.taskbar_light {
            (0.30, design::black(1.0))
        } else {
            (0.70, design::white(1.0))
        };
        let t = if self.taskbar_light {
            if luma <= target {
                return accent;
            }
            (luma - target) / luma.max(0.0001)
        } else {
            if luma >= target {
                return accent;
            }
            (target - luma) / (1.0 - luma)
        };
        Color {
            r: accent.r + (pole.r - accent.r) * t,
            g: accent.g + (pole.g - accent.g) * t,
            b: accent.b + (pole.b - accent.b) * t,
            a: accent.a,
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

    // ------------------------------------------------ expanded card (v0.4)

    pub fn card_rail(self) -> Color {
        if self.high_contrast {
            return sys_color(COLOR_WINDOW);
        }
        if self.dark {
            design::CARD_RAIL
        } else {
            design::LIGHT_CARD_RAIL
        }
    }

    pub fn card_divider(self) -> Color {
        if self.high_contrast {
            return sys_color(COLOR_WINDOWTEXT);
        }
        if self.dark {
            design::CARD_DIVIDER
        } else {
            design::LIGHT_CARD_DIVIDER
        }
    }

    pub fn rail_glyph(self, active: bool) -> Color {
        if self.high_contrast {
            return if active {
                sys_color(COLOR_HIGHLIGHTTEXT)
            } else {
                sys_color(COLOR_WINDOWTEXT)
            };
        }
        if self.dark {
            design::white(if active { 1.0 } else { 0.63 })
        } else {
            design::black(if active { 0.95 } else { 0.62 })
        }
    }

    pub fn rail_hover_fill(self) -> Color {
        if self.high_contrast {
            return Color::TRANSPARENT;
        }
        if self.dark {
            design::white(0.06)
        } else {
            design::black(0.045)
        }
    }

    pub fn rail_active_fill(self) -> Color {
        if self.high_contrast {
            return sys_color(COLOR_HIGHLIGHT);
        }
        if self.dark {
            design::white(0.10)
        } else {
            design::black(0.07)
        }
    }

    pub fn rail_press_fill(self) -> Color {
        if self.high_contrast {
            return Color::TRANSPARENT;
        }
        if self.dark {
            design::white(0.04)
        } else {
            design::black(0.03)
        }
    }

    pub fn rail_indicator(self, accent: Color) -> Color {
        if self.high_contrast {
            sys_color(COLOR_HIGHLIGHT)
        } else {
            accent
        }
    }

    pub fn card_tip_fill(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOW)
        } else if self.dark {
            design::rgb(0x2B, 0x2B, 0x2B)
        } else {
            design::rgb(0xFF, 0xFF, 0xFF)
        }
    }

    pub fn card_tip_border(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::white(0.10)
        } else {
            design::black(0.10)
        }
    }

    pub fn card_tip_text(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else {
            self.text_primary()
        }
    }

    pub fn card_status(self, accent: Color, use_accent: bool) -> Color {
        if use_accent {
            if self.high_contrast {
                sys_color(COLOR_HIGHLIGHT)
            } else {
                accent
            }
        } else {
            self.text_secondary()
        }
    }

    pub fn card_button(self) -> Color {
        if self.high_contrast {
            return sys_color(COLOR_WINDOW);
        }
        if self.dark {
            design::rgb(0x34, 0x34, 0x34)
        } else {
            design::rgb(0xE4, 0xE4, 0xE4)
        }
    }

    pub fn card_button_hover(self) -> Color {
        if self.high_contrast {
            return sys_color(COLOR_HIGHLIGHT);
        }
        if self.dark {
            design::rgb(0x3E, 0x3E, 0x3E)
        } else {
            design::rgb(0xDA, 0xDA, 0xDA)
        }
    }

    pub fn card_button_press(self) -> Color {
        if self.high_contrast {
            return sys_color(COLOR_HIGHLIGHT);
        }
        if self.dark {
            design::rgb(0x2C, 0x2C, 0x2C)
        } else {
            design::rgb(0xD0, 0xD0, 0xD0)
        }
    }

    pub fn card_button_text(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::rgb(0xE0, 0xE0, 0xE0)
        } else {
            design::rgb(0x1A, 0x1A, 0x1A)
        }
    }

    pub fn open_call_fill(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOW)
        } else if self.dark {
            design::rgb(0x1F, 0x3A, 0x2A)
        } else {
            design::rgb(0xE7, 0xF4, 0xEC)
        }
    }

    pub fn open_call_text(self, accent: Color) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            accent
        } else {
            darken(accent, 0.55)
        }
    }

    pub fn music_meta(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::white(0.69)
        } else {
            design::black(design::LIGHT_TEXT_SECONDARY_A)
        }
    }

    pub fn music_track(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::white(0.31)
        } else {
            design::black(design::LIGHT_TRACK_A)
        }
    }

    pub fn timer_idle(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::white(0.43)
        } else {
            design::black(0.45)
        }
    }

    pub fn card_track(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::rgb(0x3A, 0x3A, 0x3A)
        } else {
            design::black(design::LIGHT_TRACK_A)
        }
    }

    pub fn card_hint(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::white(0.54)
        } else {
            design::black(0.55)
        }
    }

    pub fn drop_border(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::rgb(0x4D, 0x4D, 0x4D)
        } else {
            design::rgb(0xC4, 0xC4, 0xC4)
        }
    }

    pub fn call_status_text(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::rgb(0xD6, 0xD6, 0xD6)
        } else {
            design::rgb(0x1A, 0x1A, 0x1A)
        }
    }

    // ------------------------------------------------------ converter (v0.5)

    pub fn convert_accent(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_HIGHLIGHT)
        } else if self.dark {
            design::CONVERT_ACCENT
        } else {
            design::CONVERT_ACCENT_LIGHT
        }
    }

    /// v0.6 option tiles: opaque artboard hexes on the dark card.
    pub fn convert_option_fill(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOW)
        } else if self.dark {
            design::CONVERT_OPTION_FILL
        } else {
            design::LIGHT_CONVERT_OPTION_FILL
        }
    }

    pub fn convert_option_border(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::CONVERT_OPTION_BORDER
        } else {
            design::LIGHT_CONVERT_OPTION_BORDER
        }
    }

    pub fn convert_option_hover(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_HIGHLIGHT)
        } else if self.dark {
            design::CONVERT_OPTION_HOVER
        } else {
            design::LIGHT_CONVERT_OPTION_HOVER
        }
    }

    /// Active segmented-tab pill: rgba(143,166,214,.26) in the dark card.
    pub fn convert_tab_active(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_HIGHLIGHT)
        } else if self.dark {
            design::CONVERT_TAB_ACTIVE
        } else {
            design::LIGHT_CONVERT_TAB_ACTIVE
        }
    }

    pub fn convert_tile_fill(self) -> Color {
        if self.high_contrast {
            Color::TRANSPARENT
        } else if self.dark {
            design::white(design::CONVERT_TILE_A)
        } else {
            design::black(0.05)
        }
    }

    pub fn convert_tile_border(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_WINDOWTEXT)
        } else if self.dark {
            design::white(design::CONVERT_TILE_BORDER_A)
        } else {
            design::black(0.06)
        }
    }

    pub fn convert_tile_hover(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_HIGHLIGHT)
        } else if self.dark {
            design::white(design::CONVERT_TILE_HOVER_A)
        } else {
            design::black(0.08)
        }
    }

    pub fn convert_tile_press(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_HIGHLIGHT)
        } else if self.dark {
            design::white(design::CONVERT_TILE_PRESS_A)
        } else {
            design::black(0.13)
        }
    }

    pub fn convert_chip_fill(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_HIGHLIGHT)
        } else {
            design::CONVERT_ACCENT
        }
    }

    pub fn convert_chip_text(self) -> Color {
        if self.high_contrast {
            sys_color(COLOR_HIGHLIGHTTEXT)
        } else {
            design::CONVERT_ON_ACCENT
        }
    }
}

/// Whether the rendered taskbar under the pill is light. The registry value
/// reports the shell's preference, which can disagree with the pixels: shell
/// replacements and customizers often paint the bar dark regardless.
pub fn sample_taskbar_light(taskbar: HWND, island: RECT) -> Option<bool> {
    let mut bar = RECT::default();
    // SAFETY: the taskbar handle comes from the shell and `bar` is writable.
    unsafe { GetWindowRect(taskbar, &mut bar).ok()? };
    let width = bar.right - bar.left;
    let height = bar.bottom - bar.top;
    if width <= 0 || height <= 0 {
        return None;
    }
    let y = bar.bottom - height / 5;
    // SAFETY: the screen DC belongs to this thread until ReleaseDC below.
    let dc = unsafe { GetDC(None) };
    let mut samples = Vec::with_capacity(24);
    for i in 0..24i64 {
        let x = bar.left + ((i * 2 + 1) * width as i64 / 48) as i32;
        // The island floats over the bar; its pixels are ours, not the bar's.
        if x >= island.left - 8 && x <= island.right + 8 {
            continue;
        }
        // SAFETY: `dc` is a valid screen DC and the point lies on the bar.
        let pixel = unsafe { GetPixel(dc, x, y) };
        if pixel.0 == u32::MAX {
            continue; // CLR_INVALID
        }
        let r = (pixel.0 & 0xFF) as f32 / 255.0;
        let g = ((pixel.0 >> 8) & 0xFF) as f32 / 255.0;
        let b = ((pixel.0 >> 16) & 0xFF) as f32 / 255.0;
        samples.push(0.2126 * r + 0.7152 * g + 0.0722 * b);
    }
    // SAFETY: `dc` came from GetDC above and is not used afterwards.
    unsafe { ReleaseDC(None, dc) };
    if samples.len() < 4 {
        return None;
    }
    samples.sort_by(f32::total_cmp);
    Some(samples[samples.len() / 2] > 0.55)
}

fn darken(c: Color, k: f32) -> Color {
    Color {
        r: c.r * k,
        g: c.g * k,
        b: c.b * k,
        a: c.a,
    }
}

fn sys_color(index: windows::Win32::Graphics::Gdi::SYS_COLOR_INDEX) -> Color {
    // COLORREF stores 0x00BBGGRR.
    let value = unsafe { GetSysColor(index) };
    Color {
        r: (value & 0xFF) as f32 / 255.0,
        g: ((value >> 8) & 0xFF) as f32 / 255.0,
        b: ((value >> 16) & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

fn high_contrast() -> bool {
    let mut info = HIGHCONTRASTW::default();
    // SAFETY: SPI_GETHIGHCONTRAST fills the provided HIGHCONTRASTW and expects
    // its cbSize set.
    let ok = unsafe {
        info.cbSize = core::mem::size_of::<HIGHCONTRASTW>() as u32;
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            Some(&mut info as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    ok.is_ok() && (info.dwFlags.0 & 1) != 0
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

    fn themed(taskbar_light: bool) -> Theme {
        Theme {
            dark: !taskbar_light,
            taskbar_light,
            accent: design::SHELF_BLUE,
            has_accent: false,
            reduced_motion: false,
            high_contrast: false,
            fill_percent: 9.0,
            radius_dip: 4.0,
        }
    }

    fn luma(c: Color) -> f32 {
        0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
    }

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

    #[test]
    fn dark_accent_is_lifted_on_a_dark_taskbar() {
        let slate = Color {
            r: 0x4A as f32 / 255.0,
            g: 0x54 as f32 / 255.0,
            b: 0x59 as f32 / 255.0,
            a: 1.0,
        };
        let lifted = themed(false).pill_value(slate);
        assert!(luma(lifted) > 0.65, "value text stays readable: {lifted:?}");
        assert!(lifted.b > lifted.r, "accent hue is preserved");
    }

    #[test]
    fn bright_accent_passes_through_on_a_dark_taskbar() {
        let accent = design::SHELF_BLUE;
        assert_eq!(themed(false).pill_value(accent), accent);
        assert_eq!(
            themed(false).text_pill_label(),
            design::white(design::TEXT_PILL_LABEL_A)
        );
    }

    #[test]
    fn light_taskbar_darkens_pale_accents_and_label() {
        let theme = themed(true);
        let value = theme.pill_value(design::SHELF_BLUE);
        assert!(luma(value) < 0.35, "value text stays readable: {value:?}");
        assert_eq!(theme.text_pill_label(), design::LIGHT_TEXT);
        assert_eq!(theme.pill_fill(), design::black(design::LIGHT_PILL_FILL_A));
    }
}
