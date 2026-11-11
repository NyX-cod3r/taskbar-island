//! ghostscript.rs — PDF compression.
//!
//! Ghostscript is only ever asked for one job: re-encode a PDF at the
//! `/ebook` quality preset. The argv always carries `-dSAFER` first so the
//! interpreter can never read or write outside the two paths it is given,
//! and `-dBATCH -dNOPAUSE -dQUIET` keep it non-interactive and silent.
//!
//! Compression is only a win when the result is actually smaller. When it is
//! not, nothing is delivered beside the original and the card reports
//! "no gain" instead — the user's file is never replaced by a bigger one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::exec::{self, Spec};
use super::output::{self, TargetFormat, Workspace};
use super::{
    argv_path, Action, CancelToken, EngineError, ImageFormat, Job, JobOutput, PdfTool, Progress,
    ProgressSink, Stage, DOCUMENT_TIMEOUT,
};

/// The quality preset used for "Compress". `ebook` (150 dpi, moderate
/// downsampling) is the balance the spec asks for.
const PRESET: &str = "/ebook";

pub fn run(
    exe: &Path,
    _paths: &super::EnginePaths,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    match job.action {
        Action::PdfTool(PdfTool::Compress) => compress(exe, job, progress, cancel),
        _ => Err(EngineError::Unsupported),
    }
}

fn compress(
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
    let output_path = workspace.join("out.pdf");
    let args = compress_args(&output_path, &job.input);

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
        .map_err(|_| EngineError::Failed("Ghostscript produced an unreadable file.".into()))?;

    // "No gain": a compressed copy that is not strictly smaller than the
    // original is not a result worth delivering.
    let original = std::fs::metadata(&job.input)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let compressed = std::fs::metadata(&output_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    if original > 0 && compressed >= original {
        return Ok(JobOutput {
            output: PathBuf::new(),
            format: None,
            text: None,
            note: Some("No gain — kept the original".into()),
        });
    }

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

/// The argv for a compress job. `output` is always absolute and lives in the
/// private workspace, so `-dSAFER` never has to be relaxed.
pub(crate) fn compress_args(output: &Path, input: &Path) -> Vec<OsString> {
    vec![
        // SAFETY first, non-interactive, no chatter on stdout.
        "-dSAFER".into(),
        "-dBATCH".into(),
        "-dNOPAUSE".into(),
        "-dQUIET".into(),
        "-sDEVICE=pdfwrite".into(),
        format!("-dPDFSETTINGS={PRESET}").into(),
        format!("-sOutputFile={}", output.display()).into(),
        argv_path(input),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn argv_always_starts_with_safer_and_never_uses_a_shell() {
        let args = compress_args(Path::new(r"C:\temp\out.pdf"), Path::new(r"C:\docs\a b.pdf"));
        let text: Vec<String> = args.iter().map(|a| a.to_string_lossy().into()).collect();
        assert_eq!(text[0], "-dSAFER");
        assert!(text.contains(&"-dBATCH".to_string()));
        assert!(text.contains(&"-dNOPAUSE".to_string()));
        assert!(text.contains(&"-dQUIET".to_string()));
        assert_eq!(text.last().unwrap(), r"C:\docs\a b.pdf");
        // One argv item per argument: nothing is concatenated for a shell.
        assert_eq!(text.len(), 8);
    }

    #[test]
    fn hostile_file_names_stay_one_argv_item() {
        let args = compress_args(
            Path::new(r"C:\temp\out.pdf"),
            Path::new(r"-rf & del /q *;name.pdf"),
        );
        let last = args.last().unwrap().to_string_lossy().into_owned();
        assert_eq!(last, r".\-rf & del /q *;name.pdf");
        assert_eq!(args.len(), 8);
    }

    #[test]
    fn output_file_is_one_argument_with_the_path_inside_it() {
        let args = compress_args(
            Path::new(r"C:\Users\me\AppData\Local\Temp\ws\out.pdf"),
            Path::new(r"C:\docs\a.pdf"),
        );
        let output = args
            .iter()
            .find(|a| a.to_string_lossy().starts_with("-sOutputFile="))
            .expect("output flag");
        assert!(output
            .to_string_lossy()
            .contains(r"C:\Users\me\AppData\Local\Temp\ws\out.pdf"));
    }
}
