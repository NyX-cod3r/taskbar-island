//! probe.rs — locating and probing the external engines.
//!
//! Probes run hidden, without a shell, with an argv array and a private
//! working directory. Their output is bounded before it is parsed; the
//! capability matrix is derived from what each build actually reports.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, UNIX_EPOCH};

use windows::Win32::Foundation::{HANDLE, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, WaitForSingleObject, THREAD_PRIORITY_BELOW_NORMAL,
};

use super::{
    AudioFormat, DocumentFormat, Engine, EngineCapabilities, EngineError, EngineId, ImageFormat,
    Job, JobOutput, ProgressSink, VideoFormat,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const OUTPUT_CAP: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint {
    modified: u128,
    bytes: u64,
}

/// Runs a probe on a below-normal thread so FFmpeg's DLLs cannot compete with
/// the taskbar during startup.
pub fn lower_thread_priority() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

// ------------------------------------------------------------- locations

/// Documented locations per engine, most specific first (docs/engines.md).
/// Forward slashes are separators on Windows too.
const fn engine_paths(id: EngineId) -> &'static [&'static str] {
    match id {
        EngineId::Native => &[],
        EngineId::Ffmpeg => &["ffmpeg/bin/ffmpeg.exe"],
        EngineId::Pandoc => &["pandoc/pandoc.exe"],
        EngineId::Tesseract => &["tesseract/tesseract.exe"],
        EngineId::LibreOffice => &["libreoffice/program/soffice.exe"],
        // The Windows build ships every tool in one folder; either name in
        // the layout table is accepted, siblings live next to it.
        EngineId::Poppler => &[
            "poppler/Library/bin/pdftotext.exe",
            "poppler/bin/pdftotext.exe",
        ],
        EngineId::Qpdf => &["qpdf/bin/qpdf.exe"],
        EngineId::Ghostscript => &["ghostscript/bin/gswin64c.exe"],
    }
}

/// Tolerant search for one engine executable (docs/engines.md): the exact
/// documented layout first, then the same layout inside one wrapper folder
/// (a portable archive extracted with a top-level directory). Returns `None`
/// when nothing matches — an absent engine hides its options, it is never an
/// error.
pub fn locate_engine(dir: &Path, id: EngineId) -> Option<PathBuf> {
    let paths = engine_paths(id);
    if paths.is_empty() {
        return None;
    }
    for relative in paths {
        let direct = dir.join(relative);
        if direct.is_file() {
            return Some(direct);
        }
    }
    let mut wrappers: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    // read_dir order is OS-defined; sort so a machine with several wrapper
    // candidates always resolves to the same one.
    wrappers.sort();
    for wrapper in wrappers {
        for relative in paths {
            let nested = wrapper.join(relative);
            if nested.is_file() {
                return Some(nested);
            }
        }
    }
    None
}

pub fn fingerprint(path: &Path) -> Option<Fingerprint> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(Fingerprint {
        modified,
        bytes: metadata.len(),
    })
}

// -------------------------------------------------------------- engines

pub struct NativeEngine;

impl Engine for NativeEngine {
    fn id(&self) -> EngineId {
        EngineId::Native
    }

    fn locate(&self) -> Option<&Path> {
        None
    }

    fn probe(&self) -> Result<EngineCapabilities, EngineError> {
        Ok(native_capabilities())
    }

    fn run(
        &self,
        job: &Job,
        progress: ProgressSink<'_>,
        cancel: &super::CancelToken,
    ) -> Result<JobOutput, EngineError> {
        crate::convert::jobs::run_job(job, progress, cancel)
    }
}

pub struct ExternalEngine {
    id: EngineId,
    exe: Option<PathBuf>,
    /// Every other executable a job may chain into (Pandoc finishing OCR,
    /// Poppler feeding qpdf, LibreOffice → Poppler).
    paths: super::EnginePaths,
}

