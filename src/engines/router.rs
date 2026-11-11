//! router.rs — the instant engine choice.
//!
//! The route is computed from the capability matrix only. There is no
//! benchmarking at click time: candidates are ordered by the static preference
//! in the v0.6 spec, so the answer is a table lookup in well under a
//! millisecond.

use super::{
    Action, CapabilityTable, DocumentFormat, DocumentFormat as Doc, EngineError, EngineId,
    FileKind, ImageFormat, ImageOptimize, JobOutput, MultiOp, OcrOutput, PdfTarget, PdfTool,
    PresentFormat, SheetFormat, VideoExtract, VideoFormat,
};
use crate::convert::detect::SourceFormat;

/// Which engine of record a document source has (B prompt §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocGroup {
    /// Markdown, LaTeX, EPUB, plain text — Pandoc's readers.
    Markup,
    /// HTML — LibreOffice's web filter keeps the layout.
    Web,
    /// DOCX, ODT, RTF, DOC and anything unrecognised — LibreOffice's.
    Office,
}

fn doc_group(source: Option<SourceFormat>) -> DocGroup {
    match source {
        Some(
            SourceFormat::Markdown | SourceFormat::Tex | SourceFormat::Epub | SourceFormat::Txt,
        ) => DocGroup::Markup,
        Some(SourceFormat::Html) => DocGroup::Web,
        _ => DocGroup::Office,
    }
}

/// One automatic retry, exactly as the spec asks: the chosen engine, then the
/// next candidate, then failure.
pub const MAX_ATTEMPTS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub engine: EngineId,
    /// Static cost rank for this job; lower wins. Native is 0 because it has
    /// no process-start cost.
    pub cost: u8,
}

impl Candidate {
    const fn new(engine: EngineId, cost: u8) -> Self {
        Self { engine, cost }
    }
}

/// Ordered candidates for one source class and action. An empty result means
/// the UI must hide the option entirely.
pub fn route(kind: FileKind, action: Action, capabilities: &CapabilityTable) -> Vec<Candidate> {
    route_source(kind, None, action, capabilities)
}

