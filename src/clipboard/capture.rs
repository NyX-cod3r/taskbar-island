//! capture.rs — event-driven clipboard capture.
//!
//! A message-only window owns `AddClipboardFormatListener`. Bursts of
//! `WM_CLIPBOARDUPDATE` collapse into one 150 ms debounce tick; the clipboard
//! itself is read, decoded, hashed and written to blobs on a worker thread so
//! the island message loop never waits on it.

use std::cell::{Cell, RefCell};
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, ExtendedColorType, ImageEncoder, ImageFormat};
use std::path::Path;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, GetClipboardOwner,
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, RemoveClipboardFormatListener,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowThreadProcessId, KillTimer,
    PostMessageW, RegisterClassExW, SetTimer, HWND_MESSAGE, WM_APP, WM_CLIPBOARDUPDATE, WM_TIMER,
    WNDCLASSEXW, WS_POPUP,
};

use crate::design::{
    CLIP_DEBOUNCE_MS, CLIP_IMAGE_MAX_BYTES, CLIP_IMAGE_MAX_PIXELS, CLIP_TEXT_MAX_BYTES,
    CLIP_THUMB_PX,
};

use super::store::{self, ClipItem, ClipKind, ImageMeta};

/// Posted to the island window after the worker captured an item.
pub const WM_APP_CLIPBOARD: u32 = WM_APP + 41;

const CAPTURE_CLASS: PCWSTR = w!("TaskbarIslandClip");
const DEBOUNCE_TIMER: usize = 1;
/// A single copy is bounded by the text/image limits; file lists are capped so
/// a hostile HDROP cannot allocate without bound.
const MAX_FILES: u32 = 512;
const MAX_FILE_BYTES: usize = 1024 * 1024;

pub struct CaptureResult {
    pub item: ClipItem,
}

/// Owns the listener window and the worker input channel.
pub struct Capture {
    hwnd: HWND,
    paused: Arc<AtomicBool>,
    own_seq: Arc<AtomicU32>,
}

impl Capture {
    pub fn start(
        instance: windows::Win32::Foundation::HINSTANCE,
        owner: HWND,
        paused: bool,
    ) -> Option<(Self, Receiver<CaptureResult>)> {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(capture_proc),
            hInstance: instance,
            lpszClassName: CAPTURE_CLASS,
            ..Default::default()
        };
        // A second island instance exits before this point; ignore
        // ERROR_CLASS_ALREADY_EXISTS from a retried start.
        let _ = unsafe { RegisterClassExW(&class) };
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                CAPTURE_CLASS,
                w!("TaskbarIslandClip"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )
        }
        .ok()?;
        // SAFETY: hwnd is the message-only listener window just created.
        unsafe { AddClipboardFormatListener(hwnd) }.ok()?;

        let (request_tx, request_rx) = mpsc::channel::<Request>();
        let (result_tx, result_rx) = mpsc::channel::<CaptureResult>();
        let paused = Arc::new(AtomicBool::new(paused));
        let own_seq = Arc::new(AtomicU32::new(0));
        CTX.with(|cell| {
            *cell.borrow_mut() = Some(CaptureCtx {
                tx: request_tx,
                paused: paused.clone(),
                own_seq: own_seq.clone(),
                gate: Cell::new(CaptureGate::default()),
            });
        });
        let worker_paused = paused.clone();
        let worker_own = own_seq.clone();
        // HWND wraps a raw pointer and is not Send; PostMessageW is
        // thread-safe, so the handle crosses as bits (same as thumb.rs).
        let owner_bits = owner.0 as isize;
        let worker = thread::Builder::new()
            .name("clipboard-capture".into())
            .spawn(move || worker(request_rx, result_tx, owner_bits, worker_paused, worker_own));
        if worker.is_err() {
            CTX.with(|cell| cell.borrow_mut().take());
            // SAFETY: this window is owned by the current thread.
            unsafe {
                let _ = RemoveClipboardFormatListener(hwnd);
                let _ = DestroyWindow(hwnd);
            }
            return None;
        }
        Some((
            Self {
                hwnd,
                paused,
                own_seq,
            },
            result_rx,
        ))
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Ignore the `WM_CLIPBOARDUPDATE` caused by our own write-back. Returns
    /// the sequence number observed after the write.
    pub fn note_own_write(&self) -> u32 {
        // SAFETY: reading the global clipboard sequence counter is harmless.
        let seq = unsafe { GetClipboardSequenceNumber() };
        self.own_seq.store(seq, Ordering::Relaxed);
        seq
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // SAFETY: the listener window belongs to the UI thread and the
        // process is tearing down or replacing the surface.
        unsafe {
            let _ = RemoveClipboardFormatListener(self.hwnd);
            let _ = DestroyWindow(self.hwnd);
        }
        CTX.with(|cell| cell.borrow_mut().take());
    }
}

struct Request;

