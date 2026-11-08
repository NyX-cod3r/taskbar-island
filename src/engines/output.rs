//! output.rs — where an engine's bytes land.
//!
//! Engines write into a private workspace, never beside the user's file. This
//! module then copies the bytes to a temp name in the destination folder and
//! moves that with a no-replace flag, so an output is either the complete file
//! or nothing. A collision becomes `name (2).ext`; an existing file is never
//! overwritten, and the original is never touched.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

use crate::convert::detect::SourceFormat;

use super::{
    Action, AudioFormat, DocumentFormat, EngineError, ImageFormat, ImageOptimize, OcrOutput,
    VideoExtract, VideoFormat,
};

/// The container a job will produce, independent of which engine runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFormat {
    Image(ImageFormat),
    Video(VideoFormat),
    Audio(AudioFormat),
    Document(DocumentFormat),
}

impl TargetFormat {
    pub fn extension(self) -> &'static str {
        match self {
            TargetFormat::Image(format) => match format {
                ImageFormat::Png => "png",
                ImageFormat::Jpeg => "jpg",
                ImageFormat::WebP => "webp",
                ImageFormat::Gif => "gif",
                ImageFormat::Bmp => "bmp",
                ImageFormat::Tiff => "tif",
                ImageFormat::Ico => "ico",
                ImageFormat::Avif => "avif",
                ImageFormat::Pdf => "pdf",
            },
            TargetFormat::Video(format) => match format {
                VideoFormat::Mp4 => "mp4",
                VideoFormat::Mkv => "mkv",
                VideoFormat::WebM => "webm",
                VideoFormat::Mov => "mov",
                VideoFormat::Avi => "avi",
                VideoFormat::Wmv => "wmv",
                VideoFormat::Gif => "gif",
            },
            TargetFormat::Audio(format) => match format {
                AudioFormat::Mp3 => "mp3",
                AudioFormat::Wav => "wav",
                AudioFormat::Flac => "flac",
                AudioFormat::M4a => "m4a",
                AudioFormat::Ogg => "ogg",
                AudioFormat::Opus => "opus",
                AudioFormat::Aac => "aac",
                AudioFormat::Wma => "wma",
                AudioFormat::Aiff => "aiff",
            },
            TargetFormat::Document(format) => match format {
                DocumentFormat::Docx => "docx",
                DocumentFormat::Odt => "odt",
                DocumentFormat::Markdown => "md",
                DocumentFormat::Html => "html",
                DocumentFormat::Txt => "txt",
                DocumentFormat::Epub => "epub",
                DocumentFormat::Rtf => "rtf",
                DocumentFormat::Tex => "tex",
            },
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TargetFormat::Image(format) => match format {
                ImageFormat::Png => "PNG",
                ImageFormat::Jpeg => "JPG",
                ImageFormat::WebP => "WebP",
                ImageFormat::Gif => "GIF",
                ImageFormat::Bmp => "BMP",
                ImageFormat::Tiff => "TIFF",
                ImageFormat::Ico => "ICO",
                ImageFormat::Avif => "AVIF",
                ImageFormat::Pdf => "PDF",
            },
            TargetFormat::Video(format) => match format {
                VideoFormat::Mp4 => "MP4",
                VideoFormat::Mkv => "MKV",
                VideoFormat::WebM => "WebM",
                VideoFormat::Mov => "MOV",
                VideoFormat::Avi => "AVI",
                VideoFormat::Wmv => "WMV",
                VideoFormat::Gif => "GIF",
            },
            TargetFormat::Audio(format) => match format {
                AudioFormat::Mp3 => "MP3",
                AudioFormat::Wav => "WAV",
                AudioFormat::Flac => "FLAC",
                AudioFormat::M4a => "M4A",
                AudioFormat::Ogg => "OGG",
                AudioFormat::Opus => "OPUS",
                AudioFormat::Aac => "AAC",
                AudioFormat::Wma => "WMA",
                AudioFormat::Aiff => "AIFF",
            },
            TargetFormat::Document(format) => match format {
                DocumentFormat::Docx => "DOCX",
                DocumentFormat::Odt => "ODT",
                DocumentFormat::Markdown => "MD",
                DocumentFormat::Html => "HTML",
                DocumentFormat::Txt => "TXT",
                DocumentFormat::Epub => "EPUB",
                DocumentFormat::Rtf => "RTF",
                DocumentFormat::Tex => "TEX",
            },
        }
    }
}

