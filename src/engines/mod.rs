//! engines — local executables and the instant, static conversion router.
//!
//! Nothing here touches the network. External engines are only ever started
//! from a probed, local path; the router is a pure table lookup over the
//! capability matrix built at startup, so choosing an engine never waits on a
//! benchmark or a process spawn.

pub mod exec;
pub mod ffmpeg;
pub mod ghostscript;
pub mod libreoffice;
pub mod output;
pub mod pandoc;
pub mod poppler;
pub mod probe;
pub mod qpdf;
pub mod router;
pub mod tesseract;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use output::TargetFormat;

/// Per-job ceilings. They are deliberately far above normal runtimes; a job
/// that reaches one is broken, not slow.
pub const IMAGE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const MEDIA_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
pub const DOCUMENT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const OCR_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// LibreOffice is the slowest engine by far: a cold `soffice` plus a DOCX
/// round-trip can take minutes on a cold profile, so it gets its own ceiling
/// (B prompt §3: one job at a time, 180 s timeout for the convert itself).
pub const OFFICE_TIMEOUT: Duration = Duration::from_secs(180);

/// Where development builds point at a pre-installed engine folder. Installed
/// builds resolve `<exe dir>\engines` instead.
pub const ENGINES_ENV: &str = "TBI_ENGINES_DIR";

// ---------------------------------------------------------------- identities

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EngineId {
    Native,
    Ffmpeg,
    Pandoc,
    Tesseract,
    LibreOffice,
    Poppler,
    Qpdf,
    Ghostscript,
}

impl EngineId {
    pub const ALL: [EngineId; 8] = [
        EngineId::Native,
        EngineId::Ffmpeg,
        EngineId::Pandoc,
        EngineId::Tesseract,
        EngineId::LibreOffice,
        EngineId::Poppler,
        EngineId::Qpdf,
        EngineId::Ghostscript,
    ];

    pub const fn index(self) -> usize {
        match self {
            EngineId::Native => 0,
            EngineId::Ffmpeg => 1,
            EngineId::Pandoc => 2,
            EngineId::Tesseract => 3,
            EngineId::LibreOffice => 4,
            EngineId::Poppler => 5,
            EngineId::Qpdf => 6,
            EngineId::Ghostscript => 7,
        }
    }

    /// Tooltip suffix for externally executed engines; Native stays silent.
    pub const fn name(self) -> Option<&'static str> {
        match self {
            EngineId::Native => None,
            EngineId::Ffmpeg => Some("FFmpeg"),
            EngineId::Pandoc => Some("Pandoc"),
            EngineId::Tesseract => Some("Tesseract"),
            EngineId::LibreOffice => Some("LibreOffice"),
            EngineId::Poppler => Some("Poppler"),
            EngineId::Qpdf => Some("qpdf"),
            EngineId::Ghostscript => Some("Ghostscript"),
        }
    }

    /// Every engine now has a runner. Whether it can run a particular job is
    /// decided by the capability matrix, not by this flag.
    pub const fn job_ready(self) -> bool {
        true
    }
}

/// The coarse source class the router works on. The exact container is
/// classified by magic bytes elsewhere; a mismatched Native attempt simply
/// falls through to the next candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    Image,
    Video,
    Audio,
    Document,
    /// PDF documents (v0.8): their own card with PDF tools.
    Pdf,
    /// Spreadsheets (XLSX, XLS, ODS, CSV, TSV).
    Spreadsheet,
    /// Presentations (PPTX, PPT, ODP).
    Presentation,
    /// Several selected files at once (2+): merge / zip / batch.
    Multi,
    Other,
}

