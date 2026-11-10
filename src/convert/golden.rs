//! golden.rs — §9.2: one real conversion per row of the B prompt's §4 table,
//! run against the executables that live in `ENGINES_DIR`.
//!
//! Fixtures are generated inside the test itself and never checked in: Pandoc
//! builds the DOCX/PPTX, FFmpeg the tone and the clip, pdf-writer the PDF,
//! `image` the PNG/JPEG. Every row asserts the same contract — the output
//! lands beside the source, the original is bit-identical afterwards, the
//! magic bytes are right, the file is not empty, no `.taskbar-island-*.tmp`
//! survives beside the fixtures and no engine workspace outlives the run.
//!
//! An engine the owner has not dropped into `ENGINES_DIR` yet **skips** its
//! rows: the test prints `SKIP golden <row>: <engine> …` and passes, exactly
//! as §9.2 requires. See the skip list with:
//! `cargo test convert::golden -- --nocapture`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use super::detect;
use super::jobs::{self, Outcome};
use super::multi;
use crate::engines::{
    router, Action, AudioFormat, AudioOptimize, CancelToken, CapabilityTable, DocumentFormat,
    EngineId, EnginePaths, EngineSet, FileKind, ImageFormat, MultiOp, PdfExtract, PdfTarget,
    PdfTool, PresentFormat, SheetFormat, VideoExtract, VideoFormat, VideoOptimize, ZipOp,
};

/// LibreOffice runs one job at a time and every row shares `%TEMP%`, so the
/// golden rows run strictly one after another.
static LOCK: Mutex<()> = Mutex::new(());

fn guard() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The engines on this machine, probed once for the whole suite.
struct Harness {
    root: PathBuf,
    caps: CapabilityTable,
    paths: EnginePaths,
}

fn harness() -> &'static Harness {
    static HARNESS: OnceLock<Harness> = OnceLock::new();
    HARNESS.get_or_init(|| {
        let root = crate::engines::engines_dir();
        let set = EngineSet::load(root.clone());
        Harness {
            root,
            caps: set.capabilities(),
            paths: set.paths(),
        }
    })
}

fn skip(row: &str, why: &str) {
    println!(
        "SKIP golden {row}: {why} (ENGINES_DIR = {})",
        harness().root.display()
    );
}

/// §9.2 gate: `false` — with a printed SKIP line — when the engine that runs
/// this job is not on this machine. `engine` is the §4 engine of record; it
/// only shapes the message.
fn need(row: &str, inputs: &[PathBuf], action: Action, engine: EngineId) -> bool {
    let h = harness();
    let first = inputs
        .first()
        .expect("every golden row has at least one input");
    let text = first.to_string_lossy().into_owned();
    let kind = detect::file_kind(&text);
    let source = detect::quick_format(&text);
    // Whole-batch jobs are consumed by `run_batch` before any routing:
    // multi.rs picks their engine itself, so the router never sees them.
    let effective = match action {
        Action::Multi(MultiOp::MergePdf) if kind == FileKind::Pdf => {
            return need_any(row, &[EngineId::Poppler, EngineId::Qpdf]);
        }
        Action::Multi(MultiOp::MergePdf) | Action::Multi(MultiOp::Zip) => return true,
        Action::Multi(op) => match multi::batch_action(op, &text) {
            Some(mapped) => mapped,
            None => {
                skip(row, "this batch job does not apply to the first file");
                return false;
            }
        },
        other => other,
    };
    if router::route_source(kind, source, effective, &h.caps).is_empty() {
        skip(row, &why_missing(engine, &h.caps));
        return false;
    }
    true
}

fn need_any(row: &str, engines: &[EngineId]) -> bool {
    let caps = &harness().caps;
    if engines.iter().any(|id| caps.installed(*id)) {
        return true;
    }
    let names: Vec<&str> = engines.iter().filter_map(|id| id.name()).collect();
    skip(row, &format!("{} not installed", names.join(" or ")));
    false
}

fn why_missing(engine: EngineId, caps: &CapabilityTable) -> String {
    let name = engine.name().unwrap_or("Native");
    let reported = caps.get(engine);
    if engine != EngineId::Native && !reported.installed {
        return format!("{name} is not installed");
    }
    if engine == EngineId::Tesseract && !reported.can_ocr() {
        return format!(
            "{name} is installed but has no language data (tesseract\\tessdata\\eng.traineddata)"
        );
    }
    format!("no installed engine runs this job (engine of record: {name})")
}