impl ExternalEngine {
    pub fn new(id: EngineId, exe: Option<PathBuf>) -> Self {
        Self {
            id,
            exe,
            paths: super::EnginePaths::default(),
        }
    }

    pub fn with_paths(mut self, paths: super::EnginePaths) -> Self {
        self.paths = paths;
        self
    }
}

impl Engine for ExternalEngine {
    fn id(&self) -> EngineId {
        self.id
    }

    fn locate(&self) -> Option<&Path> {
        self.exe.as_deref()
    }

    fn probe(&self) -> Result<EngineCapabilities, EngineError> {
        match &self.exe {
            Some(exe) => probe_external(self.id, exe),
            None => Err(EngineError::NotFound),
        }
    }

    fn run(
        &self,
        job: &Job,
        progress: ProgressSink<'_>,
        cancel: &super::CancelToken,
    ) -> Result<JobOutput, EngineError> {
        let exe = self.exe.as_deref().ok_or(EngineError::NotFound)?;
        super::run_external(self.id, exe, &self.paths, job, progress, cancel)
    }
}

/// In-process codecs, compiled into the executable. TIFF and AVIF are not
/// enabled in the `image` crate build, so they are absent here and route to
/// FFmpeg instead.
pub fn native_capabilities() -> EngineCapabilities {
    EngineCapabilities {
        installed: true,
        version: None,
        image_read: vec![
            ImageFormat::Jpeg,
            ImageFormat::Png,
            ImageFormat::WebP,
            ImageFormat::Gif,
            ImageFormat::Bmp,
            ImageFormat::Ico,
        ],
        // The `image` crate in this build has no TIFF encoder, so TIFF jobs
        // route to FFmpeg.
        image_write: vec![
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::WebP,
            ImageFormat::Gif,
            ImageFormat::Bmp,
            ImageFormat::Ico,
            ImageFormat::Pdf,
        ],
        image_edit: true,
        video_write: Vec::new(),
        audio_write: Vec::new(),
        documents_in: Vec::new(),
        documents_out: Vec::new(),
        ocr_languages: Vec::new(),
    }
}

pub(crate) fn probe_external(id: EngineId, exe: &Path) -> Result<EngineCapabilities, EngineError> {
    match id {
        EngineId::Ffmpeg => probe_ffmpeg(exe),
        EngineId::Pandoc => probe_pandoc(exe),
        EngineId::Tesseract => probe_tesseract(exe),
        EngineId::LibreOffice => probe_libreoffice(exe),
        EngineId::Poppler => probe_poppler(exe),
        EngineId::Qpdf => probe_version(exe, "qpdf"),
        EngineId::Ghostscript => probe_version(exe, "Ghostscript"),
        EngineId::Native => Ok(native_capabilities()),
    }
}

fn probe_ffmpeg(exe: &Path) -> Result<EngineCapabilities, EngineError> {
    let version = capture(exe, &["-version"])?;
    let encoders = capture(exe, &["-encoders"])?;
    let muxers = capture(exe, &["-muxers"])?;
    let demuxers = capture(exe, &["-demuxers"])?;
    let decoders = capture(exe, &["-decoders"])?;

    let encoder_names = parse_ffmpeg_names(&encoders.stdout);
    let muxer_names = parse_ffmpeg_names(&muxers.stdout);
    let demuxer_names = parse_ffmpeg_names(&demuxers.stdout);
    let decoder_names = parse_ffmpeg_names(&decoders.stdout);

    Ok(ffmpeg_capabilities(
        first_line(&version.stdout).unwrap_or("FFmpeg"),
        &encoder_names,
        &muxer_names,
        &demuxer_names,
        &decoder_names,
    ))
}

fn probe_pandoc(exe: &Path) -> Result<EngineCapabilities, EngineError> {
    let version = capture(exe, &["--version"])?;
    let inputs = capture(exe, &["--list-input-formats"])?;
    let outputs = capture(exe, &["--list-output-formats"])?;

    Ok(pandoc_capabilities(
        first_line(&version.stdout).unwrap_or("Pandoc"),
        &parse_words(&inputs.stdout),
        &parse_words(&outputs.stdout),
    ))
}