// ------------------------------------------------------------------ formats

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Jpeg,
    WebP,
    Gif,
    Bmp,
    Tiff,
    Ico,
    Avif,
    Pdf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoFormat {
    Mp4,
    Mkv,
    WebM,
    Mov,
    Avi,
    Wmv,
    Gif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioFormat {
    Mp3,
    Wav,
    Flac,
    M4a,
    Ogg,
    Opus,
    Aac,
    Wma,
    Aiff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocumentFormat {
    Docx,
    Odt,
    Markdown,
    Html,
    Txt,
    Epub,
    Rtf,
    Tex,
    /// "Convert to PDF" from a document (v0.8). LibreOffice writes it; it
    /// lives here rather than in `ImageFormat` because the source is a
    /// document, not a raster.
    Pdf,
    /// Legacy binary Word (v0.8); LibreOffice converts it.
    Doc,
}

impl DocumentFormat {
    pub const ALL: [DocumentFormat; 10] = [
        DocumentFormat::Docx,
        DocumentFormat::Odt,
        DocumentFormat::Markdown,
        DocumentFormat::Html,
        DocumentFormat::Txt,
        DocumentFormat::Epub,
        DocumentFormat::Rtf,
        DocumentFormat::Tex,
        DocumentFormat::Pdf,
        DocumentFormat::Doc,
    ];
}

// ------------------------------------------------------------------ actions

/// Every option in the Convert card maps to exactly one action. The options
/// table owns labels and icons; this enum only names the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    ImageConvert(ImageFormat),
    ImageOptimize(ImageOptimize),
    ImageEdit(ImageEdit),
    ImageOcr(OcrOutput),
    VideoConvert(VideoFormat),
    VideoOptimize(VideoOptimize),
    VideoEdit(VideoEdit),
    VideoExtract(VideoExtract),
    AudioConvert(AudioFormat),
    AudioOptimize(AudioOptimize),
    DocumentConvert(DocumentFormat),
    DocumentExtract(DocumentExtract),
    /// PDF file → another format (v0.8).
    PdfConvert(PdfTarget),
    /// The PDF tools tab (v0.8).
    PdfTool(PdfTool),
    /// Text/image extraction from a PDF (v0.8).
    PdfExtract(PdfExtract),
    /// Spreadsheet → another format (v0.8).
    SheetConvert(SheetFormat),
    /// Presentation → another format (v0.8).
    PresentConvert(PresentFormat),
    /// Several files at once: merge, zip, batch (v0.8).
    Multi(MultiOp),
    /// ZIP archive operations (v0.8).
    Zip(ZipOp),
}

/// Targets of "convert this PDF to …".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PdfTarget {
    Docx,
    Odt,
    Txt,
    Markdown,
    Png,
    Jpeg,
}

/// The PDF tools tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PdfTool {
    Compress,
    RotateLeft,
    RotateRight,
    Split,
    Merge,
    Searchable,
    OptimizeWeb,
}

/// Extraction actions on a PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PdfExtract {
    /// `pdftotext` → clipboard.
    CopyText,
    /// `pdftotext` → `.txt`.
    Text,
    /// `pdfimages -all`.
    Images,
    /// One page → PNG.
    PagePng,
}

/// Targets of "convert this spreadsheet to …".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SheetFormat {
    Pdf,
    Csv,
    Xlsx,
    Ods,
    Html,
    Xls,
    Tsv,
    /// CSV → JSON, built into the Native engine.
    Json,
}

/// Targets of "convert this presentation to …".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentFormat {
    Pdf,
    Pptx,
    Ppt,
    Odp,
    /// Slides → PNG (LibreOffice → Poppler chain).
    Png,
}

/// Operations over a multi-file selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MultiOp {
    MergePdf,
    Zip,
    BatchConvert,
    BatchResize,
    BatchCompress,
    BatchStripMetadata,
}

