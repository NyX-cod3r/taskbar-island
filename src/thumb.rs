//! thumb.rs — off-thread shelf thumbnails.
//!
//! Files are treated as hostile: the format is sniffed from magic bytes, the
//! declared pixel count is capped before any allocation, and a decoder panic
//! (malformed file) is caught and turned into "no thumbnail". Nothing here
//! touches the network.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::time::UNIX_EPOCH;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::view::AlbumArt;

/// Posted once per decoded (or failed) thumbnail.
pub const WM_APP_THUMB: u32 = WM_APP + 33;
/// Longest side of a cached row/tile thumbnail, in pixels.
pub const MAX_THUMB: u32 = 128;
/// Longest side of a clipboard hover-preview decode (spec 7). The overlay
/// picture is 192 × 118 DIP, so a 384 px source stays sharp at 2× DPI while
/// still being far smaller than the row thumbnail's source file.
pub const PREVIEW_PX: u32 = 384;
/// Refuse to decode anything larger than this before allocating pixels.
pub const MAX_PIXELS: u64 = 100_000_000;

/// One decode job: which file, and the box it has to fit inside. Row and shelf
/// thumbnails ask for [`MAX_THUMB`], the clipboard hover preview for
/// [`PREVIEW_PX`]; the worker never guesses.
pub struct ThumbRequest {
    pub path: String,
    pub max: u32,
}

pub struct ThumbArt {
    pub key: u64,
    /// The box this decode was requested at; results are matched back to the
    /// cache entry that asked for them (`key` alone is size-blind).
    pub max: u32,
    /// `None` means the file could not be decoded; remember it so the cache
    /// does not retry every frame.
    pub art: Option<Arc<AlbumArt>>,
}