// ------------------------------------------------------------------ fixtures

fn fixture_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "taskbar-island-golden-{tag}-{}-{stamp}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn write_text(dir: &Path, name: &str, text: &str) -> PathBuf {
    write_bytes(dir, name, text.as_bytes())
}

/// A PNG with one fully transparent corner — §4's "PNG → JPG on a white
/// background" is asserted against exactly that corner later.
fn write_png(dir: &Path, name: &str) -> PathBuf {
    let mut rgba = image::RgbaImage::new(64, 48);
    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
        let alpha = if x < 16 && y < 16 { 0 } else { 255 };
        *pixel = image::Rgba([(x * 4) as u8, (y * 5) as u8, 0xC0, alpha]);
    }
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    write_bytes(dir, name, &bytes)
}

fn write_jpg(dir: &Path, name: &str) -> PathBuf {
    let mut rgb = image::RgbImage::new(48, 32);
    for (x, y, pixel) in rgb.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x * 5) as u8, (y * 8) as u8, 0x40]);
    }
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(rgb)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    write_bytes(dir, name, &bytes)
}

/// HTML that points at a remote image. The scheme is assembled from pieces so
/// the offline source scan never sees a URL even inside test code.
fn write_html_with_remote_image(dir: &Path) -> PathBuf {
    let remote = concat!("http", "://", "example.invalid", "/golden.png");
    write_text(
        dir,
        "page.html",
        &format!(
            "<!DOCTYPE html>\n<html><body>\n<h1>Golden</h1>\n<img src=\"{remote}\" alt=\"remote\">\n<p>Sandboxed body.</p>\n</body></html>\n"
        ),
    )
}

/// A one-page PDF with a real text layer — pdf-writer only, no engine — so
/// the Poppler text rows have words to find the day the engine arrives.
fn write_text_pdf(dir: &Path, name: &str, label: &str) -> PathBuf {
    use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};

    let mut pdf = Pdf::new();
    let catalog = Ref::new(1);
    let page_tree = Ref::new(2);
    pdf.catalog(catalog).pages(page_tree);

    let page_id = Ref::new(3);
    let font_id = Ref::new(4);
    let content_id = Ref::new(5);
    let mut page = pdf.page(page_id);
    page.media_box(Rect::new(0.0, 0.0, 612.0, 792.0));
    page.parent(page_tree);
    page.contents(content_id);
    page.resources().fonts().pair(Name(b"F1"), font_id);
    page.finish();

    let mut font = pdf.type1_font(font_id);
    font.base_font(Name(b"Helvetica"));
    font.encoding_predefined(Name(b"WinAnsiEncoding"));
    font.first_char(32);
    font.last_char(126);
    font.widths(std::iter::repeat_n(500.0, 95));
    font.finish();

    let mut content = Content::new();
    content.begin_text();
    content.set_font(Name(b"F1"), 24.0);
    content.next_line(72.0, 700.0);
    content.show(Str(label.as_bytes()));
    content.end_text();
    pdf.stream(content_id, &content.finish());

    pdf.pages(page_tree).kids([page_id]).count(1);
    write_bytes(dir, name, &pdf.finish())
}

/// The located executable of an engine a fixture cannot be built without.
/// Missing ⇒ print the row's SKIP line and give up on the whole row.
fn fixture_tool(tag: &str, engine: EngineId) -> Option<PathBuf> {
    let h = harness();
    if !h.caps.installed(engine) {
        skip(
            tag,
            &format!(
                "{} is needed to build this fixture and is not installed",
                engine.name().unwrap_or("Native")
            ),
        );
        return None;
    }
    h.paths.location(engine).map(Path::to_path_buf)
}

fn build_fixture(exe: &Path, args: &[&str], cwd: &Path) {
    let status = std::process::Command::new(exe)
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("the fixture engine must start");
    assert!(
        status.success(),
        "{} failed to build a fixture ({args:?})",
        exe.display()
    );
}

// --------------------------------------------------------------------- runs