/// Same as `route`, but the source container can re-order the candidates for
/// the one case where the engine of record depends on what the file is: a
/// markup document is Pandoc's, an HTML document is LibreOffice's web filter,
/// and everything else is LibreOffice's (B prompt §4).
pub fn route_source(
    kind: FileKind,
    source: Option<SourceFormat>,
    action: Action,
    capabilities: &CapabilityTable,
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    match (kind, action) {
        (FileKind::Image, Action::ImageConvert(target)) => {
            push_image(
                &mut candidates,
                capabilities,
                EngineId::Native,
                0,
                Some(target),
            );
            push_image(
                &mut candidates,
                capabilities,
                EngineId::Ffmpeg,
                10,
                Some(target),
            );
        }
        (FileKind::Image, Action::ImageOptimize(optimize)) => {
            let output = match optimize {
                ImageOptimize::WebReady => Some(ImageFormat::WebP),
                _ => None,
            };
            push_image_edit(&mut candidates, capabilities, EngineId::Native, 0, output);
            push_image_edit(&mut candidates, capabilities, EngineId::Ffmpeg, 10, output);
        }
        (FileKind::Image, Action::ImageEdit(_)) => {
            push_image_edit(&mut candidates, capabilities, EngineId::Native, 0, None);
            push_image_edit(&mut candidates, capabilities, EngineId::Ffmpeg, 10, None);
        }
        (FileKind::Image, Action::ImageOcr(output)) => {
            let tesseract = capabilities.get(EngineId::Tesseract);
            let pandoc = capabilities.get(EngineId::Pandoc);
            let post_engine_ready = match output {
                OcrOutput::Markdown => pandoc.can_write_document(DocumentFormat::Markdown),
                OcrOutput::Docx => pandoc.can_write_document(DocumentFormat::Docx),
                OcrOutput::Clipboard | OcrOutput::Txt | OcrOutput::SearchablePdf => true,
            };
            if tesseract.installed && tesseract.can_ocr() && post_engine_ready {
                candidates.push(Candidate::new(EngineId::Tesseract, 0));
            }
        }
        (FileKind::Video, Action::VideoConvert(target)) => {
            let ffmpeg = capabilities.get(EngineId::Ffmpeg);
            if ffmpeg.installed && ffmpeg.can_write_video(target) {
                candidates.push(Candidate::new(EngineId::Ffmpeg, 0));
            }
        }
        (FileKind::Video, Action::VideoOptimize(_)) | (FileKind::Video, Action::VideoEdit(_)) => {
            let ffmpeg = capabilities.get(EngineId::Ffmpeg);
            if ffmpeg.installed && !ffmpeg.video_write.is_empty() {
                candidates.push(Candidate::new(EngineId::Ffmpeg, 0));
            }
        }
        (FileKind::Video, Action::VideoExtract(extract)) => {
            let ffmpeg = capabilities.get(EngineId::Ffmpeg);
            if !ffmpeg.installed {
                return candidates;
            }
            let capable = match extract {
                VideoExtract::Audio(target) => ffmpeg.can_write_audio(target),
                VideoExtract::Frame => ffmpeg.can_write_image(ImageFormat::Png),
                VideoExtract::Gif => ffmpeg.can_write_video(VideoFormat::Gif),
            };
            if capable {
                candidates.push(Candidate::new(EngineId::Ffmpeg, 0));
            }
        }
        (FileKind::Audio, Action::AudioConvert(target)) => {
            let ffmpeg = capabilities.get(EngineId::Ffmpeg);
            if ffmpeg.installed && ffmpeg.can_write_audio(target) {
                candidates.push(Candidate::new(EngineId::Ffmpeg, 0));
            }
        }
        (FileKind::Audio, Action::AudioOptimize(_)) => {
            let ffmpeg = capabilities.get(EngineId::Ffmpeg);
            if ffmpeg.installed && !ffmpeg.audio_write.is_empty() {
                candidates.push(Candidate::new(EngineId::Ffmpeg, 0));
            }
        }
        (FileKind::Document, Action::DocumentConvert(target)) => {
            match (doc_group(source), target) {
                // Markup (md, tex, epub, txt): Pandoc reads the source, so it
                // goes first and LibreOffice is the one automatic retry.
                (DocGroup::Markup, Doc::Docx) => {
                    push_document(&mut candidates, capabilities, EngineId::Pandoc, 0, target);
                    push_document(
                        &mut candidates,
                        capabilities,
                        EngineId::LibreOffice,
                        10,
                        target,
                    );
                }
                // Pandoc has no offline PDF writer: the job is a chain
                // (Pandoc → DOCX → LibreOffice → PDF), so LibreOffice gates
                // the route instead of retrying it.
                (DocGroup::Markup, Doc::Pdf) => {
                    if capabilities.installed(EngineId::LibreOffice) {
                        push_document(
                            &mut candidates,
                            capabilities,
                            EngineId::Pandoc,
                            0,
                            Doc::Docx,
                        );
                    }
                }
                // HTML keeps its layout in LibreOffice's web filter; Pandoc
                // is the fallback that strips it.
                (DocGroup::Web, Doc::Pdf | Doc::Docx) => {
                    push_document(
                        &mut candidates,
                        capabilities,
                        EngineId::LibreOffice,
                        0,
                        target,
                    );
                    push_document(&mut candidates, capabilities, EngineId::Pandoc, 10, target);
                }
                // Word-processing outputs: LibreOffice is the engine of record
                // (group A sources). Pandoc writes DOCX as well, so it stays
                // as the one automatic retry.
                (_, Doc::Pdf | Doc::Doc | Doc::Docx) => {
                    push_document(
                        &mut candidates,
                        capabilities,
                        EngineId::LibreOffice,
                        0,
                        target,
                    );
                    if target == Doc::Docx {
                        push_document(&mut candidates, capabilities, EngineId::Pandoc, 10, target);
                    }
                }
                (_, other) => {
                    push_document(&mut candidates, capabilities, EngineId::Pandoc, 0, other)
                }
            }
        }
        (FileKind::Document, Action::DocumentExtract(extract)) => match extract {
            // Extracting embedded images is Native's unpack job (§2.2).
            crate::engines::DocumentExtract::Images => {
                push_installed(&mut candidates, capabilities, EngineId::Native, 0);
            }
            crate::engines::DocumentExtract::PlainText => {
                push_document(&mut candidates, capabilities, EngineId::Pandoc, 0, Doc::Txt);
            }
        },
        // A GIF source offers MP4 (§2.1); FFmpeg is the only engine for it.
        (FileKind::Image, Action::VideoConvert(target)) => {
            let ffmpeg = capabilities.get(EngineId::Ffmpeg);
            if ffmpeg.installed && ffmpeg.can_write_video(target) {
                candidates.push(Candidate::new(EngineId::Ffmpeg, 0));
            }
        }
        (FileKind::Pdf, Action::PdfConvert(target)) => match target {
            PdfTarget::Docx => push_document(
                &mut candidates,
                capabilities,
                EngineId::LibreOffice,
                0,
                Doc::Docx,
            ),
            PdfTarget::Odt => push_document(
                &mut candidates,
                capabilities,
                EngineId::LibreOffice,
                0,
                Doc::Odt,
            ),
            // Text, pages and the Markdown chain all start at Poppler; the
            // Markdown chain's second engine (Pandoc) is a gate, exactly like
            // image OCR → MD: the chain runs inside one job, it is not a
            // retry.
            PdfTarget::Txt => push_installed(&mut candidates, capabilities, EngineId::Poppler, 0),
            PdfTarget::Markdown => {
                if capabilities.installed(EngineId::Poppler)
                    && capabilities.installed(EngineId::Pandoc)
                {
                    candidates.push(Candidate::new(EngineId::Poppler, 0));
                }
            }
            PdfTarget::Png | PdfTarget::Jpeg => {
                push_installed(&mut candidates, capabilities, EngineId::Poppler, 0)
            }
        },
        (FileKind::Pdf, Action::PdfTool(tool)) => match tool {
            PdfTool::Compress => {
                push_installed(&mut candidates, capabilities, EngineId::Ghostscript, 0)
            }
            PdfTool::RotateLeft | PdfTool::RotateRight => {
                push_installed(&mut candidates, capabilities, EngineId::Qpdf, 0)
            }
            PdfTool::OptimizeWeb => {
                push_installed(&mut candidates, capabilities, EngineId::Qpdf, 0)
            }
            // Poppler first, qpdf as the one automatic retry (§3).
            PdfTool::Split | PdfTool::Merge => {
                push_installed(&mut candidates, capabilities, EngineId::Poppler, 0);
                push_installed(&mut candidates, capabilities, EngineId::Qpdf, 10);
            }
            // Rasterise with Poppler, OCR with Tesseract: both are needed, so
            // Tesseract is a gate, not a retry.
            PdfTool::Searchable => {
                let tesseract = capabilities.get(EngineId::Tesseract);
                if capabilities.installed(EngineId::Poppler)
                    && tesseract.installed
                    && tesseract.can_ocr()
                {
                    candidates.push(Candidate::new(EngineId::Poppler, 0));
                }
            }
        },
        (FileKind::Pdf, Action::PdfExtract(_)) => {
            push_installed(&mut candidates, capabilities, EngineId::Poppler, 0);
        }
        (FileKind::Spreadsheet, Action::SheetConvert(target)) => {
            if target == SheetFormat::Json {
                // CSV → JSON is built into the Native engine (§2.4).
                push_installed(&mut candidates, capabilities, EngineId::Native, 0);
            } else {
                push_installed(&mut candidates, capabilities, EngineId::LibreOffice, 0);
            }
        }
        (FileKind::Presentation, Action::PresentConvert(target)) => {
            if capabilities.installed(EngineId::LibreOffice) {
                candidates.push(Candidate::new(EngineId::LibreOffice, 0));
                // Slides → PNG is LibreOffice → Poppler: Poppler finishes the
                // chain inside the same job, so it gates rather than retries.
                if target == PresentFormat::Png && !capabilities.installed(EngineId::Poppler) {
                    candidates.pop();
                }
            }
        }
        (FileKind::Multi, Action::Multi(op)) => match op {
            // Images merge in Native, PDFs through Poppler, qpdf is the
            // documented last resort (only reachable when Poppler is absent,
            // because MAX_ATTEMPTS is 2).
            MultiOp::MergePdf => {
                push_installed(&mut candidates, capabilities, EngineId::Native, 0);
                push_installed(&mut candidates, capabilities, EngineId::Poppler, 10);
                push_installed(&mut candidates, capabilities, EngineId::Qpdf, 20);
            }
            // ZIP and batch jobs are Native's; a batch delegates each file
            // back through this same router (§2.7).
            MultiOp::Zip
            | MultiOp::BatchConvert
            | MultiOp::BatchResize
            | MultiOp::BatchCompress
            | MultiOp::BatchStripMetadata => {
                push_installed(&mut candidates, capabilities, EngineId::Native, 0);
            }
        },
        // A ZIP source opens one Extract tab with one Native tile (§2.9).
        (FileKind::Document, Action::Zip(_)) => {
            push_installed(&mut candidates, capabilities, EngineId::Native, 0);
        }
        _ => {}
    }
    candidates
}