fn probe_tesseract(exe: &Path) -> Result<EngineCapabilities, EngineError> {
    let version = capture(exe, &["--version"])?;
    let langs = capture(exe, &["--list-langs"])?;

    let mut capabilities = EngineCapabilities {
        installed: true,
        version: Some(
            first_line(&version.stdout)
                .unwrap_or("Tesseract")
                .to_owned(),
        ),
        ..EngineCapabilities::default()
    };
    capabilities.ocr_languages = parse_tesseract_langs(&langs.stdout);
    Ok(capabilities)
}

/// `soffice --version` answers on stdout; the format lists are fixed because
/// the router only asks "is it installed" for office conversions (B3 passes
/// the concrete filter names on the command line).
fn probe_libreoffice(exe: &Path) -> Result<EngineCapabilities, EngineError> {
    let version = capture(exe, &["--version"])?;
    Ok(EngineCapabilities {
        installed: true,
        version: Some(
            first_line(&version.stdout)
                .unwrap_or("LibreOffice")
                .to_owned(),
        ),
        documents_in: DocumentFormat::ALL.to_vec(),
        // Writer/Calc/Impress write these (PDF and legacy DOC included);
        // Markdown, EPUB and LaTeX are Pandoc's job.
        documents_out: vec![
            DocumentFormat::Docx,
            DocumentFormat::Odt,
            DocumentFormat::Html,
            DocumentFormat::Txt,
            DocumentFormat::Rtf,
            DocumentFormat::Pdf,
            DocumentFormat::Doc,
        ],
        ..EngineCapabilities::default()
    })
}

/// `pdftotext -v` reports on stderr and exits 0; every Poppler tool in the
/// folder is the same build, so one version line proves the set.
fn probe_poppler(exe: &Path) -> Result<EngineCapabilities, EngineError> {
    let version = capture(exe, &["-v"])?;
    Ok(EngineCapabilities {
        installed: true,
        version: Some(
            first_line(&version.stdout)
                .or_else(|| first_line(&version.stderr))
                .unwrap_or("Poppler")
                .to_owned(),
        ),
        ..EngineCapabilities::default()
    })
}

/// qpdf and Ghostscript both answer `--version` on stdout with nothing else
/// to derive, so one builder covers them.
fn probe_version(exe: &Path, fallback: &str) -> Result<EngineCapabilities, EngineError> {
    let version = capture(exe, &["--version"])?;
    Ok(EngineCapabilities {
        installed: true,
        version: Some(first_line(&version.stdout).unwrap_or(fallback).to_owned()),
        ..EngineCapabilities::default()
    })
}

// ------------------------------------------------------------ derivation

