//! options.rs — the Convert card's single, data-driven option table.
//!
//! The card never invents a tile in drawing code: everything it can offer is
//! listed here as `FileKind -> Tab -> Option`, and the router decides which of
//! those jobs an installed engine can actually execute. Adding an option later
//! means adding one row to `TABLE`.

use crate::engines::router::route;
use crate::engines::{
    Action, AudioFormat, AudioOptimize, CapabilityTable, DocumentExtract, DocumentFormat, EngineId,
    FileKind, ImageEdit, ImageFormat, ImageOptimize, MultiOp, OcrOutput, PdfExtract, PdfTarget,
    PdfTool, PresentFormat, SheetFormat, VideoEdit, VideoExtract, VideoFormat, VideoOptimize,
    ZipOp,
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
    /// A PDF file — its own card (`FileKind::Pdf`).
    Pdf,
    /// A spreadsheet source (XLSX, XLS, ODS, CSV, TSV).
    Sheet(SheetFormat),
    /// A presentation source (PPTX, PPT, ODP).
    Present(PresentFormat),
    /// A ZIP archive (the special-cased Extract card).
    Zip,
}

impl SourceKind {
    /// The option-filter identity of a probed container; `None` when the
    /// format has no Convert-card representation (unknown, SVG, HEIC — the
    /// last one still opens the Image card through `ProbeKind::Image`).
    pub fn from_source(format: crate::convert::detect::SourceFormat) -> Option<SourceKind> {
        use crate::convert::detect::SourceFormat as F;
        match format {
            F::Pdf => Some(SourceKind::Pdf),
            F::Zip => Some(SourceKind::Zip),
            F::Xlsx | F::Xls | F::Ods | F::Csv | F::Tsv => {
                format.sheet_format().map(SourceKind::Sheet)
            }
            F::Pptx | F::Ppt | F::Odp => format.present_format().map(SourceKind::Present),
            other => other
                .image_format()
                .map(SourceKind::Image)
                .or_else(|| other.video_format().map(SourceKind::Video))
                .or_else(|| other.audio_format().map(SourceKind::Audio))
                .or_else(|| other.document_format().map(SourceKind::Document)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    ConvertTo,
    Optimize,
    Edit,
    Extract,
    ExtractText,
    /// The PDF card's tools tab (label exactly `PDF tools`, §9).
    PdfTools,
    /// The several-files card's tab (label exactly `Combine`, §9).
    Combine,
}

impl Tab {
    pub const fn label(self) -> &'static str {
        match self {
            Tab::ConvertTo => "Convert to",
            Tab::Optimize => "Optimize",
            Tab::Edit => "Edit",
            Tab::Extract => "Extract",
            Tab::ExtractText => "Extract text",
            Tab::PdfTools => "PDF tools",
            Tab::Combine => "Combine",
        }
    }

    pub const fn icon(self) -> IconId {
        match self {
            Tab::ConvertTo => IconId::Convert,
            Tab::Optimize => IconId::Compress,
            Tab::Edit => IconId::Edit,
            Tab::Extract => IconId::Extract,
            Tab::ExtractText => IconId::TextLines,
            // No artboard keys for these two yet (§7.2): reuse the closest
            // existing glyphs until the artboards arrive.
            Tab::PdfTools => IconId::Compress,
            Tab::Combine => IconId::Plus,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OptionDef {
    /// Stable kebab-case identity (UIA name + tests); unique across TABLE.
    pub id: &'static str,
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
    id: &'static str,
    kind: FileKind,
    action: Action,
    format: &'static str,
    label: &'static str,
) -> OptionDef {
    OptionDef {
        id,
        kind,
        tab: Tab::ConvertTo,
        action: Some(action),
        format: Some(format),
        icon: IconId::File,
        label,
    }
}

const fn action(
    id: &'static str,
    kind: FileKind,
    tab: Tab,
    action: Action,
    icon: IconId,
    label: &'static str,
) -> OptionDef {
    OptionDef {
        id,
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
        "img-png",
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Png),
        "PNG",
        "Convert to PNG",
    ),
    convert(
        "img-jpg",
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Jpeg),
        "JPG",
        "Convert to JPG",
    ),
    convert(
        "img-webp",
        FileKind::Image,
        Action::ImageConvert(ImageFormat::WebP),
        "WebP",
        "Convert to WebP",
    ),
    convert(
        "img-avif",
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Avif),
        "AVIF",
        "Convert to AVIF",
    ),
    convert(
        "img-gif",
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Gif),
        "GIF",
        "Convert to GIF",
    ),
    convert(
        "img-bmp",
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Bmp),
        "BMP",
        "Convert to BMP",
    ),
    convert(
        "img-tiff",
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Tiff),
        "TIFF",
        "Convert to TIFF",
    ),
    convert(
        "img-ico",
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Ico),
        "ICO",
        "Convert to ICO",
    ),
    convert(
        "img-pdf",
        FileKind::Image,
        Action::ImageConvert(ImageFormat::Pdf),
        "PDF",
        "Convert to PDF",
    ),
    // GIF source only (§2.1): `visible()` keys on this id.
    convert(
        "img-mp4",
        FileKind::Image,
        Action::VideoConvert(VideoFormat::Mp4),
        "MP4",
        "Convert to MP4",
    ),
    action(
        "img-opt-compress",
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::Compress),
        IconId::Compress,
        "Compress",
    ),
    action(
        "img-opt-resize-75",
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::ResizePercent(75)),
        IconId::Resize,
        "Resize to 75%",
    ),
    action(
        "img-opt-resize-50",
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::ResizePercent(50)),
        IconId::Resize,
        "Resize to 50%",
    ),
    action(
        "img-opt-resize-25",
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::ResizePercent(25)),
        IconId::Resize,
        "Resize to 25%",
    ),
    action(
        "img-opt-fit-1080",
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::Fit(1080)),
        IconId::Fit,
        "Fit to 1080p",
    ),
    action(
        "img-opt-thumb-256",
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::Thumbnail(256)),
        IconId::Thumbnail,
        "Thumbnail (256 px)",
    ),
    action(
        "img-opt-strip",
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::StripMetadata),
        IconId::EyeOff,
        "Remove metadata",
    ),
    action(
        "img-opt-webready",
        FileKind::Image,
        Tab::Optimize,
        Action::ImageOptimize(ImageOptimize::WebReady),
        IconId::Web,
        "Web-ready WebP",
    ),
    action(
        "img-edit-rotate-left",
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::RotateLeft),
        IconId::RotateLeft,
        "Rotate left",
    ),
    action(
        "img-edit-rotate-right",
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::RotateRight),
        IconId::RotateRight,
        "Rotate right",
    ),
    action(
        "img-edit-flip-h",
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::FlipHorizontal),
        IconId::FlipHorizontal,
        "Flip horizontal",
    ),
    action(
        "img-edit-flip-v",
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::FlipVertical),
        IconId::FlipVertical,
        "Flip vertical",
    ),
    action(
        "img-edit-grayscale",
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::Grayscale),
        IconId::Grayscale,
        "Grayscale",
    ),
    action(
        "img-edit-bw",
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::BlackWhite),
        IconId::Contrast,
        "Black & white",
    ),
    action(
        "img-edit-crop",
        FileKind::Image,
        Tab::Edit,
        Action::ImageEdit(ImageEdit::CropSquare),
        IconId::Crop,
        "Crop to square",
    ),
    action(
        "img-ocr-copy",
        FileKind::Image,
        Tab::ExtractText,
        Action::ImageOcr(OcrOutput::Clipboard),
        IconId::Copy,
        "Copy text",
    ),
    action(
        "img-ocr-txt",
        FileKind::Image,
        Tab::ExtractText,
        Action::ImageOcr(OcrOutput::Txt),
        IconId::File,
        "Save as TXT",
    ),
    action(
        "img-ocr-md",
        FileKind::Image,
        Tab::ExtractText,
        Action::ImageOcr(OcrOutput::Markdown),
        IconId::TextLines,
        "Save as Markdown",
    ),
    action(
        "img-ocr-docx",
        FileKind::Image,
        Tab::ExtractText,
        Action::ImageOcr(OcrOutput::Docx),
        IconId::File,
        "Save as DOCX",
    ),
    action(
        "img-ocr-searchable",
        FileKind::Image,
        Tab::ExtractText,
        Action::ImageOcr(OcrOutput::SearchablePdf),
        IconId::File,
        "Searchable PDF",
    ),
    OptionDef {
        id: "img-ocr-language",
        kind: FileKind::Image,
        tab: Tab::ExtractText,
        action: None,
        format: None,
        icon: IconId::TextLines,
        label: "OCR language",
    },
    // -------------------------------------------------------------- video
    convert(
        "vid-mp4",
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Mp4),
        "MP4",
        "Convert to MP4",
    ),
    convert(
        "vid-mkv",
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Mkv),
        "MKV",
        "Convert to MKV",
    ),
    convert(
        "vid-webm",
        FileKind::Video,
        Action::VideoConvert(VideoFormat::WebM),
        "WebM",
        "Convert to WebM",
    ),
    convert(
        "vid-mov",
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Mov),
        "MOV",
        "Convert to MOV",
    ),
    convert(
        "vid-avi",
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Avi),
        "AVI",
        "Convert to AVI",
    ),
    convert(
        "vid-gif",
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Gif),
        "GIF",
        "Convert to GIF",
    ),
    convert(
        "vid-wmv",
        FileKind::Video,
        Action::VideoConvert(VideoFormat::Wmv),
        "WMV",
        "Convert to WMV",
    ),
    action(
        "vid-opt-compress",
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Compress),
        IconId::Compress,
        "Compress",
    ),
    action(
        "vid-opt-1080",
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Height(1080)),
        IconId::Resize,
        "Resize to 1080p",
    ),
    action(
        "vid-opt-720",
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Height(720)),
        IconId::Resize,
        "Resize to 720p",
    ),
    action(
        "vid-opt-480",
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Height(480)),
        IconId::Resize,
        "Resize to 480p",
    ),
    action(
        "vid-opt-mute",
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Mute),
        IconId::SpeakerMuted,
        "Remove audio",
    ),
    action(
        "vid-opt-remux",
        FileKind::Video,
        Tab::Optimize,
        Action::VideoOptimize(VideoOptimize::Remux),
        IconId::Convert,
        "Remux",
    ),
    action(
        "vid-edit-rotate-left",
        FileKind::Video,
        Tab::Edit,
        Action::VideoEdit(VideoEdit::RotateLeft),
        IconId::RotateLeft,
        "Rotate left",
    ),
    action(
        "vid-edit-rotate-right",
        FileKind::Video,
        Tab::Edit,
        Action::VideoEdit(VideoEdit::RotateRight),
        IconId::RotateRight,
        "Rotate right",
    ),
    action(
        "vid-edit-speed-05",
        FileKind::Video,
        Tab::Edit,
        Action::VideoEdit(VideoEdit::Speed(crate::engines::Speed::Half)),
        IconId::Speed,
        "Speed 0.5x",
    ),
    action(
        "vid-edit-speed-15",
        FileKind::Video,
        Tab::Edit,
        Action::VideoEdit(VideoEdit::Speed(crate::engines::Speed::OneAndHalf)),
        IconId::Speed,
        "Speed 1.5x",
    ),
    action(
        "vid-edit-speed-2",
        FileKind::Video,
        Tab::Edit,
        Action::VideoEdit(VideoEdit::Speed(crate::engines::Speed::Double)),
        IconId::Speed,
        "Speed 2x",
    ),
    action(
        "vid-extract-mp3",
        FileKind::Video,
        Tab::Extract,
        Action::VideoExtract(VideoExtract::Audio(AudioFormat::Mp3)),
        IconId::Music,
        "Audio MP3",
    ),
    action(
        "vid-extract-wav",
        FileKind::Video,
        Tab::Extract,
        Action::VideoExtract(VideoExtract::Audio(AudioFormat::Wav)),
        IconId::Music,
        "Audio WAV",
    ),
    action(
        "vid-extract-m4a",
        FileKind::Video,
        Tab::Extract,
        Action::VideoExtract(VideoExtract::Audio(AudioFormat::M4a)),
        IconId::Music,
        "Audio M4A",
    ),
    action(
        "vid-extract-frame",
        FileKind::Video,
        Tab::Extract,
        Action::VideoExtract(VideoExtract::Frame),
        IconId::Image,
        "Frame at midpoint",
    ),
    action(
        "vid-extract-gif",
        FileKind::Video,
        Tab::Extract,
        Action::VideoExtract(VideoExtract::Gif),
        IconId::Video,
        "GIF (first 5 s)",
    ),
    // -------------------------------------------------------------- audio
    convert(
        "aud-mp3",
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Mp3),
        "MP3",
        "Convert to MP3",
    ),
    convert(
        "aud-wav",
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Wav),
        "WAV",
        "Convert to WAV",
    ),
    convert(
        "aud-flac",
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Flac),
        "FLAC",
        "Convert to FLAC",
    ),
    convert(
        "aud-m4a",
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::M4a),
        "M4A",
        "Convert to M4A",
    ),
    convert(
        "aud-ogg",
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Ogg),
        "OGG",
        "Convert to OGG",
    ),
    convert(
        "aud-opus",
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Opus),
        "OPUS",
        "Convert to OPUS",
    ),
    convert(
        "aud-aiff",
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Aiff),
        "AIFF",
        "Convert to AIFF",
    ),
    convert(
        "aud-wma",
        FileKind::Audio,
        Action::AudioConvert(AudioFormat::Wma),
        "WMA",
        "Convert to WMA",
    ),
    action(
        "aud-opt-bitrate",
        FileKind::Audio,
        Tab::Optimize,
        Action::AudioOptimize(AudioOptimize::SmallerBitrate),
        IconId::Compress,
        "Smaller bitrate",
    ),
    action(
        "aud-opt-mono",
        FileKind::Audio,
        Tab::Optimize,
        Action::AudioOptimize(AudioOptimize::Mono),
        IconId::Signal,
        "Mono",
    ),
    action(
        "aud-opt-normalize",
        FileKind::Audio,
        Tab::Optimize,
        Action::AudioOptimize(AudioOptimize::Normalize),
        IconId::Signal,
        "Normalize loudness",
    ),
    action(
        "aud-opt-trim",
        FileKind::Audio,
        Tab::Optimize,
        Action::AudioOptimize(AudioOptimize::TrimSilence),
        IconId::Crop,
        "Trim silence",
    ),
    // ---------------------------------------------------------- documents
    convert(
        "doc-pdf",
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Pdf),
        "PDF",
        "Convert to PDF",
    ),
    convert(
        "doc-md",
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Markdown),
        "MD",
        "Convert to Markdown",
    ),
    convert(
        "doc-html",
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Html),
        "HTML",
        "Convert to HTML",
    ),
    convert(
        "doc-txt",
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Txt),
        "TXT",
        "Convert to TXT",
    ),
    convert(
        "doc-epub",
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Epub),
        "EPUB",
        "Convert to EPUB",
    ),
    convert(
        "doc-odt",
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Odt),
        "ODT",
        "Convert to ODT",
    ),
    convert(
        "doc-rtf",
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Rtf),
        "RTF",
        "Convert to RTF",
    ),
    convert(
        "doc-tex",
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Tex),
        "TEX",
        "Convert to TEX",
    ),
    convert(
        "doc-docx",
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Docx),
        "DOCX",
        "Convert to DOCX",
    ),
    convert(
        "doc-doc",
        FileKind::Document,
        Action::DocumentConvert(DocumentFormat::Doc),
        "DOC",
        "Convert to DOC",
    ),
    action(
        "doc-extract-text",
        FileKind::Document,
        Tab::Extract,
        Action::DocumentExtract(DocumentExtract::PlainText),
        IconId::TextLines,
        "Plain text",
    ),
    action(
        "doc-extract-images",
        FileKind::Document,
        Tab::Extract,
        Action::DocumentExtract(DocumentExtract::Images),
        IconId::Image,
        "Embedded images",
    ),
    // ZIP source (§2.9): the archive opens this Document card with exactly
    // this one tile; `visible()` hides every other row when a ZIP is selected.
    action(
        "zip-extract",
        FileKind::Document,
        Tab::Extract,
        Action::Zip(ZipOp::ExtractHere),
        IconId::Extract,
        "Extract here",
    ),
    // ----------------------------------------------------------------- pdf
    convert(
        "pdf-docx",
        FileKind::Pdf,
        Action::PdfConvert(PdfTarget::Docx),
        "DOCX",
        "Convert to DOCX",
    ),
    convert(
        "pdf-odt",
        FileKind::Pdf,
        Action::PdfConvert(PdfTarget::Odt),
        "ODT",
        "Convert to ODT",
    ),
    convert(
        "pdf-txt",
        FileKind::Pdf,
        Action::PdfConvert(PdfTarget::Txt),
        "TXT",
        "Convert to TXT",
    ),
    convert(
        "pdf-md",
        FileKind::Pdf,
        Action::PdfConvert(PdfTarget::Markdown),
        "MD",
        "Convert to Markdown",
    ),
    convert(
        "pdf-png",
        FileKind::Pdf,
        Action::PdfConvert(PdfTarget::Png),
        "PNG",
        "Convert to PNG",
    ),
    convert(
        "pdf-jpg",
        FileKind::Pdf,
        Action::PdfConvert(PdfTarget::Jpeg),
        "JPG",
        "Convert to JPG",
    ),
    action(
        "pdf-tool-compress",
        FileKind::Pdf,
        Tab::PdfTools,
        Action::PdfTool(PdfTool::Compress),
        IconId::Compress,
        "Compress",
    ),
    action(
        "pdf-tool-rotate-left",
        FileKind::Pdf,
        Tab::PdfTools,
        Action::PdfTool(PdfTool::RotateLeft),
        IconId::RotateLeft,
        "Rotate left",
    ),
    action(
        "pdf-tool-rotate-right",
        FileKind::Pdf,
        Tab::PdfTools,
        Action::PdfTool(PdfTool::RotateRight),
        IconId::RotateRight,
        "Rotate right",
    ),
    action(
        "pdf-tool-split",
        FileKind::Pdf,
        Tab::PdfTools,
        Action::PdfTool(PdfTool::Split),
        IconId::Extract,
        "Split into pages",
    ),
    action(
        "pdf-tool-merge",
        FileKind::Pdf,
        Tab::PdfTools,
        Action::PdfTool(PdfTool::Merge),
        IconId::Plus,
        "Merge with selected",
    ),
    action(
        "pdf-tool-searchable",
        FileKind::Pdf,
        Tab::PdfTools,
        Action::PdfTool(PdfTool::Searchable),
        IconId::TextLines,
        "Make searchable",
    ),
    action(
        "pdf-tool-web",
        FileKind::Pdf,
        Tab::PdfTools,
        Action::PdfTool(PdfTool::OptimizeWeb),
        IconId::Web,
        "Optimize for web",
    ),
    action(
        "pdf-extract-copy",
        FileKind::Pdf,
        Tab::Extract,
        Action::PdfExtract(PdfExtract::CopyText),
        IconId::Copy,
        "Copy text",
    ),
    action(
        "pdf-extract-text",
        FileKind::Pdf,
        Tab::Extract,
        Action::PdfExtract(PdfExtract::Text),
        IconId::TextLines,
        "Text",
    ),
    action(
        "pdf-extract-images",
        FileKind::Pdf,
        Tab::Extract,
        Action::PdfExtract(PdfExtract::Images),
        IconId::Image,
        "Embedded images",
    ),
    action(
        "pdf-extract-page-png",
        FileKind::Pdf,
        Tab::Extract,
        Action::PdfExtract(PdfExtract::PagePng),
        IconId::Image,
        "Page \u{2192} PNG",
    ),
    // --------------------------------------------------------- spreadsheets
    convert(
        "sheet-pdf",
        FileKind::Spreadsheet,
        Action::SheetConvert(SheetFormat::Pdf),
        "PDF",
        "Convert to PDF",
    ),
    convert(
        "sheet-csv",
        FileKind::Spreadsheet,
        Action::SheetConvert(SheetFormat::Csv),
        "CSV",
        "Convert to CSV",
    ),
    convert(
        "sheet-xlsx",
        FileKind::Spreadsheet,
        Action::SheetConvert(SheetFormat::Xlsx),
        "XLSX",
        "Convert to XLSX",
    ),
    convert(
        "sheet-ods",
        FileKind::Spreadsheet,
        Action::SheetConvert(SheetFormat::Ods),
        "ODS",
        "Convert to ODS",
    ),
    convert(
        "sheet-html",
        FileKind::Spreadsheet,
        Action::SheetConvert(SheetFormat::Html),
        "HTML",
        "Convert to HTML",
    ),
    convert(
        "sheet-xls",
        FileKind::Spreadsheet,
        Action::SheetConvert(SheetFormat::Xls),
        "XLS",
        "Convert to XLS",
    ),
    convert(
        "sheet-tsv",
        FileKind::Spreadsheet,
        Action::SheetConvert(SheetFormat::Tsv),
        "TSV",
        "Convert to TSV",
    ),
    convert(
        "sheet-json",
        FileKind::Spreadsheet,
        Action::SheetConvert(SheetFormat::Json),
        "JSON",
        "Convert to JSON",
    ),
    // -------------------------------------------------------- presentations
    convert(
        "present-pdf",
        FileKind::Presentation,
        Action::PresentConvert(PresentFormat::Pdf),
        "PDF",
        "Convert to PDF",
    ),
    convert(
        "present-pptx",
        FileKind::Presentation,
        Action::PresentConvert(PresentFormat::Pptx),
        "PPTX",
        "Convert to PPTX",
    ),
    convert(
        "present-ppt",
        FileKind::Presentation,
        Action::PresentConvert(PresentFormat::Ppt),
        "PPT",
        "Convert to PPT",
    ),
    convert(
        "present-odp",
        FileKind::Presentation,
        Action::PresentConvert(PresentFormat::Odp),
        "ODP",
        "Convert to ODP",
    ),
    convert(
        "present-png",
        FileKind::Presentation,
        Action::PresentConvert(PresentFormat::Png),
        "PNG",
        "Convert to PNG",
    ),
    // -------------------------------------------------------- several files
    action(
        "multi-merge-pdf",
        FileKind::Multi,
        Tab::Combine,
        Action::Multi(MultiOp::MergePdf),
        IconId::File,
        "Merge to PDF",
    ),
    action(
        "multi-zip",
        FileKind::Multi,
        Tab::Combine,
        Action::Multi(MultiOp::Zip),
        IconId::Compress,
        "ZIP",
    ),
    action(
        "multi-batch-convert",
        FileKind::Multi,
        Tab::Combine,
        Action::Multi(MultiOp::BatchConvert),
        IconId::Convert,
        "Batch convert",
    ),
    action(
        "multi-batch-resize",
        FileKind::Multi,
        Tab::Combine,
        Action::Multi(MultiOp::BatchResize),
        IconId::Resize,
        "Batch resize",
    ),
    action(
        "multi-batch-compress",
        FileKind::Multi,
        Tab::Combine,
        Action::Multi(MultiOp::BatchCompress),
        IconId::Compress,
        "Batch compress",
    ),
    action(
        "multi-batch-strip",
        FileKind::Multi,
        Tab::Combine,
        Action::Multi(MultiOp::BatchStripMetadata),
        IconId::EyeOff,
        "Batch strip metadata",
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
        Action::SheetConvert(target) => sources.contains(&SourceKind::Sheet(target)),
        Action::PresentConvert(target) => sources.contains(&SourceKind::Present(target)),
        _ => false,
    }
}

