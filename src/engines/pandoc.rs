//! pandoc.rs — document conversion through a sandboxed Pandoc.
//!
//! Pandoc always runs with `--sandbox`: it may read only the input it was
//! given, and it writes only into the private workspace. Media extracted from
//! a document goes into that workspace too and is moved into place afterwards,
//! never written beside the original by Pandoc itself.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::convert::detect::{self, SourceFormat};

use super::exec::{self, Spec};
use super::output::{self, TargetFormat, Workspace};
use super::{
    argv_path, Action, CancelToken, DocumentExtract, DocumentFormat, EngineError, Job, JobOutput,
    Progress, ProgressSink, Stage, DOCUMENT_TIMEOUT,
};

pub fn run(
    exe: &Path,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    let input_text = job.input.to_string_lossy().into_owned();
    let source = detect::quick_format(&input_text).ok_or(EngineError::Unsupported)?;
    if source.document_format().is_none() {
        return Err(EngineError::Unsupported);
    }

    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let workspace = Workspace::new()?;
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }

    match job.action {
        Action::DocumentConvert(format) => {
            let output_path = workspace.join(&format!("out.{}", extension(format)));
            let args = plan(source, format, &output_path, job)?;
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
            let target = TargetFormat::Document(format);
            output::verify(&output_path, target)
                .map_err(|_| EngineError::Failed("Pandoc produced an unreadable file.".into()))?;
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
            })
        }
        Action::DocumentExtract(DocumentExtract::PlainText) => {
            let output_path = workspace.join("out.txt");
            let args = plan(source, DocumentFormat::Txt, &output_path, job)?;
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
            let target = TargetFormat::Document(DocumentFormat::Txt);
            output::verify(&output_path, target)
                .map_err(|_| EngineError::Failed("Pandoc produced an unreadable file.".into()))?;
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
            })
        }
        Action::DocumentExtract(DocumentExtract::Images) => {
            let media = workspace.join("media");
            let output_path = workspace.join("out.txt");
            let mut args = plan(source, DocumentFormat::Txt, &output_path, job)?;
            insert_before_output(&mut args, &format!("--extract-media={}", media.display()));
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
            let mut files = Vec::new();
            collect_files(&media, &mut files)?;
            if files.is_empty() {
                return Err(EngineError::Failed(
                    "This document has no embedded images.".into(),
                ));
            }
            progress(Progress {
                stage: Stage::Saving,
                fraction: None,
            });
            let (delivered, _) = output::deliver_folder_in(job.output.parent(), &media, &job.input)
                .map_err(EngineError::Failed)?;
            progress(Progress {
                stage: Stage::Done,
                fraction: Some(1.0),
            });
            Ok(JobOutput {
                output: delivered,
                format: None,
                text: None,
            })
        }
        _ => Err(EngineError::Unsupported),
    }
}

fn plan(
    source: SourceFormat,
    target: DocumentFormat,
    output: &Path,
    job: &Job,
) -> Result<Vec<OsString>, EngineError> {
    let input_format = input_name(source).ok_or(EngineError::Unsupported)?;
    Ok(vec![
        "--sandbox".into(),
        "-f".into(),
        input_format.into(),
        "-t".into(),
        output_name(target).into(),
        "-o".into(),
        argv_path(output),
        argv_path(&job.input),
    ])
}

/// `--extract-media` has to sit before the input; the plan keeps the input
/// last, so it is inserted just before it.
fn insert_before_output(args: &mut Vec<OsString>, extra: &str) {
    if !args.is_empty() {
        let position = args.len() - 1;
        args.insert(position, OsString::from(extra));
    }
}

fn input_name(source: SourceFormat) -> Option<&'static str> {
    match source {
        SourceFormat::Docx => Some("docx"),
        SourceFormat::Odt => Some("odt"),
        SourceFormat::Markdown => Some("markdown"),
        SourceFormat::Html => Some("html"),
        // Plain text has no dedicated reader; Pandoc reads it as markdown.
        SourceFormat::Txt => Some("markdown"),
        SourceFormat::Epub => Some("epub"),
        SourceFormat::Rtf => Some("rtf"),
        SourceFormat::Tex => Some("latex"),
        _ => None,
    }
}

fn output_name(format: DocumentFormat) -> &'static str {
    match format {
        DocumentFormat::Docx => "docx",
        DocumentFormat::Odt => "odt",
        DocumentFormat::Markdown => "markdown",
        DocumentFormat::Html => "html",
        DocumentFormat::Txt => "plain",
        DocumentFormat::Epub => "epub",
        DocumentFormat::Rtf => "rtf",
        DocumentFormat::Tex => "latex",
    }
}

fn extension(format: DocumentFormat) -> &'static str {
    TargetFormat::Document(format).extension()
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), EngineError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|_| EngineError::Failed("The embedded images couldn't be read.".into()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(action: Action) -> Job {
        Job {
            input: PathBuf::from(r"C:\docs\paper.docx"),
            output: PathBuf::from(r"C:\docs\paper.md"),
            action,
            language: None,
        }
    }

    #[test]
    fn every_conversion_is_sandboxed_and_keeps_the_input_last() {
        struct Case(SourceFormat, DocumentFormat);
        let cases = [
            Case(SourceFormat::Docx, DocumentFormat::Markdown),
            Case(SourceFormat::Odt, DocumentFormat::Html),
            Case(SourceFormat::Markdown, DocumentFormat::Docx),
            Case(SourceFormat::Html, DocumentFormat::Txt),
            Case(SourceFormat::Epub, DocumentFormat::Odt),
            Case(SourceFormat::Rtf, DocumentFormat::Epub),
            Case(SourceFormat::Tex, DocumentFormat::Rtf),
            Case(SourceFormat::Txt, DocumentFormat::Docx),
        ];
        for Case(source, target) in cases {
            let job = job(Action::DocumentConvert(target));
            let args = plan(source, target, Path::new(r"C:\temp\out"), &job).unwrap();
            let text: Vec<String> = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            assert_eq!(text[0], "--sandbox");
            assert_eq!(text.last().unwrap(), r"C:\docs\paper.docx");
            assert!(text.iter().any(|arg| arg == "-f"));
            assert!(text.iter().any(|arg| arg == "-t"));
        }
    }

    #[test]
    fn pandoc_format_names_match_the_probe() {
        assert_eq!(input_name(SourceFormat::Txt), Some("markdown"));
        assert_eq!(output_name(DocumentFormat::Txt), "plain");
        assert_eq!(output_name(DocumentFormat::Markdown), "markdown");
        assert_eq!(output_name(DocumentFormat::Tex), "latex");
    }

    #[test]
    fn extract_media_is_inserted_before_the_input() {
        let job = job(Action::DocumentExtract(DocumentExtract::Images));
        let mut args = plan(
            SourceFormat::Docx,
            DocumentFormat::Txt,
            Path::new("out"),
            &job,
        )
        .unwrap();
        insert_before_output(&mut args, "--extract-media=C:\\temp\\media");
        let text: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(text[text.len() - 2], "--extract-media=C:\\temp\\media");
        assert_eq!(text[text.len() - 1], r"C:\docs\paper.docx");
    }
}