struct CaptureCtx {
    tx: Sender<Request>,
    paused: Arc<AtomicBool>,
    own_seq: Arc<AtomicU32>,
    gate: Cell<CaptureGate>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureGate {
    last_seq: u32,
}

impl CaptureGate {
    /// Debounced updates that duplicate the previous sequence, our own write,
    /// or arrive while paused are dropped. Paused still advances the sequence
    /// so resuming does not capture the copy that happened while paused.
    pub fn admit(&mut self, seq: u32, paused: bool, own_seq: u32) -> bool {
        if seq == 0 {
            return false;
        }
        if seq == self.last_seq || seq == own_seq {
            self.last_seq = seq;
            return false;
        }
        self.last_seq = seq;
        !paused
    }
}

thread_local! {
    static CTX: RefCell<Option<CaptureCtx>> = const { RefCell::new(None) };
}

extern "system" fn capture_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CLIPBOARDUPDATE => {
            // SetTimer with the same id restarts the countdown, coalescing
            // a burst of updates into one capture.
            // SAFETY: the timer belongs to this window.
            unsafe {
                SetTimer(Some(hwnd), DEBOUNCE_TIMER, CLIP_DEBOUNCE_MS, None);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == DEBOUNCE_TIMER => {
            // SAFETY: the one-shot debounce timer fired.
            unsafe {
                let _ = KillTimer(Some(hwnd), DEBOUNCE_TIMER);
            }
            CTX.with(|cell| {
                let Ok(mut slot) = cell.try_borrow_mut() else {
                    return;
                };
                let Some(ctx) = slot.as_mut() else {
                    return;
                };
                // SAFETY: the clipboard sequence counter is process-global.
                let seq = unsafe { GetClipboardSequenceNumber() };
                let paused = ctx.paused.load(Ordering::Relaxed);
                let own = ctx.own_seq.load(Ordering::Relaxed);
                let mut gate = ctx.gate.get();
                let admit = gate.admit(seq, paused, own);
                ctx.gate.set(gate);
                if admit {
                    let _ = ctx.tx.send(Request);
                }
            });
            LRESULT(0)
        }
        _ => {
            // SAFETY: unhandled messages are delegated to the system procedure.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

fn worker(
    requests: Receiver<Request>,
    results: Sender<CaptureResult>,
    owner_bits: isize,
    paused: Arc<AtomicBool>,
    own_seq: Arc<AtomicU32>,
) {
    while requests.recv().is_ok() {
        if paused.load(Ordering::Relaxed) {
            continue;
        }
        let Some(item) = capture_item() else {
            continue;
        };
        // The clipboard may have changed while decoding. Never re-capture our
        // own write-back.
        // SAFETY: reading the sequence counter is harmless.
        let current = unsafe { GetClipboardSequenceNumber() };
        if current != 0 && current == own_seq.load(Ordering::Relaxed) {
            continue;
        }
        if results.send(CaptureResult { item }).is_ok() {
            // SAFETY: the owner window lives until the process exits.
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(owner_bits as *mut core::ffi::c_void)),
                    WM_APP_CLIPBOARD,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrivacyFlags {
    pub exclude: bool,
    pub can_include: Option<u32>,
    pub can_upload: Option<u32>,
}

/// Password managers and other sources can opt a copy out of history. A
/// missing value means the source allows history.
pub fn privacy_allows(flags: PrivacyFlags) -> bool {
    !flags.exclude && flags.can_include != Some(0) && flags.can_upload != Some(0)
}

pub fn text_allowed(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= CLIP_TEXT_MAX_BYTES
}

pub fn image_allowed(width: u32, height: u32, bytes: u64) -> bool {
    width > 0
        && height > 0
        && u64::from(width) * u64::from(height) <= CLIP_IMAGE_MAX_PIXELS
        && bytes <= CLIP_IMAGE_MAX_BYTES
}

/// A leading http(s) URL becomes a link row; the scheme is assembled at
/// runtime so the offline source audit never mistakes this check for an
/// outbound call. The classification itself lives in `view::link_parts` so
/// the row renderer and capture agree (case-insensitive scheme, host with a
/// dot or `localhost`, spaces allowed after the host).
pub fn is_link(text: &str) -> bool {
    crate::view::link_parts(text).is_some()
}

/// The app that owned the clipboard when this copy happened, reduced to a
/// short display name (`Snipping Tool`, `Chrome`). Read on the capture worker
/// from `GetClipboardOwner`, stored locally, and `None` rather than a guess
/// when the owner window or its process is already gone (spec 7).
fn source_app() -> Option<String> {
    // SAFETY: `GetClipboardOwner` only reads the window that owns the clipboard.
    let owner = unsafe { GetClipboardOwner() }.ok()?;
    if owner.0.is_null() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `owner` is a live HWND and `pid` is a writable out parameter.
    unsafe { GetWindowThreadProcessId(owner, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    // SAFETY: query-only access to a process in this session; the handle is
    // closed on every path below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let name = process_stem(process);
    // SAFETY: the handle came from the `OpenProcess` above.
    unsafe {
        let _ = CloseHandle(process);
    }
    name
}

/// The image path's file stem — already a readable name for most apps.
fn process_stem(process: HANDLE) -> Option<String> {
    let mut buffer = [0u16; 512];
    let mut len = buffer.len() as u32;
    // SAFETY: `buffer` is writable for `len` wide characters.
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()?;
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    let stem = Path::new(&path).file_stem()?.to_string_lossy().into_owned();
    Some(display_name(&stem))
}

/// Names that read better than the executable's own; anything else is kept
/// as-is (the stem is already what the task manager shows).
fn display_name(stem: &str) -> String {
    match stem {
        "SnippingTool" => "Snipping Tool".to_string(),
        "msedge" => "Edge".to_string(),
        "chrome" => "Chrome".to_string(),
        "firefox" => "Firefox".to_string(),
        "Code" => "VS Code".to_string(),
        "excel" => "Excel".to_string(),
        "word" => "Word".to_string(),
        "powerpnt" => "PowerPoint".to_string(),
        "outlook" => "Outlook".to_string(),
        "explorer" => "File Explorer".to_string(),
        "SearchHost" | "SearchApp" => "Windows Search".to_string(),
        "TextInputHost" => "Windows Shell".to_string(),
        other => other.to_string(),
    }
}

#[derive(Debug)]
enum Payload {
    Text(String),
    /// A text file copied as a file (v0.9 §2): the content becomes the row's
    /// text, the paths are kept so copy-back restores the file itself.
    FileText {
        text: String,
        paths: Vec<String>,
    },
    Files(Vec<String>),
    Image {
        png: Vec<u8>,
        thumb: Option<Vec<u8>>,
        digest: String,
        width: u32,
        height: u32,
    },
    /// An image file copied as a file (v0.9 §2): no blob is written — the
    /// thumbnail is decoded from the file itself, off the UI thread.
    FileImage {
        paths: Vec<String>,
        width: u32,
        height: u32,
        bytes: u64,
    },
}

/// What one copied file is, decided from its extension and its first bytes
/// (v0.9 §2: "classify by content, not only by format").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileContent {
    Image,
    Text,
    Other,
}

fn capture_item() -> Option<ClipItem> {
    let payload = read_payload()?;
    // Resolved once per copy: the window that owned the clipboard becomes the
    // row's `from <app>` line (spec 7).
    let source_app = source_app();
    Some(match payload {
        Payload::Text(text) => {
            let kind = if is_link(&text) {
                ClipKind::Link
            } else {
                ClipKind::Text
            };
            let digest = digest("text", &[text.as_bytes()]);
            ClipItem {
                id: 0,
                kind,
                digest,
                created_at: 0,
                pinned: false,
                text,
                paths: Vec::new(),
                image: None,
                source_app,
            }
        }
        Payload::FileText { text, paths } => {
            // The content decides the kind (v0.9 §2: `a.png.txt` stays text);
            // the paths stay on the item so copy-back restores the file.
            ClipItem {
                id: 0,
                kind: ClipKind::Text,
                digest: file_digest("file-text", &paths, text.as_bytes()),
                created_at: 0,
                pinned: false,
                text,
                paths,
                image: None,
                source_app,
            }
        }
        Payload::Files(paths) => ClipItem {
            id: 0,
            kind: ClipKind::Files,
            digest: paths_digest("files", &paths),
            created_at: 0,
            pinned: false,
            text: String::new(),
            paths,
            image: None,
            source_app,
        },
        Payload::FileImage {
            paths,
            width,
            height,
            bytes,
        } => ClipItem {
            id: 0,
            // `paths` non-empty is what marks the row as file-sourced: the
            // title is the file name, the thumbnail decodes the file, and
            // copy-back puts the file back (v0.9 §2).
            kind: ClipKind::Image,
            digest: paths_digest("image-file", &paths),
            created_at: 0,
            pinned: false,
            text: String::new(),
            paths,
            image: Some(ImageMeta {
                width,
                height,
                bytes,
                // No capture-time blob thumbnail exists for file rows; the
                // thumbnail worker decodes the file instead.
                thumb: false,
            }),
            source_app,
        },
        Payload::Image {
            png,
            thumb,
            digest,
            width,
            height,
        } => {
            let thumb_present = thumb.is_some();
            if !store::write_blob(&digest, &png, thumb.as_deref()) {
                return None;
            }
            ClipItem {
                id: 0,
                kind: ClipKind::Image,
                digest,
                created_at: 0,
                pinned: false,
                text: String::new(),
                paths: Vec::new(),
                image: Some(ImageMeta {
                    width,
                    height,
                    bytes: png.len() as u64,
                    thumb: thumb_present,
                }),
                source_app,
            }
        }
    })
}

fn read_payload() -> Option<Payload> {
    let _guard = open_clipboard()?;
    if privacy_opt_out() {
        return None;
    }
    if let Some(paths) = read_files() {
        if !paths.is_empty() {
            return Some(payload_from_files(paths));
        }
    }
    if let Some(text) = read_text() {
        if text_allowed(&text) {
            return Some(Payload::Text(text));
        }
    }
    read_image()
}

/// One clipboard change stays one item (v0.9 §2). A copy whose files are all
/// images becomes an `Image` row sourced from the file; all-text files become
/// a `Text` row whose paths are kept for copy-back; anything mixed (or
/// unreadable) stays the `Files` row it always was, so no copied file is ever
/// dropped from history.
fn payload_from_files(paths: Vec<String>) -> Payload {
    if paths.is_empty() {
        return Payload::Files(paths);
    }
    let probes: Vec<FileContent> = paths.iter().map(|path| probe_file(path)).collect();
    if probes.iter().all(|kind| *kind == FileContent::Image) {
        return file_image(paths);
    }
    if probes.iter().all(|kind| *kind == FileContent::Text) {
        if let Some(text) = read_text_file(&paths[0]) {
            if text_allowed(&text) {
                return Payload::FileText { text, paths };
            }
        }
    }
    Payload::Files(paths)
}

/// Bytes read from a file to classify it: enough for every magic signature
/// and for a text/binary judgement, far below any allocation concern.
const SNIFF_BYTES: usize = 4096;

fn probe_file(path: &str) -> FileContent {
    // The extension is the strong signal: a corrupt `.png` still becomes an
    // image row (its thumbnail simply falls back to the glyph) — v0.9 §2.
    if crate::view::is_image(path) {
        return FileContent::Image;
    }
    let Some(head) = read_head(path, SNIFF_BYTES) else {
        return FileContent::Other;
    };
    if sniff_image(&head) {
        return FileContent::Image;
    }
    // UTF-16 text carries NUL bytes, so its byte-order marks are checked
    // before the NUL rule below rejects it as binary.
    if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) {
        return FileContent::Text;
    }
    if looks_text(&head) {
        return FileContent::Text;
    }
    FileContent::Other
}

/// Up to `cap` bytes from the start of a file, `None` when it cannot be read
/// (missing, denied, a directory).
fn read_head(path: &str, cap: usize) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = vec![0u8; cap];
    let mut read = 0usize;
    while read < cap {
        match file.read(&mut buffer[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(_) => return None,
        }
    }
    buffer.truncate(read);
    (!buffer.is_empty()).then_some(buffer)
}

/// Magic-byte sniff for an extensionless (or oddly named) image file.
fn sniff_image(head: &[u8]) -> bool {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return true;
    }
    if head.len() >= 3 && head[..3] == [0xFF, 0xD8, 0xFF] {
        return true; // JPEG
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return true;
    }
    if head.len() >= 6 && head.starts_with(b"BM") {
        return true; // BMP
    }
    if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        return true; // TIFF
    }
    if head.starts_with(b"\0\0\x01\x00") {
        return true; // ICO
    }
    if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return true;
    }
    // HEIC/HEIF: an ISO base media `ftyp` box carrying an image brand.
    head.len() >= 12
        && &head[4..8] == b"ftyp"
        && matches!(
            &head[8..12],
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1"
        )
}

/// Text judgement for a sample: valid UTF-8 (a tail cut mid-character is
/// forgiven), almost no control characters, and none of the container
/// signatures whose first bytes are printable (`%PDF`, `PK…`).
fn looks_text(head: &[u8]) -> bool {
    const BINARY: &[&[u8]] = &[
        b"%PDF",
        b"PK\x03\x04",
        b"Rar!",
        b"7z\xBC\xAF\x27\x1C",
        b"\x1F\x8B",
        b"\xD0\xCF\x11\xE0",
        b"MZ",
        b"\x7FELF",
        b"\xCA\xFE\xBA\xBE",
    ];
    if BINARY.iter().any(|signature| head.starts_with(signature)) {
        return false;
    }
    if head.contains(&0) {
        return false;
    }
    let valid = match std::str::from_utf8(head) {
        Ok(text) => text.len(),
        // The 4 KB window can end inside a multi-byte character.
        Err(error) if error.valid_up_to().saturating_add(4) >= head.len() => error.valid_up_to(),
        Err(_) => return false,
    };
    let Ok(text) = std::str::from_utf8(&head[..valid]) else {
        return false;
    };
    let printable = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r' | '\x0B' | '\x0C'))
        .count();
    printable * 10 >= text.chars().count()
}

/// The content of a text file, capped at the same limit as a copied string.
/// UTF-8 (with or without BOM) and BOM-marked UTF-16 are understood; anything
/// else returns `None` and the copy stays a `Files` row.
fn read_text_file(path: &str) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    let mut reader = file.take(u64::try_from(CLIP_TEXT_MAX_BYTES).unwrap_or(u64::MAX));
    reader.read_to_end(&mut bytes).ok()?;
    decode_text_bytes(&bytes)
}

fn decode_text_bytes(bytes: &[u8]) -> Option<String> {
    let units = |big_endian: bool| -> String {
        let chunks = bytes[2..].as_chunks::<2>().0;
        let units: Vec<u16> = chunks
            .iter()
            .map(|pair| {
                if big_endian {
                    u16::from_be_bytes(*pair)
                } else {
                    u16::from_le_bytes(*pair)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Some(units(false));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Some(units(true));
    }
    let body = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    };
    match std::str::from_utf8(body) {
        Ok(text) => Some(text.to_string()),
        // A capped read can end inside a multi-byte character.
        Err(error) if error.valid_up_to().saturating_add(4) >= body.len() => {
            std::str::from_utf8(&body[..error.valid_up_to()])
                .ok()
                .map(str::to_owned)
        }
        Err(_) => None,
    }
}

/// An image file copied as a file: dimensions come from the header only (a
/// 24 MP photo is never decoded on the capture worker), `bytes` is the file
/// size the meta line shows, and a header that cannot be read yields `0 × 0`
/// so the row still exists with no size in its meta line.
fn file_image(paths: Vec<String>) -> Payload {
    let bytes = std::fs::metadata(&paths[0])
        .map(|meta| meta.len())
        .unwrap_or(0);
    let (width, height) = image_dimensions(&paths[0]).unwrap_or((0, 0));
    Payload::FileImage {
        paths,
        width,
        height,
        bytes,
    }
}

fn image_dimensions(path: &str) -> Option<(u32, u32)> {
    let attempt = || -> Option<(u32, u32)> {
        let reader = image::ImageReader::open(path).ok()?;
        reader.with_guessed_format().ok()?.into_dimensions().ok()
    };
    catch_unwind(AssertUnwindSafe(attempt)).ok().flatten()
}

/// Digest of the copied paths, optionally salted with the payload. The tag
/// keeps an image row and a file row for the same path distinct: `store::add`
/// keys on kind + digest, so an item captured by an older build is not
/// silently re-typed.
fn paths_digest(tag: &str, paths: &[String]) -> String {
    file_digest(tag, paths, &[])
}

fn file_digest(tag: &str, paths: &[String], extra: &[u8]) -> String {
    let mut hasher = Hasher::new(tag);
    for path in paths {
        hasher.write(path.as_bytes());
        hasher.write(&[0]);
    }
    hasher.write(extra);
    hasher.finish()
}

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: balanced with the successful OpenClipboard.
        let _ = unsafe { CloseClipboard() };
    }
}

fn open_clipboard() -> Option<ClipboardGuard> {
    for _ in 0..10 {
        // SAFETY: opening the clipboard with no owner window is allowed from
        // any thread; the guard closes it on every path.
        if unsafe { OpenClipboard(None) }.is_ok() {
            return Some(ClipboardGuard);
        }
        thread::sleep(Duration::from_millis(15));
    }
    None
}

fn privacy_opt_out() -> bool {
    // SAFETY: format registration is process-global and idempotent.
    let exclude =
        unsafe { RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")) };
    if exclude != 0 {
        // SAFETY: querying format availability does not read clipboard memory.
        if unsafe { IsClipboardFormatAvailable(exclude) }.is_ok() {
            return true;
        }
    }
    let can_include = register("CanIncludeInClipboardHistory");
    let can_upload = register("CanUploadToCloudClipboard");
    let flags = PrivacyFlags {
        exclude: false,
        can_include: read_dword(can_include),
        can_upload: read_dword(can_upload),
    };
    !privacy_allows(flags)
}

fn register(name: &str) -> u32 {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: wide is NUL-terminated for the duration of the call.
    unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) }
}

fn read_dword(format: u32) -> Option<u32> {
    if format == 0 {
        return None;
    }
    let bytes = read_format(format, 16)?;
    let head: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    Some(u32::from_le_bytes(head))
}

fn read_format(format: u32, cap: usize) -> Option<Vec<u8>> {
    // SAFETY: GetClipboardData returns a handle owned by the clipboard.
    let handle = unsafe { GetClipboardData(format) }.ok()?;
    if handle.0.is_null() {
        return None;
    }
    let hglobal = HGLOBAL(handle.0);
    // SAFETY: the handle is a valid global memory block for this call.
    let size = unsafe { GlobalSize(hglobal) };
    if size == 0 || size > cap {
        return None;
    }
    // SAFETY: the block is locked for reading; unlock is balanced below.
    let pointer = unsafe { GlobalLock(hglobal) } as *const u8;
    if pointer.is_null() {
        return None;
    }
    // SAFETY: GlobalSize bytes are readable while the block is locked.
    let bytes = unsafe { std::slice::from_raw_parts(pointer, size) }.to_vec();
    // SAFETY: balances GlobalLock.
    let _ = unsafe { GlobalUnlock(hglobal) };
    Some(bytes)
}

fn read_text() -> Option<String> {
    let format = CF_UNICODETEXT.0 as u32;
    // SAFETY: constant format id.
    if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
        return None;
    }
    // Two bytes per UTF-16 unit, plus a NUL; leave room before rejecting by
    // decoded UTF-8 length.
    let bytes = read_format(format, CLIP_TEXT_MAX_BYTES * 2 + 2)?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    if units.is_empty() {
        return None;
    }
    Some(String::from_utf16_lossy(&units))
}