/// The output a job produces for this source. `None` means the job writes no
/// single file (OCR to clipboard, extract-embedded-images).
pub fn target_for(action: Action, source: SourceFormat) -> Option<TargetFormat> {
    match action {
        Action::ImageConvert(format) => Some(TargetFormat::Image(format)),
        Action::ImageOptimize(ImageOptimize::WebReady) => {
            Some(TargetFormat::Image(ImageFormat::WebP))
        }
        Action::ImageOptimize(_) | Action::ImageEdit(_) => {
            source.image_format().map(TargetFormat::Image)
        }
        Action::ImageOcr(output) => match output {
            OcrOutput::Clipboard => None,
            OcrOutput::Txt => Some(TargetFormat::Document(DocumentFormat::Txt)),
            OcrOutput::Markdown => Some(TargetFormat::Document(DocumentFormat::Markdown)),
            OcrOutput::Docx => Some(TargetFormat::Document(DocumentFormat::Docx)),
            OcrOutput::SearchablePdf => Some(TargetFormat::Image(ImageFormat::Pdf)),
        },
        Action::VideoConvert(format) => Some(TargetFormat::Video(format)),
        Action::VideoOptimize(_) | Action::VideoEdit(_) => {
            source.video_format().map(TargetFormat::Video)
        }
        Action::VideoExtract(extract) => match extract {
            VideoExtract::Audio(format) => Some(TargetFormat::Audio(format)),
            VideoExtract::Frame => Some(TargetFormat::Image(ImageFormat::Png)),
            VideoExtract::Gif => Some(TargetFormat::Video(VideoFormat::Gif)),
        },
        Action::AudioConvert(format) => Some(TargetFormat::Audio(format)),
        Action::AudioOptimize(_) => source.audio_format().map(TargetFormat::Audio),
        Action::DocumentConvert(format) => Some(TargetFormat::Document(format)),
        Action::DocumentExtract(crate::engines::DocumentExtract::PlainText) => {
            Some(TargetFormat::Document(DocumentFormat::Txt))
        }
        Action::DocumentExtract(_) => None,
    }
}

/// `_small` for optimize, `_edited` for edit.
pub fn suffix(action: Action) -> Option<&'static str> {
    match action {
        Action::ImageOptimize(_) | Action::VideoOptimize(_) | Action::AudioOptimize(_) => {
            Some("_small")
        }
        Action::ImageEdit(_) | Action::VideoEdit(_) => Some("_edited"),
        _ => None,
    }
}

/// Display name of the file the job will create, before collision handling.
pub fn preview_name(input: &Path, action: Action, target: TargetFormat) -> String {
    let stem = input
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "converted".into());
    let stem = match suffix(action) {
        Some(suffix) => format!("{stem}{suffix}"),
        None => stem,
    };
    format!("{stem}.{}", target.extension())
}

// ------------------------------------------------------------- workspace

static WORKSPACE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// A private working directory for one job. Dropping it removes everything
/// the engine left behind, including a partial output after cancellation.
pub struct Workspace {
    dir: PathBuf,
}

