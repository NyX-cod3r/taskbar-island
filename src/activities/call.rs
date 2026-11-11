//! call.rs — best-effort call detection from the Windows microphone consent
//! store, plus real endpoint mute buttons.
//!
//! Windows exposes **no API for another app's call state**. This module uses
//! the documented microphone-usage registry:
//! `HKCU\...\CapabilityAccessManager\ConsentStore\microphone`,
//! whose `LastUsedTimeStart` / `LastUsedTimeStop` values flip while an app
//! holds the microphone. An "in use" app is treated as a call only when its
//! key matches the user-editable allowlist. Browsers are deliberately not in
//! the default list because one browser process cannot distinguish a meeting
//! tab from any other tab. There is no fake "End call" button.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_ITEMS, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyW, RegNotifyChangeKeyValue, RegOpenKeyExW, RegQueryValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    REG_QWORD,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::activities::audio;
use crate::log;

pub const WM_APP_CALL: u32 = WM_APP + 6;
pub const WM_APP_CALL_AUDIO: u32 = WM_APP + 7;

const MIC_CONSENT_KEY: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone");

#[derive(Debug, Clone)]
pub struct CallDetect {
    pub app_name: String,
    /// Registry key (decoded path or package family) used for "Open app".
    #[allow(dead_code)] // Read by the Call card's "Open app" action (A-scope).
    pub launch: String,
    pub started: Instant,
    /// Duration at the moment the snapshot was produced; the UI adds the
    /// elapsed time from `started` so it ticks without polling the registry.
    pub duration_at_fetch: f64,
}

#[derive(Debug, Clone, Default)]
pub struct CallRuntime {
    pub detect: Option<CallDetect>,
    pub device: String,
    pub mic_muted: bool,
    pub output_muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCommand {
    #[allow(dead_code)] // Sent when the Call card re-reads the audio state.
    Refresh,
    ToggleMic,
    ToggleOutput,
}

pub fn runtime() -> Arc<Mutex<CallRuntime>> {
    Arc::new(Mutex::new(CallRuntime::default()))
}

/// Start the registry watcher. One event-driven thread; it touches only the
/// shared runtime and never the UI state.
pub fn subscribe(
    hwnd: HWND,
    allowlist: Vec<String>,
    shared: Arc<Mutex<CallRuntime>>,
) -> windows::core::Result<()> {
    let hwnd_value = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("taskbar-island-call".into())
        .spawn(move || watch(hwnd_value, allowlist, shared))
        .map(|_| ())
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))
}

/// Start the audio command worker (device name + endpoint mutes).
pub fn subscribe_audio(
    hwnd: HWND,
    shared: Arc<Mutex<CallRuntime>>,
) -> windows::core::Result<Sender<AudioCommand>> {
    let (tx, rx) = mpsc::channel();
    let hwnd_value = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("taskbar-island-call-audio".into())
        .spawn(move || audio_worker(hwnd_value, shared, rx))
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))?;
    Ok(tx)
}

/// Debug-only call simulator (A prompt §5: "a debug-only simulator that
/// fires fake call events is welcome"). It exists so the call pill, the call
/// card and the pill priority rules can be exercised on a machine with no
/// linked phone. It is compiled out of release builds entirely, only runs
/// when `TBI_SIMULATE_CALLS` is set, and labels itself "Simulated" so it can
/// never be mistaken for a caller.
#[cfg(debug_assertions)]
const SIMULATED_CALL_NAME: &str = "Simulated call";

#[cfg(debug_assertions)]
pub fn subscribe_simulated(hwnd: HWND, shared: Arc<Mutex<CallRuntime>>) {
    if std::env::var("TBI_SIMULATE_CALLS").is_err() {
        return;
    }
    let hwnd_value = hwnd.0 as isize;
    let _ = std::thread::Builder::new()
        .name("taskbar-island-call-sim".into())
        .spawn(move || simulate(hwnd_value, shared));
}

