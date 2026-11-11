//! multi.rs — the jobs that need the whole selection at once.
//!
//! Merge, ZIP and archive extraction cannot be expressed as one-input-one-output
//! jobs, so they run before the per-file loop in `jobs::run_batch`. Everything
//! here follows the same contract as the rest of the pipeline: the conversion
//! path never deletes or overwrites anything a user already has, staging goes
//! through a private `Workspace`, and the final placement is a no-replace
//! rename. Cleanup happens only when that workspace is dropped.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::engines::output::{self, TargetFormat, Workspace};
use crate::engines::{
    self, exec, Action, AudioFormat, AudioOptimize, CancelToken, DocumentFormat, EngineError,
    EngineId, EnginePaths, FileKind, ImageFormat, ImageOptimize, MultiOp, PdfTool, PresentFormat,
    Progress, SheetFormat, Stage, VideoFormat, VideoOptimize, ZipOp,
};

use super::detect::{self, SourceFormat};
use super::jobs::{self, Created, Failure, Outcome};

// ------------------------------------------------------------- safety caps

/// A ZIP with more entries than this is not an archive the user meant to open.
const MAX_ENTRIES: usize = 10_000;
/// Total uncompressed ceiling (4 GiB) — a decompression bomb guard.
const MAX_TOTAL: u64 = 4 * 1024 * 1024 * 1024;
/// One entry may not claim more than 2 GiB.
const MAX_ENTRY: u64 = 2 * 1024 * 1024 * 1024;
/// A ratio above this, once the entry is big enough to matter, is a bomb.
const MAX_RATIO: u64 = 100;
/// Ratios are only meaningful above this uncompressed size.
const RATIO_FLOOR: u64 = 16 * 1024 * 1024;

const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp", "svg", "ico",
];

// ------------------------------------------------------------------ dispatch

/// Actions whose inputs must be processed together rather than one by one.
pub fn is_whole_batch(action: Action) -> bool {
    matches!(
        action,
        Action::Multi(MultiOp::MergePdf | MultiOp::Zip) | Action::Zip(ZipOp::ExtractHere)
    ) || action == Action::PdfTool(PdfTool::Merge)
}

#[allow(clippy::too_many_arguments)]
pub fn run_whole_batch(
    inputs: &[String],
    action: Action,
    paths: &EnginePaths,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(usize, usize, &str),
    on_engine: &mut dyn FnMut(usize, usize, Progress),
) -> Outcome {
    let total = inputs.len();
    for (index, input) in inputs.iter().enumerate() {
        progress(index, total, &jobs::file_name(input));
        on_engine(
            index,
            total,
            Progress {
                stage: Stage::Converting,
                fraction: None,
            },
        );
    }
    let mut outcome = Outcome::default();
    if cancel.is_cancelled() {
        outcome.cancelled = true;
        return outcome;
    }

    let result: Result<Vec<Created>, String> = match action {
        Action::Multi(MultiOp::MergePdf) | Action::PdfTool(PdfTool::Merge) => {
            merge(inputs, paths, cancel).map(|created| vec![created])
        }
        Action::Multi(MultiOp::Zip) => zip_create(inputs, cancel).map(|created| vec![created]),
        Action::Zip(ZipOp::ExtractHere) => match inputs.first() {
            Some(input) => zip_extract(input, cancel),
            None => Err("There is nothing to extract.".into()),
        },
        _ => Err("This option can't run on this selection.".into()),
    };

    match result {
        Ok(created) => outcome.created = created,
        Err(reason) => {
            let input = inputs.first().cloned().unwrap_or_default();
            outcome.failed.push(Failure {
                input_name: jobs::file_name(&input),
                input,
                reason,
            });
        }
    }
    if cancel.is_cancelled() {
        outcome.cancelled = true;
    }
    outcome
}

// --------------------------------------------------------------------- merge