impl Workspace {
    pub fn new() -> Result<Self, EngineError> {
        let dir = std::env::temp_dir().join(format!(
            "taskbar-island-engine-{}-{}",
            std::process::id(),
            WORKSPACE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir)
            .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

// --------------------------------------------------------------- delivery

static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Copies an engine output beside the original and renames it into place
/// without replacing anything. Returns the final path and whether the
/// Documents fallback folder had to be used.
pub fn deliver(
    engine_output: &Path,
    input: &Path,
    action: Action,
    target: TargetFormat,
) -> Result<(PathBuf, bool), String> {
    deliver_in(None, engine_output, input, action, target)
}

/// Same as `deliver`, but honours a destination folder chosen by the caller
/// (the job's output path). The folder itself must already exist; a missing or
/// read-only one falls back to the user's Documents folder.
pub fn deliver_in(
    destination: Option<&Path>,
    engine_output: &Path,
    input: &Path,
    action: Action,
    target: TargetFormat,
) -> Result<(PathBuf, bool), String> {
    if !engine_output.is_file() {
        return Err("The engine produced no output.".into());
    }
    let mut fallback = false;
    let preferred = destination
        .filter(|dir| dir.is_dir())
        .or_else(|| input.parent())
        .unwrap_or_else(|| Path::new("."));
    let fallback_dir;
    let dir = match writable_dir(preferred) {
        Some(dir) => dir,
        None => {
            fallback = true;
            fallback_dir = documents_fallback()?;
            fallback_dir.as_path()
        }
    };
    let stem = input
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "converted".into());
    let stem = match suffix(action) {
        Some(suffix) => format!("{stem}{suffix}"),
        None => stem,
    };
    let extension = target.extension();
    let temp = dir.join(format!(
        ".taskbar-island-{}-{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    copy_file(engine_output, &temp)?;
    for index in 0..1000u32 {
        let name = if index == 0 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem} ({}).{extension}", index + 1)
        };
        let destination = dir.join(&name);
        match move_new(&temp, &destination) {
            Ok(()) => return Ok((destination, fallback)),
            Err(MoveError::Exists) => continue,
            Err(MoveError::Failed(reason)) => {
                let _ = fs::remove_file(&temp);
                return Err(reason);
            }
        }
    }
    let _ = fs::remove_file(&temp);
    Err("There are too many files with this name.".into())
}

/// Moves an engine-produced folder (extracted images) beside the original.
/// The folder is first copied inside the destination directory's own volume,
/// then renamed into an unused `name_images` directory, so the user never sees
/// a half-copied folder.
pub fn deliver_folder_in(
    destination: Option<&Path>,
    source: &Path,
    input: &Path,
) -> Result<(PathBuf, bool), String> {
    if !source.is_dir() {
        return Err("The engine produced no output.".into());
    }
    let mut fallback = false;
    let preferred = destination
        .filter(|dir| dir.is_dir())
        .or_else(|| input.parent())
        .unwrap_or_else(|| Path::new("."));
    let fallback_dir;
    let dir = match writable_dir(preferred) {
        Some(dir) => dir,
        None => {
            fallback = true;
            fallback_dir = documents_fallback()?;
            fallback_dir.as_path()
        }
    };
    let stem = input
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".into());
    let temp = dir.join(format!(
        ".taskbar-island-{}-{}.tmpdir",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(reason) = copy_dir(source, &temp) {
        let _ = fs::remove_dir_all(&temp);
        return Err(reason);
    }
    for index in 0..1000u32 {
        let name = if index == 0 {
            format!("{stem}_images")
        } else {
            format!("{stem}_images ({})", index + 1)
        };
        let destination = dir.join(&name);
        match move_new(&temp, &destination) {
            Ok(()) => return Ok((destination, fallback)),
            Err(MoveError::Exists) => continue,
            Err(MoveError::Failed(reason)) => {
                let _ = fs::remove_dir_all(&temp);
                return Err(reason);
            }
        }
    }
    let _ = fs::remove_dir_all(&temp);
    Err("There are too many folders with this name.".into())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|error| format!("The output couldn't be written: {error}."))?;
    let entries = fs::read_dir(from)
        .map_err(|error| format!("The engine output couldn't be read: {error}."))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let target = to.join(&name);
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            copy_file(&path, &target)?;
        }
    }
    Ok(())
}

fn writable_dir(dir: &Path) -> Option<&Path> {
    let candidate = dir.join(format!(
        ".taskbar-island-{}-{}.probe",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&candidate)
    {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(&candidate);
            Some(dir)
        }
        Err(_) => None,
    }
}

fn documents_fallback() -> Result<PathBuf, String> {
    let documents = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|home| home.join("Documents"))
        .ok_or_else(|| "The output folder isn't writable.".to_string())?;
    let dir = documents.join("TaskbarIsland Output");
    fs::create_dir_all(&dir).map_err(|_| "The output folder isn't writable.".to_string())?;
    Ok(dir)
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    let mut source = fs::File::open(from)
        .map_err(|error| format!("The engine output couldn't be read: {error}."))?;
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(|error| format!("The file couldn't be written: {error}."))?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|error| format!("The engine output couldn't be read: {error}."))?;
        if read == 0 {
            break;
        }
        destination
            .write_all(&buffer[..read])
            .map_err(|error| format!("The file couldn't be written: {error}."))?;
    }
    destination
        .sync_all()
        .map_err(|error| format!("The file couldn't be written: {error}."))
}

