//! poppler.rs — everything that reads a PDF as pages or text.
//!
//! Poppler ships as a folder of sibling tools; the located executable is
//! `pdftotext.exe` and every other tool is looked up beside it, never on
//! `PATH`. Text extraction is checked for "this is a scan": a first-pass
//! `pdftotext` that comes back essentially empty routes to the OCR chain
//! (rasterise → Tesseract → text), which is why Tesseract gates that route
//! instead of being a retry.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::exec::{self, Spec};
use super::output::{self, TargetFormat, Workspace};
use super::tesseract;
use super::{
    argv_path, Action, CancelToken, DocumentFormat, EngineError, EngineId, EnginePaths,
    ImageFormat, Job, JobOutput, PdfExtract, PdfTarget, PdfTool, Progress, ProgressSink, Stage,
    DOCUMENT_TIMEOUT, OCR_TIMEOUT,
};

/// A scanned page produces a handful of characters of noise; anything below
/// this many non-whitespace characters is treated as an image-only PDF.
const BLANK_TEXT_CHARS: usize = 24;
/// OCR rasterisation DPI (spec: 300).
const OCR_DPI: &str = "300";
/// Preview/page-image DPI.
const IMAGE_DPI: &str = "150";

pub fn run(
    exe: &Path,
    paths: &EnginePaths,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    match job.action {
        Action::PdfConvert(target) => pdf_convert(exe, paths, job, target, progress, cancel),
        Action::PdfTool(PdfTool::Split) => split(exe, job, progress, cancel),
        Action::PdfTool(PdfTool::Searchable) => searchable(exe, paths, job, progress, cancel),
        Action::PdfExtract(extract) => pdf_extract(exe, paths, job, extract, progress, cancel),
        // Merge is a whole-selection job handled by the batch runner.
        _ => Err(EngineError::Unsupported),
    }
}

/// A Poppler tool beside the located executable. Missing siblings are an
/// `NotFound` so the router's next candidate gets its turn.
fn tool(exe: &Path, name: &str) -> Result<PathBuf, EngineError> {
    let path = exe.with_file_name(name);
    path.is_file().then_some(path).ok_or(EngineError::NotFound)
}

fn run_tool(
    exe: &Path,
    dir: &Path,
    args: &[OsString],
    timeout: std::time::Duration,
    cancel: &CancelToken,
) -> Result<(), EngineError> {
    exec::run(
        &Spec {
            exe,
            args,
            dir,
            timeout,
        },
        cancel,
        |_| {},
    )
    .map(|_| ())
}

// ------------------------------------------------------------------ convert

fn pdf_convert(
    exe: &Path,
    paths: &EnginePaths,
    job: &Job,
    target: PdfTarget,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    match target {
        PdfTarget::Txt | PdfTarget::Markdown => text_job(exe, paths, job, target, progress, cancel),
        PdfTarget::Png | PdfTarget::Jpeg => raster_folder(
            exe,
            job,
            progress,
            cancel,
            match target {
                PdfTarget::Png => ("png", ImageFormat::Png),
                _ => ("jpeg", ImageFormat::Jpeg),
            },
        ),
        // DOCX / ODT are LibreOffice's job; the router never picks Poppler
        // for them, so this is a guard, not a path.
        PdfTarget::Docx | PdfTarget::Odt => Err(EngineError::Unsupported),
    }
}