/// Merge images or PDFs — never a mix, and never fewer than two files.
fn merge(inputs: &[String], paths: &EnginePaths, cancel: &CancelToken) -> Result<Created, String> {
    if inputs.len() < 2 {
        return Err("Select at least two files to merge.".into());
    }
    let mut images = Vec::new();
    let mut pdfs = Vec::new();
    for input in inputs {
        let format = detect::quick_format(input).ok_or("This file can't be merged.")?;
        if format.image_format().is_some() {
            images.push(input.clone());
        } else if format == SourceFormat::Pdf {
            pdfs.push(input.clone());
        } else {
            return Err("Only images or only PDFs can be merged.".into());
        }
    }
    if !images.is_empty() && !pdfs.is_empty() {
        return Err("Only images or only PDFs can be merged.".into());
    }
    if !images.is_empty() {
        return merge_images(&images, cancel);
    }
    merge_pdfs(&pdfs, paths, cancel)
}

/// Native path: every image becomes one page of a single PDF, in selection
/// order (the shelf's order).
fn merge_images(inputs: &[String], cancel: &CancelToken) -> Result<Created, String> {
    let mut pages = Vec::with_capacity(inputs.len());
    for input in inputs {
        if cancel.is_cancelled() {
            return Err("Cancelled.".into());
        }
        let (image, _, _) = jobs::decode(input)?;
        pages.push(image);
    }
    let bytes = jobs::encode_pdf_pages(&pages)?;
    let workspace = Workspace::new().map_err(|error| error.to_string())?;
    let staged = workspace.join("merged.pdf");
    std::fs::write(&staged, &bytes)
        .map_err(|error| format!("The file couldn't be written: {error}."))?;
    deliver_named(&staged, inputs, Some("merged"), "pdf")
}

/// Poppler's `pdfunite` first, qpdf as the automatic retry.
fn merge_pdfs(
    inputs: &[String],
    paths: &EnginePaths,
    cancel: &CancelToken,
) -> Result<Created, String> {
    let source: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
    let workspace = Workspace::new().map_err(|error| error.to_string())?;
    let staged = workspace.join("merged.pdf");

    // (executable, argv) in the documented preference order.
    let mut attempts: Vec<(PathBuf, Vec<OsString>)> = Vec::new();
    if let Some(exe) = sibling_tool(paths, EngineId::Poppler, "pdfunite.exe") {
        attempts.push((exe, engines::poppler::merge_args(&source, &staged)));
    }
    if let Some(exe) = paths.location(EngineId::Qpdf).map(Path::to_path_buf) {
        attempts.push((exe, engines::qpdf::merge_args(&source, &staged)));
    }
    if attempts.is_empty() {
        return Err("Merging PDFs needs Poppler or qpdf.".into());
    }

    let mut last = "The PDFs couldn't be merged.".to_string();
    for (exe, args) in attempts {
        if cancel.is_cancelled() {
            return Err("Cancelled.".into());
        }
        let result = exec::run(
            &exec::Spec {
                exe: &exe,
                args: &args,
                dir: workspace.dir(),
                timeout: engines::DOCUMENT_TIMEOUT,
            },
            cancel,
            |_| {},
        );
        match result {
            Ok(_) if staged.is_file() => {
                return deliver_named(&staged, inputs, Some("merged"), "pdf");
            }
            Ok(_) => last = "The PDFs couldn't be merged.".into(),
            Err(EngineError::Cancelled) => return Err("Cancelled.".into()),
            Err(EngineError::Failed(message)) => last = message,
            Err(EngineError::Timeout) => last = "The engine took too long and was stopped.".into(),
            Err(other) => last = other.to_string(),
        }
    }
    Err(last)
}

/// `<first stem> merged.<ext>` beside the first input, collision-safe.
fn deliver_named(
    staged: &Path,
    inputs: &[String],
    suffix: Option<&str>,
    extension: &str,
) -> Result<Created, String> {
    let first = inputs
        .first()
        .ok_or_else(|| "There is nothing to write.".to_string())?;
    let dir = Path::new(first)
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| "The output folder isn't writable.".to_string())?;
    let stem = Path::new(first)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "converted".into());
    let name = match suffix {
        Some(suffix) => format!("{stem} {suffix}.{extension}"),
        None => format!("{stem}.{extension}"),
    };
    let output = output::deliver_as(dir, staged, &name)?;
    Ok(created(
        first,
        output,
        Some(TargetFormat::Image(ImageFormat::Pdf)),
        None,
        None,
    ))
}