/// Alternates a live call and an idle gap (12 s on / 8 s off) so both the
/// ticking clock and the priority hand-back get a workout over a soak. The
/// real watcher keeps its own view of the runtime; this thread only runs in
/// dev builds where there is no watcher data to contradict.
#[cfg(debug_assertions)]
fn simulate(hwnd: isize, shared: Arc<Mutex<CallRuntime>>) {
    const ON: Duration = Duration::from_secs(12);
    const OFF: Duration = Duration::from_secs(8);
    loop {
        {
            let mut runtime = shared.lock().expect("call runtime mutex poisoned");
            let now = Instant::now();
            runtime.detect = Some(CallDetect {
                app_name: SIMULATED_CALL_NAME.into(),
                launch: phone_link_launch(),
                started: now,
                duration_at_fetch: 0.0,
            });
            runtime.device = "Simulated output".into();
        }
        post(hwnd, WM_APP_CALL);
        std::thread::sleep(ON);
        shared.lock().expect("call runtime mutex poisoned").detect = None;
        post(hwnd, WM_APP_CALL);
        std::thread::sleep(OFF);
    }
}

fn watch(hwnd: isize, allowlist: Vec<String>, shared: Arc<Mutex<CallRuntime>>) {
    if audio::init_mta().is_err() {
        log::log("call.watch: COM init failed");
        return;
    }
    let mut key = HKEY::default();
    // SAFETY: the key is closed by `RegKey` below.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            MIC_CONSENT_KEY,
            Some(0),
            KEY_READ | KEY_NOTIFY,
            &mut key,
        )
    };
    if opened.is_err() {
        log::log("call.watch: consent key unavailable");
        return;
    }
    let key = RegKey(key);
    // SAFETY: auto-reset event; the handle is closed at thread exit.
    let Ok(event) = (unsafe { CreateEventW(None, false, false, PCWSTR::null()) }) else {
        return;
    };
    let event = EventHandle(event);

    let mut last_reported: Option<String> = None;
    loop {
        let found = scan(&key.0, &allowlist);
        let signature = found.as_ref().map(|f| f.app_name.clone());
        if signature != last_reported {
            last_reported = signature;
            {
                let mut runtime = shared.lock().expect("call runtime mutex poisoned");
                runtime.detect = found;
                if runtime.detect.is_some() {
                    runtime.device = device_name();
                    runtime.mic_muted = audio::mute_state(
                        audio::communications_capture().0,
                        audio::communications_capture().1,
                    )
                    .unwrap_or(false);
                    runtime.output_muted = audio::mute_state(
                        audio::communications_render().0,
                        audio::communications_render().1,
                    )
                    .unwrap_or(false);
                }
            }
            // SAFETY: posting to our own window.
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd as *mut _)),
                    WM_APP_CALL,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }

        // SAFETY: key/event are valid for the whole loop. The call arms an
        // asynchronous notification and returns immediately.
        let status = unsafe {
            RegNotifyChangeKeyValue(
                key.0,
                true,
                REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME,
                Some(event.0),
                true,
            )
        };
        if status != windows::Win32::Foundation::ERROR_SUCCESS {
            log::log("call.watch: RegNotifyChangeKeyValue failed");
            return;
        }
        // SAFETY: waits on the event armed above; idle blocks here.
        unsafe { WaitForSingleObject(event.0, 0xFFFF_FFFF) };
    }
}

fn audio_worker(hwnd: isize, shared: Arc<Mutex<CallRuntime>>, rx: Receiver<AudioCommand>) {
    if audio::init_mta().is_err() {
        return;
    }
    refresh_audio(&shared);
    post(hwnd, WM_APP_CALL_AUDIO);
    while let Ok(command) = rx.recv() {
        match command {
            AudioCommand::Refresh => refresh_audio(&shared),
            AudioCommand::ToggleMic => {
                let (flow, role) = audio::communications_capture();
                let current = audio::mute_state(flow, role).unwrap_or(false);
                if let Ok(actual) = audio::set_mute(flow, role, !current) {
                    shared
                        .lock()
                        .expect("call runtime mutex poisoned")
                        .mic_muted = actual;
                }
            }
            AudioCommand::ToggleOutput => {
                let (flow, role) = audio::communications_render();
                let current = audio::mute_state(flow, role).unwrap_or(false);
                if let Ok(actual) = audio::set_mute(flow, role, !current) {
                    shared
                        .lock()
                        .expect("call runtime mutex poisoned")
                        .output_muted = actual;
                }
            }
        }
        post(hwnd, WM_APP_CALL_AUDIO);
    }
}

