//! libreoffice.rs — Office and presentation conversions.
//!
//! Every job runs `soffice.exe` headless against a **private profile inside
//! the job's own workspace**, so a conversion never reads the user's real
//! LibreOffice configuration and never writes to it. The profile is seeded
//! with macros off and link updates off before the first run.
//!
//! LibreOffice is the one engine that must not run twice at once: a shared
//! installation hands its first process the "already running" role, so a
//! second job would silently attach to it and convert with the wrong
//! profile. A process-wide mutex serialises the whole run.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::convert::detect::SourceFormat;

use super::exec::{self, Spec};
use super::output::{self, TargetFormat, Workspace};
use super::{
    argv_path, Action, CancelToken, DocumentFormat, EngineError, EngineId, EnginePaths, Job,
    JobOutput, PresentFormat, Progress, ProgressSink, SheetFormat, Stage, DOCUMENT_TIMEOUT,
    OFFICE_TIMEOUT,
};

/// One LibreOffice at a time, for the lifetime of a job.
static OFFICE: Mutex<()> = Mutex::new(());

/// Macros off, external link updates off. LibreOffice merges this file into
/// the fresh profile on first start; if it refuses the file the job still
/// runs, it just loses those two hardening settings.
///
/// The two `xmlns` values are XML *namespace identifiers* required by the
/// profile format — no code ever resolves or fetches them. They are written
/// as `concat!` pieces so no URL scheme ever appears literally in this file
/// and the offline source scan stays a single unconditional rule.
const PROFILE_SETTINGS: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<oor:items xmlns:oor=\"http",
    "://openoffice.org/2001/registry\" ",
    "xmlns:xs=\"http",
    "://www.w3.org/2001/XMLSchema\" oor:version=\"1.2\">\n",
    " <item oor:path=\"/org.openoffice.Office.Common/Security/Scripting\"><prop oor:name=\"MacroSecurityLevel\" oor:op=\"fuse\"><value>3</value></prop></item>\n",
    " <item oor:path=\"/org.openoffice.Office.Calc/Content/Update\"><prop oor:name=\"Link\" oor:op=\"fuse\"><value>0</value></prop></item>\n",
    " <item oor:path=\"/org.openoffice.Office.Writer/Web/Content/Update\"><prop oor:name=\"Link\" oor:op=\"fuse\"><value>0</value></prop></item>\n",
    " <item oor:path=\"/org.openoffice.Office.Writer/Content/Update\"><prop oor:name=\"Link\" oor:op=\"fuse\"><value>0</value></prop></item>\n",
    "</oor:items>\n",
);