// ------------------------------------------------------------------- zip out

/// Pack the whole selection into one archive beside the first input.
fn zip_create(inputs: &[String], cancel: &CancelToken) -> Result<Created, String> {
    if inputs.len() < 2 {
        return Err("Select at least two files to zip.".into());
    }
    let workspace = Workspace::new().map_err(|error| error.to_string())?;
    let staged = workspace.join("archive.zip");
    let file = std::fs::File::create(&staged)
        .map_err(|error| format!("The archive couldn't be created: {error}."))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut used: Vec<String> = Vec::new();
    for input in inputs {
        if cancel.is_cancelled() {
            return Err("Cancelled.".into());
        }
        let path = Path::new(input);
        let base = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string());
        let name = unique_name(&mut used, base);
        let mut source = std::fs::File::open(path)
            .map_err(|error| format!("{} couldn't be read: {error}.", jobs::file_name(input)))?;
        writer
            .start_file(&name, options)
            .map_err(|error| format!("The archive couldn't be written: {error}."))?;
        std::io::copy(&mut source, &mut writer)
            .map_err(|error| format!("The archive couldn't be written: {error}."))?;
    }
    writer
        .finish()
        .map_err(|error| format!("The archive couldn't be finished: {error}."))?;

    deliver_named(&staged, inputs, None, "zip")
}

fn unique_name(used: &mut Vec<String>, candidate: String) -> String {
    if !used.contains(&candidate) {
        used.push(candidate.clone());
        return candidate;
    }
    let stem = Path::new(&candidate)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| candidate.clone());
    let extension = Path::new(&candidate)
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    for index in 2..1000 {
        let next = format!("{stem} ({index}){extension}");
        if !used.contains(&next) {
            used.push(next.clone());
            return next;
        }
    }
    candidate
}

// ------------------------------------------------------------------ zip in

/// Extract an archive beside itself. Every name is validated before a single
/// byte is written, the whole set is staged in the workspace, and only then
/// is it moved into place — so a rejected entry leaves nothing behind.
fn zip_extract(input: &str, cancel: &CancelToken) -> Result<Vec<Created>, String> {
    let path = Path::new(input);
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| "The output folder isn't writable.".to_string())?;
    let file = std::fs::File::open(path)
        .map_err(|error| format!("The archive couldn't be opened: {error}."))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|_| "This file isn't a readable ZIP archive.".to_string())?;

    // ---- pass 1: validate everything before writing anything.
    if archive.len() > MAX_ENTRIES {
        return Err("This archive has too many entries.".into());
    }
    let mut plan: Vec<(usize, String, u64)> = Vec::with_capacity(archive.len());
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| "This archive couldn't be read.".to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry
            .enclosed_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| "This archive contains an unsafe file name.".to_string())?;
        safe_entry_name(&name)?;
        let uncompressed = entry.size();
        let compressed = entry.compressed_size();
        if uncompressed > MAX_ENTRY {
            return Err("This archive contains a file that is too large.".into());
        }
        if uncompressed > RATIO_FLOOR && compressed > 0 && uncompressed / compressed > MAX_RATIO {
            return Err("This archive is compressed far too tightly to open safely.".into());
        }
        total = total.saturating_add(uncompressed);
        if total > MAX_TOTAL {
            return Err("This archive is too large to extract.".into());
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("This archive contains a link, which isn't extracted.".into());
        }
        plan.push((index, name, uncompressed));
    }
    if plan.is_empty() {
        return Err("This archive is empty.".into());
    }

    // ---- pass 2: stage every file.
    let workspace = Workspace::new().map_err(|error| error.to_string())?;
    let mut staged: Vec<(PathBuf, String, u64)> = Vec::with_capacity(plan.len());
    for (position, (index, name, declared)) in plan.into_iter().enumerate() {
        if cancel.is_cancelled() {
            return Err("Cancelled.".into());
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|_| "This archive couldn't be read.".to_string())?;
        let target = workspace.join(&format!("entry-{position}"));
        let mut out = std::fs::File::create(&target)
            .map_err(|error| format!("The file couldn't be written: {error}."))?;
        // Reading is capped at the declared size: a lying header cannot make
        // us write more than the pre-scan already approved.
        let mut limited = (&mut entry).take(declared.saturating_add(1));
        let copied = std::io::copy(&mut limited, &mut out)
            .map_err(|error| format!("The file couldn't be written: {error}."))?;
        drop(out);
        if copied > declared {
            return Err("This archive claims a smaller file than it holds.".into());
        }
        staged.push((target, name, copied));
    }
    drop(archive);

    // ---- pass 3: move into place, never replacing anything.
    let mut created = Vec::with_capacity(staged.len());
    for (target, name, bytes) in staged {
        if cancel.is_cancelled() {
            return Err("Cancelled.".into());
        }
        let output = output::deliver_as(dir, &target, &name)?;
        created.push(extracted(input, output, bytes));
    }
    Ok(created)
}

