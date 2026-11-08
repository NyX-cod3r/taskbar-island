//! options.rs — the Convert card's single, data-driven option table.
//!
//! The card never invents a tile in drawing code: everything it can offer is
//! listed here as `FileKind -> Tab -> Option`, and the router decides which of
//! those jobs an installed engine can actually execute. Adding an option later
//! means adding one row to `TABLE`.

use crate::engines::router::route;
use crate::engines::{
    Action, AudioFormat, AudioOptimize, CapabilityTable, DocumentExtract, DocumentFormat, EngineId,
    FileKind, ImageEdit, ImageFormat, ImageOptimize, OcrOutput, VideoEdit, VideoExtract,
    VideoFormat, VideoOptimize,
};
use crate::view::IconId;

/// The only source identity the option filter needs. Probes map their exact
/// container into one of these so a format is never offered for a file that
/// already is that format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Image(ImageFormat),
    Video(VideoFormat),
    Audio(AudioFormat),
    Document(DocumentFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    ConvertTo,
    Optimize,
    Edit,
    Extract,
    ExtractText,
}

impl Tab {
    pub const fn label(self) -> &'static str {
        match self {
            Tab::ConvertTo => "Convert to",
            Tab::Optimize => "Optimize",
            Tab::Edit => "Edit",
            Tab::Extract => "Extract",
            Tab::ExtractText => "Extract text",
        }
    }

    pub const fn icon(self) -> IconId {
        match self {
            Tab::ConvertTo => IconId::Convert,
            Tab::Optimize => IconId::Compress,
            Tab::Edit => IconId::Edit,
            Tab::Extract => IconId::Extract,
            Tab::ExtractText => IconId::TextLines,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OptionDef {
    pub kind: FileKind,
    pub tab: Tab,
    /// `None` is a picker (OCR language), not a job.
    pub action: Option<Action>,
    /// Format text on a format tile; actions draw `icon` instead.
    pub format: Option<&'static str>,
    pub icon: IconId,
    pub label: &'static str,
}

const fn convert(
    kind: FileKind,
    action: Action,
    format: &'static str,
    label: &'static str,
) -> OptionDef {
    OptionDef {
        kind,
        tab: Tab::ConvertTo,
        action: Some(action),
        format: Some(format),
        icon: IconId::File,
        label,
    }
}

const fn action(
    kind: FileKind,
    tab: Tab,
    action: Action,
    icon: IconId,
    label: &'static str,
) -> OptionDef {
    OptionDef {
        kind,
        tab,
        action: Some(action),
        format: None,
        icon,
        label,
    }
}

/// Every tile the Convert card can ever show, in display order. The order is
/// significant: the grid fills five columns left to right, two rows.
pub const TABLE: &[OptionDef] = &[
    // ------------------------------------------------------------- images
    convert(
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Png),
        "PNG",
        "Convert to PNG",
    ),
    convert(
        FileKind::Image,
        Action::ImageConvert(ImageFormat::WebP),
        "WebP",
        "Convert to WebP",
    ),
    convert(
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Avif),
        "AVIF",
        "Convert to AVIF",
    ),
    convert(
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Gif),
        "GIF",
        "Convert to GIF",
    ),
    convert(
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Bmp),
        "BMP",
        "Convert to BMP",
    ),
    convert(
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Tiff),
        "TIFF",
        "Convert to TIFF",
    ),
    convert(
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Ico),
        "ICO",
        "Convert to ICO",
    ),
    convert(
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Pdf),
        "PDF",
        "Convert to PDF",
    ),
    convert(
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Jpeg),
        "JPG",
        "Convert to JPG",
    ),
    action(
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::Compress),
        IconId::Compress,
        "Compress",
    ),
    action(
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::ResizePercent(75)),
        IconId::Resize,
        "Resize to 75%",
    ),
    action(
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::ResizePercent(50)),
        IconId::Resize,
        "Resize to 50%",
    ),
    action(
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::ResizePercent(25)),
        IconId::Resize,
        "Resize to 25%",
    ),
    action(
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::Fit(1080)),
        IconId::Fit,
        "Fit to 1080p",
    ),
    action(
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::Thumbnail(256)),
        IconId::Thumbnail,
        "Thumbnail (256 px)",
    ),
    action(
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::StripMetadata),
        IconId::EyeOff,
        "Remove metadata",
    ),
    action(
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::WebReady),
        IconId::Web,
        "Web-ready WebP",
    ),
    action(
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::RotateLeft),
        IconId::RotateLeft,
        "Rotate left",
    ),
    action(
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::RotateRight),
        IconId::RotateRight,
        "Rotate right",
    ),
    action(
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::FlipHorizontal),
        IconId::FlipHorizontal,
        "Flip horizontal",
    ),
    action(
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::FlipVertical),
        IconId::FlipVertical,
        "Flip vertical",
    ),
    action(
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::Grayscale),
        IconId::Grayscale,
        "Grayscale",
    ),
    action(
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::BlackWhite),
        IconId::Contrast,
        "Black & white",
    ),
    action(
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::CropSquare),
        IconId::Crop,
        "Crop to square",
    ),
    action(
        FileKind::Image,
        Tab::ExtractText,
        Action::ImageOcr(OcrOutput::Clipboard),
        IconId::Copy,
        "Copy text",
    ),
    action(
        FileKind::Image,
        Tab::ExtractText,
        Action::ImageOcr(OcrOutput::Txt),
        IconId::File,
        "Save as TXT",
    ),
    action(
        FileKind::Image,
        Tab::ExtractText,
        Action::ImageOcr(OcrOutput::Markdown),
        IconId::TextLines,
        "Save as Markdown",
    ),
    action(
        FileKind::Image,
        Tab::ExtractText,
        Action::ImageOcr(OcrOutput::Docx),
        IconId::File,
        "Save as DOCX",
    ),
    action(
        FileKind::Image,
        Tab::ExtractText,
        Action::ImageOcr(OcrOutput::SearchablePdf),
        IconId::File,
        "Searchable PDF",
    ),
    OptionDef {
        kind: FileKind::Image,
        tab: Tab::ExtractText,
        action: None,
        format: None,
        icon: IconId::TextLines,
        label: "OCR language",
    },
    // -------------------------------------------------------------- video
    convert(
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Mp4),
        "MP4",
        "Convert to MP4",
    ),
    convert(
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Mkv),
        "MKV",
        "Convert to MKV",
    ),
    convert(
        FileKind::Video,
        Action::VideoConvert(VideoFormat::WebM),
        "WebM",
        "Convert to WebM",
    ),
    convert(
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Mov),
        "MOV",
        "Convert to MOV",
    ),
    convert(
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Avi),
        "AVI",
        "Convert to AVI",
    ),
    convert(
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Gif),
        "GIF",
        "Convert to GIF",
    ),
    convert(
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Wmv),
        "WMV",
        "Convert to WMV",
    ),
    action(
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Compress),
        IconId::Compress,
        "Compress",
    ),
    action(
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Height(1080)),
        IconId::Resize,
        "Resize to 1080p",
    ),
    action(
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Height(720)),
        IconId::Resize,
        "Resize to 720p",
    ),
    action(
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Height(480)),
        IconId::Resize,
        "Resize to 480p",
    ),
    action(
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Mute),
        IconId::SpeakerMuted,
        "Remove audio",
    ),
    action(
        FileKind::Video,
        Tab::Edit,
        Action::VideoEdit(VideoEdit::RotateLeft),
        IconId::RotateLeft,
        "Rotate left",
    ),
    action(
        FileKind::Video,
        Tab::Edit,
        Action::VideoEdit(VideoEdit::RotateRight),
        IconId::RotateRight,
        "Rotate right",
    ),
    action(
        FileKind::Video,
        Tab::Edit,
        Action::VideoEdit(VideoEdit::Speed(crate::engines::Speed::Half)),
        IconId::Speed,
        "Speed 0.5x",
    ),
    action(
        FileKind::Video,
        Tab::Edit,
        Action::VideoEdit(VideoEdit::Speed(crate::engines::Speed::OneAndHalf)),
        IconId::Speed,
        "Speed 1.5x",
    ),
    action(
        FileKind::Video,
        Tab::Edit,
        Action::VideoEdit(VideoEdit::Speed(crate::engines::Speed::Double)),
        IconId::Speed,
        "Speed 2x",
    ),
    action(
        FileKind::Video,
        Tab::Extract,
        Action::VideoExtract(VideoExtract::Audio(AudioFormat::Mp3)),
        IconId::Music,
        "Audio MP3",
    ),
    action(
        FileKind::Video,
        Tab::Extract,
        Action::VideoExtract(VideoExtract::Audio(AudioFormat::Wav)),
        IconId::Music,
        "Audio WAV",
    ),
    action(
        FileKind::Video,
        Tab::Extract,
        Action::VideoExtract(VideoExtract::Audio(AudioFormat::M4a)),
        IconId::Music,
        "Audio M4A",
    ),
    action(
        FileKind::Video,
        Tab::Extract,
        Action::VideoExtract(VideoExtract::Frame),
        IconId::Image,
        "Frame at midpoint",
    ),
    action(
        FileKind::Video,
        Tab::Extract,
        Action::VideoExtract(VideoExtract::Gif),
        IconId::Video,
        "GIF (first 5 s)",
    ),
    // -------------------------------------------------------------- audio
    convert(
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Mp3),
        "MP3",
        "Convert to MP3",
    ),
    convert(
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Wav),
        "WAV",
        "Convert to WAV",
    ),
    convert(
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Flac),
        "FLAC",
        "Convert to FLAC",
    ),
    convert(
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::M4a),
        "M4A",
        "Convert to M4A",
    ),
    convert(
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Ogg),
        "OGG",
        "Convert to OGG",
    ),
    convert(
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Opus),
        "OPUS",
        "Convert to OPUS",
    ),
    convert(
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Aiff),
        "AIFF",
        "Convert to AIFF",
    ),
    convert(
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Wma),
        "WMA",
        "Convert to WMA",
    ),
    action(
        FileKind::Audio,
        Tab::Optimize,
        Action::AudioOptimize(AudioOptimize::SmallerBitrate),
        IconId::Compress,
        "Smaller bitrate",
    ),
    action(
        FileKind::Audio,
        Tab::Optimize,
        Action::AudioOptimize(AudioOptimize::Mono),
        IconId::Signal,
        "Mono",
    ),
    action(
        FileKind::Audio,
        Tab::Optimize,
        Action::AudioOptimize(AudioOptimize::Normalize),
        IconId::Signal,
        "Normalize loudness",
    ),
    action(
        FileKind::Audio,
        Tab::Optimize,
        Action::AudioOptimize(AudioOptimize::TrimSilence),
        IconId::Crop,
        "Trim silence",
    ),
    // ---------------------------------------------------------- documents
    convert(
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Markdown),
        "MD",
        "Convert to Markdown",
    ),
    convert(
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Html),
        "HTML",
        "Convert to HTML",
    ),
    convert(
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Txt),
        "TXT",
        "Convert to TXT",
    ),
    convert(
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Epub),
        "EPUB",
        "Convert to EPUB",
    ),
    convert(
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Odt),
        "ODT",
        "Convert to ODT",
    ),
    convert(
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Rtf),
        "RTF",
        "Convert to RTF",
    ),
    convert(
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Tex),
        "TEX",
        "Convert to TEX",
    ),
    convert(
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Docx),
        "DOCX",
        "Convert to DOCX",
    ),
    action(
        FileKind::Document,
        Tab::Extract,
        Action::DocumentExtract(DocumentExtract::PlainText),
        IconId::TextLines,
        "Plain text",
    ),
    action(
        FileKind::Document,
        Tab::Extract,
        Action::DocumentExtract(DocumentExtract::Images),
        IconId::Image,
        "Embedded images",
    ),
];