/// Operations on a ZIP archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZipOp {
    ExtractHere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageOptimize {
    Compress,
    /// Long-edge percentage: 75, 50, 25.
    ResizePercent(u16),
    /// Long-edge cap in pixels.
    Fit(u32),
    Thumbnail(u32),
    StripMetadata,
    /// Compress + strip + WebP.
    WebReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageEdit {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    Grayscale,
    BlackWhite,
    CropSquare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OcrOutput {
    Clipboard,
    Txt,
    Markdown,
    Docx,
    SearchablePdf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoOptimize {
    Compress,
    /// Long-edge cap in pixels.
    Height(u32),
    Mute,
    /// Stream-copy into a fresh container without re-encoding (§2.3).
    Remux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoEdit {
    RotateLeft,
    RotateRight,
    Speed(Speed),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Speed {
    Half,
    OneAndHalf,
    Double,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoExtract {
    Audio(AudioFormat),
    Frame,
    Gif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioOptimize {
    SmallerBitrate,
    Mono,
    Normalize,
    TrimSilence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocumentExtract {
    PlainText,
    Images,
}

// --------------------------------------------------------------- capability

/// What one engine can really do on this machine, built by probing rather than
/// assuming. Empty vectors mean "cannot"; `installed` is false when the
/// executable is missing or failed to answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineCapabilities {
    pub installed: bool,
    pub version: Option<String>,
    pub image_read: Vec<ImageFormat>,
    pub image_write: Vec<ImageFormat>,
    pub image_edit: bool,
    pub video_write: Vec<VideoFormat>,
    pub audio_write: Vec<AudioFormat>,
    pub documents_in: Vec<DocumentFormat>,
    pub documents_out: Vec<DocumentFormat>,
    pub ocr_languages: Vec<String>,
}

impl EngineCapabilities {
    pub fn can_read_image(&self, format: ImageFormat) -> bool {
        self.image_read.contains(&format)
    }

    pub fn can_write_image(&self, format: ImageFormat) -> bool {
        self.image_write.contains(&format)
    }

    pub fn can_write_video(&self, format: VideoFormat) -> bool {
        self.video_write.contains(&format)
    }

    pub fn can_write_audio(&self, format: AudioFormat) -> bool {
        self.audio_write.contains(&format)
    }

    pub fn can_write_document(&self, format: DocumentFormat) -> bool {
        self.documents_out.contains(&format)
    }

    pub fn can_read_document(&self, format: DocumentFormat) -> bool {
        self.documents_in.contains(&format)
    }

    pub fn can_ocr(&self) -> bool {
        !self.ocr_languages.is_empty()
    }
}

/// Capability matrix indexed by engine. Cloning it is cheap enough for the
/// UI to hold a snapshot of the table it was built from.
#[derive(Debug, Clone, Default)]
pub struct CapabilityTable {
    slots: [EngineCapabilities; 8],
}

impl CapabilityTable {
    pub fn get(&self, engine: EngineId) -> &EngineCapabilities {
        &self.slots[engine.index()]
    }

    pub fn set(&mut self, engine: EngineId, capabilities: EngineCapabilities) {
        self.slots[engine.index()] = capabilities;
    }

    pub fn installed(&self, engine: EngineId) -> bool {
        self.get(engine).installed
    }
}

// -------------------------------------------------------------------- jobs

/// One conversion. The output path is decided by the caller (next to the
/// original, collision-safe); engines only write the bytes they are given.
#[derive(Debug, Clone)]
pub struct Job {
    pub input: PathBuf,
    pub output: PathBuf,
    pub action: Action,
    /// OCR language (`eng`, `deu`, …). Ignored by every other job.
    pub language: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct JobOutput {
    pub output: PathBuf,
    /// The container that was written, when the job produces one file.
    pub format: Option<TargetFormat>,
    /// OCR-to-clipboard results; every other job leaves this `None`.
    pub text: Option<String>,
    /// A delivery note shown instead of a file name (Ghostscript's "no gain"
    /// report: the result would not have been smaller than the original, so
    /// nothing was written beside it).
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Reading,
    Converting,
    Saving,
    Done,
}

impl Stage {
    /// Reading → converting → saving, the three dots on the Convert card.
    pub const fn index(self) -> u8 {
        match self {
            Stage::Reading => 0,
            Stage::Converting => 1,
            Stage::Saving | Stage::Done => 2,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub stage: Stage,
    /// `None` means indeterminate; external engines without machine-readable
    /// progress use stage-only updates.
    pub fraction: Option<f32>,
}

pub type ProgressSink<'a> = &'a mut (dyn FnMut(Progress) + 'a);

#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The engine cannot run this job at all; try the next candidate.
    Unsupported,
    /// No engine was installed for the job.
    NotFound,
    Cancelled,
    Timeout,
    Spawn(String),
    Failed(String),
}

impl EngineError {
    pub fn is_cancel(&self) -> bool {
        matches!(self, EngineError::Cancelled)
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Unsupported => write!(f, "this engine does not support the job"),
            EngineError::NotFound => write!(f, "no engine is installed for the job"),
            EngineError::Cancelled => write!(f, "cancelled"),
            EngineError::Timeout => write!(f, "the engine took too long"),
            EngineError::Spawn(message) | EngineError::Failed(message) => f.write_str(message),
        }
    }
}

/// A local conversion backend. `run` is wired per job in the job phases; the
/// router and capability matrix do not need it.
pub trait Engine: Send + Sync {
    fn id(&self) -> EngineId;
    fn locate(&self) -> Option<&std::path::Path>;
    fn probe(&self) -> Result<EngineCapabilities, EngineError>;
    fn run(
        &self,
        job: &Job,
        progress: ProgressSink<'_>,
        cancel: &CancelToken,
    ) -> Result<JobOutput, EngineError> {
        let _ = (job, progress, cancel);
        Err(EngineError::Unsupported)
    }
}

// -------------------------------------------------------------------- set

/// The executable locations a worker needs, cloned out of the probed set so a
/// conversion never holds the engine lock while it runs.
#[derive(Debug, Clone, Default)]
pub struct EnginePaths {
    slots: [Option<PathBuf>; 8],
}

impl EnginePaths {
    pub fn location(&self, id: EngineId) -> Option<&std::path::Path> {
        self.slots[id.index()].as_deref()
    }

    /// Tesseract's Markdown/DOCX outputs are finished by Pandoc.
    pub fn helper(&self) -> Option<&std::path::Path> {
        self.location(EngineId::Pandoc)
    }
}

/// Runs one external job. Shared by `Engine::run` and the batch runner so the
/// dispatch table lives in exactly one place.
///
/// `paths` carries every other executable the job may need as a later step
/// (Pandoc finishing OCR, Poppler feeding qpdf, LibreOffice → Poppler chains).
pub fn run_external(
    id: EngineId,
    exe: &std::path::Path,
    paths: &EnginePaths,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    match id {
        EngineId::Ffmpeg => ffmpeg::run(exe, job, progress, cancel),
        EngineId::Pandoc => pandoc::run(exe, paths, job, progress, cancel),
        EngineId::Tesseract => tesseract::run(exe, paths.helper(), job, progress, cancel),
        EngineId::LibreOffice => libreoffice::run(exe, paths, job, progress, cancel),
        EngineId::Poppler => poppler::run(exe, paths, job, progress, cancel),
        EngineId::Qpdf => qpdf::run(exe, paths, job, progress, cancel),
        EngineId::Ghostscript => ghostscript::run(exe, paths, job, progress, cancel),
        EngineId::Native => Err(EngineError::Unsupported),
    }
}

/// What the UI holds: the capability matrix and the paths a worker needs.
#[derive(Debug, Clone, Default)]
pub struct EngineSnapshot {
    pub capabilities: CapabilityTable,
    pub paths: EnginePaths,
}

/// The probed set if the background probe has finished, otherwise Native only.
/// Never blocks on the probe.
pub fn snapshot() -> EngineSnapshot {
    if let Some(shared) = shared() {
        if let Ok(set) = shared.lock() {
            return set.snapshot();
        }
    }
    let mut capabilities = CapabilityTable::default();
    capabilities.set(EngineId::Native, probe::native_capabilities());
    EngineSnapshot {
        capabilities,
        paths: EnginePaths::default(),
    }
}

/// The probed set of engines. `refresh` re-runs a probe only for an executable
/// whose path, size or mtime changed since the last look.
pub struct EngineSet {
    dir: PathBuf,
    engines: Vec<(EngineId, Box<dyn Engine>)>,
    capabilities: CapabilityTable,
    locations: [Option<PathBuf>; 8],
    fingerprints: [Option<probe::Fingerprint>; 8],
}

impl EngineSet {
    pub fn load(dir: PathBuf) -> Self {
        let mut set = Self {
            dir,
            engines: Vec::new(),
            capabilities: CapabilityTable::default(),
            locations: std::array::from_fn(|_| None),
            fingerprints: std::array::from_fn(|_| None),
        };
        set.refresh();
        set
    }

    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn capabilities(&self) -> CapabilityTable {
        self.capabilities.clone()
    }

    pub fn paths(&self) -> EnginePaths {
        EnginePaths {
            slots: self.locations.clone(),
        }
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        EngineSnapshot {
            capabilities: self.capabilities(),
            paths: self.paths(),
        }
    }

    pub fn engine(&self, id: EngineId) -> Option<&dyn Engine> {
        self.engines
            .iter()
            .find(|(engine, _)| *engine == id)
            .map(|(_, engine)| engine.as_ref())
    }

    /// Re-probes changed executables in place. Called when the Convert card
    /// opens, so installing an engine without restarting is picked up.
    pub fn refresh(&mut self) {
        let mut rebuilt = self.engines.len() != EngineId::ALL.len();

        for id in EngineId::ALL {
            let index = id.index();
            if id == EngineId::Native {
                if !self.capabilities.installed(EngineId::Native) {
                    self.capabilities.set(id, probe::native_capabilities());
                    rebuilt = true;
                }
                continue;
            }

            let location = probe::locate_engine(&self.dir, id);
            let fingerprint = location
                .as_ref()
                .and_then(|path| probe::fingerprint(path.as_path()));
            if location == self.locations[index] && fingerprint == self.fingerprints[index] {
                continue;
            }

            self.locations[index] = location.clone();
            self.fingerprints[index] = fingerprint;
            let capabilities = match &location {
                Some(exe) => probe::probe_external(id, exe).unwrap_or_default(),
                None => EngineCapabilities::default(),
            };
            self.capabilities.set(id, capabilities);
            rebuilt = true;
        }

        if rebuilt {
            let paths = self.paths();
            self.engines = EngineId::ALL
                .iter()
                .map(|id| {
                    let engine: Box<dyn Engine> = match id {
                        EngineId::Native => Box::new(probe::NativeEngine),
                        _ => Box::new(
                            probe::ExternalEngine::new(*id, self.locations[id.index()].clone())
                                .with_paths(paths.clone()),
                        ),
                    };
                    (*id, engine)
                })
                .collect();
        }
    }
}

static SHARED: OnceLock<Mutex<EngineSet>> = OnceLock::new();

/// Probes on a worker so the pill never waits for FFmpeg's DLLs to load. The
/// thread priority is lowered so conversion tools cannot hitch the taskbar.
pub fn start_background_probe() {
    let _ = std::thread::Builder::new()
        .name("taskbar-island-engines".into())
        .spawn(|| {
            probe::lower_thread_priority();
            let set = EngineSet::load(engines_dir());
            let _ = SHARED.set(Mutex::new(set));
        });
}

/// The startup capability table, re-probed if an executable changed.
pub fn shared() -> Option<&'static Mutex<EngineSet>> {
    SHARED.get()
}

pub fn refresh_shared() {
    if let Some(shared) = shared() {
        if let Ok(mut set) = shared.lock() {
            set.refresh();
        }
    }
}

/// Dev builds read `TBI_ENGINES_DIR`; installed builds read the `engines`
/// folder next to the executable. When that folder does not exist (a test
/// binary under `target\`), the dev/test fallback from `docs/engines.md`
/// applies: `%LOCALAPPDATA%\Filee\engines`. This is the only resolver.
pub fn engines_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(ENGINES_ENV) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.join("engines")));
    match beside_exe {
        Some(dir) if dir.is_dir() => dir,
        // Dev / test fallback documented in docs/engines.md; absent on any
        // machine that never installed the engine payload.
        _ => std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|local| local.join("Filee").join("engines"))
            .filter(|dir| dir.is_dir())
            .or(beside_exe)
            .unwrap_or_else(|| PathBuf::from("engines")),
    }
}

/// A dropped file name is always one argv item. A leading dash is prefixed so
/// no engine can mistake a path for an option.
pub fn argv_path(path: &std::path::Path) -> std::ffi::OsString {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let text = path.as_os_str().to_string_lossy();
    if !text.starts_with('-') {
        return path.as_os_str().to_os_string();
    }
    let mut wide: Vec<u16> = ".\\".encode_utf16().collect();
    wide.extend(text.encode_utf16());
    OsString::from_wide(&wide)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_dash_paths_become_relative_argv_items() {
        let path = std::path::Path::new("-rf.png");
        assert_eq!(argv_path(path).to_string_lossy(), ".\\-rf.png");
        let normal = std::path::Path::new(r"C:\pics\photo.png");
        assert_eq!(argv_path(normal).to_string_lossy(), r"C:\pics\photo.png");
    }

    /// §9.4 hostile names: spaces, both quote styles, shell metacharacters,
    /// a 300-character name and non-ASCII text all stay exactly one argv item;
    /// only a leading dash is transformed (`.\\` prefix), never split.
    #[test]
    fn hostile_file_names_stay_single_argv_items() {
        let long = format!("{}.png", "x".repeat(300));
        for name in [
            "-rf --recursive.png",
            "two words and \"double\" quotes.png",
            "single 'quotes' here.png",
            long.as_str(),
            "नमस्ते फ़ाइल.png",
            "emoji-🎨-file.png",
            "semi;colon&amp&paren().png",
        ] {
            let argv = argv_path(std::path::Path::new(name));
            if name.starts_with('-') {
                assert!(
                    argv.to_string_lossy().starts_with(".\\"),
                    "{name:?} became {argv:?}"
                );
            } else {
                assert_eq!(
                    argv,
                    std::path::Path::new(name).as_os_str(),
                    "{name:?} must pass through unchanged"
                );
            }
            // One item: no embedded NUL can split it, and the whole name is
            // still present after the transformation.
            let text = argv.to_string_lossy();
            assert!(!text.contains('\0'), "{name:?}");
            assert!(text.contains(name), "{name:?} was mangled into {text:?}");
        }
    }

    #[test]
    fn engine_names_only_expose_external_tools() {
        assert_eq!(EngineId::Native.name(), None);
        assert_eq!(EngineId::Ffmpeg.name(), Some("FFmpeg"));
        assert_eq!(EngineId::LibreOffice.name(), Some("LibreOffice"));
        assert_eq!(EngineId::Poppler.name(), Some("Poppler"));
        assert_eq!(EngineId::Qpdf.name(), Some("qpdf"));
        assert_eq!(EngineId::Ghostscript.name(), Some("Ghostscript"));
    }

    #[test]
    fn every_engine_has_a_unique_slot() {
        let mut seen = [false; 8];
        for id in EngineId::ALL {
            let index = id.index();
            assert!(!seen[index], "{id:?} shares slot {index}");
            seen[index] = true;
        }
        assert!(seen.iter().all(|filled| *filled));
    }

    #[test]
    fn engine_stages_map_to_the_three_card_dots() {
        assert_eq!(Stage::Reading.index(), 0);
        assert_eq!(Stage::Converting.index(), 1);
        assert_eq!(Stage::Saving.index(), 2);
        assert_eq!(Stage::Done.index(), 2);
    }
}