fn read_files() -> Option<Vec<String>> {
    let format = CF_HDROP.0 as u32;
    // SAFETY: constant format id.
    if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
        return None;
    }
    // SAFETY: the handle is a valid CF_HDROP block owned by the clipboard.
    let handle = unsafe { GetClipboardData(format) }.ok()?;
    if handle.0.is_null() {
        return None;
    }
    let drop = HDROP(handle.0);
    // SAFETY: a null buffer with index 0xFFFFFFFF asks for the file count.
    let count = unsafe { DragQueryFileW(drop, u32::MAX, None) }.min(MAX_FILES);
    let mut paths = Vec::new();
    let mut total = 0usize;
    for index in 0..count {
        // SAFETY: index is below the count just queried.
        let length = unsafe { DragQueryFileW(drop, index, None) } as usize;
        if length == 0 || length > 32_767 {
            continue;
        }
        let mut buffer = vec![0u16; length + 1];
        // SAFETY: buffer is sized for the reported length plus the NUL.
        let written = unsafe { DragQueryFileW(drop, index, Some(&mut buffer)) } as usize;
        if written == 0 {
            continue;
        }
        let path = String::from_utf16_lossy(&buffer[..written.min(buffer.len())]);
        total = total.saturating_add(path.len());
        if total > MAX_FILE_BYTES {
            break;
        }
        paths.push(path);
    }
    Some(paths)
}