/// One engine, when it is installed *and* reports the document writer.
fn push_document(
    candidates: &mut Vec<Candidate>,
    capabilities: &CapabilityTable,
    engine: EngineId,
    cost: u8,
    target: DocumentFormat,
) {
    let caps = capabilities.get(engine);
    if caps.installed && caps.can_write_document(target) {
        candidates.push(Candidate::new(engine, cost));
    }
}

/// One engine, when it is installed (its capability is implied by the job
/// class: Poppler always does Poppler jobs, qpdf does qpdf jobs).
fn push_installed(
    candidates: &mut Vec<Candidate>,
    capabilities: &CapabilityTable,
    engine: EngineId,
    cost: u8,
) {
    if capabilities.installed(engine) {
        candidates.push(Candidate::new(engine, cost));
    }
}

fn push_image(
    candidates: &mut Vec<Candidate>,
    capabilities: &CapabilityTable,
    engine: EngineId,
    cost: u8,
    target: Option<ImageFormat>,
) {
    let caps = capabilities.get(engine);
    if !caps.installed {
        return;
    }
    if target.map_or(!caps.image_write.is_empty(), |format| {
        caps.can_write_image(format)
    }) {
        candidates.push(Candidate::new(engine, cost));
    }
}

fn push_image_edit(
    candidates: &mut Vec<Candidate>,
    capabilities: &CapabilityTable,
    engine: EngineId,
    cost: u8,
    output: Option<ImageFormat>,
) {
    let caps = capabilities.get(engine);
    if !caps.installed || !caps.image_edit {
        return;
    }
    if output.is_none_or(|format| caps.can_write_image(format)) {
        candidates.push(Candidate::new(engine, cost));
    }
}

