//! Volume HUD activity: transient HUD when the system volume changes.
//! Never expands. Dismisses after 1.8 s.
//!
//! The listener is a real WASAPI endpoint callback, not a keyboard hook: it
//! fires for every volume change (media keys, mixer, apps) and blocks on a
//! channel between events, so idle costs no CPU.

use std::time::Instant;

use windows::core::{implement, Result};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, AUDIO_VOLUME_NOTIFICATION_DATA,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::state::{Activity, ActivityId, Priority};

/// Posts the new level in `wparam` to the pill.
pub const WM_APP_VOLUME: u32 = WM_APP + 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VolumeActivity {
    pub active: bool,
    pub level: u32, // 0..100
    pub last_active: Option<Instant>,
}

impl VolumeActivity {
    pub fn trigger(&mut self, level: u32) {
        self.active = true;
        self.level = level.min(100);
        self.last_active = Some(Instant::now());
    }

    pub fn dismiss(&mut self) {
        self.active = false;
        self.last_active = None;
    }

    #[allow(dead_code)]
    pub fn check_dismiss(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_active {
            if now.duration_since(last).as_millis() >= 1800 {
                self.dismiss();
                return true;
            }
        }
        false
    }
}

impl Activity for VolumeActivity {
    fn id(&self) -> ActivityId {
        ActivityId::Volume
    }
    fn priority(&self) -> Priority {
        Priority::VolumeHud
    }
    fn active(&self) -> bool {
        self.active
    }
}

/// WASAPI callback. Runs on the COM worker thread; the only thing it does is
/// post a compact level message to the UI thread.
#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCallback {
    hwnd: isize,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, notify: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        if notify.is_null() {
            return Ok(());
        }
        // SAFETY: WASAPI passes a valid notification for the duration of the
        // call; we only read the two fields we need.
        let data = unsafe { &*notify };
        let level = if data.bMuted.as_bool() {
            0
        } else {
            (data.fMasterVolume * 100.0).round().clamp(0.0, 100.0) as u32
        };
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                WM_APP_VOLUME,
                WPARAM(level as usize),
                LPARAM(0),
            );
        }
        Ok(())
    }
}

/// Subscribes to the default render endpoint's volume changes. The worker
/// parks on a channel for the process lifetime, keeping the endpoint and the
/// callback alive without polling.
pub fn subscribe(hwnd: HWND) -> windows::core::Result<()> {
    let hwnd_value = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("taskbar-island-volume".into())
        .spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(
                    &MMDeviceEnumerator,
                    None,
                    CLSCTX_ALL,
                ) else {
                    return;
                };
                let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else {
                    return;
                };
                let Ok(endpoint) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                else {
                    return;
                };
                let callback: IAudioEndpointVolumeCallback =
                    VolumeCallback { hwnd: hwnd_value }.into();
                if endpoint.RegisterControlChangeNotify(&callback).is_err() {
                    return;
                }

                // Park forever; the endpoint + callback stay alive in scope.
                let (_tx, rx) = std::sync::mpsc::channel::<()>();
                let _ = rx.recv();
            }
        })
        .map(|_| ())
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_clamps_level_and_marks_active() {
        let mut volume = VolumeActivity::default();
        volume.trigger(250);
        assert!(volume.active());
        assert_eq!(volume.level, 100);
        volume.dismiss();
        assert!(!volume.active());
        assert_eq!(volume.level, 100);
    }
}