pub fn ffmpeg_capabilities(
    version: &str,
    encoders: &BTreeSet<String>,
    muxers: &BTreeSet<String>,
    demuxers: &BTreeSet<String>,
    decoders: &BTreeSet<String>,
) -> EngineCapabilities {
    let has_encoder = |names: &[&str]| names.iter().any(|name| encoders.contains(*name));
    let has_muxer = |name: &str| muxers.contains(name);
    let has_demuxer = |name: &str| demuxers.contains(name);
    let has_decoder = |names: &[&str]| names.iter().any(|name| decoders.contains(*name));

    let image2_out = has_muxer("image2");
    let mut image_write = Vec::new();
    if image2_out && has_encoder(&["png"]) {
        image_write.push(ImageFormat::Png);
    }
    if image2_out && has_encoder(&["mjpeg"]) {
        image_write.push(ImageFormat::Jpeg);
    }
    if image2_out && has_encoder(&["libwebp", "libwebp_anim"]) {
        image_write.push(ImageFormat::WebP);
    }
    if has_muxer("gif") && has_encoder(&["gif"]) {
        image_write.push(ImageFormat::Gif);
    }
    if image2_out && has_encoder(&["bmp"]) {
        image_write.push(ImageFormat::Bmp);
    }
    if image2_out && has_encoder(&["tiff"]) {
        image_write.push(ImageFormat::Tiff);
    }
    if has_muxer("avif") && has_encoder(&["libaom-av1", "libsvtav1", "librav1e"]) {
        image_write.push(ImageFormat::Avif);
    }

    let image2_in = has_demuxer("image2");
    let mut image_read = Vec::new();
    if image2_in && has_decoder(&["png"]) {
        image_read.push(ImageFormat::Png);
    }
    if image2_in && has_decoder(&["mjpeg"]) {
        image_read.push(ImageFormat::Jpeg);
    }
    if image2_in && has_decoder(&["libwebp", "webp"]) {
        image_read.push(ImageFormat::WebP);
    }
    if image2_in && has_decoder(&["gif"]) {
        image_read.push(ImageFormat::Gif);
    }
    if image2_in && has_decoder(&["bmp"]) {
        image_read.push(ImageFormat::Bmp);
    }
    if image2_in && has_decoder(&["tiff"]) {
        image_read.push(ImageFormat::Tiff);
    }
    if image2_in && has_decoder(&["libdav1d", "libaom-av1", "av1"]) {
        image_read.push(ImageFormat::Avif);
    }

    let video_encoder = has_encoder(&["libx264", "libx265", "mpeg4", "libvpx", "libvpx-vp9"]);
    let mut video_write = Vec::new();
    if video_encoder && has_muxer("mp4") {
        video_write.push(VideoFormat::Mp4);
    }
    if video_encoder && has_muxer("matroska") {
        video_write.push(VideoFormat::Mkv);
    }
    if has_muxer("webm") && has_encoder(&["libvpx", "libvpx-vp9", "libsvtav1", "libaom-av1"]) {
        video_write.push(VideoFormat::WebM);
    }
    if video_encoder && has_muxer("mov") {
        video_write.push(VideoFormat::Mov);
    }
    if video_encoder && has_muxer("avi") {
        video_write.push(VideoFormat::Avi);
    }
    if has_muxer("asf") && has_encoder(&["wmv2", "wmv1"]) {
        video_write.push(VideoFormat::Wmv);
    }
    if has_muxer("gif") && has_encoder(&["gif"]) {
        video_write.push(VideoFormat::Gif);
    }

    let mut audio_write = Vec::new();
    if has_muxer("mp3") && has_encoder(&["libmp3lame", "mp3"]) {
        audio_write.push(AudioFormat::Mp3);
    }
    if has_muxer("wav") && has_encoder(&["pcm_s16le"]) {
        audio_write.push(AudioFormat::Wav);
    }
    if has_muxer("flac") && has_encoder(&["flac"]) {
        audio_write.push(AudioFormat::Flac);
    }
    if has_muxer("ipod") && has_encoder(&["aac"]) {
        audio_write.push(AudioFormat::M4a);
    }
    if has_muxer("ogg") && has_encoder(&["libvorbis"]) {
        audio_write.push(AudioFormat::Ogg);
    }
    if (has_muxer("opus") || has_muxer("ogg")) && has_encoder(&["libopus"]) {
        audio_write.push(AudioFormat::Opus);
    }
    if has_muxer("adts") && has_encoder(&["aac"]) {
        audio_write.push(AudioFormat::Aac);
    }
    if has_muxer("asf") && has_encoder(&["wmav2", "wmav1"]) {
        audio_write.push(AudioFormat::Wma);
    }
    if has_muxer("aiff") && has_encoder(&["pcm_s16be", "pcm_s16le"]) {
        audio_write.push(AudioFormat::Aiff);
    }

    EngineCapabilities {
        installed: true,
        version: Some(version.to_owned()),
        image_read,
        image_write,
        image_edit: has_demuxer("image2"),
        video_write,
        audio_write,
        documents_in: Vec::new(),
        documents_out: Vec::new(),
        ocr_languages: Vec::new(),
    }
}