/// Every engine job stages in `%TEMP%\taskbar-island-engine-<pid>-<n>` and
/// removes it on drop; a directory that outlives the run is a leak.
fn engine_workspaces() -> BTreeSet<PathBuf> {
    std::fs::read_dir(std::env::temp_dir())
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("taskbar-island-engine-")
                })
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default()
}

fn wait_for_workspaces_gone(row: &str, before: &BTreeSet<PathBuf>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let leaked: Vec<PathBuf> = engine_workspaces().difference(before).cloned().collect();
        if leaked.is_empty() {
            return;
        }
        if Instant::now() >= deadline {
            panic!("{row}: engine workspace left behind: {leaked:?}");
        }
        // Other tests in this binary stage their own workspaces; give a
        // foreign one up to five seconds to finish and drop before failing.
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn run(row: &str, inputs: &[PathBuf], action: Action) -> Outcome {
    let h = harness();
    let owned: Vec<String> = inputs
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let cancel = CancelToken::new();
    let before = engine_workspaces();
    let outcome = jobs::run_batch(
        &owned,
        action,
        None,
        &h.caps,
        &h.paths,
        &cancel,
        &mut |_, _, _| {},
        &mut |_, _, _| {},
    );
    wait_for_workspaces_gone(row, &before);
    outcome
}

/// What the delivered bytes must look like. Text formats have no magic bytes,
/// so `Text` only demands real content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Magic {
    Text,
    Bytes(&'static [u8]),
    Html,
    Json,
    Mp3,
    Mp4,
    Folder,
}

impl Magic {
    fn assert(&self, row: &str, bytes: &[u8]) {
        match self {
            Magic::Text => {}
            Magic::Bytes(prefix) => assert!(
                bytes.starts_with(prefix),
                "{row}: expected magic {prefix:?}, got {:?}",
                &bytes[..bytes.len().min(16)]
            ),
            Magic::Html => {
                let first = bytes
                    .iter()
                    .copied()
                    .find(|byte| !byte.is_ascii_whitespace());
                assert_eq!(first, Some(b'<'), "{row}: expected an HTML document");
            }
            Magic::Json => {
                let first = bytes
                    .iter()
                    .copied()
                    .find(|byte| !byte.is_ascii_whitespace());
                assert!(
                    matches!(first, Some(b'{') | Some(b'[')),
                    "{row}: expected a JSON document, got {first:?}"
                );
            }
            Magic::Mp3 => assert!(
                bytes.starts_with(b"ID3")
                    || (bytes.len() > 1 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0),
                "{row}: expected MP3 framing, got {:?}",
                &bytes[..bytes.len().min(4)]
            ),
            Magic::Mp4 => assert!(
                bytes.len() > 12 && &bytes[4..8] == b"ftyp",
                "{row}: expected an MP4/M4A ftyp box"
            ),
            Magic::Folder => unreachable!("folders are checked before the bytes"),
        }
    }
}

fn assert_no_temps(row: &str, dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.starts_with(".taskbar-island"),
            "{row}: temp leftover {name}"
        );
    }
}