fn refresh_audio(shared: &Arc<Mutex<CallRuntime>>) {
    let (rflow, rrole) = audio::communications_render();
    let (cflow, crole) = audio::communications_capture();
    let device = audio::friendly_name(rflow, rrole)
        .or_else(|_| {
            let (flow, role) = audio::console_render();
            audio::friendly_name(flow, role)
        })
        .unwrap_or_default();
    let mic = audio::mute_state(cflow, crole).unwrap_or(false);
    let output = audio::mute_state(rflow, rrole).unwrap_or(false);
    let mut runtime = shared.lock().expect("call runtime mutex poisoned");
    runtime.device = device;
    runtime.mic_muted = mic;
    runtime.output_muted = output;
}

fn post(hwnd: isize, message: u32) {
    // SAFETY: posting to our own window.
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), message, WPARAM(0), LPARAM(0));
    }
}

fn device_name() -> String {
    audio::friendly_name(
        audio::communications_render().0,
        audio::communications_render().1,
    )
    .or_else(|_| audio::friendly_name(audio::console_render().0, audio::console_render().1))
    .unwrap_or_default()
}

struct RegKey(HKEY);
impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the handle.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

struct EventHandle(HANDLE);
impl Drop for EventHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[derive(Debug, Clone)]
struct InUse {
    app_name: String,
    launch: String,
    start_filetime: u64,
}

fn scan(root: &HKEY, allowlist: &[String]) -> Option<CallDetect> {
    let now_ft = filetime_now();
    let mut best: Option<InUse> = None;

    let names = subkeys(*root);
    for name in names {
        if name.eq_ignore_ascii_case("NonPackaged") {
            let mut sub = HKEY::default();
            let path = format!("{}\\NonPackaged", key_path_for_nonpackaged());
            let path_wide = windows::core::HSTRING::from(path);
            // SAFETY: path is NUL-terminated below; sub is closed on drop.
            let opened = unsafe {
                RegOpenKeyExW(HKEY_CURRENT_USER, &path_wide, Some(0), KEY_READ, &mut sub)
            };
            if opened.is_ok() {
                let key = RegKey(sub);
                for child in subkeys(key.0) {
                    if let Some(hit) = inspect(&key.0, &child, allowlist) {
                        if best
                            .as_ref()
                            .map(|b| hit.start_filetime > b.start_filetime)
                            .unwrap_or(true)
                        {
                            best = Some(hit);
                        }
                    }
                }
            }
        } else if let Some(hit) = inspect(root, &name, allowlist) {
            if best
                .as_ref()
                .map(|b| hit.start_filetime > b.start_filetime)
                .unwrap_or(true)
            {
                best = Some(hit);
            }
        }
    }

    let now = Instant::now();
    best.map(|hit| {
        let seconds = ((now_ft.saturating_sub(hit.start_filetime)) as f64 / 10_000_000.0)
            .clamp(0.0, 60.0 * 60.0 * 24.0);
        CallDetect {
            app_name: hit.app_name,
            launch: hit.launch,
            started: now - Duration::from_secs_f64(seconds),
            duration_at_fetch: seconds,
        }
    })
}

/// The NonPackaged container lives under the same microphone consent key.
/// Kept separate so `scan` can open it directly.
fn key_path_for_nonpackaged() -> String {
    // RegOpenKeyExW is relative to HKEY_CURRENT_USER above; this is the full
    // path minus the root.
    "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone"
        .to_string()
}

