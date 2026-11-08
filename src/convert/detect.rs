//! detect.rs — magic-byte classification and hostile-input screening.
//!
//! Extensions are never trusted. A file is classified from its first bytes;
//! only then may the image decoder look at it, and only inside hard limits.

use std::fs::{File, Metadata};
use std::io::{BufReader, Read};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use image::{ImageDecoder, ImageFormat as ImageCrateFormat, ImageReader, Limits};

use super::{MAX_DECODE_BYTES, MAX_FILE_BYTES, MAX_INPUTS, MAX_PIXELS};
use crate::design::{self, Color};
use crate::view::IconId;

use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{
    GetFileAttributesW, FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
    FILE_ATTRIBUTE_RECALL_ON_OPEN, INVALID_FILE_ATTRIBUTES,
};

/// Containers the converter understands as input. The exact container is kept
/// so the Convert card can omit the file's own format and keep-format jobs can
/// write it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Jpeg,
    Png,
    WebP,
    Gif,
    Bmp,
    Ico,
    Tiff,
    Avif,
    Svg,
    Pdf,
    Mp4,
    Mkv,
    WebM,
    Mov,
    Avi,
    Wmv,
    M4v,
    Mp3,
    Wav,
    Flac,
    M4a,
    Ogg,
    Opus,
    Aac,
    Wma,
    Aiff,
    Docx,
    Odt,
    Markdown,
    Html,
    Txt,
    Epub,
    Rtf,
    Tex,
    Unknown,
}