/// Whether the selected sources already contain this output format. The
/// format is then omitted, exactly as the artboards show.
pub fn is_own_format(action: &Action, sources: &[SourceKind]) -> bool {
    match *action {
        Action::ImageConvert(target) => sources.contains(&SourceKind::Image(target)),
        Action::VideoConvert(target) => sources.contains(&SourceKind::Video(target)),
        Action::AudioConvert(target) => sources.contains(&SourceKind::Audio(target)),
        Action::DocumentConvert(target) => sources.contains(&SourceKind::Document(target)),
        _ => false,
    }
}

/// True when an installed engine has a runner for this job right now.
pub fn action_available(kind: FileKind, action: Action, caps: &CapabilityTable) -> bool {
    route(kind, action, caps)
        .iter()
        .any(|candidate| candidate.engine.job_ready())
}

fn option_available(kind: FileKind, def: &OptionDef, caps: &CapabilityTable) -> bool {
    match def.action {
        Some(action) => action_available(kind, action, caps),
        // The language picker belongs to the OCR tab and needs Tesseract
        // installed and runnable.
        None => caps.installed(EngineId::Tesseract) && EngineId::Tesseract.job_ready(),
    }
}

fn visible(
    def: &OptionDef,
    kind: FileKind,
    caps: &CapabilityTable,
    sources: &[SourceKind],
) -> bool {
    def.kind == kind
        && option_available(kind, def, caps)
        && !def
            .action
            .is_some_and(|action| is_own_format(&action, sources))
}