/// Group A (DOCX, DOC, ODT, RTF — word processing) converts to all ten
/// targets; group B (MD, HTML, TXT, TEX, EPUB) only to PDF, DOCX, HTML, EPUB
/// and TXT (§2.3). No document source at all (empty selection, tests) keeps
/// everything on offer.
fn doc_target_allowed(target: DocumentFormat, sources: &[SourceKind]) -> bool {
    const GROUP_B: [DocumentFormat; 5] = [
        DocumentFormat::Pdf,
        DocumentFormat::Docx,
        DocumentFormat::Html,
        DocumentFormat::Epub,
        DocumentFormat::Txt,
    ];
    let mut has_source = false;
    for source in sources {
        if let SourceKind::Document(format) = *source {
            has_source = true;
            if matches!(
                format,
                DocumentFormat::Docx
                    | DocumentFormat::Doc
                    | DocumentFormat::Odt
                    | DocumentFormat::Rtf
            ) {
                return true;
            }
        }
    }
    !has_source || GROUP_B.contains(&target)
}

/// Source-shape rules that key on the row itself (§2.1, §2.3, §2.9): the
/// GIF→MP4 tile exists only for a GIF source, a ZIP archive offers only
/// "Extract here", and group-B documents see the reduced convert set.
fn source_applies(def: &OptionDef, sources: &[SourceKind]) -> bool {
    if def.id == "img-mp4" {
        return sources
            .iter()
            .any(|source| matches!(source, SourceKind::Image(ImageFormat::Gif)));
    }
    if def.kind == FileKind::Document {
        let zip = sources.contains(&SourceKind::Zip);
        if matches!(def.action, Some(Action::Zip(_))) {
            return zip;
        }
        if zip {
            return false;
        }
        if let Some(Action::DocumentConvert(target)) = def.action {
            return doc_target_allowed(target, sources);
        }
    }
    true
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
        && source_applies(def, sources)
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

/// §5: the tab strip is pointless when only one tab exists (Spreadsheet,
/// Presentation and Several files ship single-tab; a missing engine can
/// collapse any other card to one as well).
pub fn tab_strip_hidden(kind: FileKind, caps: &CapabilityTable, sources: &[SourceKind]) -> bool {
    tabs(kind, caps, sources).len() <= 1
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
/// does a job whose only candidate is not installed. The three §9 chains and
/// the two qualified second lines are spelled out exactly; a scanned-PDF card
/// swaps its own tip to `Tesseract + Poppler` at the card layer (B4).
pub fn engine_hint(kind: FileKind, action: Action, caps: &CapabilityTable) -> Option<String> {
    let candidates = route(kind, action, caps);
    if candidates.is_empty() {
        return None;
    }
    match action {
        Action::ImageOcr(OcrOutput::Markdown | OcrOutput::Docx) => {
            Some("Tesseract + Pandoc".to_owned())
        }
        Action::PdfConvert(PdfTarget::Markdown) => Some("Poppler + Pandoc".to_owned()),
        Action::PdfTool(PdfTool::Searchable) => Some("Tesseract + Poppler".to_owned()),
        Action::PdfConvert(PdfTarget::Docx) => {
            Some("via LibreOffice \u{b7} layout may differ".to_owned())
        }
        // LibreOffice writes the PDF, Poppler rasterises the slides: a chain
        // inside one job (§7.3 of the options spec).
        Action::PresentConvert(PresentFormat::Png) => {
            Some("via LibreOffice \u{b7} via Poppler".to_owned())
        }
        _ => candidates
            .into_iter()
            .next()?
            .engine
            .name()
            .map(|name| format!("via {name}")),
    }
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
        // The four v0.8 engines at full capability: test 1's fake table.
        table.set(
            EngineId::LibreOffice,
            EngineCapabilities {
                installed: true,
                version: Some("LibreOffice 25.2".into()),
                documents_in: DocumentFormat::ALL.to_vec(),
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
            },
        );
        table.set(
            EngineId::Poppler,
            EngineCapabilities {
                installed: true,
                version: Some("poppler 24.08".into()),
                ..EngineCapabilities::default()
            },
        );
        table.set(
            EngineId::Qpdf,
            EngineCapabilities {
                installed: true,
                version: Some("qpdf 11.9".into()),
                ..EngineCapabilities::default()
            },
        );
        table.set(
            EngineId::Ghostscript,
            EngineCapabilities {
                installed: true,
                version: Some("Ghostscript 10.04".into()),
                ..EngineCapabilities::default()
            },
        );
        table
    }

    /// The full-capability table with one engine removed (test 5).
    fn table_without(engine: EngineId) -> CapabilityTable {
        let mut table = full_caps();
        table.set(engine, EngineCapabilities::default());
        table
    }

    /// §6 test 1 + 2: every TABLE row is routable by at least one engine
    /// definition under full capabilities, and no (kind, tab) exceeds the
    /// 10-tile grid — including Multi and the GIF-source MP4 variant.
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
            FileKind::Pdf,
            FileKind::Spreadsheet,
            FileKind::Presentation,
            FileKind::Multi,
        ] {
            for tab in [
                Tab::ConvertTo,
                Tab::Optimize,
                Tab::Edit,
                Tab::Extract,
                Tab::ExtractText,
                Tab::PdfTools,
                Tab::Combine,
            ] {
                let count = TABLE
                    .iter()
                    .filter(|def| def.kind == kind && def.tab == tab)
                    .count();
                assert!(count <= 10, "{kind:?}/{tab:?} has {count} options");
            }
        }
    }

    /// §6 test 4: the kind → tab list is exactly the artboard list, and the
    /// single-tab kinds report their strip hidden.
    #[test]
    fn tabs_match_the_artboards_and_single_tabs_are_hidden() {
        let caps = full_caps();
        let png = [SourceKind::Image(ImageFormat::Png)];
        assert_eq!(
            tabs(FileKind::Image, &caps, &png),
            vec![Tab::ConvertTo, Tab::Optimize, Tab::Edit, Tab::ExtractText]
        );
        assert!(!tab_strip_hidden(FileKind::Image, &caps, &png));

        assert_eq!(
            tabs(FileKind::Video, &caps, &[]),
            vec![Tab::ConvertTo, Tab::Optimize, Tab::Edit, Tab::Extract]
        );
        assert_eq!(
            tabs(FileKind::Audio, &caps, &[]),
            vec![Tab::ConvertTo, Tab::Optimize]
        );
        let docx = [SourceKind::Document(DocumentFormat::Docx)];
        assert_eq!(
            tabs(FileKind::Document, &caps, &docx),
            vec![Tab::ConvertTo, Tab::Extract]
        );
        let pdf = [SourceKind::Pdf];
        assert_eq!(
            tabs(FileKind::Pdf, &caps, &pdf),
            vec![Tab::ConvertTo, Tab::PdfTools, Tab::Extract]
        );

        // §5: Spreadsheet, Presentation and Several files are single-tab.
        let sheet = [SourceKind::Sheet(SheetFormat::Csv)];
        assert_eq!(
            tabs(FileKind::Spreadsheet, &caps, &sheet),
            vec![Tab::ConvertTo]
        );
        assert!(tab_strip_hidden(FileKind::Spreadsheet, &caps, &sheet));
        let present = [SourceKind::Present(PresentFormat::Pptx)];
        assert_eq!(
            tabs(FileKind::Presentation, &caps, &present),
            vec![Tab::ConvertTo]
        );
        assert!(tab_strip_hidden(FileKind::Presentation, &caps, &present));
        assert_eq!(tabs(FileKind::Multi, &caps, &[]), vec![Tab::Combine]);
        assert!(tab_strip_hidden(FileKind::Multi, &caps, &[]));
    }

    /// §6 test 3: for every SourceKind value its own format tile is absent.
    #[test]
    fn no_source_is_offered_its_own_format() {
        let caps = full_caps();
        let cases: Vec<(SourceKind, FileKind, Option<&str>)> = vec![
            (
                SourceKind::Image(ImageFormat::Png),
                FileKind::Image,
                Some("PNG"),
            ),
            (
                SourceKind::Image(ImageFormat::Jpeg),
                FileKind::Image,
                Some("JPG"),
            ),
            (
                SourceKind::Video(VideoFormat::Mp4),
                FileKind::Video,
                Some("MP4"),
            ),
            (
                SourceKind::Audio(AudioFormat::Mp3),
                FileKind::Audio,
                Some("MP3"),
            ),
            (
                SourceKind::Document(DocumentFormat::Markdown),
                FileKind::Document,
                Some("MD"),
            ),
            (
                SourceKind::Document(DocumentFormat::Docx),
                FileKind::Document,
                Some("DOCX"),
            ),
            (
                SourceKind::Document(DocumentFormat::Doc),
                FileKind::Document,
                Some("DOC"),
            ),
            // A PDF card never offers "PDF" as a convert target.
            (SourceKind::Pdf, FileKind::Pdf, Some("PDF")),
            (
                SourceKind::Sheet(SheetFormat::Csv),
                FileKind::Spreadsheet,
                Some("CSV"),
            ),
            (
                SourceKind::Sheet(SheetFormat::Xlsx),
                FileKind::Spreadsheet,
                Some("XLSX"),
            ),
            (
                SourceKind::Present(PresentFormat::Pptx),
                FileKind::Presentation,
                Some("PPTX"),
            ),
        ];
        for (source, kind, own) in cases {
            let sources = [source];
            let formats: Vec<Option<&str>> = options(kind, Tab::ConvertTo, &caps, &sources)
                .iter()
                .map(|def| def.format)
                .collect();
            if let Some(own) = own {
                assert!(
                    !formats.contains(&Some(own)),
                    "{source:?} was offered its own {own} (got {formats:?})"
                );
            }
        }
        // A ZIP archive shows only "Extract here" — no convert tiles at all.
        let zip = [SourceKind::Zip];
        assert!(options(FileKind::Document, Tab::ConvertTo, &caps, &zip).is_empty());
        let extract = options(FileKind::Document, Tab::Extract, &caps, &zip);
        assert_eq!(extract.len(), 1);
        assert_eq!(extract[0].id, "zip-extract");
    }

    /// §2.3: group-B documents (MD, HTML, TXT, TEX, EPUB) get the reduced
    /// convert set; group-A (DOCX, DOC, ODT, RTF) get all ten.
    #[test]
    fn document_source_groups_pick_their_convert_tiles() {
        let caps = full_caps();
        let group_b = [SourceKind::Document(DocumentFormat::Markdown)];
        let formats: Vec<&str> = options(FileKind::Document, Tab::ConvertTo, &caps, &group_b)
            .iter()
            .filter_map(|def| def.format)
            .collect();
        for allowed in ["PDF", "DOCX", "HTML", "EPUB", "TXT"] {
            assert!(formats.contains(&allowed), "group B misses {allowed}");
        }
        for denied in ["MD", "ODT", "RTF", "TEX", "DOC"] {
            assert!(!formats.contains(&denied), "group B must not see {denied}");
        }

        let group_a = [SourceKind::Document(DocumentFormat::Rtf)];
        let formats: Vec<&str> = options(FileKind::Document, Tab::ConvertTo, &caps, &group_a)
            .iter()
            .filter_map(|def| def.format)
            .collect();
        // All ten targets, minus RTF itself (own format).
        assert_eq!(
            formats,
            vec!["PDF", "MD", "HTML", "TXT", "EPUB", "ODT", "TEX", "DOCX", "DOC"]
        );
    }

    /// §2.1: the GIF→MP4 tile exists only when a GIF is among the sources.
    #[test]
    fn only_a_gif_source_sees_the_mp4_tile() {
        let caps = full_caps();
        let gif = [SourceKind::Image(ImageFormat::Gif)];
        let with_mp4: Vec<&str> = options(FileKind::Image, Tab::ConvertTo, &caps, &gif)
            .iter()
            .filter_map(|def| def.format)
            .collect();
        assert!(with_mp4.contains(&"MP4"));
        assert!(!with_mp4.contains(&"GIF")); // own format

        let png = [SourceKind::Image(ImageFormat::Png)];
        let without: Vec<&str> = options(FileKind::Image, Tab::ConvertTo, &caps, &png)
            .iter()
            .filter_map(|def| def.format)
            .collect();
        assert!(!without.contains(&"MP4"));
    }

    /// §6 test 5 at the option level: a missing engine hides exactly its
    /// own tiles, never the whole card.
    #[test]
    fn missing_engines_hide_their_tiles() {
        // No LibreOffice ⇒ PDF→DOCX/ODT, every LibreOffice sheet tile and
        // every presentation tile is gone; the PDF card keeps its Poppler rows.
        let no_lo = table_without(EngineId::LibreOffice);
        let pdf_formats: Vec<&str> =
            options(FileKind::Pdf, Tab::ConvertTo, &no_lo, &[SourceKind::Pdf])
                .iter()
                .filter_map(|def| def.format)
                .collect();
        assert!(!pdf_formats.contains(&"DOCX"));
        assert!(!pdf_formats.contains(&"ODT"));
        assert!(pdf_formats.contains(&"TXT"));
        assert!(tabs(
            FileKind::Presentation,
            &no_lo,
            &[SourceKind::Present(PresentFormat::Pptx)]
        )
        .is_empty());
        // Every LibreOffice sheet tile is gone; JSON survives because §3
        // routes it to the built-in Native engine.
        let sheet_tiles: Vec<&str> = options(
            FileKind::Spreadsheet,
            Tab::ConvertTo,
            &no_lo,
            &[SourceKind::Sheet(SheetFormat::Csv)],
        )
        .iter()
        .filter_map(|def| def.format)
        .collect();
        assert_eq!(sheet_tiles, vec!["JSON"]);

        // No Poppler ⇒ TXT/pages/extract tiles go, split keeps qpdf.
        let no_poppler = table_without(EngineId::Poppler);
        let pdf_formats: Vec<&str> = options(
            FileKind::Pdf,
            Tab::ConvertTo,
            &no_poppler,
            &[SourceKind::Pdf],
        )
        .iter()
        .filter_map(|def| def.format)
        .collect();
        assert!(!pdf_formats.contains(&"TXT"));
        assert!(!pdf_formats.contains(&"PNG"));
        assert!(options(FileKind::Pdf, Tab::Extract, &no_poppler, &[SourceKind::Pdf]).is_empty());
        assert!(options(
            FileKind::Pdf,
            Tab::PdfTools,
            &no_poppler,
            &[SourceKind::Pdf]
        )
        .iter()
        .any(|def| def.id == "pdf-tool-split"));

        // No qpdf ⇒ rotate and linearize are gone.
        let no_qpdf = table_without(EngineId::Qpdf);
        let tools: Vec<&str> = options(FileKind::Pdf, Tab::PdfTools, &no_qpdf, &[SourceKind::Pdf])
            .iter()
            .map(|def| def.id)
            .collect();
        assert!(!tools.contains(&"pdf-tool-rotate-left"));
        assert!(!tools.contains(&"pdf-tool-web"));

        // No Ghostscript ⇒ compress is gone.
        let no_gs = table_without(EngineId::Ghostscript);
        assert!(
            options(FileKind::Pdf, Tab::PdfTools, &no_gs, &[SourceKind::Pdf])
                .iter()
                .all(|def| def.id != "pdf-tool-compress")
        );

        // No Tesseract ⇒ every OCR tile and the language picker are gone.
        let no_tesseract = table_without(EngineId::Tesseract);
        assert!(options(
            FileKind::Image,
            Tab::ExtractText,
            &no_tesseract,
            &[SourceKind::Image(ImageFormat::Png)]
        )
        .is_empty());
    }

    /// §6 test 7: `engine_hint()` equals the §9 copy byte-for-byte.
    #[test]
    fn engine_hints_are_the_exact_section_nine_copy() {
        let caps = full_caps();
        let hint = |action: Action| engine_hint(FileKind::Image, action, &caps);
        // Native work shows no second line.
        assert_eq!(hint(Action::ImageConvert(ImageFormat::Png)), None);
        assert_eq!(
            hint(Action::ImageConvert(ImageFormat::Avif)),
            Some("via FFmpeg".to_owned())
        );
        assert_eq!(
            hint(Action::ImageOcr(OcrOutput::Txt)),
            Some("via Tesseract".to_owned())
        );
        // Chain 1: image OCR → MD/DOCX.
        assert_eq!(
            hint(Action::ImageOcr(OcrOutput::Markdown)),
            Some("Tesseract + Pandoc".to_owned())
        );
        assert_eq!(
            hint(Action::ImageOcr(OcrOutput::Docx)),
            Some("Tesseract + Pandoc".to_owned())
        );

        let pdf = |action: Action| engine_hint(FileKind::Pdf, action, &caps);
        assert_eq!(
            pdf(Action::PdfConvert(PdfTarget::Txt)),
            Some("via Poppler".to_owned())
        );
        // Chain 2: PDF → Markdown.
        assert_eq!(
            pdf(Action::PdfConvert(PdfTarget::Markdown)),
            Some("Poppler + Pandoc".to_owned())
        );
        // Chain 3: "Make searchable".
        assert_eq!(
            pdf(Action::PdfTool(PdfTool::Searchable)),
            Some("Tesseract + Poppler".to_owned())
        );
        // DOCX from PDF carries the qualifier.
        assert_eq!(
            pdf(Action::PdfConvert(PdfTarget::Docx)),
            Some("via LibreOffice \u{b7} layout may differ".to_owned())
        );
        assert_eq!(
            pdf(Action::PdfTool(PdfTool::Compress)),
            Some("via Ghostscript".to_owned())
        );
        assert_eq!(
            pdf(Action::PdfTool(PdfTool::RotateLeft)),
            Some("via qpdf".to_owned())
        );

        assert_eq!(
            engine_hint(
                FileKind::Document,
                Action::DocumentConvert(DocumentFormat::Html),
                &caps
            ),
            Some("via Pandoc".to_owned())
        );
        assert_eq!(
            engine_hint(
                FileKind::Spreadsheet,
                Action::SheetConvert(SheetFormat::Csv),
                &caps
            ),
            Some("via LibreOffice".to_owned())
        );
        assert_eq!(
            engine_hint(
                FileKind::Spreadsheet,
                Action::SheetConvert(SheetFormat::Json),
                &caps
            ),
            None
        );
        assert_eq!(
            engine_hint(
                FileKind::Presentation,
                Action::PresentConvert(PresentFormat::Png),
                &caps
            ),
            Some("via LibreOffice \u{b7} via Poppler".to_owned())
        );

        // A missing engine means no hint at all.
        assert_eq!(
            engine_hint(
                FileKind::Pdf,
                Action::PdfConvert(PdfTarget::Txt),
                &table_without(EngineId::Poppler)
            ),
            None
        );
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
        // available because FFmpeg covers AVIF and TIFF. The MP4 tile is the
        // GIF-source row and stays hidden here.
        let order: Vec<_> = TABLE
            .iter()
            .filter(|def| def.kind == FileKind::Image && def.tab == Tab::ConvertTo)
            .map(|def| def.format.unwrap())
            .collect();
        assert_eq!(
            order,
            vec!["PNG", "JPG", "WebP", "AVIF", "GIF", "BMP", "TIFF", "ICO", "PDF", "MP4"]
        );
        let convert: Vec<_> = options(FileKind::Image, Tab::ConvertTo, &caps, &image)
            .iter()
            .map(|def| def.format.unwrap())
            .collect();
        assert_eq!(
            convert,
            vec!["JPG", "WebP", "AVIF", "GIF", "BMP", "TIFF", "ICO", "PDF"]
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
        // Native alone still unpacks embedded images (§3); every convert and
        // the text extraction need an engine that is not installed.
        assert_eq!(tabs(FileKind::Document, &caps, &[]), vec![Tab::Extract]);
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