/// `pdftotext` first. When the result is essentially empty the PDF is a scan
/// and the OCR chain takes over; without Tesseract the user gets a clear
/// reason instead of an empty file.
fn text_job(
    exe: &Path,
    paths: &EnginePaths,
    job: &Job,
    target: PdfTarget,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let workspace = Workspace::new()?;
    let txt = workspace.join("out.txt");

    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    let pdftotext = tool(exe, "pdftotext.exe")?;
    run_tool(
        &pdftotext,
        workspace.dir(),
        &[
            "-enc".into(),
            "UTF-8".into(),
            argv_path(&job.input),
            txt.as_os_str().to_os_string(),
        ],
        DOCUMENT_TIMEOUT,
        cancel,
    )?;

    let mut text = read_capped(&txt)?;
    if is_blank(&text) {
        let tesseract = paths.location(EngineId::Tesseract);
        match tesseract {
            Some(tesseract) => {
                text = ocr_text(exe, tesseract, &job.input, &workspace, cancel)?;
            }
            None => {
                return Err(EngineError::Failed(
                    "This PDF is a scan — its text needs OCR (Tesseract).".into(),
                ));
            }
        }
    }

    let markdown = target == PdfTarget::Markdown;
    // The OCR route never wrote `out.txt`; normalising here keeps the delivery
    // path identical either way.
    std::fs::write(&txt, &text)
        .map_err(|error| EngineError::Failed(format!("temp file: {error}")))?;
    let delivered_text = if markdown {
        let pandoc = paths
            .location(EngineId::Pandoc)
            .ok_or(EngineError::NotFound)?;
        to_markdown(pandoc, &txt, &text, &workspace, cancel)?
    } else {
        text
    };

    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });
    if matches!(job.action, Action::PdfExtract(PdfExtract::CopyText)) {
        tesseract::copy_to_clipboard(&delivered_text).map_err(EngineError::Failed)?;
        progress(Progress {
            stage: Stage::Done,
            fraction: Some(1.0),
        });
        return Ok(JobOutput {
            output: PathBuf::new(),
            format: None,
            text: Some(delivered_text),
            note: None,
        });
    }

    let (format, path) = if markdown {
        (DocumentFormat::Markdown, workspace.join("out.md"))
    } else {
        (DocumentFormat::Txt, txt)
    };
    let target = TargetFormat::Document(format);
    output::verify(&path, target)
        .map_err(|_| EngineError::Failed("Poppler produced an unreadable file.".into()))?;
    let (delivered, _) =
        output::deliver_in(job.output.parent(), &path, &job.input, job.action, target)
            .map_err(EngineError::Failed)?;
    progress(Progress {
        stage: Stage::Done,
        fraction: Some(1.0),
    });
    Ok(JobOutput {
        output: delivered,
        format: Some(target),
        text: None,
        note: None,
    })
}

/// Plain text → Markdown through a sandboxed Pandoc step.
fn to_markdown(
    pandoc: &Path,
    source: &Path,
    text: &str,
    workspace: &Workspace,
    cancel: &CancelToken,
) -> Result<String, EngineError> {
    // The OCR path never wrote `out.txt`; make sure the reader exists.
    if !source.is_file() {
        std::fs::write(source, text)
            .map_err(|error| EngineError::Failed(format!("temp file: {error}")))?;
    }
    let out = workspace.join("out.md");
    run_tool(
        pandoc,
        workspace.dir(),
        &[
            "--sandbox".into(),
            "-f".into(),
            "plain".into(),
            "-t".into(),
            "markdown".into(),
            "-o".into(),
            out.as_os_str().to_os_string(),
            argv_path(source),
        ],
        DOCUMENT_TIMEOUT,
        cancel,
    )?;
    read_capped(&out)
}

// ------------------------------------------------------------------- raster

/// Every page of the PDF as images, delivered as one `<stem>_pages` folder.
fn raster_folder(
    exe: &Path,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
    (format, image): (&'static str, ImageFormat),
) -> Result<JobOutput, EngineError> {
    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let workspace = Workspace::new()?;
    let pages = workspace.join("pages");
    std::fs::create_dir_all(&pages)
        .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;

    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    let pdftoppm = tool(exe, "pdftoppm.exe")?;
    let prefix = pages.join("page");
    run_tool(
        &pdftoppm,
        workspace.dir(),
        &[
            "-r".into(),
            IMAGE_DPI.into(),
            format!("-{format}").into(),
            argv_path(&job.input),
            prefix.as_os_str().to_os_string(),
        ],
        DOCUMENT_TIMEOUT,
        cancel,
    )?;

    let files = sorted_outputs(&pages, format)?;
    if files.is_empty() {
        return Err(EngineError::Failed("Poppler produced no pages.".into()));
    }
    let first = files
        .first()
        .ok_or_else(|| EngineError::Failed("Poppler produced no pages.".into()))?;
    let target = TargetFormat::Image(image);
    output::verify(first, target)
        .map_err(|_| EngineError::Failed("Poppler produced an unreadable page.".into()))?;

    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });
    let (delivered, _) =
        output::deliver_folder_named_in(job.output.parent(), &pages, &job.input, "pages")
            .map_err(EngineError::Failed)?;
    progress(Progress {
        stage: Stage::Done,
        fraction: Some(1.0),
    });
    Ok(JobOutput {
        output: delivered,
        format: None,
        text: None,
        note: None,
    })
}

