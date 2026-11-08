//! router.rs — the instant engine choice.
//!
//! The route is computed from the capability matrix only. There is no
//! benchmarking at click time: candidates are ordered by the static preference
//! in the v0.6 spec, so the answer is a table lookup in well under a
//! millisecond.

use super::{
    Action, CapabilityTable, DocumentFormat, EngineError, EngineId, FileKind, ImageFormat,
    ImageOptimize, JobOutput, OcrOutput, VideoExtract, VideoFormat,
};

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
            let pandoc = capabilities.get(EngineId::Pandoc);
            if pandoc.installed && pandoc.can_write_document(target) {
                candidates.push(Candidate::new(EngineId::Pandoc, 0));
            }
        }
        (FileKind::Document, Action::DocumentExtract(_)) => {
            let pandoc = capabilities.get(EngineId::Pandoc);
            if pandoc.installed && !pandoc.documents_out.is_empty() {
                candidates.push(Candidate::new(EngineId::Pandoc, 0));
            }
        }
        _ => {}
    }
    candidates
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
        AudioFormat, AudioOptimize, CancelToken, EngineCapabilities, ImageEdit, VideoEdit,
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
}