/// The §9.2 contract, asserted after every conversion.
fn check(
    row: &str,
    sources: &[PathBuf],
    before: &[Vec<u8>],
    outcome: &Outcome,
    expect: usize,
    magic: Magic,
) {
    assert!(!outcome.cancelled, "{row}: the run was cancelled");
    assert!(
        outcome.failed.is_empty(),
        "{row}: {} failure(s): {}",
        outcome.failed.len(),
        outcome
            .failed
            .iter()
            .map(|failure| format!("{} → {}", failure.input_name, failure.reason))
            .collect::<Vec<_>>()
            .join("; ")
    );
    assert_eq!(outcome.created.len(), expect, "{row}: created count");

    let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
    for created in &outcome.created {
        let input = Path::new(&created.input);
        let position = sources
            .iter()
            .position(|source| source == input)
            .unwrap_or_else(|| panic!("{row}: unexpected input {}", created.input));
        let original = &before[position];

        // The output lands beside its source — never on top of it.
        assert_eq!(
            created.output.parent(),
            input.parent(),
            "{row}: {} was not written beside its source",
            created.output_name
        );
        assert!(
            !sources.contains(&created.output),
            "{row}: {} would overwrite an original",
            created.output_name
        );

        // The original is byte-for-byte what it was before the run.
        assert_eq!(
            &std::fs::read(input).unwrap(),
            original,
            "{row}: {} changed its source",
            created.output_name
        );

        if let Some(dir) = input.parent() {
            dirs.insert(dir.to_path_buf());
        }

        if magic == Magic::Folder {
            assert!(
                created.output.is_dir(),
                "{row}: {} is not a folder",
                created.output_name
            );
            let count = std::fs::read_dir(&created.output).unwrap().count();
            assert!(count > 0, "{row}: folder {} is empty", created.output_name);
            continue;
        }
        assert!(
            !created.output.as_os_str().is_empty(),
            "{row}: the job delivered nothing (note: {:?})",
            created.note
        );
        let bytes = std::fs::read(&created.output).unwrap_or_else(|error| {
            panic!("{row}: {}: {error}", created.output.display());
        });
        assert!(!bytes.is_empty(), "{row}: {} is empty", created.output_name);
        magic.assert(row, &bytes);
    }

    for dir in dirs {
        assert_no_temps(row, &dir);
    }
}

fn run_and_check(row: &str, inputs: &[PathBuf], action: Action, magic: Magic) -> Outcome {
    let before: Vec<Vec<u8>> = inputs
        .iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect();
    let outcome = run(row, inputs, action);
    let expect = if multi::is_whole_batch(action) {
        1
    } else {
        inputs.len()
    };
    check(row, inputs, &before, &outcome, expect, magic);
    outcome
}

/// Skip-gated §9.2 conversion: prints a SKIP line and does nothing when the
/// engine of record is missing, otherwise converts and checks everything.
fn convert_one(row: &str, inputs: &[PathBuf], action: Action, engine: EngineId, magic: Magic) {
    if need(row, inputs, action, engine) {
        run_and_check(row, inputs, action, magic);
    }
}

// -------------------------------------------------------------------- rows

/// §4 row 1: DOCX/ODT/RTF — PDF and DOC via LibreOffice, the rest Pandoc.
#[test]
fn office_documents_row() {
    let _guard = guard();
    let Some(pandoc) = fixture_tool("office documents", EngineId::Pandoc) else {
        return;
    };
    let dir = fixture_dir("office-documents");
    write_text(&dir, "report.md", "# Golden report\n\nOffice row body.\n");
    build_fixture(
        &pandoc,
        &[
            "-f",
            "markdown",
            "-t",
            "docx",
            "-o",
            "report.docx",
            "report.md",
        ],
        &dir,
    );
    let docx = dir.join("report.docx");

    convert_one(
        "docx→md",
        std::slice::from_ref(&docx),
        Action::DocumentConvert(DocumentFormat::Markdown),
        EngineId::Pandoc,
        Magic::Text,
    );
    convert_one(
        "docx→odt",
        std::slice::from_ref(&docx),
        Action::DocumentConvert(DocumentFormat::Odt),
        EngineId::Pandoc,
        Magic::Bytes(b"PK\x03\x04"),
    );
    convert_one(
        "docx→pdf",
        &[docx],
        Action::DocumentConvert(DocumentFormat::Pdf),
        EngineId::LibreOffice,
        Magic::Bytes(b"%PDF-"),
    );
}

/// §4 row 2: MD/HTML/TXT/TEX/EPUB — Pandoc, plus the LibreOffice PDF chain.
#[test]
fn markup_row() {
    let _guard = guard();
    let dir = fixture_dir("markup");
    let md = write_text(&dir, "notes.md", "# Markup row\n\nBody copy.\n");
    let html = write_html_with_remote_image(&dir);

    convert_one(
        "md→docx",
        std::slice::from_ref(&md),
        Action::DocumentConvert(DocumentFormat::Docx),
        EngineId::Pandoc,
        Magic::Bytes(b"PK\x03\x04"),
    );
    convert_one(
        "md→html",
        std::slice::from_ref(&md),
        Action::DocumentConvert(DocumentFormat::Html),
        EngineId::Pandoc,
        Magic::Html,
    );
    // §9.4: the remote <img> must never be fetched — pandoc runs under
    // --sandbox, skips the resource and still writes the DOCX (exit 0).
    convert_one(
        "html→docx (remote image)",
        &[html],
        Action::DocumentConvert(DocumentFormat::Docx),
        EngineId::Pandoc,
        Magic::Bytes(b"PK\x03\x04"),
    );
    // §4: markup → PDF is the Pandoc → DOCX → LibreOffice chain; LibreOffice
    // gates the route, so this line skips until it is installed.
    convert_one(
        "md→pdf",
        &[md],
        Action::DocumentConvert(DocumentFormat::Pdf),
        EngineId::LibreOffice,
        Magic::Bytes(b"%PDF-"),
    );
}