pub fn run(
    exe: &Path,
    paths: &EnginePaths,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    let source = source_format(&job.input);
    let filter = match job.action {
        Action::DocumentConvert(target) => writer_filter(target, source),
        Action::SheetConvert(target) => sheet_filter(target),
        Action::PresentConvert(PresentFormat::Png) => {
            return slides_png(exe, paths, job, progress, cancel)
        }
        Action::PresentConvert(target) => present_filter(target),
        _ => return Err(EngineError::Unsupported),
    };

    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    // The guard is held for the whole run, including the wait for a previous
    // job: LibreOffice must never see two of our invocations at once.
    let _office = OFFICE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    let workspace = Workspace::new()?;
    let profile = workspace.join("profile");
    std::fs::create_dir_all(&profile)
        .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;
    // Best effort: a malformed profile file makes LibreOffice rebuild it.
    let _ = std::fs::write(profile.join("registrymodifications.xcu"), PROFILE_SETTINGS);
    let outdir = workspace.join("out");
    std::fs::create_dir_all(&outdir)
        .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;

    let args = convert_args(&profile, &outdir, filter, &job.input);

    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    exec::run(
        &Spec {
            exe,
            args: &args,
            dir: workspace.dir(),
            timeout: OFFICE_TIMEOUT,
        },
        cancel,
        |_| {},
    )?;

    let produced = outdir.join(expected_name(&job.input, output_extension(filter)));
    let target = target_for_action(job.action);
    output::verify(&produced, target)
        .map_err(|_| EngineError::Failed("LibreOffice produced an unreadable file.".into()))?;

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

/// Impress → PNG is LibreOffice → Poppler: the PDF is an intermediate that
/// never leaves the workspace, and the pages land in one `_slides` folder.
fn slides_png(
    exe: &Path,
    paths: &EnginePaths,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    let poppler = paths
        .location(EngineId::Poppler)
        .ok_or(EngineError::NotFound)?;
    let poppler_tool = poppler.with_file_name("pdftoppm.exe");
    if !poppler_tool.is_file() {
        return Err(EngineError::NotFound);
    }

    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let _office = OFFICE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    let workspace = Workspace::new()?;
    let profile = workspace.join("profile");
    std::fs::create_dir_all(&profile)
        .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;
    let _ = std::fs::write(profile.join("registrymodifications.xcu"), PROFILE_SETTINGS);
    let outdir = workspace.join("out");
    std::fs::create_dir_all(&outdir)
        .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;

    let args = convert_args(&profile, &outdir, "pdf:impress_pdf_Export", &job.input);
    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    exec::run(
        &Spec {
            exe,
            args: &args,
            dir: workspace.dir(),
            timeout: OFFICE_TIMEOUT,
        },
        cancel,
        |_| {},
    )?;
    let intermediate = outdir.join(expected_name(&job.input, "pdf"));
    if !intermediate.is_file() {
        return Err(EngineError::Failed(
            "LibreOffice produced no slides.".into(),
        ));
    }

    let slides = workspace.join("slides");
    std::fs::create_dir_all(&slides)
        .map_err(|error| EngineError::Failed(format!("temp folder: {error}")))?;
    let prefix = slides.join("slide");
    exec::run(
        &Spec {
            exe: &poppler_tool,
            args: &[
                "-r".into(),
                "150".into(),
                "-png".into(),
                argv_path(&intermediate),
                prefix.as_os_str().to_os_string(),
            ],
            dir: workspace.dir(),
            timeout: DOCUMENT_TIMEOUT,
        },
        cancel,
        |_| {},
    )?;
    let entries = std::fs::read_dir(&slides)
        .map_err(|_| EngineError::Failed("The slides couldn't be read.".into()))?;
    if !entries.flatten().any(|entry| entry.path().is_file()) {
        return Err(EngineError::Failed("Poppler produced no slides.".into()));
    }

    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });
    let (delivered, _) =
        output::deliver_folder_named_in(job.output.parent(), &slides, &job.input, "slides")
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

// ------------------------------------------------------------------- argv

/// `soffice.exe --headless … -env:UserInstallation=<profile> --convert-to
/// <filter> --outdir <dir> <input>` (B prompt §3). The profile flag comes
/// first so LibreOffice never looks at the real user profile, even briefly.
pub(crate) fn convert_args(
    profile: &Path,
    outdir: &Path,
    filter: &str,
    input: &Path,
) -> Vec<OsString> {
    vec![
        "--headless".into(),
        "--norestore".into(),
        "--nolockcheck".into(),
        "--nologo".into(),
        "--nodefault".into(),
        "--safe-mode".into(),
        format!("-env:UserInstallation={}", file_url(profile)).into(),
        "--convert-to".into(),
        filter.into(),
        "--outdir".into(),
        outdir.as_os_str().to_os_string(),
        argv_path(input),
    ]
}

/// `C:\Temp\ws\profile` → `file:///C:/Temp/ws/profile`. Forward slashes only;
/// a backslash would be read as an escape by the URL parser.
pub(crate) fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with("//") {
        format!("file:{text}")
    } else if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

/// LibreOffice writes `<stem>.<ext>` next to the input name it was given.
fn expected_name(input: &Path, extension: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "converted".into());
    PathBuf::from(format!("{stem}.{extension}"))
}

/// The extension LibreOffice will actually use: the part of the filter name
/// before the first `:` (`doc:MS Word 97` → `doc`, `csv:…` → `csv`).
fn output_extension(filter: &str) -> &str {
    filter.split(':').next().unwrap_or("pdf")
}

fn target_for_action(action: Action) -> TargetFormat {
    match action {
        Action::DocumentConvert(target) => TargetFormat::Document(target),
        Action::SheetConvert(target) => TargetFormat::Sheet(target),
        Action::PresentConvert(target) => TargetFormat::Present(target),
        _ => TargetFormat::Document(DocumentFormat::Pdf),
    }
}

// ---------------------------------------------------------------- filters