pub fn pandoc_capabilities(
    version: &str,
    inputs: &BTreeSet<String>,
    outputs: &BTreeSet<String>,
) -> EngineCapabilities {
    let readable = |name: &str| inputs.contains(name);
    let writable = |name: &str| outputs.contains(name);

    let mut documents_in = Vec::new();
    for (format, names) in [
        (DocumentFormat::Docx, &["docx"][..]),
        (DocumentFormat::Odt, &["odt"][..]),
        (DocumentFormat::Markdown, &["markdown"][..]),
        (DocumentFormat::Html, &["html"][..]),
        // Plain text has no dedicated reader; Pandoc detects it as markdown.
        (DocumentFormat::Txt, &["markdown"][..]),
        (DocumentFormat::Epub, &["epub"][..]),
        (DocumentFormat::Rtf, &["rtf"][..]),
        (DocumentFormat::Tex, &["latex"][..]),
    ] {
        if names.iter().any(|name| readable(name)) {
            documents_in.push(format);
        }
    }

    let mut documents_out = Vec::new();
    for (format, names) in [
        (DocumentFormat::Docx, &["docx"][..]),
        (DocumentFormat::Odt, &["odt"][..]),
        (DocumentFormat::Markdown, &["markdown"][..]),
        (DocumentFormat::Html, &["html"][..]),
        (DocumentFormat::Txt, &["plain"][..]),
        (DocumentFormat::Epub, &["epub"][..]),
        (DocumentFormat::Rtf, &["rtf"][..]),
        (DocumentFormat::Tex, &["latex"][..]),
    ] {
        if names.iter().any(|name| writable(name)) {
            documents_out.push(format);
        }
    }

    EngineCapabilities {
        installed: true,
        version: Some(version.to_owned()),
        documents_in,
        documents_out,
        ..EngineCapabilities::default()
    }
}

// ---------------------------------------------------------------- parsing

/// FFmpeg lists start with flag tokens (`V....D`) followed by the codec or
/// format name. Header and legend lines are filtered out by shape.
pub fn parse_ffmpeg_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        let mut tokens = line.split_whitespace();
        let Some(flags) = tokens.next() else {
            continue;
        };
        if flags.is_empty() || !flags.chars().all(|c| "VASDEFXBd.".contains(c)) {
            continue;
        }
        let Some(name) = tokens.next() else {
            continue;
        };
        if name == "=" || name.starts_with('=') {
            continue;
        }
        for part in name.split(',') {
            names.insert(part.to_ascii_lowercase());
        }
    }
    names
}

pub fn parse_words(text: &str) -> BTreeSet<String> {
    text.split_whitespace()
        .map(|word| word.to_ascii_lowercase())
        .collect()
}

pub fn parse_tesseract_langs(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("List of available languages"))
        .filter(|line| *line != "osd")
        .map(|line| line.to_owned())
        .collect()
}

fn first_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

// ---------------------------------------------------------------- process

struct Captured {
    stdout: String,
    stderr: String,
}