/// Cache key: the path plus size and modification time, so replacing a file
/// with the same name refreshes its thumbnail.
pub fn key_for(path: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.to_ascii_lowercase().hash(&mut hasher);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(&mut hasher);
        if let Ok(modified) = meta.modified() {
            if let Ok(stamp) = modified.duration_since(UNIX_EPOCH) {
                stamp.as_nanos().hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

/// Bitmap-cache identity: the path key salted with the requested box, so a
/// 128 px row thumbnail and a 384 px hover preview of the same file are two
/// `ID2D1Bitmap`s instead of one bitmap drawn at the wrong size.
pub fn art_id(key: u64, max: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    max.hash(&mut hasher);
    hasher.finish()
}

/// Fits `(w, h)` inside a `max` box without upscaling.
pub fn fit(w: u32, h: u32, max: u32) -> (u32, u32) {
    let longest = w.max(h);
    if longest == 0 || longest <= max {
        return (w.max(1), h.max(1));
    }
    let scale = max as f64 / longest as f64;
    (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    )
}

pub struct ThumbWorker {
    tx: Option<mpsc::Sender<ThumbRequest>>,
    results: Arc<Mutex<Vec<ThumbArt>>>,
}

impl ThumbWorker {
    pub fn start(hwnd: HWND) -> Self {
        let (tx, rx) = mpsc::channel::<ThumbRequest>();
        let results = Arc::new(Mutex::new(Vec::new()));
        let shared = results.clone();
        let target = hwnd.0 as isize;
        let spawned = std::thread::Builder::new()
            .name("island-thumbs".into())
            .spawn(move || {
                while let Ok(request) = rx.recv() {
                    let key = key_for(&request.path);
                    let art = decode(&request.path, key, request.max);
                    shared
                        .lock()
                        .expect("thumbnail cache mutex poisoned")
                        .push(ThumbArt {
                            key,
                            max: request.max,
                            art,
                        });
                    // SAFETY: posting to a window owned by this process.
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(target as *mut _)),
                            WM_APP_THUMB,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            });
        Self {
            tx: if spawned.is_ok() { Some(tx) } else { None },
            results,
        }
    }

    /// Queue one file for decoding at `max` (`MAX_THUMB` for rows and shelf
    /// tiles, `PREVIEW_PX` for the clipboard hover preview).
    pub fn request(&self, path: &str, max: u32) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(ThumbRequest {
                path: path.to_owned(),
                max,
            });
        }
    }

    pub fn drain(&self) -> Vec<ThumbArt> {
        std::mem::take(&mut *self.results.lock().expect("thumbnail cache mutex poisoned"))
    }
}

fn decode(path: &str, key: u64, max: u32) -> Option<Arc<AlbumArt>> {
    let attempt = || -> Option<AlbumArt> {
        // Dimensions first: `into_dimensions` stops at the header, so a
        // decompression bomb is rejected before pixels are allocated.
        let reader = image::ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?;
        let (w, h) = reader.into_dimensions().ok()?;
        if w == 0 || h == 0 || w as u64 * h as u64 > MAX_PIXELS {
            return None;
        }
        let mut reader = image::ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(20_000);
        limits.max_image_height = Some(20_000);
        limits.max_alloc = Some(MAX_PIXELS * 4);
        reader.limits(limits);
        let image = reader.decode().ok()?;
        let (tw, th) = fit(image.width(), image.height(), max);
        let small = image.thumbnail(tw, th).to_rgba8();
        let (tw, th) = small.dimensions();
        let mut bgra = Vec::with_capacity((tw * th * 4) as usize);
        for pixel in small.pixels() {
            bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
        Some(AlbumArt {
            id: art_id(key, max),
            width: tw,
            height: th,
            bgra: Arc::new(bgra),
        })
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(attempt))
        .ok()
        .flatten()
        .map(Arc::new)
}

/// The path's file name, matching the tile tooltip.
pub fn display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_never_upscales_and_keeps_aspect() {
        assert_eq!(fit(64, 32, 128), (64, 32));
        assert_eq!(fit(1000, 500, 128), (128, 64));
        assert_eq!(fit(500, 1000, 128), (64, 128));
        assert_eq!(fit(0, 0, 128), (1, 1));
    }

    #[test]
    fn decodes_a_local_image_to_bgra_within_the_cap() {
        let dir = std::env::temp_dir().join("taskbar-island-thumb-test");
        std::fs::create_dir_all(&dir).unwrap();
        let mut image = image::RgbaImage::new(300, 200);
        for (x, y, px) in image.enumerate_pixels_mut() {
            *px = image::Rgba([x as u8, y as u8, 7, 255]);
        }
        let path = dir.join("sample.png");
        image.save(&path).unwrap();
        let key = key_for(&path.display().to_string());
        let art = decode(&path.display().to_string(), key, MAX_THUMB).expect("png decodes");
        assert_eq!((art.width, art.height), (128, 85));
        assert_eq!(art.bgra.len(), 128 * 85 * 4);
        assert_eq!(art.bgra[3], 255, "alpha channel survives");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The clipboard hover preview decodes the same file at `PREVIEW_PX` and
    /// gets its own bitmap id, so it never reuses (or overwrites) the row
    /// thumbnail in the renderer's cache (spec 7).
    #[test]
    fn preview_decode_is_bigger_and_gets_its_own_art_id() {
        let dir = std::env::temp_dir().join("taskbar-island-preview-test");
        std::fs::create_dir_all(&dir).unwrap();
        let mut image = image::RgbaImage::new(640, 480);
        for (x, y, px) in image.enumerate_pixels_mut() {
            *px = image::Rgba([x as u8, y as u8, 9, 255]);
        }
        let path = dir.join("preview.png");
        image.save(&path).unwrap();
        let text = path.display().to_string();
        let key = key_for(&text);
        let row = decode(&text, key, MAX_THUMB).expect("row thumbnail");
        let preview = decode(&text, key, PREVIEW_PX).expect("preview");
        assert_eq!((row.width, row.height), (128, 96));
        assert_eq!((preview.width, preview.height), (384, 288));
        assert_ne!(row.id, preview.id, "two sizes of one file are two bitmaps");
        assert_eq!(row.id, art_id(key, MAX_THUMB));
        assert_eq!(preview.id, art_id(key, PREVIEW_PX));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn garbage_files_yield_no_thumbnail_without_panicking() {
        let dir = std::env::temp_dir().join("taskbar-island-thumb-bad-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.png");
        std::fs::write(&path, b"not really a png").unwrap();
        assert!(decode(&path.display().to_string(), 1, MAX_THUMB).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
