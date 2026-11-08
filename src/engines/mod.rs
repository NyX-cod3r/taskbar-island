//! engines — local executables and the instant, static conversion router.
//!
//! Nothing here touches the network. External engines are only ever started
//! from a probed, local path; the router is a pure table lookup over the
//! capability matrix built at startup, so choosing an engine never waits on a
//! benchmark or a process spawn.

pub mod exec;
pub mod ffmpeg;
pub mod output;
pub mod pandoc;
pub mod probe;
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
}

impl EngineId {
    pub const ALL: [EngineId; 4] = [
        EngineId::Native,
        EngineId::Ffmpeg,
        EngineId::Pandoc,
        EngineId::Tesseract,
    ];

    pub const fn index(self) -> usize {
        match self {
            EngineId::Native => 0,
            EngineId::Ffmpeg => 1,
            EngineId::Pandoc => 2,
            EngineId::Tesseract => 3,
        }
    }

    /// Tooltip suffix for externally executed engines; Native stays silent.
    pub const fn name(self) -> Option<&'static str> {
        match self {
            EngineId::Native => None,
            EngineId::Ffmpeg => Some("FFmpeg"),
            EngineId::Pandoc => Some("Pandoc"),
            EngineId::Tesseract => Some("Tesseract"),
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
}

impl DocumentFormat {
    pub const ALL: [DocumentFormat; 8] = [
        DocumentFormat::Docx,
        DocumentFormat::Odt,
        DocumentFormat::Markdown,
        DocumentFormat::Html,
        DocumentFormat::Txt,
        DocumentFormat::Epub,
        DocumentFormat::Rtf,
        DocumentFormat::Tex,
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
    slots: [EngineCapabilities; 4],
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

#[derive(Debug, Clone)]
pub struct JobOutput {
    pub output: PathBuf,
    /// The container that was written, when the job produces one file.
    pub format: Option<TargetFormat>,
    /// OCR-to-clipboard results; every other job leaves this `None`.
    pub text: Option<String>,
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
    slots: [Option<PathBuf>; 4],
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
pub fn run_external(
    id: EngineId,
    exe: &std::path::Path,
    helper: Option<&std::path::Path>,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    match id {
        EngineId::Ffmpeg => ffmpeg::run(exe, job, progress, cancel),
        EngineId::Pandoc => pandoc::run(exe, job, progress, cancel),
        EngineId::Tesseract => tesseract::run(exe, helper, job, progress, cancel),
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
    locations: [Option<PathBuf>; 4],
    fingerprints: [Option<probe::Fingerprint>; 4],
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
            let pandoc = self.locations[EngineId::Pandoc.index()].clone();
            self.engines = EngineId::ALL
                .iter()
                .map(|id| {
                    let engine: Box<dyn Engine> = match id {
                        EngineId::Native => Box::new(probe::NativeEngine),
                        EngineId::Tesseract => Box::new(
                            probe::ExternalEngine::new(*id, self.locations[id.index()].clone())
                                .with_helper(pandoc.clone()),
                        ),
                        _ => Box::new(probe::ExternalEngine::new(
                            *id,
                            self.locations[id.index()].clone(),
                        )),
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
/// folder next to the executable. This is the only resolver.
pub fn engines_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(ENGINES_ENV) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.join("engines")))
        .unwrap_or_else(|| PathBuf::from("engines"))
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

    #[test]
    fn engine_names_only_expose_external_tools() {
        assert_eq!(EngineId::Native.name(), None);
        assert_eq!(EngineId::Ffmpeg.name(), Some("FFmpeg"));
    }

    #[test]
    fn engine_stages_map_to_the_three_card_dots() {
        assert_eq!(Stage::Reading.index(), 0);
        assert_eq!(Stage::Converting.index(), 1);
        assert_eq!(Stage::Saving.index(), 2);
        assert_eq!(Stage::Done.index(), 2);
    }
}
