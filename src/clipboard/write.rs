//! Copy-back: put a history row back on the system clipboard (v0.7 Phase 4).
//!
//! The capture worker stored exactly what it read, so this module writes the
//! same payloads back: `CF_UNICODETEXT` for text and links, `CF_HDROP` for file
//! rows (reused from `convert::shell`) and a `CF_DIB` rebuilt from the stored
//! PNG for images. `ClipboardRuntime::copy_back` arms the own-write guard
//! straight after a successful write, so the `WM_CLIPBOARDUPDATE` caused here
//! is dropped instead of being recorded as a new copy (spec 5.7).

use std::path::PathBuf;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_DIB, CF_UNICODETEXT};

use crate::design;

use super::store::{blob_path, ClipItem, ClipKind};

/// Puts `item` back on the clipboard in the format its kind was captured as.
/// `owner` is the window the clipboard is associated with for the duration of
/// the write; it is only used by the file-list path.
pub fn write(item: &ClipItem, owner: HWND) -> Result<(), String> {
    match item.kind {
        ClipKind::Text | ClipKind::Link => {
            set_format(CF_UNICODETEXT.0 as u32, &text_payload(&item.text))
        }
        ClipKind::Files => {
            // A row whose files are all gone is dimmed in the list; copying it
            // would hand the target app a dead drop (spec 4).
            let paths = live_paths(&item.paths);
            if paths.is_empty() {
                return Err("File moved or deleted".to_string());
            }
            crate::convert::shell::copy_paths(owner, &paths)
        }
        ClipKind::Image => write_image(item),
    }
}

/// The subset of `paths` that still exists, in the original order.
fn live_paths(paths: &[String]) -> Vec<PathBuf> {
    paths
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect()
}

/// UTF-16LE with one trailing NUL, as `CF_UNICODETEXT` wants. Embedded NULs
/// would truncate the string for every consumer, so they are dropped.
fn text_payload(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity((text.chars().count() + 1) * 2);
    for unit in text.encode_utf16().filter(|unit| *unit != 0) {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// Rebuilds a `CF_DIB` block: 40-byte `BITMAPINFOHEADER`, bottom-up, 32 bpp
/// `BI_RGB`, one BGRA pixel per source RGBA pixel. Written by hand rather than
/// through `BITMAPINFOHEADER` so the layout is plain bytes a test can assert.
fn dib_payload(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }
    let stride = width as usize * 4;
    let pixels = stride.checked_mul(height as usize)?;
    if rgba.len() != pixels {
        return None;
    }
    let mut out = Vec::with_capacity(40 + pixels);
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&(width as i32).to_le_bytes()); // biWidth
                                                          // Positive height: the first row in the block is the image's bottom row.
    out.extend_from_slice(&(height as i32).to_le_bytes()); // biHeight
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    out.extend_from_slice(&(pixels as u32).to_le_bytes()); // biSizeImage
    out.extend_from_slice(&0u32.to_le_bytes()); // biXPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes()); // biYPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant
    for row in (0..height as usize).rev() {
        let source = &rgba[row * stride..row * stride + stride];
        for pixel in source.as_chunks::<4>().0 {
            // RGBA -> BGRA.
            out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    Some(out)
}

fn write_image(item: &ClipItem) -> Result<(), String> {
    let bytes = std::fs::read(blob_path(&item.digest))
        .map_err(|_| "That image is no longer on disk.".to_string())?;
    if bytes.len() as u64 > design::CLIP_IMAGE_MAX_BYTES {
        return Err("That image is too large to copy back.".to_string());
    }
    // The blob is ours, but a decoder panic must never take the UI thread down
    // (the capture worker wraps its decodes the same way).
    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
    }))
    .map_err(|_| "That image could not be read back.".to_string())?
    .map_err(|_| "That image could not be read back.".to_string())?;
    let rgba = decoded.to_rgba8();
    let dib = dib_payload(rgba.as_raw(), rgba.width(), rgba.height())
        .ok_or_else(|| "That image could not be read back.".to_string())?;
    set_format(CF_DIB.0 as u32, &dib)
}