/// pdftoppm names its output `<prefix>-<n>.<ext>`; a numeric sort keeps page
/// 10 after page 9 whatever the padding.
fn sorted_outputs(dir: &Path, extension: &str) -> Result<Vec<PathBuf>, EngineError> {
    let entries = std::fs::read_dir(dir).map_err(|error| {
        EngineError::Failed(format!("the engine output couldn't be read: {error}"))
    })?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == extension)
        })
        .collect();
    files.sort_by_key(|path| {
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        stem.rsplit('-')
            .next()
            .and_then(|tail| tail.parse::<u64>().ok())
            .unwrap_or(u64::MAX)
    });
    Ok(files)
}

// -------------------------------------------------------------------- tools

fn split(
    exe: &Path,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let workspace = Workspace::new()?;
    let pages = workspace.join("pages");
    std::fs::create_dir_all(&pages)
        .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;

    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    let pdfseparate = tool(exe, "pdfseparate.exe")?;
    let pattern = pages.join("page-%d.pdf");
    run_tool(
        &pdfseparate,
        workspace.dir(),
        &[argv_path(&job.input), argv_path(&pattern)],
        DOCUMENT_TIMEOUT,
        cancel,
    )?;

    let files = sorted_outputs(&pages, "pdf")?;
    if files.is_empty() {
        return Err(EngineError::Failed("Poppler produced no pages.".into()));
    }

    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });
    let (delivered, _) =
        output::deliver_folder_named_in(job.output.parent(), &pages, &job.input, "pages")
            .map_err(EngineError::Failed)?;
    progress(Progress {
        stage: Stage::Done,
        fraction: Some(1.0),
    });
    Ok(JobOutput {
        output: delivered,
        format: None,
        text: None,
        note: None,
    })
}

/// Rasterise at 300 dpi, OCR each page, staple the page PDFs back together.
fn searchable(
    exe: &Path,
    paths: &EnginePaths,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    let tesseract_exe = paths
        .location(EngineId::Tesseract)
        .ok_or(EngineError::NotFound)?;
    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let workspace = Workspace::new()?;
    let raster = workspace.join("raster");
    std::fs::create_dir_all(&raster)
        .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;

    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    let pdftoppm = tool(exe, "pdftoppm.exe")?;
    let prefix = raster.join("page");
    run_tool(
        &pdftoppm,
        workspace.dir(),
        &[
            "-r".into(),
            OCR_DPI.into(),
            "-png".into(),
            argv_path(&job.input),
            prefix.as_os_str().to_os_string(),
        ],
        OCR_TIMEOUT,
        cancel,
    )?;
    let pages = sorted_outputs(&raster, "png")?;
    if pages.is_empty() {
        return Err(EngineError::Failed("Poppler produced no pages.".into()));
    }

    let mut layer: Vec<PathBuf> = Vec::with_capacity(pages.len());
    for (index, page) in pages.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let base = raster.join(format!("ocr-{index}"));
        let mut args: Vec<OsString> = vec![
            argv_path(page),
            argv_path(&base),
            "-l".into(),
            ocr_language(job.language.as_deref()),
            "--psm".into(),
            "3".into(),
            "pdf".into(),
        ];
        if let Some(tessdata) = tesseract::tessdata_dir(tesseract_exe) {
            args.push("--tessdata-dir".into());
            args.push(argv_path(&tessdata));
        }
        run_tool(tesseract_exe, workspace.dir(), &args, OCR_TIMEOUT, cancel)?;
        let produced = base.with_extension("pdf");
        if produced.is_file() {
            layer.push(produced);
        }
    }
    if layer.is_empty() {
        return Err(EngineError::Failed(
            "Tesseract couldn't read these pages.".into(),
        ));
    }

    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });
    let output_path = workspace.join("out.pdf");
    let pdfunite = tool(exe, "pdfunite.exe")?;
    let mut args: Vec<OsString> = layer.iter().map(|path| argv_path(path)).collect();
    args.push(output_path.as_os_str().to_os_string());
    run_tool(&pdfunite, workspace.dir(), &args, DOCUMENT_TIMEOUT, cancel)?;

    let target = TargetFormat::Image(ImageFormat::Pdf);
    output::verify(&output_path, target)
        .map_err(|_| EngineError::Failed("The searchable PDF couldn't be written.".into()))?;
    let (delivered, _) = output::deliver_in(
        job.output.parent(),
        &output_path,
        &job.input,
        job.action,
        target,
    )
    .map_err(EngineError::Failed)?;
    progress(Progress {
        stage: Stage::Done,
        fraction: Some(1.0),
    });
    Ok(JobOutput {
        output: delivered,
        format: Some(target),
        text: None,
        note: None,
    })
}