/// Tabs with at least one visible option, in table order.
pub fn tabs(kind: FileKind, caps: &CapabilityTable, sources: &[SourceKind]) -> Vec<Tab> {
    let mut tabs: Vec<Tab> = Vec::new();
    for def in TABLE {
        if visible(def, kind, caps, sources) && !tabs.contains(&def.tab) {
            tabs.push(def.tab);
        }
    }
    tabs
}

/// Visible options for one tab, in table order.
pub fn options(
    kind: FileKind,
    tab: Tab,
    caps: &CapabilityTable,
    sources: &[SourceKind],
) -> Vec<&'static OptionDef> {
    TABLE
        .iter()
        .filter(|def| def.tab == tab && visible(def, kind, caps, sources))
        .collect()
}

/// The static tooltip title for an action, used by the tile tips and the
/// accessibility names.
pub fn label_for(action: Action) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|def| def.action == Some(action))
        .map(|def| def.label)
}

/// "via FFmpeg" for the tip's muted second line. Native stays silent, and so
/// does a job whose only candidate is not installed.
pub fn engine_hint(kind: FileKind, action: Action, caps: &CapabilityTable) -> Option<String> {
    let candidate = route(kind, action, caps).into_iter().next()?;
    candidate.engine.name().map(|name| format!("via {name}"))
}

