//! audio.rs — shared WASAPI helpers for call audio controls and the output
//! sheets (v0.9 §4).
//!
//! WASAPI is COM/WinRT-ish and can block, so every caller runs these helpers
//! on its own worker thread. The UI only sees plain booleans/strings and the
//! snapshot in [`endpoints`].

use std::sync::Mutex;

use windows::core::{Error, GUID, PWSTR};
use windows::Win32::Foundation::{PROPERTYKEY, RPC_E_CHANGED_MODE};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantClear, PropVariantToString, PROPVARIANT,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};

pub fn console_render() -> (EDataFlow, ERole) {
    (eRender, eConsole)
}

pub fn communications_render() -> (EDataFlow, ERole) {
    (eRender, eCommunications)
}

pub fn communications_capture() -> (EDataFlow, ERole) {
    (eCapture, eCommunications)
}

pub(crate) fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    // SAFETY: in-proc COM object creation; CLSCTX_ALL stays in-process here.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

pub fn endpoint_volume(
    flow: EDataFlow,
    role: ERole,
) -> windows::core::Result<IAudioEndpointVolume> {
    let enumerator = enumerator()?;
    // SAFETY: enumerator is a live COM object; the role is a valid enum.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(flow, role)? };
    // SAFETY: activation of the standard endpoint interface on the device.
    unsafe { device.Activate(CLSCTX_ALL, None) }
}

pub fn set_mute(flow: EDataFlow, role: ERole, mute: bool) -> windows::core::Result<bool> {
    let endpoint = endpoint_volume(flow, role)?;
    // SAFETY: null event context is allowed; the call sets the endpoint mute.
    unsafe { endpoint.SetMute(mute, std::ptr::null())? };
    // SAFETY: read back so the UI shows the real state, not our guess.
    let actual = unsafe { endpoint.GetMute()? };
    Ok(actual.as_bool())
}

pub fn mute_state(flow: EDataFlow, role: ERole) -> windows::core::Result<bool> {
    let endpoint = endpoint_volume(flow, role)?;
    // SAFETY: plain getter on a live endpoint.
    let actual = unsafe { endpoint.GetMute()? };
    Ok(actual.as_bool())
}

/// Default endpoint friendly name (`Headset (Realtek)` style). Empty when the
/// property is unavailable; the renderer then leaves the line blank.
pub fn friendly_name(flow: EDataFlow, role: ERole) -> windows::core::Result<String> {
    let enumerator = enumerator()?;
    // SAFETY: enumerator is a live COM object; the role is a valid enum.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(flow, role)? };
    device_name(&device)
}

/// The `Device_Friendly_Name` property of one endpoint.
fn device_name(device: &IMMDevice) -> windows::core::Result<String> {
    // SAFETY: device outlives the property read.
    let store: IPropertyStore = unsafe { device.OpenPropertyStore(STGM_READ)? };
    // SAFETY: property key is a constant; the store owns the result.
    let mut value: PROPVARIANT = unsafe { store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME)? };
    let mut buffer = [0u16; 512];
    // SAFETY: buffer is writable and sized; value is a live PROPVARIANT.
    let result = unsafe { PropVariantToString(&value, &mut buffer) };
    // SAFETY: value came from GetValue and must be cleared exactly once.
    let _ = unsafe { PropVariantClear(&mut value) };
    result?;
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..len]))
}

/// Endpoint id of the default device for `flow`/`role`
/// (`{0.0.0.00000000}.{guid}` style). The output sheets compare ids rather
/// than names, so two endpoints sharing a friendly name stay distinct.
/// Empty when the query fails: the caller then claims nothing.
pub fn default_id(flow: EDataFlow, role: ERole) -> String {
    let Ok(enumerator) = enumerator() else {
        return String::new();
    };
    // SAFETY: enumerator is a live COM object; the role is a valid enum.
    let Ok(device) = (unsafe { enumerator.GetDefaultAudioEndpoint(flow, role) }) else {
        return String::new();
    };
    device_id(&device)
}