fn pdf_extract(
    exe: &Path,
    paths: &EnginePaths,
    job: &Job,
    extract: PdfExtract,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    match extract {
        PdfExtract::CopyText | PdfExtract::Text => {
            text_job(exe, paths, job, PdfTarget::Txt, progress, cancel)
        }
        PdfExtract::PagePng => first_page(exe, job, progress, cancel),
        PdfExtract::Images => embedded_images(exe, job, progress, cancel),
    }
}

fn first_page(
    exe: &Path,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let workspace = Workspace::new()?;
    let page = workspace.join("page");

    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    let pdftoppm = tool(exe, "pdftoppm.exe")?;
    run_tool(
        &pdftoppm,
        workspace.dir(),
        &[
            "-f".into(),
            "1".into(),
            "-l".into(),
            "1".into(),
            "-singlefile".into(),
            "-r".into(),
            IMAGE_DPI.into(),
            "-png".into(),
            argv_path(&job.input),
            page.as_os_str().to_os_string(),
        ],
        DOCUMENT_TIMEOUT,
        cancel,
    )?;
    let produced = page.with_extension("png");
    let target = TargetFormat::Image(ImageFormat::Png);
    output::verify(&produced, target)
        .map_err(|_| EngineError::Failed("Poppler produced an unreadable page.".into()))?;

    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });
    let (delivered, _) = output::deliver_in(
        job.output.parent(),
        &produced,
        &job.input,
        job.action,
        target,
    )
    .map_err(EngineError::Failed)?;
    progress(Progress {
        stage: Stage::Done,
        fraction: Some(1.0),
    });
    Ok(JobOutput {
        output: delivered,
        format: Some(target),
        text: None,
        note: None,
    })
}

fn embedded_images(
    exe: &Path,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let workspace = Workspace::new()?;
    let folder = workspace.join("images");
    std::fs::create_dir_all(&folder)
        .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;

    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    let pdfimages = tool(exe, "pdfimages.exe")?;
    let prefix = folder.join("image");
    run_tool(
        &pdfimages,
        workspace.dir(),
        &[
            "-all".into(),
            argv_path(&job.input),
            prefix.as_os_str().to_os_string(),
        ],
        DOCUMENT_TIMEOUT,
        cancel,
    )?;
    let entries = std::fs::read_dir(&folder)
        .map_err(|_| EngineError::Failed("The images couldn't be read.".into()))?;
    if !entries.flatten().any(|entry| entry.path().is_file()) {
        return Err(EngineError::Failed(
            "This PDF has no embedded images.".into(),
        ));
    }

    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });
    let (delivered, _) =
        output::deliver_folder_named_in(job.output.parent(), &folder, &job.input, "images")
            .map_err(EngineError::Failed)?;
    progress(Progress {
        stage: Stage::Done,
        fraction: Some(1.0),
    });
    Ok(JobOutput {
        output: delivered,
        format: None,
        text: None,
        note: None,
    })
}

// -------------------------------------------------------------------- ocr