/// Rejects anything that could escape the destination folder or confuse
/// Windows: traversal, absolute and drive paths, streams, NUL and the
/// reserved device names.
pub(crate) fn safe_entry_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("This archive contains an unnamed entry.".into());
    }
    if name.contains('\0') {
        return Err("This archive contains an unsafe file name.".into());
    }
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err("This archive contains an absolute path.".into());
    }
    for component in normalized.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err("This archive contains a path outside itself.".into());
        }
        if component.contains(':') {
            return Err("This archive contains an unsafe file name.".into());
        }
        let base = component.split('.').next().unwrap_or("");
        if RESERVED_NAMES
            .iter()
            .any(|reserved| base.eq_ignore_ascii_case(reserved))
        {
            return Err("This archive contains a reserved file name.".into());
        }
    }
    Ok(())
}

// ------------------------------------------------------- native single-file

/// CSV → JSON, built into the Native engine (§2.4).
pub fn csv_to_json(input: &str) -> Result<Created, String> {
    let text = read_capped(input, 64 * 1024 * 1024)?;
    let rows = parse_delimited(&text);
    if rows.len() < 2 {
        return Err("This file has no data rows.".into());
    }
    let header = &rows[0];
    if header.is_empty() {
        return Err("This file has no columns.".into());
    }
    let mut out = String::from("[\n");
    for (index, row) in rows.iter().skip(1).enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("  {");
        for (column, cell) in header.iter().enumerate() {
            if column > 0 {
                out.push(',');
            }
            let value = row.get(column).map(String::as_str).unwrap_or("");
            out.push_str(&format!("{}: {}", json_string(cell), json_string(value)));
        }
        out.push('}');
    }
    out.push_str("\n]\n");

    let workspace = Workspace::new().map_err(|error| error.to_string())?;
    let staged = workspace.join("out.json");
    std::fs::write(&staged, out)
        .map_err(|error| format!("The file couldn't be written: {error}."))?;
    deliver_single(
        &staged,
        input,
        "json",
        TargetFormat::Sheet(SheetFormat::Json),
    )
}

/// DOCX / ODT / EPUB media unpack: every image inside the package lands in
/// one `<stem>_images` folder beside the document.
pub fn unpack_document_images(input: &str) -> Result<Created, String> {
    let file = std::fs::File::open(input)
        .map_err(|error| format!("{} couldn't be read: {error}.", jobs::file_name(input)))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|_| "This document's package couldn't be opened.".to_string())?;
    if archive.len() > MAX_ENTRIES {
        return Err("This document has too many parts.".into());
    }

    let mut picks: Vec<(usize, String, u64)> = Vec::new();
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| "This document's package couldn't be read.".to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry
            .name()
            .trim_end_matches('/')
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .to_string();
        let extension = Path::new(&name)
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }
        let size = entry.size();
        if size > MAX_ENTRY {
            return Err("This document has an image that is too large.".into());
        }
        total = total.saturating_add(size);
        if total > MAX_TOTAL {
            return Err("This document's images are too large to unpack.".into());
        }
        safe_entry_name(&name)?;
        picks.push((index, name, size));
    }
    if picks.is_empty() {
        return Err("This document has no embedded images.".into());
    }

    let dir = Path::new(input)
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| "The output folder isn't writable.".to_string())?;
    let workspace = Workspace::new().map_err(|error| error.to_string())?;
    let folder = workspace.join("images");
    std::fs::create_dir_all(&folder)
        .map_err(|error| format!("The folder couldn't be created: {error}."))?;

    let mut used: Vec<String> = Vec::new();
    for (index, name, declared) in picks {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| "This document's package couldn't be read.".to_string())?;
        let unique = unique_name(&mut used, name);
        let target = folder.join(&unique);
        let mut out = std::fs::File::create(&target)
            .map_err(|error| format!("The file couldn't be written: {error}."))?;
        let mut limited = (&mut entry).take(declared.saturating_add(1));
        let copied = std::io::copy(&mut limited, &mut out)
            .map_err(|error| format!("The file couldn't be written: {error}."))?;
        drop(out);
        if copied > declared {
            return Err("This document claims a smaller image than it holds.".into());
        }
    }

    let (output, _) =
        output::deliver_folder_named_in(Some(dir), &folder, Path::new(input), "images")?;
    Ok(created(input, output, None, None, None))
}