impl SourceFormat {
    pub fn label(self) -> &'static str {
        match self {
            SourceFormat::Jpeg => "JPG",
            SourceFormat::Png => "PNG",
            SourceFormat::WebP => "WebP",
            SourceFormat::Gif => "GIF",
            SourceFormat::Bmp => "BMP",
            SourceFormat::Ico => "ICO",
            SourceFormat::Tiff => "TIFF",
            SourceFormat::Avif => "AVIF",
            SourceFormat::Svg => "SVG",
            SourceFormat::Pdf => "PDF",
            SourceFormat::Mp4 => "MP4",
            SourceFormat::Mkv => "MKV",
            SourceFormat::WebM => "WebM",
            SourceFormat::Mov => "MOV",
            SourceFormat::Avi => "AVI",
            SourceFormat::Wmv => "WMV",
            SourceFormat::M4v => "M4V",
            SourceFormat::Mp3 => "MP3",
            SourceFormat::Wav => "WAV",
            SourceFormat::Flac => "FLAC",
            SourceFormat::M4a => "M4A",
            SourceFormat::Ogg => "OGG",
            SourceFormat::Opus => "OPUS",
            SourceFormat::Aac => "AAC",
            SourceFormat::Wma => "WMA",
            SourceFormat::Aiff => "AIFF",
            SourceFormat::Docx => "DOCX",
            SourceFormat::Odt => "ODT",
            SourceFormat::Markdown => "MD",
            SourceFormat::Html => "HTML",
            SourceFormat::Txt => "TXT",
            SourceFormat::Epub => "EPUB",
            SourceFormat::Rtf => "RTF",
            SourceFormat::Tex => "TEX",
            SourceFormat::Unknown => "FILE",
        }
    }

    pub fn badge_color(self) -> Color {
        match self {
            SourceFormat::Pdf => design::FILE_PDF,
            SourceFormat::Mp4
            | SourceFormat::Mkv
            | SourceFormat::WebM
            | SourceFormat::Mov
            | SourceFormat::Avi
            | SourceFormat::Wmv
            | SourceFormat::M4v => design::FILE_VIDEO,
            SourceFormat::Mp3
            | SourceFormat::Wav
            | SourceFormat::Flac
            | SourceFormat::M4a
            | SourceFormat::Ogg
            | SourceFormat::Opus
            | SourceFormat::Aac
            | SourceFormat::Wma
            | SourceFormat::Aiff => design::FILE_AUDIO,
            SourceFormat::Docx
            | SourceFormat::Odt
            | SourceFormat::Markdown
            | SourceFormat::Html
            | SourceFormat::Txt
            | SourceFormat::Epub
            | SourceFormat::Rtf
            | SourceFormat::Tex => design::FILE_DOC,
            SourceFormat::Jpeg
            | SourceFormat::Png
            | SourceFormat::WebP
            | SourceFormat::Gif
            | SourceFormat::Bmp
            | SourceFormat::Ico
            | SourceFormat::Tiff
            | SourceFormat::Avif => design::FILE_IMAGE,
            SourceFormat::Svg => design::FILE_IMAGE,
            SourceFormat::Unknown => design::BADGE_OTHER,
        }
    }

    pub fn icon(self) -> IconId {
        match self {
            SourceFormat::Mp4
            | SourceFormat::Mkv
            | SourceFormat::WebM
            | SourceFormat::Mov
            | SourceFormat::Avi
            | SourceFormat::Wmv
            | SourceFormat::M4v => IconId::Video,
            SourceFormat::Mp3
            | SourceFormat::Wav
            | SourceFormat::Flac
            | SourceFormat::M4a
            | SourceFormat::Ogg
            | SourceFormat::Opus
            | SourceFormat::Aac
            | SourceFormat::Wma
            | SourceFormat::Aiff => IconId::Music,
            SourceFormat::Docx
            | SourceFormat::Odt
            | SourceFormat::Markdown
            | SourceFormat::Html
            | SourceFormat::Txt
            | SourceFormat::Epub
            | SourceFormat::Rtf
            | SourceFormat::Tex
            | SourceFormat::Pdf => IconId::File,
            SourceFormat::Unknown => IconId::File,
            _ => IconId::Image,
        }
    }

    pub(crate) fn image_crate(self) -> Option<ImageCrateFormat> {
        match self {
            SourceFormat::Jpeg => Some(ImageCrateFormat::Jpeg),
            SourceFormat::Png => Some(ImageCrateFormat::Png),
            SourceFormat::WebP => Some(ImageCrateFormat::WebP),
            SourceFormat::Gif => Some(ImageCrateFormat::Gif),
            SourceFormat::Bmp => Some(ImageCrateFormat::Bmp),
            SourceFormat::Ico => Some(ImageCrateFormat::Ico),
            _ => None,
        }
    }

    pub fn image_format(self) -> Option<crate::engines::ImageFormat> {
        use crate::engines::ImageFormat;
        match self {
            SourceFormat::Jpeg => Some(ImageFormat::Jpeg),
            SourceFormat::Png => Some(ImageFormat::Png),
            SourceFormat::WebP => Some(ImageFormat::WebP),
            SourceFormat::Gif => Some(ImageFormat::Gif),
            SourceFormat::Bmp => Some(ImageFormat::Bmp),
            SourceFormat::Ico => Some(ImageFormat::Ico),
            SourceFormat::Tiff => Some(ImageFormat::Tiff),
            SourceFormat::Avif => Some(ImageFormat::Avif),
            _ => None,
        }
    }

    pub fn video_format(self) -> Option<crate::engines::VideoFormat> {
        use crate::engines::VideoFormat;
        match self {
            SourceFormat::Mp4 | SourceFormat::M4v => Some(VideoFormat::Mp4),
            SourceFormat::Mkv => Some(VideoFormat::Mkv),
            SourceFormat::WebM => Some(VideoFormat::WebM),
            SourceFormat::Mov => Some(VideoFormat::Mov),
            SourceFormat::Avi => Some(VideoFormat::Avi),
            SourceFormat::Wmv => Some(VideoFormat::Wmv),
            SourceFormat::Gif => Some(VideoFormat::Gif),
            _ => None,
        }
    }

    pub fn audio_format(self) -> Option<crate::engines::AudioFormat> {
        use crate::engines::AudioFormat;
        match self {
            SourceFormat::Mp3 => Some(AudioFormat::Mp3),
            SourceFormat::Wav => Some(AudioFormat::Wav),
            SourceFormat::Flac => Some(AudioFormat::Flac),
            SourceFormat::M4a => Some(AudioFormat::M4a),
            SourceFormat::Ogg => Some(AudioFormat::Ogg),
            SourceFormat::Opus => Some(AudioFormat::Opus),
            SourceFormat::Aac => Some(AudioFormat::Aac),
            SourceFormat::Wma => Some(AudioFormat::Wma),
            SourceFormat::Aiff => Some(AudioFormat::Aiff),
            _ => None,
        }
    }

    pub fn document_format(self) -> Option<crate::engines::DocumentFormat> {
        use crate::engines::DocumentFormat;
        match self {
            SourceFormat::Docx => Some(DocumentFormat::Docx),
            SourceFormat::Odt => Some(DocumentFormat::Odt),
            SourceFormat::Markdown => Some(DocumentFormat::Markdown),
            SourceFormat::Html => Some(DocumentFormat::Html),
            SourceFormat::Txt => Some(DocumentFormat::Txt),
            SourceFormat::Epub => Some(DocumentFormat::Epub),
            SourceFormat::Rtf => Some(DocumentFormat::Rtf),
            SourceFormat::Tex => Some(DocumentFormat::Tex),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub format: SourceFormat,
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
    pub has_exif: bool,
}

#[derive(Debug, Clone)]
pub enum ProbeKind {
    Image(ImageInfo),
    Pdf,
    Video(SourceFormat),
    Audio(SourceFormat),
    Document(SourceFormat),
    Unsupported { reason: String },
    CloudPending,
    TooLarge { reason: String },
    Missing,
    Folder,
}

impl ProbeKind {
    pub fn convertible(&self) -> bool {
        matches!(
            self,
            ProbeKind::Image(_)
                | ProbeKind::Video(_)
                | ProbeKind::Audio(_)
                | ProbeKind::Document(_)
        )
    }

    /// Short reason shown in the dedicated notice state. `None` for input the
    /// converter can take.
    pub fn notice(&self) -> Option<&str> {
        match self {
            ProbeKind::Image(_)
            | ProbeKind::Video(_)
            | ProbeKind::Audio(_)
            | ProbeKind::Document(_) => None,
            ProbeKind::Pdf => Some("PDF pages can't be converted yet."),
            ProbeKind::Unsupported { reason } => Some(reason.as_str()),
            ProbeKind::CloudPending => Some("This file isn't downloaded on this PC yet."),
            ProbeKind::TooLarge { reason } => Some(reason.as_str()),
            ProbeKind::Missing => Some("This file is no longer where it was."),
            ProbeKind::Folder => Some("Folders can't be converted. Drop a file instead."),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Probe {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub kind: ProbeKind,
}

impl Probe {
    pub fn format(&self) -> SourceFormat {
        match &self.kind {
            ProbeKind::Image(info) => info.format,
            ProbeKind::Pdf => SourceFormat::Pdf,
            ProbeKind::Video(format) | ProbeKind::Audio(format) | ProbeKind::Document(format) => {
                *format
            }
            _ => SourceFormat::Unknown,
        }
    }

    pub fn badge(&self) -> &'static str {
        self.format().label()
    }

    pub fn badge_color(&self) -> Color {
        self.format().badge_color()
    }

    pub fn icon(&self) -> IconId {
        self.format().icon()
    }

    /// "2.4 MB · 4032 × 3024" for the Convert header.
    pub fn meta_line(&self) -> String {
        match &self.kind {
            ProbeKind::Image(info) => format!(
                "{} \u{00b7} {} \u{00d7} {}",
                crate::view::fmt_size(self.bytes),
                info.width,
                info.height
            ),
            _ => crate::view::fmt_size(self.bytes),
        }
    }
}

pub fn probe_all(paths: &[String]) -> Vec<Probe> {
    let mut out: Vec<Probe> = Vec::new();
    for path in paths {
        if out.len() >= MAX_INPUTS {
            break;
        }
        if out
            .iter()
            .any(|probe| probe.path.eq_ignore_ascii_case(path))
        {
            continue;
        }
        out.push(probe_path(path));
    }
    out
}

pub fn probe_path(path: &str) -> Probe {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned());
    // A decoder must never take the process down on a hostile file. The
    // release profile unwinds so this guard is meaningful there too.
    let result = catch_unwind(AssertUnwindSafe(|| probe_inner(path)));
    match result {
        Ok((bytes, kind)) => Probe {
            path: path.to_owned(),
            name,
            bytes,
            kind,
        },
        Err(_) => Probe {
            path: path.to_owned(),
            name,
            bytes: 0,
            kind: ProbeKind::Unsupported {
                reason: "This file couldn't be read safely.".into(),
            },
        },
    }
}

fn probe_inner(path: &str) -> (u64, ProbeKind) {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(_) => return (0, ProbeKind::Missing),
    };
    if meta.is_dir() {
        return (0, ProbeKind::Folder);
    }
    if is_cloud_placeholder(path) {
        return (meta.len(), ProbeKind::CloudPending);
    }
    if meta.len() > MAX_FILE_BYTES {
        return (
            meta.len(),
            ProbeKind::TooLarge {
                reason: "This file is larger than the 1 GB conversion limit.".into(),
            },
        );
    }
    let format = match sniff_file(path) {
        Ok(format) => format,
        Err(_) => {
            return (
                meta.len(),
                ProbeKind::Unsupported {
                    reason: "This file couldn't be read.".into(),
                },
            )
        }
    };
    let kind = match format {
        SourceFormat::Pdf => ProbeKind::Pdf,
        SourceFormat::Svg => ProbeKind::Unsupported {
            reason: "SVG isn't supported yet. Try PNG or PDF instead.".into(),
        },
        // These are raster images the bundled decoder cannot inspect; FFmpeg
        // handles them. Dimensions are left at zero rather than decoding a
        // hostile image on the UI thread.
        SourceFormat::Tiff | SourceFormat::Avif => ProbeKind::Image(ImageInfo {
            format,
            width: 0,
            height: 0,
            has_alpha: false,
            has_exif: false,
        }),
        SourceFormat::Unknown => ProbeKind::Unsupported {
            reason: "This type can't be converted yet.".into(),
        },
        SourceFormat::Jpeg
        | SourceFormat::Png
        | SourceFormat::WebP
        | SourceFormat::Gif
        | SourceFormat::Bmp
        | SourceFormat::Ico => match probe_image(path, format) {
            Ok(info) => ProbeKind::Image(info),
            Err(ImageProbeError::TooLarge(reason)) => ProbeKind::TooLarge { reason },
            Err(ImageProbeError::Unreadable) => ProbeKind::Unsupported {
                reason: "This image couldn't be read. It may be damaged.".into(),
            },
        },
        other => other.kind(),
    };
    (meta.len(), kind)
}

impl SourceFormat {
    fn kind(self) -> ProbeKind {
        match self {
            SourceFormat::Mp4
            | SourceFormat::Mkv
            | SourceFormat::WebM
            | SourceFormat::Mov
            | SourceFormat::Avi
            | SourceFormat::Wmv
            | SourceFormat::M4v => ProbeKind::Video(self),
            SourceFormat::Mp3
            | SourceFormat::Wav
            | SourceFormat::Flac
            | SourceFormat::M4a
            | SourceFormat::Ogg
            | SourceFormat::Opus
            | SourceFormat::Aac
            | SourceFormat::Wma
            | SourceFormat::Aiff => ProbeKind::Audio(self),
            SourceFormat::Docx
            | SourceFormat::Odt
            | SourceFormat::Markdown
            | SourceFormat::Html
            | SourceFormat::Txt
            | SourceFormat::Epub
            | SourceFormat::Rtf
            | SourceFormat::Tex => ProbeKind::Document(self),
            _ => ProbeKind::Unsupported {
                reason: "This type can't be converted yet.".into(),
            },
        }
    }
}

/// Coarse class for routing a single file without decoding it. Used by the
/// conversion worker right before it picks an engine.
pub fn file_kind(path: &str) -> crate::engines::FileKind {
    let Some(format) = quick_format(path) else {
        return crate::engines::FileKind::Other;
    };
    // Images are probed for dimensions elsewhere, so `kind()` reports them as
    // unsupported; the coarse class still has to say Image for routing.
    if format.image_format().is_some() {
        return crate::engines::FileKind::Image;
    }
    match format.kind() {
        ProbeKind::Video(_) => crate::engines::FileKind::Video,
        ProbeKind::Audio(_) => crate::engines::FileKind::Audio,
        ProbeKind::Document(_) => crate::engines::FileKind::Document,
        _ => crate::engines::FileKind::Other,
    }
}

pub fn quick_format(path: &str) -> Option<SourceFormat> {
    sniff_file(path)
        .ok()
        .filter(|format| *format != SourceFormat::Unknown)
}

enum ImageProbeError {
    TooLarge(String),
    Unreadable,
}

fn probe_image(path: &str, format: SourceFormat) -> Result<ImageInfo, ImageProbeError> {
    let Some(crate_format) = format.image_crate() else {
        return Err(ImageProbeError::Unreadable);
    };
    let file = File::open(path).map_err(|_| ImageProbeError::Unreadable)?;
    let mut reader = ImageReader::with_format(BufReader::new(file), crate_format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(40_000);
    limits.max_image_height = Some(40_000);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| ImageProbeError::Unreadable)?;
    let (width, height) = decoder.dimensions();
    if width < 1 || height < 1 {
        return Err(ImageProbeError::Unreadable);
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(ImageProbeError::TooLarge(
            "This image is larger than 150 megapixels.".into(),
        ));
    }
    if decoder.total_bytes() > MAX_DECODE_BYTES {
        return Err(ImageProbeError::TooLarge(
            "This image would need too much memory to decode.".into(),
        ));
    }
    let has_alpha = decoder.color_type().has_alpha();
    let has_exif = decoder.exif_metadata().ok().flatten().is_some();
    Ok(ImageInfo {
        format,
        width,
        height,
        has_alpha,
        has_exif,
    })
}

/// A container is decided from at most this many leading bytes. ZIP-based
/// documents keep their entry names close enough to the start for this to be
/// enough without unpacking anything.
const SNIFF_BYTES: usize = 64 * 1024;

fn sniff_file(path: &str) -> std::io::Result<SourceFormat> {
    let file = File::open(path)?;
    let mut head = Vec::with_capacity(SNIFF_BYTES);
    file.take(SNIFF_BYTES as u64).read_to_end(&mut head)?;
    Ok(sniff_named(&head, path))
}

fn extension_hint(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Magic-byte classification. Public so tests can pin every container without
/// touching the filesystem.
pub fn sniff(bytes: &[u8]) -> SourceFormat {
    sniff_bytes(bytes, "")
}

/// Same classification, with the file name as a tie-breaker for containers
/// whose bytes are genuinely ambiguous (WMA/WMV share ASF, M4A/M4V share the
/// ISO base media header, AAC and MP3 share a frame sync).
pub fn sniff_named(bytes: &[u8], path: &str) -> SourceFormat {
    sniff_bytes(bytes, &extension_hint(path))
}

fn sniff_bytes(bytes: &[u8], ext: &str) -> SourceFormat {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return SourceFormat::Jpeg;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return SourceFormat::Png;
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" {
        return match &bytes[8..12] {
            b"WEBP" => SourceFormat::WebP,
            b"WAVE" => SourceFormat::Wav,
            b"AVI " => SourceFormat::Avi,
            _ => SourceFormat::Unknown,
        };
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return SourceFormat::Gif;
    }
    if bytes.starts_with(b"BM") {
        return SourceFormat::Bmp;
    }
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return SourceFormat::Ico;
    }
    if bytes.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || bytes.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
    {
        return SourceFormat::Tiff;
    }
    if bytes.starts_with(b"%PDF-") {
        return SourceFormat::Pdf;
    }
    if bytes.starts_with(b"fLaC") {
        return SourceFormat::Flac;
    }
    if bytes.starts_with(b"OggS") {
        let probe = &bytes[..bytes.len().min(128)];
        return if contains(probe, b"OpusHead") {
            SourceFormat::Opus
        } else {
            SourceFormat::Ogg
        };
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"FORM" {
        return match &bytes[8..12] {
            b"AIFF" | b"AIFC" => SourceFormat::Aiff,
            _ => SourceFormat::Unknown,
        };
    }
    // ISO base media (`ftyp`): the major brand names the family.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        return match brand {
            b"avif" | b"avis" => SourceFormat::Avif,
            b"qt  " => SourceFormat::Mov,
            b"M4A " | b"M4B " => SourceFormat::M4a,
            b"M4V " | b"M4VH" | b"M4VP" => SourceFormat::M4v,
            b"mp41" | b"mp42" | b"isom" | b"iso2" | b"avc1" | b"dash" => match ext {
                "m4a" => SourceFormat::M4a,
                "m4v" => SourceFormat::M4v,
                "mov" => SourceFormat::Mov,
                _ => SourceFormat::Mp4,
            },
            _ => SourceFormat::Mp4,
        };
    }
    // EBML: check the doctype, which sits in the first few hundred bytes.
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        let probe = &bytes[..bytes.len().min(512)];
        return if contains(probe, b"webm") {
            SourceFormat::WebM
        } else {
            SourceFormat::Mkv
        };
    }
    // ASF (Advanced Systems Format) GUID. Audio and video share it, so only
    // the extension can tell WMA from WMV.
    const ASF_GUID: [u8; 16] = [
        0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE,
        0x6C,
    ];
    if bytes.starts_with(&ASF_GUID) {
        return match ext {
            "wma" => SourceFormat::Wma,
            _ => SourceFormat::Wmv,
        };
    }
    if bytes.starts_with(b"ID3") {
        return SourceFormat::Mp3;
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0 {
        // ADTS (AAC) uses 0xF1/0xF9; MPEG audio layers use 0xFA..0xFD.
        return match bytes[1] {
            0xF1 | 0xF9 => SourceFormat::Aac,
            _ if ext == "aac" => SourceFormat::Aac,
            _ => SourceFormat::Mp3,
        };
    }
    // ZIP-based documents: local headers carry their entry names near the
    // start. We never open the archive, we only read what is already here.
    if bytes.starts_with(b"PK\x03\x04") {
        let probe = &bytes[..bytes.len().min(SNIFF_BYTES)];
        if contains(probe, b"application/epub+zip") {
            return SourceFormat::Epub;
        }
        if contains(probe, b"word/") {
            return SourceFormat::Docx;
        }
        if contains(probe, b"content.xml") {
            return SourceFormat::Odt;
        }
        return SourceFormat::Unknown;
    }
    let head = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if head.starts_with(b"{\\rtf") {
        return SourceFormat::Rtf;
    }
    let trimmed = head
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map(|i| &head[i..])
        .unwrap_or_default();
    if starts_with_ci(trimmed, b"<svg") || starts_with_ci(trimmed, b"<?xml") {
        return SourceFormat::Svg;
    }
    if starts_with_ci(trimmed, b"<!doctype html")
        || starts_with_ci(trimmed, b"<html")
        || (ext == "html" || ext == "htm") && looks_like_text(trimmed)
    {
        return SourceFormat::Html;
    }
    if looks_like_text(trimmed) {
        return match ext {
            "md" | "markdown" => SourceFormat::Markdown,
            "tex" | "latex" => SourceFormat::Tex,
            "html" | "htm" => SourceFormat::Html,
            "txt" => SourceFormat::Txt,
            _ => SourceFormat::Unknown,
        };
    }
    SourceFormat::Unknown
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn starts_with_ci(haystack: &[u8], prefix: &[u8]) -> bool {
    haystack.len() >= prefix.len()
        && haystack[..prefix.len()]
            .iter()
            .zip(prefix)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// Printable UTF-8 with no NULs. A truncated multi-byte sequence at the sniff
/// boundary is allowed, so a 64 KiB cut never turns a text file into junk.
fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.contains(&0) {
        return false;
    }
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(error) => {
            error.error_len().is_none()
                && std::str::from_utf8(&bytes[..error.valid_up_to()]).is_ok()
        }
    }
}

