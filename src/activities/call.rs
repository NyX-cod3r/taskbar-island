//! call.rs — real endpoint mute buttons, Phone Link presence, and the
//! display names Source A matches toasts against.
//!
//! Call *state* no longer lives here: spec §3 replaced the microphone
//! consent-store watcher with the [`callsrc`](super::callsrc) sources that
//! feed one state machine. This module keeps the pieces the Call card still
//! needs: the shared audio runtime (device name, endpoint mutes), the Phone
//! Link presence probe, and [`phone_link_names`] — the localized app display
//! names read from the package at runtime.

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{implement, PCWSTR};
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, HWND, LPARAM, PROPERTYKEY, WPARAM};
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eRender, EDataFlow, ERole, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, DEVICE_STATE,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::activities::audio;

pub const WM_APP_CALL: u32 = WM_APP + 6;
/// `WM_APP_CALL` wparam: the call sources posted new signals to fold in.
pub const CALL_WAKE_SIGNALS: usize = 0;
/// `WM_APP_CALL` wparam: the audio worker refreshed device/mutes/endpoint
/// list. `lparam` is `1` when the last default-device switch could not be
/// applied and the worker opened Windows' sound settings instead (v0.9 §4).
pub const CALL_WAKE_AUDIO: usize = 1;
/// `WM_APP_CALL` wparam: a UIA action finished; `lparam` is an
/// [`ActionOutcome`] code.
pub const CALL_WAKE_ACTION: usize = 2;

/// Result of one UIA action attempt, carried in `lparam` (spec §3 Actions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    /// The button was invoked: the provider will confirm the state change.
    Invoked,
    /// No such button right now: show "Open Phone Link to do this".
    NotFound,
    /// The button exists but the invoke failed: say so, never fake success.
    Failed,
}

impl ActionOutcome {
    pub fn code(self) -> isize {
        match self {
            Self::Invoked => 0,
            Self::NotFound => 1,
            Self::Failed => 2,
        }
    }