/// Why an option is not offered; tests use it to pin the table to the router.
#[cfg(test)]
pub fn routable(kind: FileKind, action: Action, caps: &CapabilityTable) -> bool {
    !route(kind, action, caps).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::probe::native_capabilities;
    use crate::engines::{AudioFormat, DocumentFormat, EngineCapabilities, ImageFormat};

    /// Every engine installed with its full matrix: the definitions exist.
    fn full_caps() -> CapabilityTable {
        let mut table = CapabilityTable::default();
        table.set(EngineId::Native, native_capabilities());
        table.set(
            EngineId::Ffmpeg,
            EngineCapabilities {
                installed: true,
                image_read: vec![
                    ImageFormat::Png,
                    ImageFormat::Jpeg,
                    ImageFormat::WebP,
                    ImageFormat::Gif,
                    ImageFormat::Bmp,
                    ImageFormat::Tiff,
                    ImageFormat::Avif,
                ],
                image_write: vec![
                    ImageFormat::Png,
                    ImageFormat::Jpeg,
                    ImageFormat::WebP,
                    ImageFormat::Gif,
                    ImageFormat::Bmp,
                    ImageFormat::Tiff,
                    ImageFormat::Avif,
                ],
                image_edit: true,
                video_write: vec![
                    VideoFormat::Mp4,
                    VideoFormat::Mkv,
                    VideoFormat::WebM,
                    VideoFormat::Mov,
                    VideoFormat::Avi,
                    VideoFormat::Wmv,
                    VideoFormat::Gif,
                ],
                audio_write: vec![
                    AudioFormat::Mp3,
                    AudioFormat::Wav,
                    AudioFormat::Flac,
                    AudioFormat::M4a,
                    AudioFormat::Ogg,
                    AudioFormat::Opus,
                    AudioFormat::Aac,
                    AudioFormat::Wma,
                    AudioFormat::Aiff,
                ],
                ..EngineCapabilities::default()
            },
        );
        table.set(
            EngineId::Pandoc,
            EngineCapabilities {
                installed: true,
                documents_in: DocumentFormat::ALL.to_vec(),
                documents_out: DocumentFormat::ALL.to_vec(),
                ..EngineCapabilities::default()
            },
        );
        table.set(
            EngineId::Tesseract,
            EngineCapabilities {
                installed: true,
                ocr_languages: vec!["eng".into()],
                ..EngineCapabilities::default()
            },
        );
        table
    }

    #[test]
    fn every_option_is_routable_and_no_tab_exceeds_ten() {
        let caps = full_caps();
        for def in TABLE {
            let Some(action) = def.action else {
                continue;
            };
            assert!(
                routable(def.kind, action, &caps),
                "{} has no engine definition",
                def.label
            );
        }
        for kind in [
            FileKind::Image,
            FileKind::Video,
            FileKind::Audio,
            FileKind::Document,
        ] {
            for tab in [
                Tab::ConvertTo,
                Tab::Optimize,
                Tab::Edit,
                Tab::Extract,
                Tab::ExtractText,
            ] {
                let count = TABLE
                    .iter()
                    .filter(|def| def.kind == kind && def.tab == tab)
                    .count();
                assert!(count <= 10, "{kind:?}/{tab:?} has {count} options");
            }
        }
    }

    #[test]
    fn image_tabs_follow_the_artboard() {
        let caps = full_caps();
        let image = [SourceKind::Image(ImageFormat::Png)];
        assert_eq!(
            tabs(FileKind::Image, &caps, &image),
            vec![Tab::ConvertTo, Tab::Optimize, Tab::Edit, Tab::ExtractText]
        );
        // The table's full ordering (all engines), then what the installed
        // engines can actually run: PNG is the source, everything else is
        // available because FFmpeg covers AVIF and TIFF.
        let order: Vec<_> = TABLE
            .iter()
            .filter(|def| def.kind == FileKind::Image && def.tab == Tab::ConvertTo)
            .map(|def| def.format.unwrap())
            .collect();
        assert_eq!(
            order,
            vec!["PNG", "WebP", "AVIF", "GIF", "BMP", "TIFF", "ICO", "PDF", "JPG"]
        );
        let convert: Vec<_> = options(FileKind::Image, Tab::ConvertTo, &caps, &image)
            .iter()
            .map(|def| def.format.unwrap())
            .collect();
        assert_eq!(
            convert,
            vec!["WebP", "AVIF", "GIF", "BMP", "TIFF", "ICO", "PDF", "JPG"]
        );
        assert_eq!(
            options(FileKind::Image, Tab::Optimize, &caps, &image).len(),
            8
        );
        assert_eq!(options(FileKind::Image, Tab::Edit, &caps, &image).len(), 7);
        assert_eq!(
            options(FileKind::Image, Tab::ExtractText, &caps, &image).len(),
            6
        );
    }

    #[test]
    fn the_source_format_is_never_offered_again() {
        let caps = full_caps();
        let jpeg = [SourceKind::Image(ImageFormat::Jpeg)];
        let convert = options(FileKind::Image, Tab::ConvertTo, &caps, &jpeg);
        assert!(convert.iter().all(|def| def.format != Some("JPG")));
        assert_eq!(convert.first().unwrap().format, Some("PNG"));

        // A mixed selection also omits a format that any file already has.
        let mixed = [
            SourceKind::Image(ImageFormat::Jpeg),
            SourceKind::Image(ImageFormat::WebP),
        ];
        let convert = options(FileKind::Image, Tab::ConvertTo, &caps, &mixed);
        assert!(convert.iter().all(|def| def.format != Some("JPG")));
        assert!(convert.iter().all(|def| def.format != Some("WebP")));
    }

    #[test]
    fn installed_engines_expand_the_offered_jobs() {
        let caps = full_caps();
        let png = [SourceKind::Image(ImageFormat::Png)];
        let formats: Vec<_> = options(FileKind::Image, Tab::ConvertTo, &caps, &png)
            .iter()
            .filter_map(|def| def.format)
            .collect();
        assert!(formats.contains(&"PDF"));
        assert!(formats.contains(&"AVIF"));
        assert!(formats.contains(&"TIFF"));
        assert_eq!(
            options(FileKind::Image, Tab::ExtractText, &caps, &png).len(),
            6
        );
        assert!(!tabs(FileKind::Video, &caps, &[]).is_empty());
        assert!(!tabs(FileKind::Audio, &caps, &[]).is_empty());
        assert!(!tabs(FileKind::Document, &caps, &[]).is_empty());
    }

    #[test]
    fn missing_engines_hide_their_jobs() {
        let mut caps = CapabilityTable::default();
        caps.set(EngineId::Native, native_capabilities());
        let png = [SourceKind::Image(ImageFormat::Png)];
        let formats: Vec<_> = options(FileKind::Image, Tab::ConvertTo, &caps, &png)
            .iter()
            .filter_map(|def| def.format)
            .collect();
        assert!(formats.contains(&"PDF"));
        assert!(!formats.contains(&"AVIF"));
        assert!(!formats.contains(&"TIFF"));
        assert!(options(FileKind::Image, Tab::ExtractText, &caps, &png).is_empty());
        assert!(tabs(FileKind::Video, &caps, &[]).is_empty());
        assert!(tabs(FileKind::Audio, &caps, &[]).is_empty());
        assert!(tabs(FileKind::Document, &caps, &[]).is_empty());
    }

    #[test]
    fn unsupported_kinds_offer_nothing() {
        let caps = full_caps();
        assert!(tabs(FileKind::Other, &caps, &[]).is_empty());
        assert!(options(FileKind::Other, Tab::ConvertTo, &caps, &[]).is_empty());
    }

    #[test]
    fn every_tile_has_a_tooltip_label() {
        for def in TABLE {
            assert!(!def.label.is_empty());
            if let Some(action) = def.action {
                assert_eq!(label_for(action), Some(def.label));
            }
        }
    }
}