pub(crate) fn is_cloud_placeholder(path: &str) -> bool {
    let wide = HSTRING::from(path);
    // SAFETY: the path is a NUL-terminated HSTRING owned by this call.
    let attributes = unsafe { GetFileAttributesW(&wide) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return false;
    }
    let mask = FILE_ATTRIBUTE_OFFLINE.0
        | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0
        | FILE_ATTRIBUTE_RECALL_ON_OPEN.0;
    attributes & mask != 0
}

#[allow(dead_code)]
fn _meta_marker(_: Metadata) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_beats_extensions() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), SourceFormat::Jpeg);
        assert_eq!(
            sniff(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            SourceFormat::Png
        );
        assert_eq!(sniff(b"GIF89a-rest"), SourceFormat::Gif);
        assert_eq!(sniff(b"BM123456"), SourceFormat::Bmp);
        assert_eq!(sniff(b"%PDF-1.7"), SourceFormat::Pdf);
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WEBPVP8 "), SourceFormat::WebP);
        assert_eq!(sniff(b"\x00\x00\x01\x00\x01\x00"), SourceFormat::Ico);
        assert_eq!(sniff(b"II*\x00rest"), SourceFormat::Tiff);
        assert_eq!(sniff(b"\x00\x00\x00\x18ftypmp42"), SourceFormat::Mp4);
        assert_eq!(sniff(b"  <svg xmlns="), SourceFormat::Svg);
        assert_eq!(sniff(b"hello world"), SourceFormat::Unknown);
    }

    #[test]
    fn every_phase_four_container_is_recognised() {
        assert_eq!(sniff(b"\x00\x00\x00\x18ftypavifrest"), SourceFormat::Avif);
        assert_eq!(sniff(b"\x00\x00\x00\x18ftypM4A rest"), SourceFormat::M4a);
        assert_eq!(sniff(b"\x00\x00\x00\x18ftypM4V rest"), SourceFormat::M4v);
        assert_eq!(sniff(b"\x00\x00\x00\x18ftypqt  rest"), SourceFormat::Mov);
        assert_eq!(
            sniff_named(b"\x00\x00\x00\x18ftypmp42rest", "clip.mov"),
            SourceFormat::Mov
        );
        assert_eq!(
            sniff_named(b"\x00\x00\x00\x18ftypmp42rest", "song.m4a"),
            SourceFormat::M4a
        );
        assert_eq!(
            sniff(b"\x1A\x45\xDF\xA3\x01\x00\x00\x00matroska"),
            SourceFormat::Mkv
        );
        assert_eq!(
            sniff(b"\x1A\x45\xDF\xA3\x01\x00\x00\x00webm"),
            SourceFormat::WebM
        );
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WAVEfmt "), SourceFormat::Wav);
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00AVI LIST"), SourceFormat::Avi);
        assert_eq!(sniff(b"fLaC\x00\x00\x00\x22"), SourceFormat::Flac);
        assert_eq!(sniff(b"OggS\x00\x02\x00\x00OpusHead"), SourceFormat::Opus);
        assert_eq!(sniff(b"OggS\x00\x02\x00\x00vorbis"), SourceFormat::Ogg);
        assert_eq!(sniff(b"FORM\x00\x00\x00\x00AIFFCOMM"), SourceFormat::Aiff);
        assert_eq!(sniff(b"ID3\x04\x00\x00\x00\x00\x00\x00"), SourceFormat::Mp3);
        assert_eq!(sniff(&[0xFF, 0xFB, 0x90, 0x64]), SourceFormat::Mp3);
        assert_eq!(sniff(&[0xFF, 0xF1, 0x50, 0x80]), SourceFormat::Aac);
        assert_eq!(sniff_named(b"{\\rtf1\\ansi", "note.rtf"), SourceFormat::Rtf);
        assert_eq!(sniff(b"<!DOCTYPE html><html>"), SourceFormat::Html);
        assert_eq!(
            sniff_named(b"# Title\n\nBody", "readme.md"),
            SourceFormat::Markdown
        );
        assert_eq!(
            sniff_named(b"\\documentclass{article}", "paper.tex"),
            SourceFormat::Tex
        );
        assert_eq!(sniff_named(b"plain words", "notes.txt"), SourceFormat::Txt);
        assert_eq!(
            sniff_named(b"PK\x03\x04\x14\x00\x00\x00word/document.xml", "a.docx"),
            SourceFormat::Docx
        );
        assert_eq!(
            sniff_named(b"PK\x03\x04\x14\x00\x00\x00content.xml", "a.odt"),
            SourceFormat::Odt
        );
        assert_eq!(
            sniff_named(b"PK\x03\x04\x14\x00\x00\x00application/epub+zip", "a.epub"),
            SourceFormat::Epub
        );
    }

    #[test]
    fn asf_uses_the_name_to_split_audio_from_video() {
        let mut head = vec![
            0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62,
            0xCE, 0x6C,
        ];
        head.extend_from_slice(b"rest");
        assert_eq!(sniff_named(&head, "song.wma"), SourceFormat::Wma);
        assert_eq!(sniff_named(&head, "movie.wmv"), SourceFormat::Wmv);
    }

    #[test]
    fn a_png_named_txt_is_still_a_png() {
        let dir = std::env::temp_dir().join("taskbar-island-detect-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("photo.txt");
        // 1 x 1 transparent PNG.
        let png: [u8; 67] = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        std::fs::write(&path, png).unwrap();
        let probe = probe_path(&path.display().to_string());
        assert_eq!(probe.format(), SourceFormat::Png);
        assert!(probe.kind.convertible());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn oversized_files_are_refused_before_reading() {
        let dir = std::env::temp_dir().join("taskbar-island-detect-big-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("huge.bin");
        let file = File::create(&path).unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        drop(file);
        let probe = probe_path(&path.display().to_string());
        assert!(matches!(probe.kind, ProbeKind::TooLarge { .. }));
        let _ = std::fs::remove_dir_all(dir);
    }
}
