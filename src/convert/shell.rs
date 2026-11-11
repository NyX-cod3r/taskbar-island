//! shell.rs — Explorer integration for conversion results.
//!
//! Nothing here converts or downloads anything. It only calls the shell:
//! reveal a finished file, undo by sending it to the Recycle Bin, and put the
//! paths on the clipboard as `CF_HDROP`.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::HSTRING;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::UI::Shell::{
    ILFree, SHFileOperationW, SHOpenFolderAndSelectItems, SHParseDisplayName, DROPFILES,
    FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW,
};

use windows::core::BOOL;

/// Selects `path` in File Explorer without opening or changing anything.
pub fn reveal(path: &Path) -> Result<(), String> {
    let wide = HSTRING::from(path.as_os_str());
    let mut pidl = std::ptr::null_mut();
    unsafe {
        SHParseDisplayName(
            &wide,
            None::<&windows::Win32::System::Com::IBindCtx>,
            &mut pidl,
            0,
            None,
        )
        .map_err(|_| "Explorer couldn't find this file.".to_string())?;
        let result = SHOpenFolderAndSelectItems(pidl, None, 0);
        ILFree(Some(pidl));
        result.map_err(|_| "Explorer couldn't show this file.".to_string())
    }
}

/// Sends `paths` to the Recycle Bin. `FOF_ALLOWUNDO` is always set; there is no
/// permanent-delete path in this app.
pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut from: Vec<u16> = Vec::new();
    for path in paths {
        from.extend(path.as_os_str().encode_wide());
        from.push(0);
    }
    from.push(0); // SHFileOperation wants a double-NUL list.

    let flags = FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_NOERRORUI.0 | FOF_SILENT.0;
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: windows::core::PCWSTR(from.as_ptr()),
        fFlags: flags as u16,
        ..Default::default()
    };
    // SAFETY: `from` is double-NUL terminated and outlives the call.
    let code = unsafe { SHFileOperationW(&mut operation) };
    if code != 0 {
        return Err("The Recycle Bin refused the file.".into());
    }
    if operation.fAnyOperationsAborted.as_bool() {
        return Err("The undo was cancelled.".into());
    }
    Ok(())
}

/// Puts `paths` on the clipboard as a file list, so they can be pasted in
/// Explorer or another app.
pub fn copy_paths(owner: HWND, paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut wide: Vec<u16> = Vec::new();
    for path in paths {
        wide.extend(path.as_os_str().encode_wide());
        wide.push(0);
    }
    wide.push(0);

    let header = std::mem::size_of::<DROPFILES>();
    let total = header + wide.len() * std::mem::size_of::<u16>();
    unsafe {
        OpenClipboard(Some(owner)).map_err(|_| "The clipboard is busy. Try again.".to_string())?;
        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|_| "The clipboard couldn't be cleared.".to_string())?;
            let handle: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, total)
                .map_err(|_| "Not enough memory for the clipboard.".to_string())?;
            let base = GlobalLock(handle);
            if base.is_null() {
                let _ = GlobalFree(Some(handle));
                return Err("Not enough memory for the clipboard.".to_string());
            }
            let header_ptr = base as *mut DROPFILES;
            *header_ptr = DROPFILES {
                pFiles: header as u32,
                pt: windows::Win32::Foundation::POINT { x: 0, y: 0 },
                fNC: BOOL(0),
                fWide: BOOL(1),
            };
            std::ptr::copy_nonoverlapping(
                wide.as_ptr(),
                (base as *mut u8).add(header) as *mut u16,
                wide.len(),
            );
            let _ = GlobalUnlock(handle);
            match SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(handle.0))) {
                // Ownership transfers to the system on success.
                Ok(_) => Ok(()),
                Err(_) => {
                    let _ = GlobalFree(Some(handle));
                    Err("The file list couldn't be put on the clipboard.".to_string())
                }
            }
        })();
        let _ = CloseClipboard();
        result
    }
}
