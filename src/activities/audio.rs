//! audio.rs — shared WASAPI helpers for call audio controls.
//!
//! WASAPI is COM/WinRT-ish and can block, so every caller runs these helpers
//! on its own worker thread. The UI only sees plain booleans/strings.

use windows::core::{Error, GUID, PWSTR};
use windows::Win32::Foundation::{PROPERTYKEY, RPC_E_CHANGED_MODE};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eRender, EDataFlow, ERole, IMMDeviceEnumerator,
    MMDeviceEnumerator,
};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantClear, PropVariantToString, PROPVARIANT,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};
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

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
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