/// Runs a short probe hidden. Output is capped; the process is killed after a
/// generous timeout so a broken executable can never hang startup.
fn capture(exe: &Path, args: &[&str]) -> Result<Captured, EngineError> {
    let mut command = Command::new(exe);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(probe_dir())
        .creation_flags(CREATE_NO_WINDOW);
    let mut child = command
        .spawn()
        .map_err(|error| EngineError::Spawn(format!("{}: {error}", exe.display())))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| EngineError::Spawn("missing stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| EngineError::Spawn("missing stderr".into()))?;
    let stdout_reader = read_capped(stdout);
    let stderr_reader = read_capped(stderr);

    let handle = HANDLE(child.as_raw_handle());
    let waited = unsafe { WaitForSingleObject(handle, PROBE_TIMEOUT.as_millis() as u32) };
    let timed_out = waited == WAIT_TIMEOUT;
    if timed_out {
        let _ = child.kill();
    }
    let status = child.wait().ok();

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    if timed_out {
        return Err(EngineError::Timeout);
    }
    if let Some(status) = status {
        if !status.success() {
            let message = String::from_utf8_lossy(&stderr).trim().to_owned();
            return Err(EngineError::Failed(if message.is_empty() {
                format!("{} exited with {status}", exe.display())
            } else {
                message
            }));
        }
    }
    Ok(Captured {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

fn read_capped(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if kept.len() < OUTPUT_CAP {
                        let take = (OUTPUT_CAP - kept.len()).min(read);
                        kept.extend_from_slice(&chunk[..take]);
                    }
                }
            }
        }
        kept
    })
}