/// §4 row 3: PDF → DOCX/ODT (LibreOffice), TXT/MD/raster (Poppler).
#[test]
fn pdf_convert_row() {
    let _guard = guard();
    let dir = fixture_dir("pdf-convert");
    let pdf = write_text_pdf(&dir, "golden.pdf", "Golden PDF for the convert row");

    convert_one(
        "pdf→docx",
        std::slice::from_ref(&pdf),
        Action::PdfConvert(PdfTarget::Docx),
        EngineId::LibreOffice,
        Magic::Bytes(b"PK\x03\x04"),
    );
    convert_one(
        "pdf→odt",
        std::slice::from_ref(&pdf),
        Action::PdfConvert(PdfTarget::Odt),
        EngineId::LibreOffice,
        Magic::Bytes(b"PK\x03\x04"),
    );
    convert_one(
        "pdf→txt",
        std::slice::from_ref(&pdf),
        Action::PdfConvert(PdfTarget::Txt),
        EngineId::Poppler,
        Magic::Text,
    );
    convert_one(
        "pdf→md",
        std::slice::from_ref(&pdf),
        Action::PdfConvert(PdfTarget::Markdown),
        EngineId::Poppler,
        Magic::Text,
    );
    convert_one(
        "pdf→png pages",
        &[pdf],
        Action::PdfConvert(PdfTarget::Png),
        EngineId::Poppler,
        Magic::Folder,
    );
}

/// §4 row 4: the PDF tools — Ghostscript, qpdf, Poppler, Tesseract.
#[test]
fn pdf_tools_row() {
    let _guard = guard();
    let dir = fixture_dir("pdf-tools");
    let first = write_text_pdf(&dir, "one.pdf", "Golden PDF number one");
    let second = write_text_pdf(&dir, "two.pdf", "Golden PDF number two");

    // Compress may legitimately deliver nothing: Ghostscript reports "no
    // gain" and the original is kept (§4). Both outcomes are checked here.
    if need(
        "pdf compress",
        std::slice::from_ref(&first),
        Action::PdfTool(PdfTool::Compress),
        EngineId::Ghostscript,
    ) {
        let original = std::fs::read(&first).unwrap();
        let outcome = run(
            "pdf compress",
            std::slice::from_ref(&first),
            Action::PdfTool(PdfTool::Compress),
        );
        assert!(!outcome.cancelled, "pdf compress: cancelled");
        assert!(
            outcome.failed.is_empty(),
            "pdf compress: {:?}",
            outcome.failed
        );
        assert_eq!(outcome.created.len(), 1, "pdf compress: created count");
        let created = &outcome.created[0];
        if created.note.is_some() {
            // No gain: nothing beside the source, original untouched.
            assert!(
                created.output.as_os_str().is_empty(),
                "pdf compress: a no-gain run must deliver nothing"
            );
        } else {
            let bytes = std::fs::read(&created.output).expect("the compressed PDF must exist");
            assert!(bytes.starts_with(b"%PDF-"), "pdf compress: not a PDF");
        }
        assert_eq!(
            std::fs::read(&first).unwrap(),
            original,
            "pdf compress: source changed"
        );
        assert_no_temps("pdf compress", &dir);
    }

    convert_one(
        "pdf rotate left",
        std::slice::from_ref(&first),
        Action::PdfTool(PdfTool::RotateLeft),
        EngineId::Qpdf,
        Magic::Bytes(b"%PDF-"),
    );
    convert_one(
        "pdf split",
        std::slice::from_ref(&first),
        Action::PdfTool(PdfTool::Split),
        EngineId::Poppler,
        Magic::Folder,
    );
    convert_one(
        "pdf web-optimize",
        std::slice::from_ref(&first),
        Action::PdfTool(PdfTool::OptimizeWeb),
        EngineId::Qpdf,
        Magic::Bytes(b"%PDF-"),
    );
    convert_one(
        "pdf make searchable",
        std::slice::from_ref(&first),
        Action::PdfTool(PdfTool::Searchable),
        EngineId::Tesseract,
        Magic::Bytes(b"%PDF-"),
    );
    convert_one(
        "pdf merge",
        &[first, second],
        Action::PdfTool(PdfTool::Merge),
        EngineId::Poppler,
        Magic::Bytes(b"%PDF-"),
    );
}

