//! volume.rs — transient HUD state plus the event-driven default-render volume
//! listener. The listener is a real WASAPI endpoint callback (media keys,
//! mixer, apps); it blocks on a channel between events, so idle costs nothing.

use std::time::Instant;

use windows::core::{implement, Result};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, AUDIO_VOLUME_NOTIFICATION_DATA,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::activities::audio;

/// `wparam` low byte = level 0..100, bit 8 = muted.
pub const WM_APP_VOLUME: u32 = WM_APP + 5;

pub const VOLUME_MUTED_BIT: usize = 1 << 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeHud {
    pub active: bool,
    pub level: u32,
    pub muted: bool,
    pub last_change: Option<Instant>,
}

impl Default for VolumeHud {
    fn default() -> Self {
        Self {
            active: false,
            level: 0,
            muted: false,
            last_change: None,
        }
    }
}

impl VolumeHud {
    pub fn trigger(&mut self, level: u32, muted: bool, now: Instant) {
        self.active = true;
        self.level = level.min(100);
        self.muted = muted;
        self.last_change = Some(now);
    }

    pub fn dismiss(&mut self) {
        self.active = false;
        self.last_change = None;
    }

    pub fn expired(&self, now: Instant) -> bool {
        self.active
            && self
                .last_change
                .map(|t| now.saturating_duration_since(t).as_millis() >= 1800)
                .unwrap_or(true)
    }
}

pub fn decode(wparam: usize) -> (u32, bool) {
    ((wparam & 0xFF) as u32, wparam & VOLUME_MUTED_BIT != 0)
}

pub fn encode(level: u32, muted: bool) -> usize {
    (level.min(100) as usize) | if muted { VOLUME_MUTED_BIT } else { 0 }
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCallback {
    hwnd: isize,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, notify: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        if notify.is_null() {
            return Ok(());
        }
        // SAFETY: WASAPI owns the notification for the duration of the call;
        // only two fields are read.
        let data = unsafe { &*notify };
        let muted = data.bMuted.as_bool();
        let level = (data.fMasterVolume * 100.0).round().clamp(0.0, 100.0) as u32;
        // SAFETY: posting to our own window.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                WM_APP_VOLUME,
                WPARAM(encode(level, muted)),
                LPARAM(0),
            );
        }
        Ok(())
    }
}

/// Subscribes to the default console render endpoint. The worker parks on a
/// channel for the process lifetime, keeping the endpoint and callback alive.
pub fn subscribe(hwnd: HWND) -> windows::core::Result<()> {
    let hwnd_value = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("taskbar-island-volume".into())
        .spawn(move || {
            if audio::init_mta().is_err() {
                return;
            }
            // SAFETY: COM is initialized on this worker above.
            let Ok(enumerator) = (unsafe {
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
            }) else {
                return;
            };
            // SAFETY: enumerator is a live COM object on this worker.
            let Ok(device) = (unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) })
            else {
                return;
            };
            // SAFETY: standard endpoint activation on a live device.
            let Ok(endpoint) =
                (unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) })
            else {
                return;
            };
            let callback: IAudioEndpointVolumeCallback = VolumeCallback { hwnd: hwnd_value }.into();
            // SAFETY: callback is held alive for the worker's lifetime.
            if unsafe { endpoint.RegisterControlChangeNotify(&callback) }.is_err() {
                return;
            }

            // Park forever; the endpoint + callback stay alive in scope.
            let (_tx, rx) = std::sync::mpsc::channel::<()>();
            let _ = rx.recv();
        })
        .map(|_| ())
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_triggers_clamps_and_expires() {
        let now = Instant::now();
        let mut hud = VolumeHud::default();
        hud.trigger(250, false, now);
        assert!(hud.active);
        assert_eq!(hud.level, 100);
        assert!(!hud.expired(now + std::time::Duration::from_millis(1799)));
        assert!(hud.expired(now + std::time::Duration::from_millis(1800)));
        hud.dismiss();
        assert!(!hud.active);
    }

    #[test]
    fn wparam_round_trip() {
        assert_eq!(decode(encode(42, false)), (42, false));
        assert_eq!(decode(encode(0, true)), (0, true));
    }
}