// --------------------------------------------------------------- batch maps

/// The per-file action a batch op becomes for this input, or `None` when the
/// op does not apply to this kind of file.
pub fn batch_action(op: MultiOp, input: &str) -> Option<Action> {
    let kind = detect::file_kind(input);
    match op {
        MultiOp::BatchConvert => match kind {
            FileKind::Image => Some(Action::ImageConvert(ImageFormat::Png)),
            FileKind::Video => Some(Action::VideoConvert(VideoFormat::Mp4)),
            FileKind::Audio => Some(Action::AudioConvert(AudioFormat::Mp3)),
            FileKind::Document => Some(Action::DocumentConvert(DocumentFormat::Pdf)),
            FileKind::Spreadsheet => Some(Action::SheetConvert(SheetFormat::Pdf)),
            FileKind::Presentation => Some(Action::PresentConvert(PresentFormat::Pdf)),
            _ => None,
        },
        MultiOp::BatchResize => matches!(kind, FileKind::Image)
            .then_some(Action::ImageOptimize(ImageOptimize::ResizePercent(50))),
        MultiOp::BatchCompress => match kind {
            FileKind::Image => Some(Action::ImageOptimize(ImageOptimize::Compress)),
            FileKind::Video => Some(Action::VideoOptimize(VideoOptimize::Compress)),
            FileKind::Audio => Some(Action::AudioOptimize(AudioOptimize::SmallerBitrate)),
            _ => None,
        },
        MultiOp::BatchStripMetadata => matches!(kind, FileKind::Image)
            .then_some(Action::ImageOptimize(ImageOptimize::StripMetadata)),
        // Handled as whole-batch jobs, never per file.
        MultiOp::MergePdf | MultiOp::Zip => None,
    }
}

/// Why a batch op skipped this file, when it did.
pub fn batch_skip_reason(op: MultiOp, input: &str) -> &'static str {
    if batch_action(op, input).is_some() {
        return "";
    }
    match detect::quick_format(input) {
        Some(SourceFormat::Pdf) => "PDFs don't take part in batch jobs.",
        Some(SourceFormat::Zip) => "ZIP archives don't take part in batch jobs.",
        _ => "This file's type isn't part of this batch job.",
    }
}

// ------------------------------------------------------------------ helpers

fn created(
    input: &str,
    output: PathBuf,
    format: Option<TargetFormat>,
    text: Option<String>,
    note: Option<String>,
) -> Created {
    Created {
        input: input.to_owned(),
        input_name: jobs::file_name(input),
        output_name: output
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        input_bytes: std::fs::metadata(input).map(|meta| meta.len()).unwrap_or(0),
        output_bytes: jobs::path_bytes(&output),
        output,
        format,
        text,
        note,
    }
}

fn extracted(input: &str, output: PathBuf, bytes: u64) -> Created {
    Created {
        input: input.to_owned(),
        input_name: jobs::file_name(input),
        output_name: output
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        input_bytes: std::fs::metadata(input).map(|meta| meta.len()).unwrap_or(0),
        output_bytes: bytes,
        output,
        format: None,
        text: None,
        note: None,
    }
}