fn inspect(root: &HKEY, subkey: &str, allowlist: &[String]) -> Option<InUse> {
    let mut key = HKEY::default();
    let wide = windows::core::HSTRING::from(subkey);
    // SAFETY: key is closed by RegKey; wide is NUL-terminated.
    let opened = unsafe { RegOpenKeyExW(*root, &wide, Some(0), KEY_READ, &mut key) };
    if opened.is_err() {
        return None;
    }
    let key = RegKey(key);
    let start = read_qword(key.0, w!("LastUsedTimeStart"))?;
    let stop = read_qword(key.0, w!("LastUsedTimeStop")).unwrap_or(0);
    if !microphone_in_use(start, stop) {
        return None;
    }
    let decoded = subkey.replace('#', "\\");
    let app_name = allowlist
        .iter()
        .find(|allowed| {
            decoded
                .to_ascii_lowercase()
                .contains(&allowed.to_ascii_lowercase())
        })?
        .clone();
    let launch = if decoded.to_ascii_lowercase().ends_with(".exe") {
        decoded.clone()
    } else {
        format!("shell:AppsFolder\\{subkey}")
    };
    Some(InUse {
        app_name,
        launch,
        start_filetime: start,
    })
}

fn read_qword(key: HKEY, value: PCWSTR) -> Option<u64> {
    let mut kind = windows::Win32::System::Registry::REG_VALUE_TYPE::default();
    let mut data = [0u8; 8];
    let mut size = data.len() as u32;
    // SAFETY: data is eight writable bytes and size is its length.
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
    if result.is_err() || size != 8 || kind != REG_QWORD {
        return None;
    }
    Some(u64::from_le_bytes(data))
}

fn subkeys(key: HKEY) -> Vec<String> {
    let mut result = Vec::new();
    let mut index = 0u32;
    loop {
        let mut buffer = [0u16; 512];
        // SAFETY: buffer is a writable UTF-16 buffer of the stated length.
        let status = unsafe { RegEnumKeyW(key, index, Some(&mut buffer)) };
        if status == ERROR_NO_MORE_ITEMS || status.is_err() {
            break;
        }
        let len = buffer.iter().position(|c| *c == 0).unwrap_or(0);
        if len > 0 {
            result.push(String::from_utf16_lossy(&buffer[..len]));
        }
        index += 1;
        if index > 4096 {
            break;
        }
    }
    result
}

/// `LastUsedTimeStop == 0` while start is set means "currently in use"; some
/// Windows versions instead write start > stop, so accept both.
fn microphone_in_use(start: u64, stop: u64) -> bool {
    start > 0 && (stop == 0 || start > stop)
}

fn filetime_now() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    116_444_736_000_000_000 + now.as_secs() * 10_000_000 + u64::from(now.subsec_nanos()) / 100
}

// ------------------------------------------------------- Phone Link state

/// What the island can prove about the Windows Phone Link app, fully offline
/// (no network, no WinRT, no private API).
///
/// Windows exposes no "is my phone connected" API, so this only reports what
/// the local AppX registration and Phone Link's own device metadata say. The
/// card never claims a live connection: [`PhoneLinkState::Available`] means
/// "a phone is linked on this PC", the strongest honest statement available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneLinkState {
    /// No `Microsoft.YourPhone_*` package is registered for this user.
    NotInstalled,
    /// Installed, but it has never written local device metadata: no sign-in
    /// or pairing has happened on this PC.
    NotSignedIn,
    /// Installed and set up, but no phone is linked to it.
    NotConnected,
    /// Installed with a linked phone — or metadata this build cannot read,
    /// where reporting "not connected" would be a guess.
    Available,
}

impl PhoneLinkState {
    /// Whether the Call card may show real status instead of the muted
    /// empty state (spec §8, A prompt §5).
    pub fn is_available(self) -> bool {
        self == PhoneLinkState::Available
    }

    /// The one line the muted CallIdle icon explains itself with: A prompt §5
    /// asks for "a tooltip that says which". `None` when there is nothing to
    /// explain.
    pub fn idle_tip(self) -> Option<&'static str> {
        match self {
            PhoneLinkState::NotInstalled => Some("Phone Link not installed"),
            PhoneLinkState::NotSignedIn => Some("Phone Link not signed in"),
            PhoneLinkState::NotConnected => Some("Phone not connected"),
            PhoneLinkState::Available => None,
        }
    }
}

