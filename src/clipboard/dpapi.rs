//! dpapi.rs — current-user DPAPI encryption for the clipboard store.
//!
//! Clipboard data can contain secrets, so the on-disk history is encrypted with
//! `CryptProtectData` (current-user scope) and never written as plain text.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input points at `data` for its full length; output is an owned
    // DPAPI allocation released with LocalFree below.
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .ok()?;
    }
    if output.pbData.is_null() || output.cbData == 0 {
        return None;
    }
    // SAFETY: DPAPI returned a buffer of cbData bytes.
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    // SAFETY: the buffer belongs to the caller and is no longer used.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData as _)));
    }
    Some(bytes)
}

pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input points at `data` for its full length; the description
    // pointer is optional and output is DPAPI-owned.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .ok()?;
    }
    if output.pbData.is_null() || output.cbData == 0 {
        return None;
    }
    // SAFETY: DPAPI returned a buffer of cbData bytes.
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    // SAFETY: the buffer belongs to the caller and is no longer used.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData as _)));
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_current_user_blob() {
        let plain = br#"{"hello":"world"}"#;
        let sealed = protect(plain).expect("DPAPI protect");
        assert_ne!(sealed.as_slice(), plain.as_slice());
        let opened = unprotect(&sealed).expect("DPAPI unprotect");
        assert_eq!(opened, plain);
    }

    #[test]
    fn rejects_garbage() {
        assert!(unprotect(b"not a dpapi blob").is_none());
    }
}