/// `GetId`'s NUL-terminated buffer, read and freed here so no caller can
/// leak it.
fn device_id(device: &IMMDevice) -> String {
    // SAFETY: GetId allocates the buffer freed below.
    let Ok(id) = (unsafe { device.GetId() }) else {
        return String::new();
    };
    // SAFETY: NUL-terminated UTF-16 written by GetId, read before the free.
    let text = unsafe { id.to_string() }.unwrap_or_default();
    // SAFETY: the buffer came from GetId and is ours to release now.
    unsafe { CoTaskMemFree(Some(id.0 as *const _)) };
    text
}

/// One active render endpoint an output sheet can switch to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Endpoint id: ids, never names, so duplicates stay distinct.
    pub id: String,
    /// Friendly name from the endpoint's own property store.
    pub name: String,
}

/// Everything the output sheets need about the machine's render endpoints,
/// read on the audio worker and only cloned by the UI thread.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EndpointList {
    /// Every `eRender` endpoint in `DEVICE_STATE_ACTIVE`, Windows' own order.
    pub endpoints: Vec<Endpoint>,
    /// Id of the default multimedia render endpoint (what music plays on).
    pub default_id: String,
    /// Its friendly name — the music chip's text. Empty when unknown.
    pub default_name: String,
}

/// The snapshot the UI reads. Cloned under a short lock: the worker is the
/// only writer and the UI only ever takes a copy.
static ENDPOINTS: Mutex<EndpointList> = Mutex::new(EndpointList {
    endpoints: Vec::new(),
    default_id: String::new(),
    default_name: String::new(),
});

/// Re-enumerate every active render endpoint and the current default.
/// Worker thread only — WASAPI can block.
pub fn refresh_endpoints() {
    let list = read_endpoints();
    if let Ok(mut slot) = ENDPOINTS.lock() {
        *slot = list;
    }
}

/// The current snapshot for the UI thread.
pub fn endpoints() -> EndpointList {
    ENDPOINTS
        .lock()
        .map(|slot| slot.clone())
        .unwrap_or_default()
}

/// Enumerate `DEVICE_STATE_ACTIVE` render endpoints plus the default one.
/// A failed query returns whatever was still readable — an empty list is a
/// fact about the query, never a claim that the machine has no devices.
fn read_endpoints() -> EndpointList {
    let default = default_id(eRender, eMultimedia);
    let mut list = EndpointList {
        endpoints: Vec::new(),
        default_id: default.clone(),
        default_name: String::new(),
    };
    let Ok(enumerator) = enumerator() else {
        return list;
    };
    // SAFETY: enumerator is a live COM object; the mask is a valid flag set.
    let Ok(collection) = (unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) })
    else {
        return list;
    };
    // SAFETY: live collection; the count is what it reports.
    let Ok(count) = (unsafe { collection.GetCount() }) else {
        return list;
    };
    for index in 0..count {
        // SAFETY: `index` is inside the count just read.
        let Ok(device) = (unsafe { collection.Item(index) }) else {
            break;
        };
        let id = device_id(&device);
        if id.is_empty() {
            continue;
        }
        let name = device_name(&device).unwrap_or_default();
        if id == default {
            list.default_name = name.clone();
        }
        list.endpoints.push(Endpoint { id, name });
    }
    // The default can sit outside the active list (or its property read can
    // fail while the enumeration succeeded): ask for the name directly
    // instead of leaving the chip blank or guessing.
    if list.default_name.is_empty() && !list.default_id.is_empty() {
        list.default_name = friendly_name(eRender, eMultimedia).unwrap_or_default();
    }
    list
}

/// Make `id` the default endpoint for **every** role (console, multimedia,
/// communications) so music and a running call follow the sheet's choice
/// together. Returns whether Windows accepted at least one role.
pub fn set_default_all(id: &str) -> bool {
    let mut accepted = false;
    for role in [eConsole, eMultimedia, eCommunications] {
        accepted |= crate::win::policy_config::set_default_endpoint(id, role);
    }
    accepted
}

/// COM initialization for worker threads. `RPC_E_CHANGED_MODE` means COM was
/// already initialized as STA; that is fine for these calls.
pub fn init_mta() -> windows::core::Result<()> {
    // SAFETY: balanced by thread exit; explicit uninit is not required.
    let hr = unsafe {
        windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        )
    };
    if hr.is_ok() || hr == RPC_E_CHANGED_MODE {
        Ok(())
    } else {
        Err(Error::from_hresult(hr))
    }
}

#[allow(dead_code)]
fn _pwstr_marker(_: PWSTR) {}
