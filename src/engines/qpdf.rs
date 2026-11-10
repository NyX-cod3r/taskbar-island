//! qpdf.rs — structural PDF work: rotate, linearise, split, merge.
//!
//! qpdf changes the container, never the page content, so every job here is
//! a pure structure rewrite with no quality decision involved. `--empty
//! --pages … --` is the merge form: the output is created empty and the
//! inputs are copied into it in the order they were selected, which keeps
//! shelf order.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::exec::{self, Spec};
use super::output::{self, TargetFormat, Workspace};
use super::{
    argv_path, Action, CancelToken, EngineError, ImageFormat, Job, JobOutput, PdfTool, Progress,
    ProgressSink, Stage, DOCUMENT_TIMEOUT,
};

pub fn run(
    exe: &Path,
    _paths: &super::EnginePaths,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    match job.action {
        Action::PdfTool(PdfTool::RotateLeft) => rotate(exe, job, progress, cancel, "+90:1-z"),
        Action::PdfTool(PdfTool::RotateRight) => rotate(exe, job, progress, cancel, "-90:1-z"),
        Action::PdfTool(PdfTool::OptimizeWeb) => linearize(exe, job, progress, cancel),
        Action::PdfTool(PdfTool::Split) => split(exe, job, progress, cancel),
        // Merge is a whole-selection job (jobs.rs `run_batch`); the argv
        // builder below is what that path calls. A single-file job cannot
        // merge anything.
        _ => Err(EngineError::Unsupported),
    }
}

fn workspace_job(
    exe: &Path,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
    build: impl FnOnce(&Path) -> Vec<OsString>,
    label: &'static str,
) -> Result<JobOutput, EngineError> {
    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let workspace = Workspace::new()?;
    let output_path = workspace.join("out.pdf");
    let args = build(&output_path);

    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    exec::run(
        &Spec {
            exe,
            args: &args,
            dir: workspace.dir(),
            timeout: DOCUMENT_TIMEOUT,
        },
        cancel,
        |_| {},
    )?;

    let target = TargetFormat::Image(ImageFormat::Pdf);
    output::verify(&output_path, target)
        .map_err(|_| EngineError::Failed(format!("{label} produced an unreadable file.")))?;

    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });
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

fn rotate(
    exe: &Path,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
    degrees: &str,
) -> Result<JobOutput, EngineError> {
    let flag = rotate_flag(degrees);
    workspace_job(
        exe,
        job,
        progress,
        cancel,
        |output| {
            vec![
                flag.clone().into(),
                argv_path(&job.input),
                output.as_os_str().to_os_string(),
            ]
        },
        "qpdf",
    )
}

/// `+90:1-z` → `--rotate=+90:1-z`: rotate every page of the document.
pub(crate) fn rotate_flag(degrees: &str) -> String {
    format!("--rotate={degrees}")
}

fn linearize(
    exe: &Path,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    workspace_job(
        exe,
        job,
        progress,
        cancel,
        |output| {
            vec![
                "--linearize".into(),
                argv_path(&job.input),
                output.as_os_str().to_os_string(),
            ]
        },
        "qpdf",
    )
}

/// `qpdf --split-pages` writes `<prefix>-01.pdf`, `-02.pdf`, … into the
/// workspace; the whole set is delivered as one `_pages` folder.
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
    let prefix = pages.join("page");
    let args = vec![
        "--split-pages".into(),
        argv_path(&job.input),
        prefix.as_os_str().to_os_string(),
    ];

    progress(Progress {
        stage: Stage::Converting,
        fraction: None,
    });
    exec::run(
        &Spec {
            exe,
            args: &args,
            dir: workspace.dir(),
            timeout: DOCUMENT_TIMEOUT,
        },
        cancel,
        |_| {},
    )?;

    let count = count_files(&pages)?;
    if count == 0 {
        return Err(EngineError::Failed("qpdf produced no pages.".into()));
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

fn count_files(dir: &Path) -> Result<usize, EngineError> {
    let entries = std::fs::read_dir(dir).map_err(|error| {
        EngineError::Failed(format!("the engine output couldn't be read: {error}"))
    })?;
    Ok(entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .count())
}

/// `qpdf --empty --pages a b -- out.pdf`: the output starts empty and the
/// inputs are appended in selection order. Used by the whole-batch merge.
pub(crate) fn merge_args(inputs: &[PathBuf], output: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["--empty".into(), "--pages".into()];
    for input in inputs {
        args.push(argv_path(input));
    }
    // `--` ends the page-range list so a file named like a range cannot be
    // parsed as one.
    args.push("--".into());
    args.push(output.as_os_str().to_os_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn rotate_flags_point_the_right_way() {
        // Left is `+90`, right is `-90`, both scoped to every page (`1-z`).
        assert_eq!(rotate_flag("+90:1-z"), "--rotate=+90:1-z");
        assert_eq!(rotate_flag("-90:1-z"), "--rotate=-90:1-z");
        assert_ne!(rotate_flag("+90:1-z"), rotate_flag("-90:1-z"));
    }

    #[test]
    fn merge_needs_a_terminator_before_the_output() {
        let inputs = vec![PathBuf::from(r"C:\docs\a b.pdf"), PathBuf::from("-odd.pdf")];
        let args = merge_args(&inputs, Path::new(r"C:\docs\out.pdf"));
        let text: Vec<String> = args.iter().map(|a| a.to_string_lossy().into()).collect();
        assert_eq!(text[0], "--empty");
        assert_eq!(text[1], "--pages");
        assert_eq!(text[2], r"C:\docs\a b.pdf");
        assert_eq!(text[3], r".\-odd.pdf");
        assert_eq!(text[4], "--");
        assert_eq!(text[5], r"C:\docs\out.pdf");
    }

    #[test]
    fn merge_with_no_inputs_still_terminates_the_list() {
        let args = merge_args(&[], Path::new(r"C:\out.pdf"));
        let text: Vec<String> = args.iter().map(|a| a.to_string_lossy().into()).collect();
        assert_eq!(text, vec!["--empty", "--pages", "--", r"C:\out.pdf"]);
    }
}
