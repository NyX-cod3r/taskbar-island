//! policy_config.rs — `IPolicyConfig[.Vista]::SetDefaultEndpoint`, the
//! interface Windows itself uses to change the default audio endpoint.
//!
//! The `windows` crate ships no binding for it (it is undocumented;
//! microsoft/windows-rs#1355), so the two class/interface pairs are
//! activated here and the method is called through its raw vtable slot —
//! the same layout the reference bindings in `yan0lovesha/AudioSwitch`
//! (`src/policy_config.rs`) use.

use std::ffi::c_void;
use std::ptr;

use windows::core::{IUnknown, Interface, GUID, PCWSTR};
use windows::Win32::Media::Audio::ERole;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

/// `CPolicyConfigVistaClient` / `IPolicyConfigVista` (Vista and later).
const VISTA_CLSID: GUID = GUID::from_u128(0x294935ce_f637_4e7c_a41b_ab255460b862);
const VISTA_IID: GUID = GUID::from_u128(0x568b9108_44bf_40b4_9006_86afe5b5a620);
/// `CPolicyConfigClient` / `IPolicyConfig` (Windows 7+): both class ids are
/// cited in the wild; the first is the one Windows ships.
const CLIENT_CLSID: GUID = GUID::from_u128(0x870af99a_179d_40c1_8c9c_223e84184558);
const CLIENT_CLSID_ALT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
const CLIENT_IID: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);

/// Vtable slot of `SetDefaultEndpoint` (IUnknown owns slots 0..3): the Vista
/// interface has it at 12; `IPolicyConfig` adds one method
/// (`ResetDeviceFormat`) before it, so 13.
const VISTA_SLOT: usize = 12;
const CLIENT_SLOT: usize = 13;

/// One `SetDefaultEndpoint(this, device id, role)` vtable entry.
type SetDefaultEndpoint =
    unsafe extern "system" fn(this: *mut c_void, id: PCWSTR, role: i32) -> windows::core::HRESULT;

/// Make `id` the default endpoint for one role. Returns whether Windows
/// accepted it — `false` means the caller must fall back (sound settings),
/// never pretend the switch landed.
pub fn set_default_endpoint(id: &str, role: ERole) -> bool {
    if id.is_empty() {
        return false;
    }
    let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
    // The interface moved between Windows versions, so try each known
    // pairing in order; a failed attempt costs one COM activation and one
    // QueryInterface and shows the user nothing.
    for (clsid, iid, slot) in [
        (VISTA_CLSID, VISTA_IID, VISTA_SLOT),
        (CLIENT_CLSID, CLIENT_IID, CLIENT_SLOT),
        (CLIENT_CLSID_ALT, CLIENT_IID, CLIENT_SLOT),
    ] {
        // SAFETY: in-proc activation of a system COM class. `IUnknown` is
        // only the hand-shake; the real interface arrives via QueryInterface.
        let unknown: IUnknown = match unsafe { CoCreateInstance(&clsid, None, CLSCTX_ALL) } {
            Ok(value) => value,
            Err(_) => continue,
        };
        let mut out: *mut c_void = ptr::null_mut();
        // SAFETY: `iid` names an interface the class may implement and `out`
        // is a valid out-pointer for it.
        if unsafe { unknown.query(&iid, &mut out) }.is_err() || out.is_null() {
            continue;
        }
        let ok = unsafe {
            // SAFETY: `out` came from a QI for `iid`, so its vtable *is*
            // that interface's and `slot` holds `SetDefaultEndpoint`.
            let vtable = *(out as *const *const *const c_void);
            let entry = *vtable.add(slot);
            let set: SetDefaultEndpoint = std::mem::transmute(entry);
            // `wide` is NUL-terminated and outlives the call; `role` is a
            // valid ERole value.
            set(out, PCWSTR::from_raw(wide.as_ptr()), role.0)
        };
        // The QI reference is ours: wrapping it makes the drop path call
        // Release, whether or not the call succeeded.
        let _release = unsafe { IUnknown::from_raw(out) };
        if ok.is_ok() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty id must never reach COM: the caller treats `false` as "open
    /// the sound settings", and a blank id would make Windows' own page look
    /// like the failure path for a device that does not exist.
    #[test]
    fn empty_id_is_refused_without_touching_com() {
        assert!(!set_default_endpoint("", ERole(0)));
    }
}