enum MoveError {
    Exists,
    Failed(String),
}

fn move_new(from: &Path, destination: &Path) -> Result<(), MoveError> {
    let from_wide = HSTRING::from(from.as_os_str());
    let to_wide = HSTRING::from(destination.as_os_str());
    // SAFETY: both strings are NUL-terminated and live for the call.
    let result = unsafe { MoveFileExW(&from_wide, &to_wide, MOVEFILE_WRITE_THROUGH) };
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            let code = error.code().0 as u32 & 0xFFFF;
            // ERROR_FILE_EXISTS (80) / ERROR_ALREADY_EXISTS (183)
            if code == 80 || code == 183 {
                Err(MoveError::Exists)
            } else {
                Err(MoveError::Failed(format!(
                    "The file couldn't be moved into place: {error}."
                )))
            }
        }
    }
}

/// Rejects an engine that claims success but wrote something else.
pub fn verify(path: &Path, target: TargetFormat) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|_| "The engine produced no output.".to_string())?;
    if metadata.len() == 0 {
        return Err("The engine produced an empty file.".into());
    }
    let mut head = [0u8; 16];
    let mut file =
        fs::File::open(path).map_err(|_| "The engine output couldn't be read.".to_string())?;
    let read = file.read(&mut head).unwrap_or(0);
    let head = &head[..read];
    if engine_magic_ok(target, head) {
        Ok(())
    } else {
        Err("The engine produced an unreadable file.".into())
    }
}