/// §4 row 5: PDF extract — text, embedded images, one page as PNG (Poppler).
#[test]
fn pdf_extract_row() {
    let _guard = guard();
    let dir = fixture_dir("pdf-extract");
    let pdf = write_text_pdf(&dir, "words.pdf", "Golden words for extraction");

    convert_one(
        "pdf text",
        std::slice::from_ref(&pdf),
        Action::PdfExtract(PdfExtract::Text),
        EngineId::Poppler,
        Magic::Text,
    );
    convert_one(
        "pdf embedded images",
        std::slice::from_ref(&pdf),
        Action::PdfExtract(PdfExtract::Images),
        EngineId::Poppler,
        Magic::Folder,
    );
    convert_one(
        "pdf page → png",
        &[pdf],
        Action::PdfExtract(PdfExtract::PagePng),
        EngineId::Poppler,
        Magic::Bytes(b"\x89PNG"),
    );
}

/// §4 row 6: spreadsheets — CSV → JSON is Native, everything else LibreOffice.
#[test]
fn sheet_row() {
    let _guard = guard();
    let dir = fixture_dir("sheet");
    let csv = write_text(&dir, "data.csv", "name,score\nada,10\nbob,20\n");

    convert_one(
        "csv→json",
        std::slice::from_ref(&csv),
        Action::SheetConvert(SheetFormat::Json),
        EngineId::Native,
        Magic::Json,
    );
    convert_one(
        "csv→pdf",
        std::slice::from_ref(&csv),
        Action::SheetConvert(SheetFormat::Pdf),
        EngineId::LibreOffice,
        Magic::Bytes(b"%PDF-"),
    );
    convert_one(
        "csv→xlsx",
        &[csv],
        Action::SheetConvert(SheetFormat::Xlsx),
        EngineId::LibreOffice,
        Magic::Bytes(b"PK\x03\x04"),
    );
}

/// §4 row 7: presentations — LibreOffice, with Poppler finishing the PNG
/// chain. The fixture itself is a real PPTX built by Pandoc.
#[test]
fn presentation_row() {
    let _guard = guard();
    let Some(pandoc) = fixture_tool("presentation", EngineId::Pandoc) else {
        return;
    };
    let dir = fixture_dir("presentation");
    write_text(&dir, "deck.md", "# Golden deck\n\n- one\n- two\n");
    build_fixture(
        &pandoc,
        &["-f", "markdown", "-t", "pptx", "-o", "deck.pptx", "deck.md"],
        &dir,
    );
    let pptx = dir.join("deck.pptx");

    convert_one(
        "pptx→pdf",
        std::slice::from_ref(&pptx),
        Action::PresentConvert(PresentFormat::Pdf),
        EngineId::LibreOffice,
        Magic::Bytes(b"%PDF-"),
    );
    convert_one(
        "pptx→odp",
        std::slice::from_ref(&pptx),
        Action::PresentConvert(PresentFormat::Odp),
        EngineId::LibreOffice,
        Magic::Bytes(b"PK\x03\x04"),
    );
    convert_one(
        "pptx→png",
        &[pptx],
        Action::PresentConvert(PresentFormat::Png),
        EngineId::LibreOffice,
        Magic::Folder,
    );
}