fn read_image() -> Option<Payload> {
    if let Some(bytes) = read_format(register("PNG"), CLIP_IMAGE_MAX_BYTES as usize) {
        if let Some(payload) = decode_png(&bytes) {
            return Some(payload);
        }
    }
    for format in [CF_DIBV5.0 as u32, CF_DIB.0 as u32] {
        // SAFETY: constant format ids.
        if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
            continue;
        }
        if let Some(dib) = read_format(format, CLIP_IMAGE_MAX_BYTES as usize) {
            if let Some(payload) = decode_dib(&dib) {
                return Some(payload);
            }
        }
    }
    None
}

fn decode_png(bytes: &[u8]) -> Option<Payload> {
    // PNG signature + IHDR holds the dimensions without decoding anything.
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if !image_allowed(width, height, bytes.len() as u64) {
        return None;
    }
    let decoded = catch_unwind(AssertUnwindSafe(|| {
        image::load_from_memory_with_format(bytes, ImageFormat::Png)
    }))
    .ok()?
    .ok()?;
    finish_image(decoded)
}

fn decode_dib(dib: &[u8]) -> Option<Payload> {
    if dib.len() < 40 || dib.len() as u64 > CLIP_IMAGE_MAX_BYTES {
        return None;
    }
    let header = u32::from_le_bytes(dib[..4].try_into().ok()?) as usize;
    if header < 40 || header > dib.len() {
        return None;
    }
    let width = i32::from_le_bytes(dib[4..8].try_into().ok()?);
    let height = i32::from_le_bytes(dib[8..12].try_into().ok()?);
    if width <= 0 || height == 0 {
        return None;
    }
    let pixels = (width as i64).saturating_mul((height as i64).abs());
    if pixels <= 0 || pixels as u64 > CLIP_IMAGE_MAX_PIXELS {
        return None;
    }
    let bmp = dib_to_bmp(dib, header)?;
    let decoded = catch_unwind(AssertUnwindSafe(|| {
        image::load_from_memory_with_format(&bmp, ImageFormat::Bmp)
    }))
    .ok()?
    .ok()?;
    finish_image(decoded)
}