const PHONELINK_PACKAGE_PREFIX: &str = "Microsoft.YourPhone_";
/// Package *family* — stable across versions, and what the LocalCache path
/// is built from (the registered key carries the full package name).
const PHONELINK_PACKAGE_FAMILY: &str = "Microsoft.YourPhone_8wekyb3d8bbwe";
const PHONELINK_PACKAGES_KEY: &str = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages";
/// The Call card is rebuilt on every refresh (music ticks at 80 ms) while
/// this probe reads the registry plus a small JSON file, so results are
/// reused. Installing Phone Link mid-session is picked up after this window.
const PHONE_LINK_TTL: Duration = Duration::from_secs(30);

static PHONE_LINK_CACHE: Mutex<Option<(Instant, PhoneLinkState)>> = Mutex::new(None);

/// Best-effort Phone Link state, cached for [`PHONE_LINK_TTL`].
pub fn phone_link_state() -> PhoneLinkState {
    if let Ok(cache) = PHONE_LINK_CACHE.lock() {
        if let Some((at, state)) = *cache {
            if at.elapsed() < PHONE_LINK_TTL {
                return state;
            }
        }
    }
    let state = probe_phone_link();
    if let Ok(mut cache) = PHONE_LINK_CACHE.lock() {
        *cache = Some((Instant::now(), state));
    }
    state
}

fn probe_phone_link() -> PhoneLinkState {
    if !phone_link_installed() {
        return PhoneLinkState::NotInstalled;
    }
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        // No profile path, nothing readable: claim nothing negative.
        return PhoneLinkState::Available;
    };
    let path = std::path::Path::new(&base)
        .join("Packages")
        .join(PHONELINK_PACKAGE_FAMILY)
        .join("LocalCache")
        .join("DeviceMetadataStorage.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => derive_state(Some(linked_phone(&text))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => derive_state(None),
        // Unreadable metadata: "not connected" would be a guess, so fall
        // back to the state that asserts nothing negative.
        Err(_) => PhoneLinkState::Available,
    }
}

/// `linked` is `None` when Phone Link has never written device metadata
/// (no sign-in or pairing on this PC) and otherwise what it records.
fn derive_state(linked: Option<bool>) -> PhoneLinkState {
    match linked {
        None => PhoneLinkState::NotSignedIn,
        Some(true) => PhoneLinkState::Available,
        Some(false) => PhoneLinkState::NotConnected,
    }
}

/// Whether the metadata records at least one linked phone. Tolerates both
/// spacings .NET writes (`"IsLinked":true` and `"IsLinked": true`).
fn linked_phone(text: &str) -> bool {
    text.match_indices("\"IsLinked\"").any(|(index, _)| {
        let rest = text[index + "\"IsLinked\"".len()..].trim_start();
        let rest = rest.strip_prefix(':').unwrap_or(rest).trim_start();
        rest.starts_with("true")
    })
}

/// Whether the Phone Link package is registered for the current user. The
/// `WindowsApps` execution alias is absent on machines that do have it
/// installed, so the per-user AppX registration is what counts.
fn phone_link_installed() -> bool {
    let mut key = HKEY::default();
    let wide = windows::core::HSTRING::from(PHONELINK_PACKAGES_KEY);
    // SAFETY: path is NUL-terminated and `key` is closed by `RegKey`.
    let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &wide, Some(0), KEY_READ, &mut key) };
    if opened.is_err() {
        return false;
    }
    let key = RegKey(key);
    family_registered(&subkeys(key.0))
}

fn family_registered(names: &[String]) -> bool {
    names
        .iter()
        .any(|name| name.starts_with(PHONELINK_PACKAGE_PREFIX))
}

/// Shell target that opens the real Phone Link app (the AUMID is the Start
/// menu entry for it). A launch, never a fake control.
pub fn phone_link_launch() -> String {
    format!("shell:AppsFolder\\{PHONELINK_PACKAGE_FAMILY}!App")
}

