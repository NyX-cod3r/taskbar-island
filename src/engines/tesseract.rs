//! tesseract.rs — OCR, plus the Pandoc post-step for Markdown and DOCX.
//!
//! Tesseract writes plain text into the private workspace. The card never
//! renders that text as markup; it is only copied to the clipboard or handed
//! to Pandoc, which runs sandboxed like every other document job.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

use super::exec::{self, Spec};
use super::output::{self, TargetFormat, Workspace};
use super::{
    argv_path, Action, CancelToken, DocumentFormat, EngineError, ImageFormat, Job, JobOutput,
    OcrOutput, Progress, ProgressSink, Stage, DOCUMENT_TIMEOUT, OCR_TIMEOUT,
};

/// OCR text is copied to the clipboard, so it is bounded like every other
/// engine output instead of being read without a limit.
const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;

pub fn run(
    exe: &Path,
    pandoc: Option<&Path>,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    let output_kind = match job.action {
        Action::ImageOcr(output) => output,
        _ => return Err(EngineError::Unsupported),
    };
    let language = language(job.language.as_deref())
        .ok_or_else(|| EngineError::Failed("Choose an OCR language first.".into()))?;

    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let workspace = Workspace::new()?;
    let base = workspace.join("ocr");
    let mut args: Vec<OsString> = vec![
        argv_path(&job.input),
        argv_path(&base),
        "-l".into(),
        language.clone().into(),
        "--psm".into(),
        "3".into(),
    ];
    if let Some(tessdata) = tessdata_dir(exe) {
        args.push("--tessdata-dir".into());
        args.push(argv_path(&tessdata));
    }
    if matches!(output_kind, OcrOutput::SearchablePdf) {
        args.push("pdf".into());
    }
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    exec::run(
        &Spec {
            exe,
            args: &args,
            dir: workspace.dir(),
            timeout: OCR_TIMEOUT,
        },
        cancel,
        |_| {},
    )
    .map_err(|error| match error {
        EngineError::Failed(message) => EngineError::Failed(ocr_failure(&message)),
        other => other,
    })?;

    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });

    let result = match output_kind {
        OcrOutput::Clipboard => {
            let text = read_text(&base.with_extension("txt"))?;
            copy_to_clipboard(&text).map_err(|error| {
                EngineError::Failed(format!("The text couldn't reach the clipboard: {error}."))
            })?;
            JobOutput {
                output: PathBuf::new(),
                format: None,
                text: Some(text),
            }
        }
        OcrOutput::Txt => finish_file(
            &base.with_extension("txt"),
            TargetFormat::Document(DocumentFormat::Txt),
            job,
        )?,
        OcrOutput::SearchablePdf => finish_file(
            &base.with_extension("pdf"),
            TargetFormat::Image(ImageFormat::Pdf),
            job,
        )?,
        OcrOutput::Markdown | OcrOutput::Docx => {
            let pandoc = pandoc
                .ok_or_else(|| EngineError::Failed("Pandoc is needed for this format.".into()))?;
            let (format, extension) = if matches!(output_kind, OcrOutput::Markdown) {
                (DocumentFormat::Markdown, "md")
            } else {
                (DocumentFormat::Docx, "docx")
            };
            let converted = workspace.join(&format!("converted.{extension}"));
            let args = vec![
                "--sandbox".into(),
                "-f".into(),
                "markdown".into(),
                "-t".into(),
                if matches!(format, DocumentFormat::Markdown) {
                    "markdown".into()
                } else {
                    "docx".into()
                },
                "-o".into(),
                argv_path(&converted),
                argv_path(&base.with_extension("txt")),
            ];
            exec::run(
                &Spec {
                    exe: pandoc,
                    args: &args,
                    dir: workspace.dir(),
                    timeout: DOCUMENT_TIMEOUT,
                },
                cancel,
                |_| {},
            )?;
            finish_file(&converted, TargetFormat::Document(format), job)?
        }
    };

    progress(Progress {
        stage: Stage::Done,
        fraction: Some(1.0),
    });
    Ok(result)
}

fn finish_file(path: &Path, target: TargetFormat, job: &Job) -> Result<JobOutput, EngineError> {
    output::verify(path, target)
        .map_err(|_| EngineError::Failed("Tesseract produced an unreadable file.".into()))?;
    let (delivered, _) =
        output::deliver_in(job.output.parent(), path, &job.input, job.action, target)
            .map_err(EngineError::Failed)?;
    Ok(JobOutput {
        output: delivered,
        format: Some(target),
        text: None,
    })
}

/// Tesseract accepts language names such as `eng` or `chi_sim`. Anything else
/// is rejected rather than passed through as an argv item.
fn language(requested: Option<&str>) -> Option<String> {
    let language = requested?.trim();
    let valid = !language.is_empty()
        && language.len() <= 40
        && language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+');
    valid.then(|| language.to_owned())
}

fn tessdata_dir(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?.join("tessdata");
    dir.is_dir().then_some(dir)
}

fn read_text(path: &Path) -> Result<String, EngineError> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| EngineError::Failed("Tesseract produced no text.".into()))?;
    if metadata.len() > MAX_TEXT_BYTES {
        return Err(EngineError::Failed(
            "The recognized text is too large to copy.".into(),
        ));
    }
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|_| EngineError::Failed("The recognized text couldn't be read.".into()))
}

fn ocr_failure(message: &str) -> String {
    if message.contains("Failed loading language")
        || message.contains("Could not initialize tesseract")
        || message.contains("tessdata")
    {
        "Tesseract couldn't load its language data.".into()
    } else {
        "Tesseract couldn't read this image.".into()
    }
}

/// Puts plain text on the clipboard. The worker owns the clipboard only for
/// the duration of this call; retries cover another app holding it open.
fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let mut opened = false;
    for _ in 0..10 {
        // SAFETY: no window owns the clipboard; the call is paired with
        // CloseClipboard below.
        if unsafe { OpenClipboard(None) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !opened {
        return Err("another app is using it".into());
    }
    // SAFETY: clipboard ownership was acquired above and is released on every
    // exit path below.
    unsafe {
        EmptyClipboard().map_err(|error| error.to_string())?;
        let handle =
            GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).map_err(|error| error.to_string())?;
        let pointer = GlobalLock(handle);
        if pointer.is_null() {
            let _ = CloseClipboard();
            return Err("the clipboard allocation failed".into());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer.cast::<u16>(), wide.len());
        let _ = GlobalUnlock(handle);
        let result = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(handle.0)))
            .map(|_| ())
            .map_err(|error| error.to_string());
        let _ = CloseClipboard();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_language_names_are_accepted() {
        assert_eq!(language(Some("eng")), Some("eng".into()));
        assert_eq!(language(Some("chi_sim")), Some("chi_sim".into()));
        assert_eq!(language(Some("eng+deu")), Some("eng+deu".into()));
        assert_eq!(language(Some("-rf")), None);
        assert_eq!(language(Some("a b")), None);
        assert_eq!(language(None), None);
    }

    #[test]
    fn generated_language_file_lives_beside_the_engine() {
        assert_eq!(language(Some("deu")), Some("deu".into()));
    }
}