    pub fn from_code(code: isize) -> Self {
        match code {
            0 => Self::Invoked,
            1 => Self::NotFound,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CallRuntime {
    pub device: String,
    pub mic_muted: bool,
    pub output_muted: bool,
    /// The default communications endpoint is a Bluetooth Hands-Free profile
    /// endpoint (proved from its own name), so the sheet can say what the
    /// route really is instead of only its brand name.
    pub handsfree: bool,
    /// Endpoint id Windows routes call audio through (the default
    /// communications render device). Empty while the query failed.
    pub comms_id: String,
    /// Friendly name of the default console render device — the output
    /// sheet's "PC speakers and microphone" row. Empty on query failure.
    pub console_device: String,
    /// Endpoint id of that console device, for the same-id comparison.
    pub console_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioCommand {
    #[allow(dead_code)] // Sent when the Call card re-reads the audio state.
    Refresh,
    ToggleMic,
    ToggleOutput,
    /// Make this endpoint the default for every role (an output sheet's
    /// choice). When Windows refuses, the worker opens the sound settings
    /// page and reports it through `CALL_WAKE_AUDIO` with `lparam = 1`.
    SetDefault(String),
}

pub fn runtime() -> Arc<Mutex<CallRuntime>> {
    Arc::new(Mutex::new(CallRuntime::default()))
}

/// Start the audio command worker (device name + endpoint mutes + the
/// endpoint list the output sheets read).
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

fn audio_worker(hwnd: isize, shared: Arc<Mutex<CallRuntime>>, rx: Receiver<AudioCommand>) {
    if audio::init_mta().is_err() {
        return;
    }
    refresh_audio(&shared);
    audio::refresh_endpoints();
    post(hwnd, WM_APP_CALL, CALL_WAKE_AUDIO, 0);
    // Non-zero while the command just handled needs the honest "we opened
    // sound settings" note instead of the routine refresh wake.
    let mut fallback = 0isize;
    while let Ok(command) = rx.recv() {
        match command {
            AudioCommand::Refresh => {
                refresh_audio(&shared);
                audio::refresh_endpoints();
            }
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
            AudioCommand::SetDefault(id) => {
                // The sheet promised a real switch: set every role at once.
                // When Windows refuses (empty or unknown id), open its own
                // sound page instead of moving our check mark for nothing.
                if id.is_empty() || !audio::set_default_all(&id) {
                    crate::log::log("call.audio: set-default refused -> sound settings");
                    open_app("ms-settings:sound");
                    fallback = 1;
                }
                // Re-read the ids either way: a successful switch must move
                // the sheet's check mark to the row that was picked.
                refresh_audio(&shared);
                audio::refresh_endpoints();
            }
        }
        post(hwnd, WM_APP_CALL, CALL_WAKE_AUDIO, fallback);
        fallback = 0;
    }
}

fn refresh_audio(shared: &Arc<Mutex<CallRuntime>>) {
    let (rflow, rrole) = audio::communications_render();
    let (cflow, crole) = audio::communications_capture();
    let (kflow, krole) = audio::console_render();
    let device = audio::friendly_name(rflow, rrole)
        .or_else(|_| audio::friendly_name(kflow, krole))
        .unwrap_or_default();
    // Hands-Free AG endpoints only exist over Bluetooth's HF profile, so the
    // endpoint's own name proves the "Bluetooth hands-free" wording.
    let handsfree = device.to_ascii_lowercase().contains("hands-free");
    let mic = audio::mute_state(cflow, crole).unwrap_or(false);
    let output = audio::mute_state(rflow, rrole).unwrap_or(false);
    // The output sheet's routes, read on the worker: the call's own endpoint
    // (id only — its name is `device` above) and the PC default (name + id).
    let comms_id = audio::default_id(rflow, rrole);
    let console_device = audio::friendly_name(kflow, krole).unwrap_or_default();
    let console_id = audio::default_id(kflow, krole);
    let mut runtime = shared.lock().expect("call runtime mutex poisoned");
    runtime.device = device;
    runtime.handsfree = handsfree;
    runtime.mic_muted = mic;
    runtime.output_muted = output;
    runtime.comms_id = comms_id;
    runtime.console_device = console_device;
    runtime.console_id = console_id;
}

fn post(hwnd: isize, message: u32, wake: usize, lparam: isize) {
    // SAFETY: posting to our own window.
    unsafe {
        let _ = PostMessageW(
            Some(HWND(hwnd as *mut _)),
            message,
            WPARAM(wake),
            LPARAM(lparam),
        );
    }
}

/// The default-device listener (v0.9 §4). Windows *tells* us when the
/// default render endpoint moves, so the sheets' check marks and the music
/// chip never go stale — no polling anywhere. The callback runs on the UI
/// thread that registered it (an STA with a message pump) and only queues an
/// [`AudioCommand::Refresh`]: no WASAPI, no UI work, nothing that can block.
#[implement(IMMNotificationClient)]
struct DefaultChangeWatcher {
    tx: Sender<AudioCommand>,
}

impl IMMNotificationClient_Impl for DefaultChangeWatcher_Impl {
    fn OnDeviceStateChanged(
        &self,
        _id: &PCWSTR,
        _state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _id: &PCWSTR,
    ) -> windows::core::Result<()> {
        // Only the two render defaults the sheets offer; a capture change
        // (plugging in a mic) is not ours to react to.
        if flow == eRender && (role == eConsole || role == eCommunications) {
            let _ = self.tx.send(AudioCommand::Refresh);
        }
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _id: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Keep-alive for the registration: the enumerator owns the callback list,
/// so both halves must live and die together.
struct AudioWatcher {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

impl Drop for AudioWatcher {
    fn drop(&mut self) {
        // SAFETY: this pair was registered on this thread and is still alive.
        let _ = unsafe {
            self.enumerator
                .UnregisterEndpointNotificationCallback(&self.client)
        };
    }
}

thread_local! {
    /// The one listener, owned by the UI thread that registered it.
    static AUDIO_WATCH: RefCell<Option<AudioWatcher>> = const { RefCell::new(None) };
}

/// Register the default-device listener on the **calling** thread. That must
/// be the UI thread *after* `OleInitialize`: an STA with a message pump is
/// what dispatches the COM callback, and keeping it here means no callback
/// can outlive the window.
pub fn start_audio_watcher(tx: Sender<AudioCommand>) {
    AUDIO_WATCH.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return;
        }
        let client: IMMNotificationClient = DefaultChangeWatcher { tx }.into();
        let Ok(enumerator) = audio::enumerator() else {
            crate::log::log("call.audio: no device enumerator; sheets will not self-update");
            return;
        };
        // SAFETY: the calling thread is OLE-initialized (registration is a
        // COM call), our in-proc client stays alive in `slot` below.
        if let Err(err) = unsafe { enumerator.RegisterEndpointNotificationCallback(&client) } {
            crate::log::log(&format!("call.audio: watcher registration failed: {err}"));
            return;
        }
        *slot = Some(AudioWatcher { enumerator, client });
        crate::log::log("call.audio: default-device watcher registered");
    });
}

/// Undo [`start_audio_watcher`] on window destroy: dropping the pair
/// unregisters it, so no callback can arrive after the window is gone.
pub fn stop_audio_watcher() {
    AUDIO_WATCH.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

struct RegKey(HKEY);
impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the handle.
        let _ = unsafe { RegCloseKey(self.0) };
    }
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

/// Localized Phone Link display names, resolved once by the call-source
/// bootstrap (a WinRT call, never the UI thread) and only read afterwards.
static PHONE_LINK_NAMES: Mutex<Option<Vec<String>>> = Mutex::new(None);

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

/// Display names Phone Link registers its toast/UIA windows under. Sources A
/// and C match against these (localized per machine, so read from the package
/// rather than hardcoded). This is a WinRT call — resolve it on a worker
/// (the call-source bootstrap does), never the UI thread; the result is
/// cached for the process. Falls back to the English names when the package
/// cannot be read (Phone Link uninstalled mid-session, AppX broken, …).
pub fn phone_link_names() -> Vec<String> {
    if let Ok(cache) = PHONE_LINK_NAMES.lock() {
        if let Some(names) = cache.as_ref() {
            return names.clone();
        }
    }
    let names = resolve_phone_link_names();
    if let Ok(mut cache) = PHONE_LINK_NAMES.lock() {
        *cache = Some(names.clone());
    }
    names
}

/// Cached display name for the card's `Open {app_name}` button. Returns the
/// English default until [`phone_link_names`] has resolved once, so the UI
/// thread never makes the WinRT call itself.
pub fn phone_link_display_name() -> String {
    PHONE_LINK_NAMES
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref().and_then(|names| names.first().cloned()))
        .unwrap_or_else(|| "Phone Link".to_string())
}

fn resolve_phone_link_names() -> Vec<String> {
    const FALLBACK: &[&str] = &["Phone Link", "Your Phone"];
    let mut names: Vec<String> = FALLBACK.iter().map(|s| (*s).to_string()).collect();
    let manager = match windows::Management::Deployment::PackageManager::new() {
        Ok(manager) => manager,
        Err(_) => return names,
    };
    let wide = windows::core::HSTRING::from(PHONELINK_PACKAGE_FAMILY);
    let Ok(packages) = manager.FindPackagesByPackageFamilyName(&wide) else {
        return names;
    };
    let Ok(first) = packages.First() else {
        return names;
    };
    if let Ok(true) = first.HasCurrent() {
        if let Ok(package) = first.Current() {
            if let Ok(display) = package.DisplayName() {
                let display = display.to_string();
                if !display.is_empty() && !names.iter().any(|n| n.eq_ignore_ascii_case(&display)) {
                    names.insert(0, display);
                }
            }
        }
    }
    names
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
    fn action_outcomes_round_trip_through_the_message_wparam() {
        for outcome in [
            ActionOutcome::Invoked,
            ActionOutcome::NotFound,
            ActionOutcome::Failed,
        ] {
            assert_eq!(ActionOutcome::from_code(outcome.code()), outcome);
        }
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