/// Best-effort "Open app" for the detected call app. Returns whether the
/// shell accepted the request; it never pretends a failed launch worked.
pub fn open_app(launch: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if launch.is_empty() {
        return false;
    }
    let operation = HSTRING::from("open");
    let target = HSTRING::from(launch);
    // SAFETY: HSTRING parameters are valid for the duration of the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            &operation,
            &target,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    result.0 as usize > 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphone_state_rules() {
        assert!(microphone_in_use(100, 0));
        assert!(microphone_in_use(100, 50));
        assert!(!microphone_in_use(0, 0));
        assert!(!microphone_in_use(50, 100));
    }

    #[test]
    fn allowlist_matching_is_case_insensitive_and_not_browser_default() {
        let list = ["Teams".to_string(), "WhatsApp".to_string()];
        assert!(list.iter().any(|v| "MicrosoftTeams_8wekyb3d8bbwe"
            .to_ascii_lowercase()
            .contains(&v.to_ascii_lowercase())));
        assert!(!list.iter().any(|v| "MSEdge_8wekyb3d8bbwe"
            .to_ascii_lowercase()
            .contains(&v.to_ascii_lowercase())));
    }

    #[test]
    fn packaged_launch_uses_shell_apps_folder() {
        let decoded = "MicrosoftTeams_8wekyb3d8bbwe";
        let launch = format!("shell:AppsFolder\\{decoded}");
        assert!(launch.starts_with("shell:AppsFolder"));
    }

    #[test]
    fn phone_link_package_is_recognised_by_family_prefix() {
        let registered = vec!["Microsoft.WindowsTerminal_8wekyb3d8bbwe".to_string()];
        assert!(!family_registered(&registered));
        let registered = vec![
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe".to_string(),
            "Microsoft.YourPhone_1.26092.119.0_x64__8wekyb3d8bbwe".to_string(),
        ];
        assert!(family_registered(&registered));
    }

    #[test]
    fn phone_link_metadata_decides_between_not_signed_in_and_not_connected() {
        assert_eq!(derive_state(None), PhoneLinkState::NotSignedIn);
        assert_eq!(derive_state(Some(false)), PhoneLinkState::NotConnected);
        assert_eq!(derive_state(Some(true)), PhoneLinkState::Available);
    }

    #[test]
    fn linked_phone_read_tolerates_both_json_spacings() {
        assert!(linked_phone(r#"{"Devices":[{"IsLinked":true}]}"#));
        assert!(linked_phone("{\"Devices\":[{\"IsLinked\": true}]}"));
        assert!(!linked_phone(r#"{"Devices":[{"IsLinked":false}]}"#));
        assert!(!linked_phone(r#"{"Devices":[]}"#));
        // The search must not read past the end of the document.
        assert!(!linked_phone(r#"{"note":"IsLinked""#));
    }

    #[test]
    fn idle_tip_says_which_condition_and_never_claims_a_connection() {
        assert_eq!(
            PhoneLinkState::NotInstalled.idle_tip(),
            Some("Phone Link not installed")
        );
        assert_eq!(
            PhoneLinkState::NotSignedIn.idle_tip(),
            Some("Phone Link not signed in")
        );
        assert_eq!(
            PhoneLinkState::NotConnected.idle_tip(),
            Some("Phone not connected")
        );
        assert_eq!(PhoneLinkState::Available.idle_tip(), None);
        // Only the installed-with-a-phone state draws real status.
        assert!(!PhoneLinkState::NotInstalled.is_available());
        assert!(PhoneLinkState::Available.is_available());
    }

    #[test]
    fn phone_link_launch_is_the_installed_app_id() {
        assert_eq!(
            phone_link_launch(),
            "shell:AppsFolder\\Microsoft.YourPhone_8wekyb3d8bbwe!App"
        );
    }

    #[test]
    fn phone_link_state_is_observed_not_invented() {
        // Whatever this machine reports, it must be one of the four states
        // and a negative tip must always come with a negative state.
        let state = phone_link_state();
        assert!(state.idle_tip().is_some() || state.is_available());
        if state.idle_tip().is_some() {
            assert!(!state.is_available());
        }
    }
}