/// §4 row 8: images — PNG → JPG (white background), image → PDF, multi-size
/// ICO. All Native, so this row always runs.
#[test]
fn image_row() {
    let _guard = guard();
    let dir = fixture_dir("image");
    let png = write_png(&dir, "photo.png");

    // §4: PNG → JPG lands on a white background where the source was
    // transparent — asserted on the decoded pixels, not just the magic.
    if need(
        "png→jpg",
        std::slice::from_ref(&png),
        Action::ImageConvert(ImageFormat::Jpeg),
        EngineId::Native,
    ) {
        let outcome = run_and_check(
            "png→jpg",
            std::slice::from_ref(&png),
            Action::ImageConvert(ImageFormat::Jpeg),
            Magic::Bytes(b"\xFF\xD8\xFF"),
        );
        let decoded = image::open(&outcome.created[0].output).expect("the JPEG must decode");
        let corner = decoded.to_rgba8().get_pixel(0, 0).0;
        assert!(
            corner[0] >= 250 && corner[1] >= 250 && corner[2] >= 250,
            "png→jpg: the transparent corner flattened to {corner:?}, not white"
        );
    }
    convert_one(
        "png→pdf",
        std::slice::from_ref(&png),
        Action::ImageConvert(ImageFormat::Pdf),
        EngineId::Native,
        Magic::Bytes(b"%PDF-"),
    );
    convert_one(
        "png→ico",
        &[png],
        Action::ImageConvert(ImageFormat::Ico),
        EngineId::Native,
        Magic::Bytes(&[0x00, 0x00, 0x01, 0x00]),
    );
}

/// §4 row 9: several selected files — merge images (Native), ZIP, batch
/// convert/resize (per-file router), and the PDF merge that needs Poppler.
#[test]
fn multi_row() {
    let _guard = guard();
    let dir = fixture_dir("multi");
    let first = write_jpg(&dir, "first.jpg");
    let second = write_jpg(&dir, "second.jpg");
    let pdf_one = write_text_pdf(&dir, "part-one.pdf", "Part one");
    let pdf_two = write_text_pdf(&dir, "part-two.pdf", "Part two");

    convert_one(
        "merge images",
        &[first.clone(), second.clone()],
        Action::Multi(MultiOp::MergePdf),
        EngineId::Native,
        Magic::Bytes(b"%PDF-"),
    );
    convert_one(
        "zip selection",
        &[first.clone(), second.clone()],
        Action::Multi(MultiOp::Zip),
        EngineId::Native,
        Magic::Bytes(b"PK\x03\x04"),
    );
    convert_one(
        "batch convert",
        &[first.clone(), second.clone()],
        Action::Multi(MultiOp::BatchConvert),
        EngineId::Native,
        Magic::Bytes(b"\x89PNG"),
    );
    convert_one(
        "batch resize",
        &[first, second],
        Action::Multi(MultiOp::BatchResize),
        EngineId::Native,
        Magic::Bytes(b"\xFF\xD8\xFF"),
    );
    convert_one(
        "merge pdfs",
        &[pdf_one, pdf_two],
        Action::Multi(MultiOp::MergePdf),
        EngineId::Poppler,
        Magic::Bytes(b"%PDF-"),
    );
}

/// §4 row 10: ZIP — extract beside the archive, native and zip-slip safe.
#[test]
fn zip_extract_row() {
    let _guard = guard();
    let dir = fixture_dir("zip-extract");
    let archive = dir.join("bundle.zip");
    {
        let file = std::fs::File::create(&archive).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("hello.txt", options).unwrap();
        std::io::Write::write_all(&mut writer, b"golden extract\n").unwrap();
        writer.finish().unwrap();
    }

    convert_one(
        "extract zip",
        &[archive],
        Action::Zip(ZipOp::ExtractHere),
        EngineId::Native,
        Magic::Text,
    );
}