/// Empties the clipboard and hands `bytes` to the system as `format`. Mirrors
/// `convert::shell::copy_paths`: on success ownership of the allocation
/// transfers to the system, otherwise it is freed here.
fn set_format(format: u32, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("There is nothing to put on the clipboard.".to_string());
    }
    // SAFETY: balanced Open/CloseClipboard around the whole write.
    unsafe {
        OpenClipboard(None).map_err(|_| "The clipboard is busy. Try again.".to_string())?;
        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|_| "The clipboard couldn't be cleared.".to_string())?;
            let handle: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
                .map_err(|_| "Not enough memory for the clipboard.".to_string())?;
            let base = GlobalLock(handle);
            if base.is_null() {
                let _ = GlobalFree(Some(handle));
                return Err("Not enough memory for the clipboard.".to_string());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), base as *mut u8, bytes.len());
            let _ = GlobalUnlock(handle);
            match SetClipboardData(format, Some(HANDLE(handle.0))) {
                // Ownership transfers to the system on success.
                Ok(_) => Ok(()),
                Err(_) => {
                    let _ = GlobalFree(Some(handle));
                    Err("The clipboard couldn't be updated.".to_string())
                }
            }
        })();
        let _ = CloseClipboard();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_payload_is_utf16_with_one_nul() {
        assert_eq!(text_payload("hi"), vec![b'h', 0, b'i', 0, 0, 0]);
        // A non-ASCII unit survives as its UTF-16 code unit.
        assert_eq!(text_payload("\u{e9}"), vec![0xE9, 0, 0, 0]);
        // An embedded NUL would truncate the string for every consumer.
        assert_eq!(text_payload("a\0b"), vec![b'a', 0, b'b', 0, 0, 0]);
        assert_eq!(text_payload(""), vec![0, 0]);
    }

    #[test]
    fn dib_payload_is_bottom_up_bgra_behind_a_40_byte_header() {
        // Top row: red, green. Bottom row: blue, white.
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, // top
            0, 0, 255, 255, 255, 255, 255, 255, // bottom
        ];
        let dib = dib_payload(&rgba, 2, 2).expect("a 2x2 raster is valid");
        assert_eq!(dib.len(), 40 + 16);
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(dib[4..8].try_into().unwrap()), 2);
        // Positive height marks the block bottom-up.
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(dib[12..14].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 32);
        assert_eq!(u32::from_le_bytes(dib[16..20].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(dib[20..24].try_into().unwrap()), 16);
        // First stored row is the source's bottom row (blue, white), channels
        // swapped: blue RGBA (0,0,255) becomes BGRA (255,0,0).
        assert_eq!(&dib[40..48], &[255, 0, 0, 255, 255, 255, 255, 255]);
        // Second stored row is the source's top row: red -> BGRA (0,0,255),
        // green -> BGRA (0,255,0).
        assert_eq!(&dib[48..56], &[0, 0, 255, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn dib_payload_rejects_a_raster_that_does_not_match_its_size() {
        assert!(dib_payload(&[0u8; 15], 2, 2).is_none());
        assert!(dib_payload(&[0u8; 16], 2, 2).is_some());
        assert!(dib_payload(&[], 0, 0).is_none());
    }

    #[test]
    fn live_paths_drops_missing_files_and_keeps_order() {
        let present = std::env::temp_dir().to_string_lossy().into_owned();
        let missing = "C:\\no\\such\\taskbar-island\\row.txt".to_string();
        assert!(live_paths(std::slice::from_ref(&missing)).is_empty());
        let mixed = live_paths(&[present.clone(), missing.clone(), missing]);
        assert_eq!(mixed.len(), 1);
        assert_eq!(mixed[0], PathBuf::from(present));
    }
}