/// Rasterise every page and OCR it to plain text.
fn ocr_text(
    poppler: &Path,
    tesseract_exe: &Path,
    input: &Path,
    workspace: &Workspace,
    cancel: &CancelToken,
) -> Result<String, EngineError> {
    let raster = workspace.join("raster");
    std::fs::create_dir_all(&raster)
        .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;
    let pdftoppm = tool(poppler, "pdftoppm.exe")?;
    let prefix = raster.join("page");
    run_tool(
        &pdftoppm,
        workspace.dir(),
        &[
            "-r".into(),
            OCR_DPI.into(),
            "-png".into(),
            argv_path(input),
            prefix.as_os_str().to_os_string(),
        ],
        OCR_TIMEOUT,
        cancel,
    )?;
    let pages = sorted_outputs(&raster, "png")?;
    if pages.is_empty() {
        return Err(EngineError::Failed("Poppler produced no pages.".into()));
    }

    let mut text = String::new();
    for (index, page) in pages.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let base = raster.join(format!("ocr-{index}"));
        let mut args: Vec<OsString> = vec![
            argv_path(page),
            argv_path(&base),
            "-l".into(),
            "eng".into(),
            "--psm".into(),
            "3".into(),
        ];
        if let Some(tessdata) = tesseract::tessdata_dir(tesseract_exe) {
            args.push("--tessdata-dir".into());
            args.push(argv_path(&tessdata));
        }
        run_tool(tesseract_exe, workspace.dir(), &args, OCR_TIMEOUT, cancel)?;
        let produced = base.with_extension("txt");
        if produced.is_file() {
            let page_text = tesseract::read_text(&produced)?;
            text.push_str(&page_text);
            text.push('\n');
        }
    }
    if is_blank(&text) {
        return Err(EngineError::Failed(
            "Tesseract couldn't read this scan.".into(),
        ));
    }
    Ok(text)
}

fn ocr_language(requested: Option<&str>) -> OsString {
    // `Job::language` is already validated by the OCR option; the default is
    // English, exactly like the installer's default language pack.
    match requested {
        Some(language) if !language.is_empty() => OsString::from(language),
        _ => OsString::from("eng"),
    }
}

/// `pdfunite in1 in2 … out`: Poppler's merge, in selection order.
pub(crate) fn merge_args(inputs: &[PathBuf], output: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = inputs.iter().map(|input| argv_path(input)).collect();
    args.push(output.as_os_str().to_os_string());
    args
}

// ------------------------------------------------------------------ helpers

fn read_capped(path: &Path) -> Result<String, EngineError> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| EngineError::Failed("Poppler produced no text.".into()))?;
    if metadata.len() > 8 * 1024 * 1024 {
        return Err(EngineError::Failed(
            "This PDF's text is too large to handle.".into(),
        ));
    }
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|_| EngineError::Failed("The extracted text couldn't be read.".into()))
}

fn is_blank(text: &str) -> bool {
    text.chars().filter(|c| !c.is_whitespace()).count() < BLANK_TEXT_CHARS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_tools_resolve_next_to_the_located_executable() {
        let exe = Path::new(r"C:\engines\poppler\Library\bin\pdftotext.exe");
        assert_eq!(
            exe.with_file_name("pdftoppm.exe"),
            Path::new(r"C:\engines\poppler\Library\bin\pdftoppm.exe")
        );
    }

    #[test]
    fn blank_detection_separates_a_scan_from_a_text_page() {
        assert!(is_blank(""));
        assert!(is_blank("\n \n\t"));
        assert!(is_blank(" .\n"));
        assert!(!is_blank("Invoice total: 1,204.55 due on 3 May."));
    }

    #[test]
    fn page_outputs_sort_numerically() {
        let dir = std::env::temp_dir().join(format!("ti-poppler-sort-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["page-1.png", "page-10.png", "page-2.png", "page-x.png"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let sorted = sorted_outputs(&dir, "png").unwrap();
        let names: Vec<String> = sorted
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["page-1.png", "page-2.png", "page-10.png", "page-x.png"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ocr_language_defaults_to_english() {
        assert_eq!(ocr_language(None), OsString::from("eng"));
        assert_eq!(ocr_language(Some("deu")), OsString::from("deu"));
        assert_eq!(ocr_language(Some("")), OsString::from("eng"));
    }
}