fn deliver_single(
    staged: &Path,
    input: &str,
    extension: &str,
    target: TargetFormat,
) -> Result<Created, String> {
    let dir = Path::new(input)
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty());
    let output = output::deliver_ext(dir, staged, Path::new(input), extension, None)?.0;
    Ok(created(input, output, Some(target), None, None))
}

fn read_capped(path: &str, cap: u64) -> Result<String, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("{} couldn't be read: {error}.", jobs::file_name(path)))?;
    if metadata.len() > cap {
        return Err("This file is too large to process.".into());
    }
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|error| format!("This file couldn't be read: {error}."))
}

/// Minimal RFC 4180 reader: quoted fields, doubled quotes, `,` or tab.
fn parse_delimited(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            match ch {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                other => field.push(other),
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => quoted = true,
            ',' | '\t' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            other => field.push(other),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.into_iter()
        .filter(|row| row.iter().any(|cell| !cell.trim().is_empty()))
        .collect()
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A tool that sits beside a located engine executable.
fn sibling_tool(paths: &EnginePaths, engine: EngineId, name: &str) -> Option<PathBuf> {
    let exe = paths.location(engine)?;
    let sibling = exe.with_file_name(name);
    sibling.is_file().then_some(sibling)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_slip_and_hostile_names_are_rejected() {
        for name in [
            "../evil.txt",
            "a/../../b.txt",
            "/abs/path.txt",
            "\\\\server\\share\\x.txt",
            "C:\\windows\\system32\\x.dll",
            "sub\\..\\..\\out.txt",
            "file.txt:stream",
            "CON",
            "con.txt",
            "LPT9.dat",
            "nul",
        ] {
            assert!(
                safe_entry_name(name).is_err(),
                "{name:?} should have been rejected"
            );
        }
    }

    #[test]
    fn normal_names_are_accepted() {
        for name in [
            "a.txt",
            "folder/b c.png",
            "deep/nested/file-01.pdf",
            ".hidden",
            "report.final.docx",
            "com10.txt",
            "CONSULT.txt",
        ] {
            assert!(
                safe_entry_name(name).is_ok(),
                "{name:?} should have been accepted"
            );
        }
    }

    #[test]
    fn nul_bytes_are_rejected() {
        assert!(safe_entry_name("a\0b.txt").is_err());
    }

    /// §9.4 zip bomb: 32 MiB of zeros compresses well past the 100:1 ratio
    /// cap (and over the 16 MiB floor that arms it), so extraction must be
    /// refused in the validation pass — before a single byte is written.
    #[test]
    fn zip_bombs_are_refused_before_anything_is_written() {
        let dir = batch_fixture_dir("bomb");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bomb.zip");
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("zeros.bin", options).unwrap();
            let zeros = vec![0u8; 32 * 1024 * 1024];
            std::io::Write::write_all(&mut writer, &zeros).unwrap();
            writer.finish().unwrap();
        }

        let cancel = CancelToken::new();
        let error = zip_extract(path.to_str().unwrap(), &cancel).unwrap_err();
        assert!(
            error.contains("compressed far too tightly"),
            "unexpected refusal: {error}"
        );

        // Nothing but the archive itself exists beside it.
        let mut entries: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        entries.sort();
        assert_eq!(entries, vec!["bomb.zip".to_string()]);
    }

    #[test]
    fn csv_parses_quotes_commas_and_tabs() {
        let rows = parse_delimited("a,b\n\"x,1\",\"y\"\"z\"\n");
        assert_eq!(rows[0], vec!["a", "b"]);
        assert_eq!(rows[1], vec!["x,1", "y\"z"]);

        let rows = parse_delimited("name\tvalue\none\t1\n");
        assert_eq!(rows[0], vec!["name", "value"]);
        assert_eq!(rows[1], vec!["one", "1"]);
    }

    #[test]
    fn csv_skips_blank_lines() {
        let rows = parse_delimited("a,b\n\n   \nc,d\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1], vec!["c", "d"]);
    }

    #[test]
    fn json_strings_escape_control_characters() {
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string("line\nbreak"), "\"line\\nbreak\"");
        assert_eq!(json_string("tab\there"), "\"tab\\there\"");
    }

    #[test]
    fn whole_batch_actions_are_the_multi_and_zip_ones() {
        assert!(is_whole_batch(Action::Multi(MultiOp::MergePdf)));
        assert!(is_whole_batch(Action::Multi(MultiOp::Zip)));
        assert!(is_whole_batch(Action::Zip(ZipOp::ExtractHere)));
        assert!(is_whole_batch(Action::PdfTool(PdfTool::Merge)));
        assert!(!is_whole_batch(Action::Multi(MultiOp::BatchConvert)));
        assert!(!is_whole_batch(Action::ImageConvert(ImageFormat::Png)));
    }

    /// Writes a fixture with the real magic bytes for its container, so the
    /// batch mapper sniffs exactly what it will see in production.
    fn fixture(dir: &Path, name: &str, head: &[u8]) -> String {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        let mut bytes = head.to_vec();
        // A little tail so text sniffing has something to look at.
        bytes.extend_from_slice(b"\nfixture\n");
        std::fs::write(&path, &bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn batch_fixture_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ti-batch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn batch_mapping_covers_the_documented_targets() {
        let dir = batch_fixture_dir("map");
        let png = fixture(&dir, "a.png", b"\x89PNG\r\n\x1a\n");
        let mp4 = fixture(&dir, "a.mp4", b"\0\0\0\x20ftypisom\0\0\0\0");
        let mp3 = fixture(&dir, "a.mp3", b"ID3\x04\0\0\0\0\0\0");
        let docx = fixture(&dir, "a.docx", b"PK\x03\x04word/document.xml");
        let xlsx = fixture(&dir, "a.xlsx", b"PK\x03\x04xl/workbook.xml");
        let pptx = fixture(&dir, "a.pptx", b"PK\x03\x04ppt/presentation.xml");
        let pdf = fixture(&dir, "a.pdf", b"%PDF-1.7\n");

        assert_eq!(
            batch_action(MultiOp::BatchConvert, &png),
            Some(Action::ImageConvert(ImageFormat::Png))
        );
        assert_eq!(
            batch_action(MultiOp::BatchConvert, &mp4),
            Some(Action::VideoConvert(VideoFormat::Mp4))
        );
        assert_eq!(
            batch_action(MultiOp::BatchConvert, &mp3),
            Some(Action::AudioConvert(AudioFormat::Mp3))
        );
        assert_eq!(
            batch_action(MultiOp::BatchConvert, &docx),
            Some(Action::DocumentConvert(DocumentFormat::Pdf))
        );
        assert_eq!(
            batch_action(MultiOp::BatchConvert, &xlsx),
            Some(Action::SheetConvert(SheetFormat::Pdf))
        );
        assert_eq!(
            batch_action(MultiOp::BatchConvert, &pptx),
            Some(Action::PresentConvert(PresentFormat::Pdf))
        );
        assert_eq!(batch_action(MultiOp::BatchConvert, &pdf), None);
        assert_eq!(
            batch_action(MultiOp::BatchResize, &png),
            Some(Action::ImageOptimize(ImageOptimize::ResizePercent(50)))
        );
        assert_eq!(batch_action(MultiOp::BatchResize, &mp4), None);
        assert_eq!(
            batch_action(MultiOp::BatchStripMetadata, &png),
            Some(Action::ImageOptimize(ImageOptimize::StripMetadata))
        );
        assert_eq!(
            batch_action(MultiOp::BatchCompress, &mp3),
            Some(Action::AudioOptimize(AudioOptimize::SmallerBitrate))
        );
        assert_eq!(batch_action(MultiOp::BatchCompress, &pdf), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skipped_batch_inputs_get_a_reason() {
        let dir = batch_fixture_dir("skip");
        let pdf = fixture(&dir, "a.pdf", b"%PDF-1.7\n");
        let png = fixture(&dir, "a.png", b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            batch_skip_reason(MultiOp::BatchConvert, &pdf),
            "PDFs don't take part in batch jobs."
        );
        assert_eq!(batch_skip_reason(MultiOp::BatchConvert, &png), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