fn engine_magic_ok(target: TargetFormat, head: &[u8]) -> bool {
    let ftyp =
        |brand: &[u8]| head.len() >= 12 && &head[4..8] == b"ftyp" && head[8..].starts_with(brand);
    let riff = |kind: &[u8]| head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == kind;
    let asf = head.starts_with(&[
        0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE,
        0x6C,
    ]);
    match target {
        TargetFormat::Image(format) => match format {
            ImageFormat::Jpeg => head.starts_with(&[0xFF, 0xD8, 0xFF]),
            ImageFormat::Png => head.starts_with(&[0x89, b'P', b'N', b'G']),
            ImageFormat::WebP => riff(b"WEBP"),
            ImageFormat::Gif => head.starts_with(b"GIF8"),
            ImageFormat::Bmp => head.starts_with(b"BM"),
            ImageFormat::Tiff => {
                head.starts_with(&[0x49, 0x49, 0x2A, 0x00])
                    || head.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
            }
            ImageFormat::Ico => head.starts_with(&[0x00, 0x00, 0x01, 0x00]),
            ImageFormat::Avif => ftyp(b"avif") || ftyp(b"avis"),
            ImageFormat::Pdf => head.starts_with(b"%PDF-"),
        },
        TargetFormat::Video(format) => match format {
            VideoFormat::Mp4 | VideoFormat::Mov => head.len() >= 8 && &head[4..8] == b"ftyp",
            VideoFormat::Mkv | VideoFormat::WebM => head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]),
            VideoFormat::Avi => riff(b"AVI "),
            VideoFormat::Wmv => asf,
            VideoFormat::Gif => head.starts_with(b"GIF8"),
        },
        TargetFormat::Audio(format) => match format {
            AudioFormat::Mp3 => head.starts_with(b"ID3") || head.starts_with(&[0xFF, 0xFB]),
            AudioFormat::Wav => riff(b"WAVE"),
            AudioFormat::Flac => head.starts_with(b"fLaC"),
            AudioFormat::M4a => head.len() >= 8 && &head[4..8] == b"ftyp",
            AudioFormat::Ogg | AudioFormat::Opus => head.starts_with(b"OggS"),
            AudioFormat::Aac => head.starts_with(&[0xFF, 0xF1]) || head.starts_with(&[0xFF, 0xF9]),
            AudioFormat::Wma => asf,
            AudioFormat::Aiff => {
                head.len() >= 12 && &head[0..4] == b"FORM" && &head[8..12] == b"AIFF"
            }
        },
        TargetFormat::Document(format) => match format {
            DocumentFormat::Docx | DocumentFormat::Odt | DocumentFormat::Epub => {
                head.starts_with(b"PK\x03\x04")
            }
            DocumentFormat::Rtf => head.starts_with(b"{\\rtf"),
            DocumentFormat::Markdown
            | DocumentFormat::Html
            | DocumentFormat::Txt
            | DocumentFormat::Tex => {
                // Text formats have no magic; any non-empty bytes that are not
                // a binary container are acceptable.
                !head.is_empty()
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::VideoOptimize;

    #[test]
    fn targets_follow_the_source_for_keep_format_actions() {
        assert_eq!(
            target_for(
                Action::ImageOptimize(ImageOptimize::Compress),
                SourceFormat::Jpeg
            ),
            Some(TargetFormat::Image(ImageFormat::Jpeg))
        );
        assert_eq!(
            target_for(
                Action::VideoEdit(super::super::VideoEdit::RotateLeft),
                SourceFormat::Mkv
            ),
            Some(TargetFormat::Video(VideoFormat::Mkv))
        );
        assert_eq!(
            target_for(
                Action::AudioOptimize(super::super::AudioOptimize::Mono),
                SourceFormat::Flac
            ),
            Some(TargetFormat::Audio(AudioFormat::Flac))
        );
        assert_eq!(
            target_for(Action::ImageOcr(OcrOutput::Clipboard), SourceFormat::Png),
            None
        );
    }

    #[test]
    fn preview_names_use_the_action_suffix() {
        let input = Path::new(r"C:\pics\clip.mov");
        assert_eq!(
            preview_name(
                input,
                Action::VideoOptimize(VideoOptimize::Compress),
                TargetFormat::Video(VideoFormat::Mp4)
            ),
            "clip_small.mp4"
        );
        assert_eq!(
            preview_name(
                input,
                Action::DocumentConvert(DocumentFormat::Markdown),
                TargetFormat::Document(DocumentFormat::Markdown)
            ),
            "clip.md"
        );
    }

    #[test]
    fn delivery_never_replaces_an_existing_file() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "taskbar-island-deliver-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("v.mp4");
        fs::write(&input, b"original").unwrap();
        fs::write(dir.join("v.mkv"), b"keep me").unwrap();
        let workspace = Workspace::new().unwrap();
        let output = workspace.join("out.mkv");
        fs::write(&output, b"new bytes").unwrap();
        let (delivered, fallback) = deliver(
            &output,
            &input,
            Action::VideoConvert(VideoFormat::Mkv),
            TargetFormat::Video(VideoFormat::Mkv),
        )
        .unwrap();
        assert!(!fallback);
        assert_eq!(delivered.file_name().unwrap(), "v (2).mkv");
        assert_eq!(fs::read(dir.join("v.mkv")).unwrap(), b"keep me");
        let leftovers: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");
    }

    #[test]
    fn a_missing_or_wrong_engine_output_is_rejected() {
        let workspace = Workspace::new().unwrap();
        let missing = workspace.join("none.png");
        assert!(verify(&missing, TargetFormat::Image(ImageFormat::Png)).is_err());
        let wrong = workspace.join("wrong.png");
        fs::write(&wrong, b"not a png").unwrap();
        assert!(verify(&wrong, TargetFormat::Image(ImageFormat::Png)).is_err());
        let right = workspace.join("right.png");
        fs::write(&right, &[0x89, b'P', b'N', b'G', 0, 0, 0]).unwrap();
        assert!(verify(&right, TargetFormat::Image(ImageFormat::Png)).is_ok());
    }
}