// -------------------------------------------------------------- execution

#[derive(Debug)]
pub struct Attempt<T> {
    pub value: T,
    pub engine: EngineId,
    pub attempts: usize,
}

#[derive(Debug)]
pub struct FallbackFailure {
    pub error: EngineError,
    pub attempts: usize,
    pub last_engine: Option<EngineId>,
}

/// Runs the chosen engine and retries exactly once with the next candidate on
/// a non-cancel error. Cancellation never falls through to another engine.
pub fn run_with_fallback<T>(
    candidates: &[Candidate],
    mut attempt: impl FnMut(&Candidate) -> Result<T, EngineError>,
) -> Result<Attempt<T>, FallbackFailure> {
    if candidates.is_empty() {
        return Err(FallbackFailure {
            error: EngineError::NotFound,
            attempts: 0,
            last_engine: None,
        });
    }

    let mut last_error = EngineError::NotFound;
    let mut last_engine = None;
    for (index, candidate) in candidates.iter().take(MAX_ATTEMPTS).enumerate() {
        last_engine = Some(candidate.engine);
        match attempt(candidate) {
            Ok(value) => {
                return Ok(Attempt {
                    value,
                    engine: candidate.engine,
                    attempts: index + 1,
                })
            }
            Err(error) if error.is_cancel() => {
                return Err(FallbackFailure {
                    error,
                    attempts: index + 1,
                    last_engine,
                })
            }
            Err(error) => last_error = error,
        }
    }

    Err(FallbackFailure {
        error: last_error,
        attempts: candidates.len().min(MAX_ATTEMPTS),
        last_engine,
    })
}