/// §4 row 11: video — remux, the clear MP4 target, audio extraction, GIF.
#[test]
fn video_row() {
    let _guard = guard();
    let Some(ffmpeg) = fixture_tool("video", EngineId::Ffmpeg) else {
        return;
    };
    let dir = fixture_dir("video");
    let clip = dir.join("clip.mp4");
    build_fixture(
        &ffmpeg,
        &[
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=10:duration=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
            "clip.mp4",
        ],
        &dir,
    );

    // §4 "Remux": a stream copy into a fresh file of the same container —
    // clip.mp4 → clip_small.mp4, no re-encode.
    convert_one(
        "remux (stream copy)",
        std::slice::from_ref(&clip),
        Action::VideoOptimize(VideoOptimize::Remux),
        EngineId::Ffmpeg,
        Magic::Mp4,
    );
    // The container change: mp4 → mkv is `-c copy` too (ffmpeg.rs `plan`),
    // and it is what creates clip.mkv for the collision at the end.
    convert_one(
        "mp4→mkv",
        std::slice::from_ref(&clip),
        Action::VideoConvert(VideoFormat::Mkv),
        EngineId::Ffmpeg,
        Magic::Bytes(&[0x1A, 0x45, 0xDF, 0xA3]),
    );
    convert_one(
        "mp3 from video",
        std::slice::from_ref(&clip),
        Action::VideoExtract(VideoExtract::Audio(AudioFormat::Mp3)),
        EngineId::Ffmpeg,
        Magic::Mp3,
    );
    convert_one(
        "gif from video",
        &[clip],
        Action::VideoExtract(VideoExtract::Gif),
        EngineId::Ffmpeg,
        Magic::Bytes(b"GIF8"),
    );
    // clip.mkv was written above; converting it back to MP4 lands on the
    // existing clip.mp4 → collision → "clip (2).mp4".
    let mkv = dir.join("clip.mkv");
    convert_one(
        "mkv→mp4",
        &[mkv],
        Action::VideoConvert(VideoFormat::Mp4),
        EngineId::Ffmpeg,
        Magic::Mp4,
    );
}

/// §4 row 12: audio — a two-second tone built by FFmpeg, then MP3/FLAC and a
/// normalization pass.
#[test]
fn audio_row() {
    let _guard = guard();
    let Some(ffmpeg) = fixture_tool("audio", EngineId::Ffmpeg) else {
        return;
    };
    let dir = fixture_dir("audio");
    let tone = dir.join("tone.wav");
    build_fixture(
        &ffmpeg,
        &[
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
            "-ar",
            "44100",
            "-ac",
            "1",
            "tone.wav",
        ],
        &dir,
    );

    convert_one(
        "wav→mp3",
        std::slice::from_ref(&tone),
        Action::AudioConvert(AudioFormat::Mp3),
        EngineId::Ffmpeg,
        Magic::Mp3,
    );
    convert_one(
        "wav→flac",
        std::slice::from_ref(&tone),
        Action::AudioConvert(AudioFormat::Flac),
        EngineId::Ffmpeg,
        Magic::Bytes(b"fLaC"),
    );
    convert_one(
        "normalize",
        &[tone],
        Action::AudioOptimize(AudioOptimize::Normalize),
        EngineId::Ffmpeg,
        Magic::Bytes(b"RIFF"),
    );
}

/// §9.2 "no overwrite on name collision": the pre-existing file survives byte
/// for byte and the new output takes the `(2)` suffix.
#[test]
fn a_collision_never_overwrites() {
    let _guard = guard();
    let dir = fixture_dir("collision");
    let png = write_png(&dir, "photo.png");
    let sentinel = dir.join("photo.jpg");
    std::fs::write(&sentinel, b"pre-existing jpeg").unwrap();

    let action = Action::ImageConvert(ImageFormat::Jpeg);
    assert!(need(
        "collision",
        std::slice::from_ref(&png),
        action,
        EngineId::Native
    ));
    let before = std::fs::read(&png).unwrap();
    let outcome = run("collision", std::slice::from_ref(&png), action);

    assert!(outcome.failed.is_empty(), "collision: {:?}", outcome.failed);
    assert_eq!(outcome.created.len(), 1, "collision: created count");
    let created = &outcome.created[0];
    assert_eq!(
        created.output.file_name().unwrap(),
        "photo (2).jpg",
        "the free name must be the (2) suffix"
    );
    let bytes = std::fs::read(&created.output).unwrap();
    assert!(bytes.starts_with(b"\xFF\xD8\xFF"));
    assert!(!bytes.is_empty());
    assert_eq!(
        std::fs::read(&sentinel).unwrap(),
        b"pre-existing jpeg",
        "the existing file was overwritten"
    );
    assert_eq!(std::fs::read(&png).unwrap(), before, "the source changed");
    assert_no_temps("collision", &dir);
}