fn probe_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("taskbar-island-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCODERS: &str = "\
Encoders:
 V..... = Video
 A..... = Audio
 S..... = Subtitle
 .F.... = Frame-level multithreading
 ..S... = Slice-level multithreading
 ...X.. = Codec is experimental
 ....B. = Supports draw_horiz_band
 ------
 V....D libaom-av1           libaom AV1 (codec av1)
 V..... libsvtav1            SVT-AV1(Scalable Video Technology for AV1) encoder (codec av1)
 V....D bmp                  BMP (Windows and OS/2 bitmap)
 V....D gif                  GIF (Graphics Interchange Format)
 V....D libx264              libx264 H.264 / AVC
 V....D libx265              libx265 H.265 / HEVC
 VFS..D mjpeg                MJPEG (Motion JPEG)
 V.S..D mpeg4                MPEG-4 part 2
 VF...D png                  PNG (Portable Network Graphics) image
 VF...D tiff                 TIFF image
 V....D libvpx               libvpx VP8 (codec vp8)
 V....D libvpx-vp9           libvpx VP9 (codec vp9)
 V....D libwebp              libwebp WebP image (codec webp)
 A....D aac                  AAC (Advanced Audio Coding)
 A....D flac                 FLAC (Free Lossless Audio Codec)
 A....D libmp3lame           libmp3lame MP3 (MPEG audio layer 3)
 A....D libopus               libopus Opus
 A....D pcm_s16le            PCM signed 16-bit little-endian
 A....D libvorbis            libvorbis (codec vorbis)
 A....D wmav2                Windows Media Audio 2
";

    const MUXERS: &str = "\
Formats:
 D.. = Demuxing supported
 .E. = Muxing supported
 ..d = Is a device
 ------
  E  adts            ADTS AAC (Advanced Audio Coding)
  E  aiff            Audio IFF
  E  asf             ASF (Advanced / Active Streaming Format)
  E  avi             AVI (Audio Video Interleaved)
  E  avif            AVIF
  E  flac            raw FLAC
  E  gif             CompuServe Graphics Interchange Format (GIF)
  E  image2          image2 sequence
  E  ipod            iPod H.264 MP4 (MPEG-4 Part 14)
  E  matroska        Matroska
  E  mov             QuickTime / MOV
  E  mp3             MP3 (MPEG audio layer 3)
  E  mp4             MP4 (MPEG-4 Part 14)
  E  ogg             Ogg
  E  opus            Ogg Opus
  E  wav             WAV / WAVE (Waveform Audio)
  E  webm            WebM
";

    const DEMUXERS: &str = "\
Formats:
 D.. = Demuxing supported
 .E. = Muxing supported
 ------
 D   aac             raw ADTS AAC
 D   aiff            Audio IFF
 D   asf             ASF
 D   avi             AVI
 D   flac            raw FLAC
 D   gif             GIF
 D   image2          image2 sequence
 D   matroska,webm   Matroska / WebM
 D   mov,mp4,m4a,3gp,3g2,mj2 QuickTime / MOV
 D   mp3             MP2/3
 D   ogg             Ogg
 D   wav             WAV
";

    const DECODERS: &str = "\
Decoders:
 V..... = Video
 A..... = Audio
 ------
 V..... libdav1d             dav1d AV1 decoder
 V....D libaom-av1           libaom AV1
 VFS..D h264                 H.264 / AVC
 V....D mjpeg                MJPEG
 VF...D png                  PNG
 VF...D tiff                 TIFF
 V....D libvpx                libvpx VP8
 V....D libvpx-vp9           libvpx VP9
 V....D libwebp              libwebp WebP
 V....D gif                  GIF
 V....D bmp                  BMP
 A....D aac                  AAC
";

    #[test]
    fn ffmpeg_names_skip_legends_and_split_aliases() {
        let names = parse_ffmpeg_names(DEMUXERS);
        assert!(names.contains("matroska"));
        assert!(names.contains("webm"));
        assert!(names.contains("mp4"));
        assert!(names.contains("mj2"));
        assert!(!names.contains("formats:"));
        assert!(!names.contains("demuxing"));
    }

    #[test]
    fn ffmpeg_capabilities_reflect_the_build() {
        let caps = ffmpeg_capabilities(
            "ffmpeg 9.0.2",
            &parse_ffmpeg_names(ENCODERS),
            &parse_ffmpeg_names(MUXERS),
            &parse_ffmpeg_names(DEMUXERS),
            &parse_ffmpeg_names(DECODERS),
        );
        assert!(caps.installed);
        assert!(caps.can_write_image(ImageFormat::Png));
        assert!(caps.can_write_image(ImageFormat::WebP));
        assert!(caps.can_write_image(ImageFormat::Gif));
        assert!(caps.can_write_image(ImageFormat::Bmp));
        assert!(caps.can_write_image(ImageFormat::Tiff));
        assert!(caps.can_write_image(ImageFormat::Avif));
        assert!(!caps.can_write_image(ImageFormat::Ico));
        assert!(!caps.can_write_image(ImageFormat::Pdf));
        assert!(caps.image_edit);
        assert!(caps.can_write_video(VideoFormat::Mp4));
        assert!(caps.can_write_video(VideoFormat::WebM));
        assert!(caps.can_write_video(VideoFormat::Mkv));
        assert!(caps.can_write_audio(AudioFormat::Mp3));
        assert!(caps.can_write_audio(AudioFormat::M4a));
        assert!(caps.can_write_audio(AudioFormat::Opus));
        assert!(!caps.can_write_document(DocumentFormat::Markdown));
        assert!(!caps.can_ocr());
    }

    #[test]
    fn pandoc_maps_its_reported_formats() {
        let inputs = parse_words(
            "asciidoc bibtex commonmark csv docx epub html latex markdown odt rtf \
             typst xlsx xml",
        );
        let outputs = parse_words(
            "asciidoc beamer docx epub html latex markdown odt plain rtf \
             texinfo typst",
        );
        let caps = pandoc_capabilities("pandoc 3.12", &inputs, &outputs);
        assert!(caps.can_read_document(DocumentFormat::Docx));
        assert!(caps.can_read_document(DocumentFormat::Txt));
        assert!(caps.can_write_document(DocumentFormat::Markdown));
        assert!(caps.can_write_document(DocumentFormat::Txt));
        assert!(caps.can_write_document(DocumentFormat::Tex));
    }

    #[test]
    fn probes_the_installed_engines_when_configured() {
        let Ok(dir) = std::env::var(crate::engines::ENGINES_ENV) else {
            return;
        };
        let set = crate::engines::EngineSet::load(PathBuf::from(dir));
        let caps = set.capabilities();
        for id in EngineId::ALL {
            let engine = caps.get(id);
            println!(
                "{}: installed={} version={:?} images_out={:?} video={:?} audio={:?} docs_out={:?} langs={:?}",
                id.name().unwrap_or("Native"),
                engine.installed,
                engine.version,
                engine.image_write,
                engine.video_write,
                engine.audio_write,
                engine.documents_out,
                engine.ocr_languages
            );
        }
        assert!(caps.installed(EngineId::Native));
        assert!(caps.installed(EngineId::Ffmpeg));
        assert!(caps.installed(EngineId::Pandoc));
        assert!(caps.installed(EngineId::Tesseract));
        // The four v0.8 engines are optional; absent ones report not
        // installed instead of failing the probe.
        for id in [
            EngineId::LibreOffice,
            EngineId::Poppler,
            EngineId::Qpdf,
            EngineId::Ghostscript,
        ] {
            let engine = caps.get(id);
            println!(
                "{}: installed={} version={:?}",
                id.name().unwrap_or("?"),
                engine.installed,
                engine.version
            );
        }
    }

    #[test]
    fn tesseract_languages_ignore_the_header_and_osd() {
        let languages = parse_tesseract_langs(
            "List of available languages in \"C:\\engines\\tessdata/\" (2):\neng\nosd\n",
        );
        assert_eq!(languages, vec!["eng"]);
        assert!(parse_tesseract_langs(
            "List of available languages in \"C:\\engines\\tessdata/\" (0):\n"
        )
        .is_empty());
    }

    /// Unique per-run scratch: tests never delete anything, so reusing a
    /// folder would make the layout assertions depend on earlier runs.
    fn scratch(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "taskbar-island-resolver-{name}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn dummy(dir: &Path, relative: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"").unwrap();
    }

    /// Test 8: every engine resolves in the documented layout (exact, its
    /// own sub-folder nesting, and one wrapper folder); absent ⇒ None.
    #[test]
    fn the_resolver_finds_every_engine_in_the_documented_layouts() {
        const LAYOUTS: &[(EngineId, &str)] = &[
            (EngineId::Ffmpeg, "ffmpeg/bin/ffmpeg.exe"),
            (EngineId::Pandoc, "pandoc/pandoc.exe"),
            (EngineId::Tesseract, "tesseract/tesseract.exe"),
            (EngineId::LibreOffice, "libreoffice/program/soffice.exe"),
            (EngineId::Poppler, "poppler/Library/bin/pdftotext.exe"),
            (EngineId::Qpdf, "qpdf/bin/qpdf.exe"),
            (EngineId::Ghostscript, "ghostscript/bin/gswin64c.exe"),
        ];
        for (id, layout) in LAYOUTS {
            // Exact: as documented, including the engine's own nesting.
            let exact = scratch("exact");
            dummy(&exact, layout);
            assert_eq!(locate_engine(&exact, *id), Some(exe(&exact, layout)));

            // Wrapper: one extra top-level folder around the same layout.
            let wrapped = scratch("wrapped");
            dummy(&wrapped.join("wrapper"), layout);
            assert_eq!(
                locate_engine(&wrapped, *id),
                Some(exe(&wrapped.join("wrapper"), layout))
            );

            // Absent: an empty root resolves to nothing, never an error.
            let empty = scratch("empty");
            assert_eq!(locate_engine(&empty, *id), None);
        }
        // Native has no executable, wrapper folder or not.
        let root = scratch("native");
        dummy(&root, "ffmpeg/bin/ffmpeg.exe");
        assert_eq!(locate_engine(&root, EngineId::Native), None);
    }

    fn exe(root: &Path, relative: &str) -> PathBuf {
        root.join(relative)
    }

    /// Poppler's alternate documented folder (`bin` instead of `Library\bin`)
    /// is accepted too.
    #[test]
    fn poppler_resolves_in_either_documented_folder() {
        let root = scratch("poppler-bin");
        dummy(&root, "poppler/bin/pdftotext.exe");
        assert_eq!(
            locate_engine(&root, EngineId::Poppler),
            Some(root.join("poppler/bin/pdftotext.exe"))
        );
    }
}