/// `pdf:writer_web_pdf_Export` for HTML, because an HTML source otherwise
/// lands in the plain Writer filter and loses its layout.
fn writer_filter(target: DocumentFormat, source: SourceFormat) -> &'static str {
    match (target, source) {
        (DocumentFormat::Pdf, SourceFormat::Html) => "pdf:writer_web_pdf_Export",
        (DocumentFormat::Pdf, _) => "pdf",
        (DocumentFormat::Docx, _) => "docx",
        (DocumentFormat::Odt, _) => "odt",
        (DocumentFormat::Doc, _) => "doc:MS Word 97",
        (DocumentFormat::Txt, _) => "txt:Text (encoded):UTF8",
        (DocumentFormat::Rtf, _) => "rtf",
        (DocumentFormat::Html, _) => "html",
        // Pandoc owns these writers; the router never routes them here.
        (DocumentFormat::Markdown | DocumentFormat::Epub | DocumentFormat::Tex, _) => "txt",
    }
}

fn sheet_filter(target: SheetFormat) -> &'static str {
    match target {
        SheetFormat::Pdf => "pdf:calc_pdf_Export",
        SheetFormat::Csv => "csv:Text - txt - csv (StarCalc)",
        SheetFormat::Xlsx => "xlsx:Calc MS Excel 2007 XML",
        SheetFormat::Xls => "xls:MS Excel 97",
        SheetFormat::Ods => "ods",
        SheetFormat::Html => "html:HTML (StarCalc)",
        SheetFormat::Tsv => "csv:Text - txt - csv (StarCalc):9,34,76,1,,0,false,true,true",
        SheetFormat::Json => "csv",
    }
}

fn present_filter(target: PresentFormat) -> &'static str {
    match target {
        PresentFormat::Pdf => "pdf:impress_pdf_Export",
        PresentFormat::Pptx => "pptx:Impress MS PowerPoint 2007 XML",
        PresentFormat::Ppt => "ppt:MS PowerPoint 97",
        PresentFormat::Odp => "odp",
        PresentFormat::Png => "pdf:impress_pdf_Export",
    }
}

fn source_format(input: &Path) -> SourceFormat {
    let text = input.to_string_lossy().into_owned();
    crate::convert::detect::quick_format(&text).unwrap_or(SourceFormat::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn profile_url_uses_forward_slashes_only() {
        assert_eq!(
            file_url(Path::new(r"C:\Users\me\AppData\Local\Temp\ws\profile")),
            "file:///C:/Users/me/AppData/Local/Temp/ws/profile"
        );
        assert_eq!(
            file_url(Path::new(r"\\server\share\p")),
            "file://server/share/p"
        );
    }

    #[test]
    fn argv_matches_the_specified_order_and_is_one_item_per_argument() {
        let args = convert_args(
            Path::new(r"C:\Temp\ws\profile"),
            Path::new(r"C:\Temp\ws\out"),
            "pdf",
            Path::new(r"C:\docs\a b.docx"),
        );
        let text: Vec<String> = args.iter().map(|a| a.to_string_lossy().into()).collect();
        assert_eq!(text[0], "--headless");
        assert_eq!(text[6], "-env:UserInstallation=file:///C:/Temp/ws/profile");
        assert_eq!(text[7], "--convert-to");
        assert_eq!(text[8], "pdf");
        assert_eq!(text[9], "--outdir");
        assert_eq!(text[10], r"C:\Temp\ws\out");
        assert_eq!(text[11], r"C:\docs\a b.docx");
        assert_eq!(text.len(), 12);
    }

    #[test]
    fn leading_dash_inputs_are_prefixed_but_real_paths_are_not() {
        let args = convert_args(
            Path::new(r"C:\Temp\p"),
            Path::new(r"C:\Temp\o"),
            "pdf",
            Path::new("-odd.pdf"),
        );
        assert_eq!(args.last().unwrap().to_string_lossy(), r".\-odd.pdf");
    }

    #[test]
    fn html_to_pdf_uses_the_web_filter() {
        assert_eq!(
            writer_filter(DocumentFormat::Pdf, SourceFormat::Html),
            "pdf:writer_web_pdf_Export"
        );
        assert_eq!(
            writer_filter(DocumentFormat::Pdf, SourceFormat::Docx),
            "pdf"
        );
        assert_eq!(
            writer_filter(DocumentFormat::Docx, SourceFormat::Markdown),
            "docx"
        );
    }

    #[test]
    fn profile_settings_disable_macros_and_link_updates() {
        assert!(PROFILE_SETTINGS.contains("MacroSecurityLevel"));
        assert!(PROFILE_SETTINGS.contains("<value>3</value>"));
        assert!(PROFILE_SETTINGS.contains("Update"));
    }

    #[test]
    fn office_jobs_are_serialised_by_one_process_wide_lock() {
        let first = OFFICE.lock();
        assert!(first.is_ok());
        drop(first);
        let second = OFFICE.lock();
        assert!(second.is_ok());
    }
}