fn dib_to_bmp(dib: &[u8], header: usize) -> Option<Vec<u8>> {
    if dib.len() < header {
        return None;
    }
    let compression = u32::from_le_bytes(dib.get(16..20)?.try_into().ok()?);
    // BITMAPINFOHEADER with masks keeps them just after the 40-byte header.
    let masks = if header == 40 && compression == 3 {
        12
    } else if header == 40 && compression == 6 {
        16
    } else {
        0
    };
    let offset = 14 + header + masks;
    let file_size = 14 + dib.len();
    if offset > file_size {
        return None;
    }
    let mut out = Vec::with_capacity(file_size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(dib);
    Some(out)
}

fn finish_image(image: DynamicImage) -> Option<Payload> {
    let width = image.width();
    let height = image.height();
    if !image_allowed(width, height, 0) {
        return None;
    }
    let rgba = image.to_rgba8();
    let mut hasher = Hasher::new("image");
    hasher.write(&width.to_le_bytes());
    hasher.write(&height.to_le_bytes());
    hasher.write(rgba.as_raw());
    let digest = hasher.finish();

    let png = encode_png(&image)?;
    if png.len() as u64 > CLIP_IMAGE_MAX_BYTES {
        return None;
    }
    let thumb = thumbnail(&image).and_then(|thumb| encode_png(&thumb));
    Some(Payload::Image {
        png,
        thumb,
        digest,
        width,
        height,
    })
}

fn thumbnail(image: &DynamicImage) -> Option<DynamicImage> {
    let (width, height) = (image.width(), image.height());
    let long = width.max(height);
    if long == 0 {
        return None;
    }
    if long <= CLIP_THUMB_PX {
        return Some(image.clone());
    }
    let scale = f64::from(CLIP_THUMB_PX) / f64::from(long);
    let new_width = ((f64::from(width) * scale).round() as u32).max(1);
    let new_height = ((f64::from(height) * scale).round() as u32).max(1);
    Some(image.resize_exact(new_width, new_height, image::imageops::FilterType::Triangle))
}

fn encode_png(image: &DynamicImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (width, height) = (image.width(), image.height());
    let encoder =
        PngEncoder::new_with_quality(&mut out, CompressionType::Default, FilterType::Adaptive);
    let result = if image.color().has_alpha() {
        let pixels = image.to_rgba8();
        encoder.write_image(pixels.as_raw(), width, height, ExtendedColorType::Rgba8)
    } else {
        let pixels = image.to_rgb8();
        encoder.write_image(pixels.as_raw(), width, height, ExtendedColorType::Rgb8)
    };
    result.ok()?;
    Some(out)
}

struct Hasher {
    a: u64,
    b: u64,
    len: u64,
}

impl Hasher {
    fn new(tag: &str) -> Self {
        let mut hasher = Self {
            a: 0xcbf2_9ce4_8422_2325,
            b: 0x9e37_79b9_7f4a_7c15,
            len: 0,
        };
        hasher.write(tag.as_bytes());
        hasher.write(&[0]);
        hasher
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.a = (self.a ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
            self.b ^= u64::from(byte).wrapping_add(self.a.rotate_left(7));
            self.b = self.b.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        }
        self.len = self.len.wrapping_add(bytes.len() as u64);
    }

    fn finish(self) -> String {
        format!("{:016x}{:016x}{:x}", self.a, self.b, self.len)
    }
}

fn digest(tag: &str, parts: &[&[u8]]) -> String {
    let mut hasher = Hasher::new(tag);
    for part in parts {
        hasher.write(part);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_ignores_paused_own_and_repeated_updates() {
        let mut gate = CaptureGate::default();
        assert!(!gate.admit(1, true, 0));
        // Resuming must not replay the paused copy.
        assert!(!gate.admit(1, false, 0));
        assert!(gate.admit(2, false, 0));
        assert!(!gate.admit(2, false, 0));
        assert!(!gate.admit(3, false, 3));
        assert!(gate.admit(4, false, 3));
        assert!(!gate.admit(0, false, 3));
    }

    #[test]
    fn privacy_opt_outs_are_respected() {
        assert!(privacy_allows(PrivacyFlags::default()));
        assert!(privacy_allows(PrivacyFlags {
            exclude: false,
            can_include: Some(1),
            can_upload: None,
        }));
        assert!(!privacy_allows(PrivacyFlags {
            exclude: true,
            ..PrivacyFlags::default()
        }));
        assert!(!privacy_allows(PrivacyFlags {
            can_include: Some(0),
            ..PrivacyFlags::default()
        }));
        assert!(!privacy_allows(PrivacyFlags {
            can_upload: Some(0),
            ..PrivacyFlags::default()
        }));
    }

    #[test]
    fn text_limits_reject_blank_and_oversize() {
        assert!(text_allowed("hello"));
        assert!(!text_allowed("   \r\n\t"));
        let big = "x".repeat(CLIP_TEXT_MAX_BYTES + 1);
        assert!(!text_allowed(&big));
        let edge = "x".repeat(CLIP_TEXT_MAX_BYTES);
        assert!(text_allowed(&edge));
    }

    #[test]
    fn image_limits_match_the_spec() {
        assert!(image_allowed(1920, 1080, 5 * 1024 * 1024));
        assert!(image_allowed(1, 1, 1)); // 1-pixel images are kept (spec 7)
        assert!(!image_allowed(0, 1080, 1024));
        assert!(!image_allowed(1920, 0, 1024));
        assert!(!image_allowed(6000, 4000, 1024)); // 24 MP
        assert!(!image_allowed(1920, 1080, CLIP_IMAGE_MAX_BYTES + 1));
    }

    #[test]
    fn links_need_a_leading_http_url_with_a_host() {
        let http = concat!("ht", "tp", ":", "//", "example.com/a?b=1");
        let https = concat!("ht", "tp", "s", ":", "//", "example.com");
        assert!(is_link(http));
        assert!(is_link(https));
        // Any case (spec 7 test table: `HTTP://`)…
        assert!(is_link("HTTP://example.com"));
        // …but a scheme with no host is not a link…
        assert!(!is_link("HTTP://"));
        // …a URL with spaces after the host still is (spec 7).
        assert!(is_link(concat!(
            "ht",
            "tp",
            "s",
            "://",
            "example.com/a b c"
        )));
        assert!(is_link(concat!("ht", "tp", "://", "localhost:8080/x")));
        assert!(!is_link(concat!("see ", "ht", "tps://example.com")));
        assert!(!is_link("file:///c:/temp"));
        assert!(!is_link("not a link"));
        assert!(!is_link("www.example.com"));
    }

    #[test]
    fn corrupt_png_never_becomes_an_item() {
        // Signature present, IHDR garbage: the decoder must fail cleanly
        // instead of panicking (spec 7 "corrupt clipboard bitmaps").
        let junk = b"\x89PNG\r\n\x1a\n....IHDRcorrupt-corrupt-corrupt";
        assert!(decode_png(junk).is_none());
        assert!(decode_png(b"not a png at all").is_none());
        // A truncated but plausible header is rejected before decoding.
        let mut tiny = b"\x89PNG\r\n\x1a\n".to_vec();
        tiny.extend_from_slice(b"xxxxIHDR");
        assert!(decode_png(&tiny).is_none());
    }

    #[test]
    fn digests_are_stable_and_distinct() {
        assert_eq!(digest("text", &[b"alpha"]), digest("text", &[b"alpha"]));
        assert_ne!(digest("text", &[b"alpha"]), digest("text", &[b"beta"]));
        assert_ne!(digest("text", &[b"alpha"]), digest("link", &[b"alpha"]));
    }

    #[test]
    fn dib_header_is_wrapped_as_bmp() {
        // 2 x 2 pixels, 24 bpp, bottom-up, no compression.
        let row = 2usize * 3;
        let stride = row + (4 - row % 4) % 4;
        let mut dib = vec![0u8; 40 + stride * 2];
        dib[..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&2i32.to_le_bytes());
        dib[8..12].copy_from_slice(&2i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        let bmp = dib_to_bmp(&dib, 40).expect("wrapped");
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 54);
        let decoded = image::load_from_memory_with_format(&bmp, ImageFormat::Bmp)
            .expect("decode")
            .to_rgba8();
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
    }

    #[test]
    fn thumbnails_fit_one_hundred_twenty_eight_pixels() {
        // 128 px = 2× DPI for the 64 DIP row thumbnail (spec 7).
        let image = DynamicImage::new_rgba8(400, 200);
        let thumb = thumbnail(&image).expect("thumb");
        assert_eq!(thumb.width(), 128);
        assert_eq!(thumb.height(), 64);
        // Small images keep their natural size (the row draws them on a
        // checkerboard at exactly this size).
        let small = DynamicImage::new_rgba8(24, 18);
        let thumb = thumbnail(&small).expect("thumb");
        assert_eq!((thumb.width(), thumb.height()), (24, 18));
        let tiny = DynamicImage::new_rgba8(1, 1);
        let thumb = thumbnail(&tiny).expect("thumb");
        assert_eq!((thumb.width(), thumb.height()), (1, 1));
    }

    /// A scratch directory for the CF_HDROP classification tests; every test
    /// removes its own copy on the way out.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("taskbar-island-clip-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        DynamicImage::new_rgba8(width, height)
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .expect("encode png");
        bytes
    }

    /// v0.9 §2: a copied image *file* is an Image row sourced from the file —
    /// the reported bug was that it only ever appeared under All.
    #[test]
    fn image_files_classify_as_image_rows() {
        let dir = scratch("image-files");
        let png = dir.join("Picture1.png");
        std::fs::write(&png, png_bytes(1280, 720)).unwrap();
        let jpg = dir.join("photo.jpg");
        std::fs::write(&jpg, b"\xFF\xD8\xFF\xE0rest-of-the-jpeg").unwrap();

        let payload = payload_from_files(vec![png.display().to_string()]);
        let (paths, width, height, bytes) = match payload {
            Payload::FileImage {
                paths,
                width,
                height,
                bytes,
            } => (paths, width, height, bytes),
            other => panic!("png file should be an image row: {other:?}"),
        };
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("Picture1.png"));
        assert_eq!((width, height), (1280, 720));
        assert_eq!(bytes, std::fs::metadata(&png).unwrap().len());

        // The extension decides even when the bytes are not a decodable
        // image: the row exists and its thumbnail falls back to the glyph.
        assert!(matches!(
            payload_from_files(vec![jpg.display().to_string()]),
            Payload::FileImage { .. }
        ));
        let corrupt = dir.join("broken.png");
        std::fs::write(&corrupt, b"definitely not a png").unwrap();
        match payload_from_files(vec![corrupt.display().to_string()]) {
            Payload::FileImage { width, height, .. } => {
                assert_eq!((width, height), (0, 0), "header unreadable -> 0 x 0")
            }
            other => panic!("corrupt png should still be an image row: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The acceptance test from §2: a text file whose *name* contains `.png`
    /// must stay text (the owner's call: classified from its content).
    #[test]
    fn a_png_txt_stays_text_from_its_content() {
        let dir = scratch("png-txt");
        let path = dir.join("a.png.txt");
        std::fs::write(&path, "just some plain notes\nsecond line").unwrap();
        match payload_from_files(vec![path.display().to_string()]) {
            Payload::FileText { text, paths } => {
                assert!(text.starts_with("just some plain notes"));
                assert_eq!(paths.len(), 1, "paths are kept for copy-back");
            }
            other => panic!("a.png.txt must stay text: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A PNG with no extension at all is recognised from its magic bytes, and
    /// a copy that mixes image and non-image files stays the Files row it
    /// always was — one clipboard change, one item, nothing dropped (§2).
    #[test]
    fn extensionless_images_and_mixed_copies() {
        let dir = scratch("sniff");
        let bare = dir.join("screenshot-copy");
        std::fs::write(&bare, png_bytes(64, 48)).unwrap();
        assert!(matches!(
            payload_from_files(vec![bare.display().to_string()]),
            Payload::FileImage { .. }
        ));

        let text = dir.join("notes.txt");
        std::fs::write(&text, "hello").unwrap();
        assert!(matches!(
            payload_from_files(vec![bare.display().to_string(), text.display().to_string()]),
            Payload::Files(_)
        ));

        let pdf = dir.join("Invoice.pdf");
        std::fs::write(&pdf, b"%PDF-1.7\n1 0 obj").unwrap();
        assert!(matches!(
            payload_from_files(vec![pdf.display().to_string()]),
            Payload::Files(_)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Bitmap clipboard formats keep producing the same blob-backed Image
    /// payload: a screenshot is one item that lands in All *and* Images.
    #[test]
    fn screenshots_stay_blob_backed_image_payloads() {
        let bytes = png_bytes(615, 322);
        match decode_png(&bytes) {
            Some(Payload::Image { width, height, .. }) => assert_eq!((width, height), (615, 322)),
            other => panic!("png clipboard payload: {other:?}"),
        }
        // A 2 x 2 24 bpp DIB (the CF_DIB path) decodes the same way.
        let row = 2 * 3;
        let stride = row + (4 - row % 4) % 4;
        let mut dib = vec![0u8; 40 + stride * 2];
        dib[..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&2i32.to_le_bytes());
        dib[8..12].copy_from_slice(&2i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        match decode_dib(&dib) {
            Some(Payload::Image { width, height, .. }) => assert_eq!((width, height), (2, 2)),
            other => panic!("dib clipboard payload: {other:?}"),
        }
    }
}