/// `JobOutput` is named here so the job phases share the same signature.
pub type AttemptedOutput = Attempt<JobOutput>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::probe::native_capabilities;
    use crate::engines::{
        AudioFormat, AudioOptimize, CancelToken, EngineCapabilities, ImageEdit, PdfExtract,
        VideoEdit,
    };

    fn table() -> CapabilityTable {
        let mut table = CapabilityTable::default();
        table.set(EngineId::Native, native_capabilities());

        let ffmpeg = EngineCapabilities {
            installed: true,
            version: Some("ffmpeg 9.0.2".into()),
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
        };
        table.set(EngineId::Ffmpeg, ffmpeg);

        let pandoc = EngineCapabilities {
            installed: true,
            version: Some("pandoc 3.12".into()),
            documents_in: DocumentFormat::ALL.to_vec(),
            documents_out: DocumentFormat::ALL.to_vec(),
            ..EngineCapabilities::default()
        };
        table.set(EngineId::Pandoc, pandoc);

        let tesseract = EngineCapabilities {
            installed: true,
            version: Some("tesseract 5.4.0".into()),
            ocr_languages: vec!["eng".into()],
            ..EngineCapabilities::default()
        };
        table.set(EngineId::Tesseract, tesseract);

        // The four v0.8 engines, at full capability (test 1's "fake engine"
        // table): every option in TABLE must be routable against this.
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
        let mut table = table();
        table.set(engine, EngineCapabilities::default());
        table
    }

    fn engines(kind: FileKind, action: Action) -> Vec<EngineId> {
        route(kind, action, &table())
            .into_iter()
            .map(|candidate| candidate.engine)
            .collect()
    }

    #[test]
    fn image_jobs_prefer_native_and_fall_back_to_ffmpeg() {
        assert_eq!(
            engines(FileKind::Image, Action::ImageConvert(ImageFormat::Png)),
            vec![EngineId::Native, EngineId::Ffmpeg]
        );
        assert_eq!(
            engines(FileKind::Image, Action::ImageConvert(ImageFormat::WebP)),
            vec![EngineId::Native, EngineId::Ffmpeg]
        );
        assert_eq!(
            engines(
                FileKind::Image,
                Action::ImageOptimize(ImageOptimize::WebReady)
            ),
            vec![EngineId::Native, EngineId::Ffmpeg]
        );
        assert_eq!(
            engines(FileKind::Image, Action::ImageEdit(ImageEdit::Grayscale)),
            vec![EngineId::Native, EngineId::Ffmpeg]
        );
    }

    #[test]
    fn formats_native_cannot_write_go_straight_to_ffmpeg() {
        assert_eq!(
            engines(FileKind::Image, Action::ImageConvert(ImageFormat::Avif)),
            vec![EngineId::Ffmpeg]
        );
        assert_eq!(
            engines(FileKind::Image, Action::ImageConvert(ImageFormat::Tiff)),
            vec![EngineId::Ffmpeg]
        );
    }

    #[test]
    fn image_to_pdf_is_native_only() {
        assert_eq!(
            engines(FileKind::Image, Action::ImageConvert(ImageFormat::Pdf)),
            vec![EngineId::Native]
        );
    }

    #[test]
    fn media_and_documents_use_their_only_engine() {
        assert_eq!(
            engines(FileKind::Video, Action::VideoConvert(VideoFormat::Mp4)),
            vec![EngineId::Ffmpeg]
        );
        assert_eq!(
            engines(
                FileKind::Video,
                Action::VideoExtract(VideoExtract::Audio(AudioFormat::Mp3))
            ),
            vec![EngineId::Ffmpeg]
        );
        assert_eq!(
            engines(FileKind::Video, Action::VideoEdit(VideoEdit::RotateLeft)),
            vec![EngineId::Ffmpeg]
        );
        assert_eq!(
            engines(FileKind::Audio, Action::AudioConvert(AudioFormat::Flac)),
            vec![EngineId::Ffmpeg]
        );
        assert_eq!(
            engines(FileKind::Audio, Action::AudioOptimize(AudioOptimize::Mono)),
            vec![EngineId::Ffmpeg]
        );
        assert_eq!(
            engines(
                FileKind::Document,
                Action::DocumentConvert(DocumentFormat::Html)
            ),
            vec![EngineId::Pandoc]
        );
        assert_eq!(
            engines(
                FileKind::Document,
                Action::DocumentExtract(crate::engines::DocumentExtract::PlainText)
            ),
            vec![EngineId::Pandoc]
        );
    }

    #[test]
    fn ocr_needs_tesseract_and_its_post_engine() {
        assert_eq!(
            engines(FileKind::Image, Action::ImageOcr(OcrOutput::Txt)),
            vec![EngineId::Tesseract]
        );
        assert_eq!(
            engines(FileKind::Image, Action::ImageOcr(OcrOutput::Docx)),
            vec![EngineId::Tesseract]
        );
        assert_eq!(
            engines(FileKind::Image, Action::ImageOcr(OcrOutput::SearchablePdf)),
            vec![EngineId::Tesseract]
        );
    }

    #[test]
    fn missing_engines_hide_options() {
        let mut no_ffmpeg = table();
        no_ffmpeg.set(EngineId::Ffmpeg, EngineCapabilities::default());
        assert!(route(
            FileKind::Image,
            Action::ImageConvert(ImageFormat::Avif),
            &no_ffmpeg
        )
        .is_empty());
        assert!(route(
            FileKind::Video,
            Action::VideoConvert(VideoFormat::Mp4),
            &no_ffmpeg
        )
        .is_empty());

        let mut no_pandoc = table();
        no_pandoc.set(EngineId::Pandoc, EngineCapabilities::default());
        assert!(route(
            FileKind::Document,
            Action::DocumentConvert(DocumentFormat::Html),
            &no_pandoc
        )
        .is_empty());
        assert!(route(
            FileKind::Image,
            Action::ImageOcr(OcrOutput::Markdown),
            &no_pandoc
        )
        .is_empty());

        let mut no_langs = table();
        no_langs.set(
            EngineId::Tesseract,
            EngineCapabilities {
                installed: true,
                ..EngineCapabilities::default()
            },
        );
        assert!(route(FileKind::Image, Action::ImageOcr(OcrOutput::Txt), &no_langs).is_empty());

        let mut no_native = table();
        no_native.set(EngineId::Native, EngineCapabilities::default());
        assert_eq!(
            route(
                FileKind::Image,
                Action::ImageConvert(ImageFormat::Png),
                &no_native
            )
            .into_iter()
            .map(|candidate| candidate.engine)
            .collect::<Vec<_>>(),
            vec![EngineId::Ffmpeg]
        );
    }

    #[test]
    fn unsupported_pairs_offer_nothing() {
        assert!(route(
            FileKind::Other,
            Action::ImageConvert(ImageFormat::Png),
            &table()
        )
        .is_empty());
        assert!(route(
            FileKind::Video,
            Action::ImageConvert(ImageFormat::Png),
            &table()
        )
        .is_empty());
    }

    #[test]
    fn fallback_tries_the_next_candidate_once() {
        let candidates = vec![
            Candidate::new(EngineId::Native, 0),
            Candidate::new(EngineId::Ffmpeg, 10),
        ];
        let mut attempts = 0;
        let result = run_with_fallback(&candidates, |candidate| {
            attempts += 1;
            match candidate.engine {
                EngineId::Native => Err(EngineError::Failed("unreadable".into())),
                EngineId::Ffmpeg => Ok("done"),
                _ => unreachable!(),
            }
        })
        .expect("the second candidate should succeed");
        assert_eq!(result.value, "done");
        assert_eq!(result.engine, EngineId::Ffmpeg);
        assert_eq!(result.attempts, 2);
        assert_eq!(attempts, 2);
    }

    #[test]
    fn fallback_stops_on_cancel_and_reports_failure() {
        let candidates = vec![
            Candidate::new(EngineId::Native, 0),
            Candidate::new(EngineId::Ffmpeg, 10),
        ];
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = run_with_fallback(&candidates, |_| Err::<(), _>(EngineError::Cancelled));
        assert!(matches!(result, Err(FallbackFailure { attempts: 1, .. })));

        let result = run_with_fallback(&candidates, |_| {
            Err::<(), _>(EngineError::Failed("broken".into()))
        });
        let failure = result.expect_err("both candidates fail");
        assert_eq!(failure.attempts, 2);
        assert_eq!(failure.last_engine, Some(EngineId::Ffmpeg));
    }

    // ---------------------------------------------------- v0.8 routes (§3)

    #[test]
    fn pdf_converts_and_tools_follow_the_spec_chain() {
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfConvert(PdfTarget::Docx)),
            vec![EngineId::LibreOffice]
        );
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfConvert(PdfTarget::Odt)),
            vec![EngineId::LibreOffice]
        );
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfConvert(PdfTarget::Txt)),
            vec![EngineId::Poppler]
        );
        // Markdown: Poppler is the candidate, Pandoc the chain gate.
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfConvert(PdfTarget::Markdown)),
            vec![EngineId::Poppler]
        );
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfConvert(PdfTarget::Png)),
            vec![EngineId::Poppler]
        );
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfConvert(PdfTarget::Jpeg)),
            vec![EngineId::Poppler]
        );

        assert_eq!(
            engines(FileKind::Pdf, Action::PdfTool(PdfTool::Compress)),
            vec![EngineId::Ghostscript]
        );
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfTool(PdfTool::RotateLeft)),
            vec![EngineId::Qpdf]
        );
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfTool(PdfTool::OptimizeWeb)),
            vec![EngineId::Qpdf]
        );
        // Split and merge: Poppler first, qpdf as the one automatic retry.
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfTool(PdfTool::Split)),
            vec![EngineId::Poppler, EngineId::Qpdf]
        );
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfTool(PdfTool::Merge)),
            vec![EngineId::Poppler, EngineId::Qpdf]
        );
        // Searchable: Poppler candidate, Tesseract gate (chain, not retry).
        assert_eq!(
            engines(FileKind::Pdf, Action::PdfTool(PdfTool::Searchable)),
            vec![EngineId::Poppler]
        );

        for extract in [
            PdfExtract::CopyText,
            PdfExtract::Text,
            PdfExtract::Images,
            PdfExtract::PagePng,
        ] {
            assert_eq!(
                engines(FileKind::Pdf, Action::PdfExtract(extract)),
                vec![EngineId::Poppler]
            );
        }
    }

    #[test]
    fn documents_route_pdf_doc_and_docx_to_libreoffice() {
        assert_eq!(
            engines(
                FileKind::Document,
                Action::DocumentConvert(DocumentFormat::Pdf)
            ),
            vec![EngineId::LibreOffice]
        );
        assert_eq!(
            engines(
                FileKind::Document,
                Action::DocumentConvert(DocumentFormat::Doc)
            ),
            vec![EngineId::LibreOffice]
        );
        // LibreOffice first, Pandoc as the single automatic retry.
        assert_eq!(
            engines(
                FileKind::Document,
                Action::DocumentConvert(DocumentFormat::Docx)
            ),
            vec![EngineId::LibreOffice, EngineId::Pandoc]
        );
        assert_eq!(
            engines(
                FileKind::Document,
                Action::DocumentConvert(DocumentFormat::Markdown)
            ),
            vec![EngineId::Pandoc]
        );
        // Extract: images unpack in Native, text goes through Pandoc.
        assert_eq!(
            engines(
                FileKind::Document,
                Action::DocumentExtract(crate::engines::DocumentExtract::Images)
            ),
            vec![EngineId::Native]
        );
        assert_eq!(
            engines(
                FileKind::Document,
                Action::DocumentExtract(crate::engines::DocumentExtract::PlainText)
            ),
            vec![EngineId::Pandoc]
        );
    }

    #[test]
    fn sheet_presentation_multi_and_zip_route_per_spec() {
        // Everything but JSON is LibreOffice; JSON is built into Native.
        for target in [
            SheetFormat::Pdf,
            SheetFormat::Csv,
            SheetFormat::Xlsx,
            SheetFormat::Ods,
            SheetFormat::Html,
            SheetFormat::Xls,
            SheetFormat::Tsv,
        ] {
            assert_eq!(
                engines(FileKind::Spreadsheet, Action::SheetConvert(target)),
                vec![EngineId::LibreOffice]
            );
        }
        assert_eq!(
            engines(
                FileKind::Spreadsheet,
                Action::SheetConvert(SheetFormat::Json)
            ),
            vec![EngineId::Native]
        );

        for target in [
            PresentFormat::Pdf,
            PresentFormat::Pptx,
            PresentFormat::Ppt,
            PresentFormat::Odp,
        ] {
            assert_eq!(
                engines(FileKind::Presentation, Action::PresentConvert(target)),
                vec![EngineId::LibreOffice]
            );
        }
        // PNG slides are a LibreOffice → Poppler chain: Poppler gates it.
        assert_eq!(
            engines(
                FileKind::Presentation,
                Action::PresentConvert(PresentFormat::Png)
            ),
            vec![EngineId::LibreOffice]
        );

        assert_eq!(
            engines(FileKind::Multi, Action::Multi(MultiOp::MergePdf)),
            vec![EngineId::Native, EngineId::Poppler, EngineId::Qpdf]
        );
        for op in [
            MultiOp::Zip,
            MultiOp::BatchConvert,
            MultiOp::BatchResize,
            MultiOp::BatchCompress,
            MultiOp::BatchStripMetadata,
        ] {
            assert_eq!(
                engines(FileKind::Multi, Action::Multi(op)),
                vec![EngineId::Native]
            );
        }

        assert_eq!(
            engines(
                FileKind::Document,
                Action::Zip(crate::engines::ZipOp::ExtractHere)
            ),
            vec![EngineId::Native]
        );
    }

    #[test]
    fn gif_source_offers_mp4_through_ffmpeg() {
        assert_eq!(
            engines(FileKind::Image, Action::VideoConvert(VideoFormat::Mp4)),
            vec![EngineId::Ffmpeg]
        );
        // Without FFmpeg the GIF→MP4 tile hides like every other FFmpeg job.
        let mut no_ffmpeg = table();
        no_ffmpeg.set(EngineId::Ffmpeg, EngineCapabilities::default());
        assert!(route(
            FileKind::Image,
            Action::VideoConvert(VideoFormat::Mp4),
            &no_ffmpeg
        )
        .is_empty());
    }

    /// Test 5: a missing engine hides exactly its own options.
    #[test]
    fn missing_v08_engines_hide_their_options() {
        // No LibreOffice ⇒ PDF→DOCX/ODT, doc→PDF/DOC, every sheet and every
        // presentation tile is gone.
        let no_lo = table_without(EngineId::LibreOffice);
        assert!(route(FileKind::Pdf, Action::PdfConvert(PdfTarget::Docx), &no_lo).is_empty());
        assert!(route(FileKind::Pdf, Action::PdfConvert(PdfTarget::Odt), &no_lo).is_empty());
        assert!(route(
            FileKind::Document,
            Action::DocumentConvert(DocumentFormat::Pdf),
            &no_lo
        )
        .is_empty());
        for target in [SheetFormat::Pdf, SheetFormat::Csv, SheetFormat::Xls] {
            assert!(route(FileKind::Spreadsheet, Action::SheetConvert(target), &no_lo).is_empty());
        }
        for target in [PresentFormat::Pdf, PresentFormat::Pptx, PresentFormat::Png] {
            assert!(route(
                FileKind::Presentation,
                Action::PresentConvert(target),
                &no_lo
            )
            .is_empty());
        }

        // No Poppler ⇒ TXT, page raster, Markdown (gate) and every extract
        // tile is gone; split/merge keep their qpdf retry.
        let no_poppler = table_without(EngineId::Poppler);
        for target in [
            PdfTarget::Txt,
            PdfTarget::Png,
            PdfTarget::Jpeg,
            PdfTarget::Markdown,
        ] {
            assert!(route(FileKind::Pdf, Action::PdfConvert(target), &no_poppler).is_empty());
        }
        for extract in [
            PdfExtract::CopyText,
            PdfExtract::Text,
            PdfExtract::Images,
            PdfExtract::PagePng,
        ] {
            assert!(route(FileKind::Pdf, Action::PdfExtract(extract), &no_poppler).is_empty());
        }
        assert_eq!(
            route(FileKind::Pdf, Action::PdfTool(PdfTool::Split), &no_poppler)
                .into_iter()
                .map(|candidate| candidate.engine)
                .collect::<Vec<_>>(),
            vec![EngineId::Qpdf]
        );

        // No qpdf ⇒ rotate and linearize are gone.
        let no_qpdf = table_without(EngineId::Qpdf);
        for tool in [
            PdfTool::RotateLeft,
            PdfTool::RotateRight,
            PdfTool::OptimizeWeb,
        ] {
            assert!(route(FileKind::Pdf, Action::PdfTool(tool), &no_qpdf).is_empty());
        }

        // No Ghostscript ⇒ compress is gone.
        let no_gs = table_without(EngineId::Ghostscript);
        assert!(route(FileKind::Pdf, Action::PdfTool(PdfTool::Compress), &no_gs).is_empty());

        // No Tesseract ⇒ searchable (and image OCR) are gone.
        let no_tesseract = table_without(EngineId::Tesseract);
        assert!(route(
            FileKind::Pdf,
            Action::PdfTool(PdfTool::Searchable),
            &no_tesseract
        )
        .is_empty());
        assert!(route(
            FileKind::Image,
            Action::ImageOcr(OcrOutput::Txt),
            &no_tesseract
        )
        .is_empty());
    }

    /// Test 6: every action class with more than one candidate retries on the
    /// second one exactly once (MAX_ATTEMPTS).
    #[test]
    fn every_multi_candidate_class_retries_once() {
        let classes: Vec<(FileKind, Action)> = vec![
            (
                FileKind::Document,
                Action::DocumentConvert(DocumentFormat::Docx),
            ),
            (FileKind::Pdf, Action::PdfTool(PdfTool::Split)),
            (FileKind::Pdf, Action::PdfTool(PdfTool::Merge)),
            (FileKind::Multi, Action::Multi(MultiOp::MergePdf)),
            (FileKind::Image, Action::ImageConvert(ImageFormat::Png)),
        ];
        for (kind, action) in classes {
            let candidates = route(kind, action, &table());
            assert!(
                candidates.len() >= 2,
                "{kind:?}/{action:?} should offer a retry candidate"
            );
            let result = run_with_fallback(&candidates, |_| {
                Err::<(), _>(EngineError::Failed("broken".into()))
            });
            let failure = result.expect_err("every candidate fails");
            assert_eq!(
                failure.attempts, MAX_ATTEMPTS,
                "{kind:?}/{action:?} must retry exactly once"
            );
            assert_eq!(
                failure.last_engine,
                Some(candidates[MAX_ATTEMPTS - 1].engine),
                "{kind:?}/{action:?} must fail on its second candidate"
            );
        }
    }
}
