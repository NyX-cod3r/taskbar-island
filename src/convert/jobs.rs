//! jobs.rs — Native engine jobs: local decoders and encoders only.
//!
//! One job reads exactly one input, produces exactly one new file next to it
//! and never touches, renames or deletes anything that already existed. The
//! bytes are written to a private temp name and moved into place without the
//! replace flag, so a collision always yields `name (2).ext` instead of
//! overwriting the earlier file. There is no `remove_file`/`DeleteFile` call in
//! this path: a rare failed write leaves its temp behind rather than deleting
//! anything.
//!
//! Every decoder call is wrapped in `catch_unwind` by the batch runner;
//! malformed files become a per-file error, never a process exit.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use image::codecs::bmp::BmpEncoder;
use image::codecs::gif::GifEncoder;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::metadata::Orientation;
use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageReader, Limits, RgbaImage,
};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};

use crate::design::{self, Color};
use crate::engines::output::{TargetFormat, Workspace};
use crate::engines::router::{self, Candidate, FallbackFailure};
use crate::engines::{
    self, Action, CancelToken, CapabilityTable, DocumentFormat, EngineError, EngineId, EnginePaths,
    FileKind, ImageEdit, ImageFormat, ImageOptimize, Job, JobOutput, Progress, ProgressSink, Stage,
};

use super::detect::{self, SourceFormat};
use super::{MAX_DECODE_BYTES, MAX_FILE_BYTES, MAX_INPUTS, MAX_PIXELS};

#[derive(Debug, Clone)]
#[allow(dead_code)] // input/format fields feed the Done list (B2).
pub struct Created {
    pub input: String,
    pub input_name: String,
    pub output: PathBuf,
    pub output_name: String,
    /// The single file the job produced. `None` for OCR-to-clipboard (text
    /// only) and for extract-embedded-images (a folder).
    pub format: Option<TargetFormat>,
    /// OCR text that went to the clipboard; every other job leaves this `None`.
    pub text: Option<String>,
    pub input_bytes: u64,
    pub output_bytes: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // input fields feed the Failed list (B2).
pub struct Failure {
    pub input: String,
    pub input_name: String,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub created: Vec<Created>,
    pub failed: Vec<Failure>,
    pub cancelled: bool,
}

impl Outcome {
    pub fn input_bytes(&self) -> u64 {
        self.created.iter().map(|c| c.input_bytes).sum()
    }

    pub fn output_bytes(&self) -> u64 {
        self.created.iter().map(|c| c.output_bytes).sum()
    }

    /// "54% smaller" for the done card; `None` when the file grew.
    pub fn saved_percent(&self) -> Option<u32> {
        let input = self.input_bytes();
        let output = self.output_bytes();
        if input == 0 || output >= input {
            return None;
        }
        Some((((input - output) as f64 / input as f64) * 100.0).round() as u32)
    }
}

// ------------------------------------------------------------- description

pub fn extension(format: ImageFormat) -> &'static str {
    TargetFormat::Image(format).extension()
}

pub fn badge_color(target: TargetFormat) -> Color {
    match target {
        TargetFormat::Image(format) => match format {
            ImageFormat::Jpeg | ImageFormat::Pdf => design::BADGE_PDF,
            ImageFormat::Png | ImageFormat::Bmp => design::BADGE_IMAGE,
            ImageFormat::WebP | ImageFormat::Gif | ImageFormat::Avif => design::BADGE_VIDEO,
            ImageFormat::Ico => design::BADGE_ARCHIVE,
            ImageFormat::Tiff => design::BADGE_OTHER,
        },
        TargetFormat::Video(_) => design::BADGE_VIDEO,
        TargetFormat::Audio(_) => design::FILE_AUDIO,
        // A PDF produced from a document still gets the PDF badge.
        TargetFormat::Document(DocumentFormat::Pdf) => design::BADGE_PDF,
        TargetFormat::Document(_) => design::BADGE_DOC,
        TargetFormat::Sheet(_) | TargetFormat::Present(_) => design::BADGE_DOC,
    }
}

/// Optimize keeps the format and writes `_small`; edit writes `_edited`;
/// convert keeps the plain stem.
pub fn suffix(action: Action) -> Option<&'static str> {
    engines::output::suffix(action)
}

pub fn preview_name(input: &str, action: Action, target: TargetFormat) -> String {
    engines::output::preview_name(Path::new(input), action, target)
}

// ------------------------------------------------------------------ batch

/// Runs every input through the router, sequentially, and reports one outcome
/// per file. A cancelled batch keeps the files it already produced. `progress`
/// reports per-file boundaries; `on_engine` forwards the running engine's own
/// stage and fraction.
#[allow(clippy::too_many_arguments)]
pub fn run_batch(
    inputs: &[String],
    action: Action,
    language: Option<&str>,
    caps: &CapabilityTable,
    paths: &EnginePaths,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(usize, usize, &str),
    on_engine: &mut dyn FnMut(usize, usize, Progress),
) -> Outcome {
    let mut outcome = Outcome::default();
    let inputs: Vec<&String> = inputs.iter().take(MAX_INPUTS).collect();
    let total = inputs.len();
    for (index, input) in inputs.iter().enumerate() {
        if cancel.is_cancelled() {
            outcome.cancelled = true;
            return outcome;
        }
        let name = file_name(input);
        progress(index, total, &name);
        let mut sink = |progress: Progress| on_engine(index, total, progress);
        match catch_unwind(AssertUnwindSafe(|| {
            run_routed(input, action, language, caps, paths, cancel, &mut sink)
        })) {
            Ok(Ok(created)) => outcome.created.push(created),
            Ok(Err(reason)) => outcome.failed.push(Failure {
                input: (*input).clone(),
                input_name: name,
                reason,
            }),
            Err(_) => outcome.failed.push(Failure {
                input: (*input).clone(),
                input_name: name,
                reason: "This file couldn't be converted safely.".into(),
            }),
        }
    }
    progress(total, total, "");
    if cancel.is_cancelled() {
        outcome.cancelled = true;
    }
    outcome
}

/// Picks the first working engine from the static route and converts one file.
/// A non-cancel failure falls through to the next candidate exactly once.
fn run_routed(
    input: &str,
    action: Action,
    language: Option<&str>,
    caps: &CapabilityTable,
    paths: &EnginePaths,
    cancel: &CancelToken,
    on_engine: &mut dyn FnMut(Progress),
) -> Result<Created, String> {
    let kind = detect::file_kind(input);
    let candidates = router::route(kind, action, caps);
    if candidates.is_empty() {
        return Err(missing_engine(kind));
    }
    let job = Job {
        input: PathBuf::from(input),
        // External runners only use the parent as the destination folder; the
        // actual name is chosen collision-safely by `output::deliver_in`.
        output: PathBuf::from(input).with_extension("out"),
        action,
        language: language.map(str::to_owned),
    };
    let mut attempt = |candidate: &Candidate| -> Result<JobOutput, EngineError> {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        match candidate.engine {
            EngineId::Native => run_job(&job, &mut |progress| on_engine(progress), cancel),
            id => {
                let exe = paths.location(id).ok_or(EngineError::NotFound)?;
                engines::run_external(
                    id,
                    exe,
                    paths.helper(),
                    &job,
                    &mut |progress| on_engine(progress),
                    cancel,
                )
            }
        }
    };
    let result =
        router::run_with_fallback(&candidates, &mut attempt).map_err(reason_from_failure)?;
    Ok(created_from(input, result.value))
}

fn missing_engine(kind: FileKind) -> String {
    if kind == FileKind::Other {
        "This type can't be converted yet.".into()
    } else {
        "This option needs an installed engine.".into()
    }
}

fn reason_from_failure(failure: FallbackFailure) -> String {
    match failure.error {
        EngineError::Cancelled => "Cancelled.".into(),
        EngineError::NotFound => "This option needs an installed engine.".into(),
        EngineError::Timeout => "The engine took too long and was stopped.".into(),
        EngineError::Unsupported => "This option can't run on this file.".into(),
        EngineError::Spawn(message) | EngineError::Failed(message) => message,
    }
}

fn created_from(input: &str, output: JobOutput) -> Created {
    let output_name = output
        .output
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Created {
        input: input.to_owned(),
        input_name: file_name(input),
        output_bytes: path_bytes(&output.output),
        input_bytes: std::fs::metadata(input).map(|meta| meta.len()).unwrap_or(0),
        output: output.output,
        output_name,
        format: output.format,
        text: output.text,
    }
}

fn path_bytes(path: &Path) -> u64 {
    if path.is_dir() {
        return std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| path_bytes(&entry.path()))
                    .sum()
            })
            .unwrap_or(0);
    }
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Converts exactly one file with the Native engine. `Err` is a user-facing
/// reason. Tests use it; the router runs batches through `run_batch`.
#[allow(dead_code)] // Single-file native path kept for tests.
pub fn run_one(path: &str, action: Action) -> Result<Created, String> {
    let (image, source, input_bytes) = decode(path)?;
    let format = native_output_format(action, source)
        .ok_or_else(|| "This option can't run on this file.".to_string())?;
    if !native_write(format) {
        return Err("This format needs an installed engine.".into());
    }
    let image = apply(image, action);
    let encoded = encode(&image, format, action)?;
    verify(&encoded, format)?;
    let output = write_beside(path, action, format, &encoded)?;
    Ok(Created {
        input: path.to_owned(),
        input_name: file_name(path),
        output_name: output
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        output,
        format: Some(TargetFormat::Image(format)),
        text: None,
        input_bytes,
        output_bytes: encoded.len() as u64,
    })
}

/// The input's own container, when Native can both read and write it.
fn native_output_format(action: Action, source: SourceFormat) -> Option<ImageFormat> {
    match action {
        Action::ImageConvert(target) => Some(target),
        Action::ImageOptimize(ImageOptimize::WebReady) => Some(ImageFormat::WebP),
        Action::ImageOptimize(_) | Action::ImageEdit(_) => source.image_format(),
        _ => None,
    }
}

/// The `Engine` trait entry point: one job in, one new file out.
pub fn run_job(
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    if !matches!(
        job.action,
        Action::ImageConvert(_) | Action::ImageOptimize(_) | Action::ImageEdit(_)
    ) {
        return Err(EngineError::Unsupported);
    }
    let input = job.input.to_string_lossy().into_owned();
    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    let (image, source, _) = decode(&input).map_err(EngineError::Failed)?;
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    progress(Progress {
        stage: Stage::Converting,
        fraction: Some(0.5),
    });
    let format = native_output_format(job.action, source).ok_or(EngineError::Unsupported)?;
    if !native_write(format) {
        return Err(EngineError::Unsupported);
    }
    let image = apply(image, job.action);
    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });
    let encoded = encode(&image, format, job.action).map_err(EngineError::Failed)?;
    verify(&encoded, format).map_err(EngineError::Failed)?;
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    let dir = job
        .output
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty());
    let output =
        write_beside_in(dir, &input, job.action, format, &encoded).map_err(EngineError::Failed)?;
    progress(Progress {
        stage: Stage::Done,
        fraction: Some(1.0),
    });
    Ok(JobOutput {
        output,
        format: Some(TargetFormat::Image(format)),
        text: None,
    })
}

fn native_write(format: ImageFormat) -> bool {
    matches!(
        format,
        ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::WebP
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::Ico
            | ImageFormat::Pdf
    )
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

// ----------------------------------------------------------------- decode

fn decode(path: &str) -> Result<(DynamicImage, SourceFormat, u64), String> {
    let meta =
        std::fs::metadata(path).map_err(|_| "This file is no longer where it was.".to_string())?;
    if meta.is_dir() {
        return Err("Folders can't be converted. Drop a file instead.".into());
    }
    if detect::is_cloud_placeholder(path) {
        return Err("This file isn't downloaded on this PC yet.".into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err("This file is larger than the 1 GB conversion limit.".into());
    }

    let mut file = File::open(path).map_err(|_| "This file couldn't be opened.".to_string())?;
    let mut head = [0u8; 32];
    let read = file
        .read(&mut head)
        .map_err(|_| "This file couldn't be read.".to_string())?;
    let format = detect::sniff(&head[..read]);
    let Some(crate_format) = format.image_crate() else {
        return Err("This type can't be converted yet.".into());
    };
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "This file couldn't be read.".to_string())?;

    let mut reader = ImageReader::with_format(BufReader::new(file), crate_format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(40_000);
    limits.max_image_height = Some(40_000);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "This image couldn't be read. It may be damaged.".to_string())?;
    let (width, height) = decoder.dimensions();
    if width < 1 || height < 1 {
        return Err("This image couldn't be read. It may be damaged.".into());
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("This image is larger than 150 megapixels.".into());
    }
    if decoder.total_bytes() > MAX_DECODE_BYTES {
        return Err("This image would need too much memory to decode.".into());
    }
    // Every output is re-encoded, so an EXIF rotation is baked into pixels
    // exactly once, here, instead of being carried to the new file.
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|_| "This image couldn't be read. It may be damaged.".to_string())?;
    if orientation != Orientation::NoTransforms {
        image.apply_orientation(orientation);
    }
    Ok((image, format, meta.len()))
}

// ----------------------------------------------------------------- pixels

fn apply(mut image: DynamicImage, action: Action) -> DynamicImage {
    match action {
        Action::ImageOptimize(optimize) => match optimize {
            ImageOptimize::ResizePercent(percent) => {
                let long = image.width().max(image.height());
                let cap = (f64::from(long) * f64::from(percent) / 100.0)
                    .round()
                    .max(1.0) as u32;
                image = resize_long_edge(image, cap, image::imageops::FilterType::Lanczos3);
            }
            ImageOptimize::Fit(px) => {
                image = resize_long_edge(image, px, image::imageops::FilterType::Lanczos3);
            }
            ImageOptimize::Thumbnail(px) => {
                image = resize_long_edge(image, px, image::imageops::FilterType::Triangle);
            }
            ImageOptimize::WebReady => {
                image = resize_long_edge(image, 1920, image::imageops::FilterType::Lanczos3);
            }
            ImageOptimize::Compress | ImageOptimize::StripMetadata => {}
        },
        Action::ImageEdit(edit) => {
            image = match edit {
                ImageEdit::RotateLeft => image.rotate270(),
                ImageEdit::RotateRight => image.rotate90(),
                ImageEdit::FlipHorizontal => image.fliph(),
                ImageEdit::FlipVertical => image.flipv(),
                ImageEdit::Grayscale => image.grayscale(),
                ImageEdit::BlackWhite => {
                    let luma = image.to_luma8();
                    let thresholded: image::GrayImage =
                        image::ImageBuffer::from_fn(luma.width(), luma.height(), |x, y| {
                            if luma.get_pixel(x, y).0[0] >= 128 {
                                image::Luma([255u8])
                            } else {
                                image::Luma([0u8])
                            }
                        });
                    DynamicImage::ImageLuma8(thresholded)
                }
                ImageEdit::CropSquare => {
                    let size = image.width().min(image.height());
                    let x = (image.width() - size) / 2;
                    let y = (image.height() - size) / 2;
                    image.crop_imm(x, y, size, size)
                }
            };
        }
        _ => {}
    }
    image
}

fn resize_long_edge(
    image: DynamicImage,
    max: u32,
    filter: image::imageops::FilterType,
) -> DynamicImage {
    let (width, height) = (image.width(), image.height());
    let long = width.max(height);
    if long <= max {
        return image;
    }
    let scale = f64::from(max) / f64::from(long);
    let new_width = ((f64::from(width) * scale).round() as u32).max(1);
    let new_height = ((f64::from(height) * scale).round() as u32).max(1);
    image.resize_exact(new_width, new_height, filter)
}

// ----------------------------------------------------------------- encode

fn encode(image: &DynamicImage, format: ImageFormat, action: Action) -> Result<Vec<u8>, String> {
    let encoded: Result<Vec<u8>, String> = match format {
        ImageFormat::Jpeg => encode_jpeg(image, jpeg_quality(action))
            .map_err(|_| "This image couldn't be encoded.".to_string()),
        ImageFormat::Png => {
            encode_png(image).map_err(|_| "This image couldn't be encoded.".to_string())
        }
        ImageFormat::WebP => encode_webp(image, webp_quality(action)),
        ImageFormat::Gif => {
            encode_gif(image).map_err(|_| "This image couldn't be encoded.".to_string())
        }
        ImageFormat::Bmp => {
            encode_bmp(image).map_err(|_| "This image couldn't be encoded.".to_string())
        }
        ImageFormat::Ico => {
            encode_ico(image).map_err(|_| "This image couldn't be encoded.".to_string())
        }
        ImageFormat::Pdf => {
            encode_pdf(image).map_err(|_| "This image couldn't be encoded.".to_string())
        }
        ImageFormat::Tiff | ImageFormat::Avif => {
            return Err("This format needs an installed engine.".into())
        }
    };
    encoded
}

fn jpeg_quality(action: Action) -> u8 {
    match action {
        Action::ImageOptimize(ImageOptimize::Compress) => 68,
        Action::ImageOptimize(_) => 88,
        Action::ImageEdit(_) => 92,
        _ => 85,
    }
}

fn webp_quality(action: Action) -> f32 {
    match action {
        Action::ImageOptimize(ImageOptimize::Compress) => 60.0,
        Action::ImageOptimize(ImageOptimize::WebReady) => 75.0,
        Action::ImageOptimize(_) => 85.0,
        Action::ImageEdit(_) => 90.0,
        _ => 80.0,
    }
}

fn encode_png(image: &DynamicImage) -> image::ImageResult<Vec<u8>> {
    let mut out = Vec::new();
    let width = image.width();
    let height = image.height();
    let encoder =
        PngEncoder::new_with_quality(&mut out, CompressionType::Default, FilterType::Adaptive);
    if image.color().has_alpha() {
        let pixels = image.to_rgba8();
        encoder.write_image(pixels.as_raw(), width, height, ExtendedColorType::Rgba8)?;
    } else {
        let pixels = image.to_rgb8();
        encoder.write_image(pixels.as_raw(), width, height, ExtendedColorType::Rgb8)?;
    }
    Ok(out)
}

/// JPEG has no alpha; transparency is composited onto white so the result is
/// never a black box. Opaque images skip the RGBA copy and the blend.
fn encode_jpeg(image: &DynamicImage, quality: u8) -> image::ImageResult<Vec<u8>> {
    let mut out = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut out, quality);
    if image.color().has_alpha() {
        let rgba = image.to_rgba8();
        let mut rgb = Vec::with_capacity((image.width() * image.height() * 3) as usize);
        for pixel in rgba.pixels() {
            let alpha = u32::from(pixel[3]);
            for channel in &pixel.0[..3] {
                let blended = (u32::from(*channel) * alpha + 255 * (255 - alpha)) / 255;
                rgb.push(blended as u8);
            }
        }
        encoder.write_image(&rgb, image.width(), image.height(), ExtendedColorType::Rgb8)?;
    } else {
        let rgb = image.to_rgb8();
        encoder.write_image(
            rgb.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgb8,
        )?;
    }
    Ok(out)
}

fn encode_webp(image: &DynamicImage, quality: f32) -> Result<Vec<u8>, String> {
    let rgba = image.to_rgba8();
    let encoded =
        webp::Encoder::from_rgba(rgba.as_raw(), image.width(), image.height()).encode(quality);
    Ok(encoded.to_vec())
}

/// One still frame per file: the native pipeline never re-encodes animation.
fn encode_gif(image: &DynamicImage) -> image::ImageResult<Vec<u8>> {
    let mut out = Vec::new();
    let rgba = image.to_rgba8();
    GifEncoder::new(&mut out).write_image(
        rgba.as_raw(),
        image.width(),
        image.height(),
        ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

fn encode_bmp(image: &DynamicImage) -> image::ImageResult<Vec<u8>> {
    let mut out = Vec::new();
    let width = image.width();
    let height = image.height();
    let encoder = BmpEncoder::new(&mut out);
    if image.color().has_alpha() {
        let pixels = image.to_rgba8();
        encoder.write_image(pixels.as_raw(), width, height, ExtendedColorType::Rgba8)?;
    } else {
        let pixels = image.to_rgb8();
        encoder.write_image(pixels.as_raw(), width, height, ExtendedColorType::Rgb8)?;
    }
    Ok(out)
}

fn encode_pdf(image: &DynamicImage) -> image::ImageResult<Vec<u8>> {
    let width = image.width();
    let height = image.height();
    let has_alpha = image.color().has_alpha();

    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let image_id = Ref::new(4);
    let mask_id = Ref::new(5);
    let content_id = Ref::new(6);

    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.pages(page_tree_id).kids([page_id]).count(1);

    let mut page = pdf.page(page_id);
    page.media_box(Rect::new(0.0, 0.0, width as f32, height as f32));
    page.parent(page_tree_id);
    page.contents(content_id);
    page.resources().x_objects().pair(Name(b"Im1"), image_id);
    page.finish();

    let (filter, payload, mask) = if has_alpha {
        let rgb = image.to_rgb8();
        let rgba = image.to_rgba8();
        let alphas: Vec<u8> = rgba.pixels().map(|pixel| pixel.0[3]).collect();
        (Filter::FlateDecode, zlib(rgb.as_raw()), Some(zlib(&alphas)))
    } else {
        (Filter::DctDecode, encode_jpeg(image, 88)?, None)
    };

    let mut xobject = pdf.image_xobject(image_id, &payload);
    xobject.filter(filter);
    xobject.width(width as i32);
    xobject.height(height as i32);
    xobject.color_space().device_rgb();
    xobject.bits_per_component(8);
    if mask.is_some() {
        xobject.s_mask(mask_id);
    }
    xobject.finish();

    if let Some(mask) = &mask {
        let mut smask = pdf.image_xobject(mask_id, mask);
        smask.filter(Filter::FlateDecode);
        smask.width(width as i32);
        smask.height(height as i32);
        smask.color_space().device_gray();
        smask.bits_per_component(8);
        smask.finish();
    }

    let mut content = Content::new();
    content.save_state();
    content.transform([width as f32, 0.0, 0.0, height as f32, 0.0, 0.0]);
    content.x_object(Name(b"Im1"));
    content.restore_state();
    pdf.stream(content_id, &content.finish());

    Ok(pdf.finish())
}

fn zlib(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing into a Vec cannot fail.
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

/// Windows explorer-style icon: every common size, square with transparent
/// padding so non-square photos keep their aspect.
fn encode_ico(image: &DynamicImage) -> image::ImageResult<Vec<u8>> {
    const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
    let mut frames = Vec::with_capacity(SIZES.len());
    for size in SIZES {
        let scaled = image.thumbnail(size, size);
        let mut square: RgbaImage = RgbaImage::new(size, size);
        let x = (size - scaled.width()) / 2;
        let y = (size - scaled.height()) / 2;
        image::imageops::overlay(&mut square, &scaled.to_rgba8(), i64::from(x), i64::from(y));
        frames.push(IcoFrame::as_png(
            square.as_raw(),
            size,
            size,
            ExtendedColorType::Rgba8,
        )?);
    }
    let mut out = Vec::new();
    IcoEncoder::new(&mut out).encode_images(&frames)?;
    Ok(out)
}

fn verify(bytes: &[u8], format: ImageFormat) -> Result<(), String> {
    let ok = match format {
        ImageFormat::Jpeg => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        ImageFormat::Png => bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
        ImageFormat::WebP => {
            bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
        }
        ImageFormat::Gif => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        ImageFormat::Bmp => bytes.starts_with(b"BM"),
        ImageFormat::Pdf => bytes.starts_with(b"%PDF-"),
        ImageFormat::Ico => bytes.len() >= 6 && bytes[0..4] == [0x00, 0x00, 0x01, 0x00],
        ImageFormat::Tiff | ImageFormat::Avif => false,
    };
    if bytes.is_empty() || !ok {
        return Err("The encoder produced an unreadable file.".into());
    }
    Ok(())
}

// ------------------------------------------------------------------ write

/// Writes `bytes` beside the original, picking the first free `(n)` suffix and
/// starting collisions at `(2)` as the spec says. Nothing is ever overwritten
/// or deleted: the temp file is moved with a no-replace flag, so losing a race
/// still fails safely.
#[allow(dead_code)] // Reached only through run_one (tests).
fn write_beside(
    input: &str,
    action: Action,
    format: ImageFormat,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    write_beside_in(None, input, action, format, bytes)
}

fn write_beside_in(
    dir: Option<&Path>,
    input: &str,
    action: Action,
    format: ImageFormat,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    let original = Path::new(input);
    let dir = dir
        .or_else(|| original.parent())
        .unwrap_or_else(|| Path::new("."));
    let stem = original
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "converted".into());
    let stem = match suffix(action) {
        Some(suffix) => format!("{stem}{suffix}"),
        None => stem,
    };
    let extension = extension(format);
    let temp = dir.join(format!(
        ".taskbar-island-{}-{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    write_temp(&temp, bytes)?;
    for index in 0..1000u32 {
        let name = if index == 0 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem} ({}).{extension}", index + 1)
        };
        let destination = dir.join(&name);
        match move_new(&temp, &destination) {
            Ok(()) => return Ok(destination),
            Err(MoveError::Exists) => continue,
            Err(MoveError::Failed(reason)) => return Err(reason),
        }
    }
    Err("There are too many files with this name.".into())
}

fn write_temp(temp: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
        .map_err(|e| format!("The file couldn't be written: {e}."))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("The file couldn't be written: {e}."))
}

enum MoveError {
    Exists,
    Failed(String),
}

fn move_new(from: &Path, destination: &Path) -> Result<(), MoveError> {
    let from = windows::core::HSTRING::from(from.as_os_str());
    let to = windows::core::HSTRING::from(destination.as_os_str());
    let result = unsafe {
        windows::Win32::Storage::FileSystem::MoveFileExW(
            &from,
            &to,
            windows::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
        )
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            let code = error.code().0 as u32 & 0xFFFF;
            // ERROR_FILE_EXISTS (80) / ERROR_ALREADY_EXISTS (183)
            if code == 80 || code == 183 {
                Err(MoveError::Exists)
            } else {
                Err(MoveError::Failed(format!(
                    "The file couldn't be moved into place: {error}."
                )))
            }
        }
    }
}

// -------------------------------------------------------------- pipeline

/// A stage body: it sees the job's private workspace and the cancel token so
/// a running child process can be killed.
type StageRun = Box<dyn FnOnce(&Workspace, &CancelToken) -> Result<(), EngineError>>;

/// One stage of a multi-step job (LibreOffice → Poppler → Pandoc …). Every
/// stage writes only inside the job's private workspace.
#[allow(dead_code)] // Wired into the PDF/Office runners in B3.
pub struct Step {
    /// Engine name for the log line ("libreoffice", "pdftoppm", …).
    pub label: &'static str,
    run: StageRun,
}

#[allow(dead_code)] // Wired into the PDF/Office runners in B3.
impl Step {
    pub fn new(
        label: &'static str,
        run: impl FnOnce(&Workspace, &CancelToken) -> Result<(), EngineError> + 'static,
    ) -> Self {
        Self {
            label,
            run: Box::new(run),
        }
    }
}

/// Runs a whole `Vec<Step>` inside one private workspace behind a single
/// progress bar. Each finished step reports its equal share `(i + 1) / n`, so
/// the marks sum to exactly 1.0 at the end of the run, and the delivery
/// callback gets one final `1.0` with `Stage::Done`. Cancelling before,
/// between or inside a stage stops the run with `EngineError::Cancelled` and
/// `finish` is never called. The workspace is owned here and dropped on every
/// path — that drop is what removes the temp dir, so this module itself
/// contains no deletion call (the security test scans it).
#[allow(dead_code)] // Wired into the PDF/Office runners in B3.
pub fn run_pipeline<T>(
    workspace: Workspace,
    steps: Vec<Step>,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
    finish: impl FnOnce(&Workspace) -> Result<T, EngineError>,
) -> Result<T, EngineError> {
    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    let count = steps.len().max(1);
    let total = count as f32;
    for (index, step) in steps.into_iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        crate::log::log(&format!(
            "convert.pipeline step {}/{}: {}",
            index + 1,
            count,
            step.label
        ));
        (step.run)(&workspace, cancel)?;
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        progress(Progress {
            stage: Stage::Converting,
            fraction: Some((index + 1) as f32 / total),
        });
    }
    let value = finish(&workspace)?;
    progress(Progress {
        stage: Stage::Done,
        fraction: Some(1.0),
    });
    Ok(value)
}

static TEMP_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::{DocumentExtract, VideoFormat};
    use image::{Rgba, RgbaImage};

    fn scratch(test: &str) -> PathBuf {
        // Unique per run: tests never delete anything, so reusing a folder
        // would make collision assertions depend on earlier runs.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "taskbar-island-jobs-{test}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(width: u32, height: u32) -> DynamicImage {
        let mut rgba = RgbaImage::new(width, height);
        for (x, y, pixel) in rgba.enumerate_pixels_mut() {
            *pixel = Rgba([
                (x * 7 % 256) as u8,
                (y * 13 % 256) as u8,
                ((x + y) % 256) as u8,
                if x < width / 2 { 255 } else { 128 },
            ]);
        }
        DynamicImage::ImageRgba8(rgba)
    }

    fn write_png(dir: &Path, name: &str, width: u32, height: u32) -> (PathBuf, Vec<u8>) {
        let bytes = encode_png(&sample(width, height)).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, &bytes).unwrap();
        (path, bytes)
    }

    fn dimensions(path: &Path) -> (u32, u32) {
        let reader = ImageReader::open(path)
            .unwrap()
            .with_guessed_format()
            .unwrap();
        reader.into_dimensions().unwrap()
    }

    fn convert(format: ImageFormat) -> Action {
        Action::ImageConvert(format)
    }

    fn native_caps() -> CapabilityTable {
        let mut caps = CapabilityTable::default();
        caps.set(
            EngineId::Native,
            crate::engines::probe::native_capabilities(),
        );
        caps
    }

    /// Native-only batch, the way these tests exercise the job path.
    fn batch(
        inputs: &[String],
        action: Action,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(usize, usize, &str),
    ) -> Outcome {
        run_batch(
            inputs,
            action,
            None,
            &native_caps(),
            &EnginePaths::default(),
            cancel,
            progress,
            &mut |_, _, _| {},
        )
    }

    #[test]
    fn every_native_output_format_writes_a_verifiable_file_and_keeps_the_original() {
        let dir = scratch("matrix");
        let (input, original) = write_png(&dir, "photo.png", 96, 48);
        let input = input.display().to_string();
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::WebP,
            ImageFormat::Gif,
            ImageFormat::Bmp,
            ImageFormat::Ico,
            ImageFormat::Pdf,
        ] {
            let created = run_one(&input, convert(format)).unwrap();
            let bytes = std::fs::read(&created.output).unwrap();
            verify(&bytes, format).unwrap();
            assert_eq!(created.input_bytes, original.len() as u64);
            assert_eq!(created.output_bytes, bytes.len() as u64);
            assert_eq!(created.format, Some(TargetFormat::Image(format)));
            assert!(created
                .output_name
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", extension(format))));
        }
        assert_eq!(std::fs::read(dir.join("photo.png")).unwrap(), original);
        for file in std::fs::read_dir(&dir).unwrap() {
            let name = file.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!name.ends_with(".tmp"), "a temp file leaked: {name}");
        }
    }

    #[test]
    fn conversions_are_pixel_dimension_correct() {
        let dir = scratch("dims");
        let (input, _) = write_png(&dir, "dims.png", 80, 40);
        let input = input.display().to_string();
        let png = run_one(&input, convert(ImageFormat::Png)).unwrap();
        assert_eq!(dimensions(&png.output), (80, 40));
        let pdf = run_one(&input, convert(ImageFormat::Pdf)).unwrap();
        let pdf_bytes = std::fs::read(&pdf.output).unwrap();
        let needle = b"80 40";
        assert!(pdf_bytes
            .windows(needle.len())
            .any(|window| window == needle));
    }

    #[test]
    fn ico_contains_all_common_sizes() {
        let dir = scratch("ico");
        let (input, _) = write_png(&dir, "icon.png", 300, 200);
        let created = run_one(&input.display().to_string(), convert(ImageFormat::Ico)).unwrap();
        let bytes = std::fs::read(&created.output).unwrap();
        assert_eq!(&bytes[0..4], &[0, 0, 1, 0]);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 7);
    }

    #[test]
    fn existing_files_are_never_overwritten_and_collisions_start_at_two() {
        let dir = scratch("collision");
        let (input, _) = write_png(&dir, "dupe.png", 32, 32);
        std::fs::write(dir.join("dupe.jpg"), b"keep me").unwrap();
        let input = input.display().to_string();
        // PNG to JPG is a convert; the first free name after the existing
        // dupe.jpg is "dupe (2).jpg".
        let first = run_one(&input, convert(ImageFormat::Jpeg)).unwrap();
        assert_eq!(first.output_name, "dupe (2).jpg");
        assert_eq!(std::fs::read(dir.join("dupe.jpg")).unwrap(), b"keep me");
    }

    #[test]
    fn optimize_and_edit_keep_the_format_and_write_small_or_edited() {
        let dir = scratch("suffix");
        let (input, _) = write_png(&dir, "wide.png", 3000, 1500);
        let input = input.display().to_string();

        let resized = run_one(
            &input,
            Action::ImageOptimize(ImageOptimize::ResizePercent(50)),
        )
        .unwrap();
        assert_eq!(resized.output_name, "wide_small.png");
        assert_eq!(dimensions(&resized.output), (1500, 750));

        let fit = run_one(&input, Action::ImageOptimize(ImageOptimize::Fit(1280))).unwrap();
        assert_eq!(dimensions(&fit.output), (1280, 640));

        let thumb = run_one(&input, Action::ImageOptimize(ImageOptimize::Thumbnail(256))).unwrap();
        assert_eq!(dimensions(&thumb.output), (256, 128));

        let edited = run_one(&input, Action::ImageEdit(ImageEdit::RotateLeft)).unwrap();
        assert_eq!(edited.output_name, "wide_edited.png");
        assert_eq!(dimensions(&edited.output), (1500, 3000));

        let crop = run_one(&input, Action::ImageEdit(ImageEdit::CropSquare)).unwrap();
        assert_eq!(dimensions(&crop.output), (1500, 1500));
    }

    #[test]
    fn web_ready_writes_webp_small() {
        let dir = scratch("webready");
        let (input, _) = write_png(&dir, "page.png", 2000, 1000);
        let created = run_one(
            &input.display().to_string(),
            Action::ImageOptimize(ImageOptimize::WebReady),
        )
        .unwrap();
        assert_eq!(created.format, Some(TargetFormat::Image(ImageFormat::WebP)));
        assert_eq!(created.output_name, "page_small.webp");
        assert_eq!(dimensions(&created.output), (1920, 960));
    }

    #[test]
    fn extension_is_ignored_for_decoding() {
        let dir = scratch("magic");
        let (input, _) = write_png(&dir, "actually-png.txt", 24, 24);
        let created = run_one(&input.display().to_string(), convert(ImageFormat::WebP)).unwrap();
        assert!(created.output_name.ends_with(".webp"));
    }

    #[test]
    fn junk_and_truncated_files_fail_without_panicking() {
        let dir = scratch("junk");
        let junk = dir.join("junk.png");
        std::fs::write(&junk, b"definitely not an image").unwrap();
        assert!(run_one(&junk.display().to_string(), convert(ImageFormat::Png)).is_err());

        let truncated = dir.join("truncated.png");
        std::fs::write(
            &truncated,
            [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00],
        )
        .unwrap();
        let cancel = CancelToken::new();
        let outcome = batch(
            &[junk.display().to_string(), truncated.display().to_string()],
            convert(ImageFormat::Png),
            &cancel,
            &mut |_, _, _| {},
        );
        assert!(outcome.created.is_empty());
        assert_eq!(outcome.failed.len(), 2);
    }

    #[test]
    fn cancel_before_start_writes_nothing() {
        let dir = scratch("cancel");
        let (a, _) = write_png(&dir, "a.png", 16, 16);
        let (b, _) = write_png(&dir, "b.png", 16, 16);
        let cancel = CancelToken::new();
        cancel.cancel();
        let outcome = batch(
            &[a.display().to_string(), b.display().to_string()],
            convert(ImageFormat::Jpeg),
            &cancel,
            &mut |_, _, _| {},
        );
        assert!(outcome.cancelled);
        assert!(outcome.created.is_empty());
        assert!(!dir.join("a.jpg").exists());
    }

    #[test]
    fn exif_orientation_is_baked_and_stripped() {
        let dir = scratch("orientation");
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 90)
            .write_image(
                sample(64, 32).to_rgb8().as_raw(),
                64,
                32,
                ExtendedColorType::Rgb8,
            )
            .unwrap();
        let jpeg = with_exif_orientation(&jpeg, 6);
        let path = dir.join("rotated.jpg");
        std::fs::write(&path, &jpeg).unwrap();

        let created = run_one(&path.display().to_string(), convert(ImageFormat::Png)).unwrap();
        assert_eq!(dimensions(&created.output), (32, 64));

        let stripped = run_one(
            &path.display().to_string(),
            Action::ImageOptimize(ImageOptimize::StripMetadata),
        )
        .unwrap();
        assert_eq!(
            stripped.format,
            Some(TargetFormat::Image(ImageFormat::Jpeg))
        );
        assert_eq!(dimensions(&stripped.output), (32, 64));
        let out = std::fs::read(&stripped.output).unwrap();
        assert!(!out.windows(2).any(|window| window == [0xFF, 0xE1]));
    }

    /// Inserts a minimal APP1/Exif segment carrying only the orientation tag.
    fn with_exif_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        let sos = jpeg
            .windows(2)
            .position(|window| window == [0xFF, 0xDA])
            .expect("SOS marker");
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"Exif\0\0");
        tiff.extend_from_slice(b"II*\0");
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&0u16.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        let mut out = Vec::with_capacity(jpeg.len() + tiff.len() + 4);
        out.extend_from_slice(&jpeg[..2]);
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((tiff.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&tiff);
        out.extend_from_slice(&jpeg[2..sos]);
        out.extend_from_slice(&jpeg[sos..]);
        out
    }

    #[test]
    fn batch_reports_progress_per_file() {
        let dir = scratch("progress");
        let (a, _) = write_png(&dir, "a.png", 16, 16);
        let (b, _) = write_png(&dir, "b.png", 16, 16);
        let mut seen = Vec::new();
        let cancel = CancelToken::new();
        let outcome = batch(
            &[a.display().to_string(), b.display().to_string()],
            convert(ImageFormat::Jpeg),
            &cancel,
            &mut |done, total, name| seen.push((done, total, name.to_owned())),
        );
        assert_eq!(outcome.created.len(), 2);
        assert_eq!(seen.first(), Some(&(0, 2, "a.png".to_string())));
        assert_eq!(seen.last(), Some(&(2, 2, String::new())));
    }

    #[test]
    fn run_job_rejects_actions_native_cannot_run() {
        let dir = scratch("unsupported");
        let (input, _) = write_png(&dir, "a.png", 8, 8);
        let job = Job {
            input,
            output: dir.join("a.avif"),
            action: convert(ImageFormat::Avif),
            language: None,
        };
        let cancel = CancelToken::new();
        let result = run_job(&job, &mut |_| {}, &cancel);
        assert!(matches!(result, Err(EngineError::Unsupported)));
    }

    #[test]
    fn preview_names_are_display_ready() {
        assert_eq!(
            preview_name(
                r"C:\pics\photo 2024.jpg",
                convert(ImageFormat::WebP),
                TargetFormat::Image(ImageFormat::WebP)
            ),
            "photo 2024.webp"
        );
        assert_eq!(
            preview_name(
                r"C:\pics\photo.jpg",
                Action::ImageOptimize(ImageOptimize::Compress),
                TargetFormat::Image(ImageFormat::Jpeg)
            ),
            "photo_small.jpg"
        );
    }

    /// Runs the real executables from `TBI_ENGINES_DIR` (or `target/debug/engines`)
    /// through the full batch path. Ignored by default so CI never needs engines.
    #[test]
    #[ignore = "runs the installed FFmpeg/Pandoc binaries"]
    fn live_external_engines_convert_real_files() {
        use crate::engines::{AudioFormat, DocumentFormat, EngineSet, VideoExtract};

        let root = crate::engines::engines_dir();
        let set = EngineSet::load(root.clone());
        let caps = set.capabilities();
        let paths = set.paths();
        assert!(caps.installed(EngineId::Ffmpeg), "no FFmpeg in {root:?}");
        let ffmpeg = paths
            .location(EngineId::Ffmpeg)
            .expect("FFmpeg path")
            .to_path_buf();

        let dir = scratch("live");
        let generate = |args: &[&str]| {
            let status = std::process::Command::new(&ffmpeg)
                .arg("-hide_banner")
                .arg("-loglevel")
                .arg("error")
                .args(args)
                .arg("-y")
                .status()
                .expect("spawn ffmpeg");
            assert!(status.success(), "ffmpeg could not build the fixture");
        };
        generate(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=10:duration=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-shortest",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            &dir.join("clip.mp4").display().to_string(),
        ]);
        generate(&[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=660:duration=1",
            &dir.join("tone.wav").display().to_string(),
        ]);
        std::fs::write(dir.join("notes.md"), "# Live check\n\nHello **engine**.\n").unwrap();
        let (png, _) = write_png(&dir, "photo.png", 64, 64);

        let run = |action: Action, input: &Path| {
            let cancel = CancelToken::new();
            let mut engine_progress = 0usize;
            let outcome = run_batch(
                &[input.display().to_string()],
                action,
                None,
                &caps,
                &paths,
                &cancel,
                &mut |_, _, _| {},
                &mut |_, _, _| engine_progress += 1,
            );
            assert!(
                outcome.failed.is_empty(),
                "{action:?}: {:?}",
                outcome.failed
            );
            assert_eq!(outcome.created.len(), 1, "{action:?}");
            let created = &outcome.created[0];
            assert!(
                created.output.is_file(),
                "{action:?} produced no file: {}",
                created.output.display()
            );
            assert!(created.output_bytes > 0, "{action:?} wrote nothing");
            assert!(engine_progress > 0, "{action:?} reported no progress");
            created.output.clone()
        };

        let tiff = run(Action::ImageConvert(ImageFormat::Tiff), &png);
        let bytes = std::fs::read(&tiff).unwrap();
        assert!(
            bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*"),
            "not a TIFF: {bytes:?}"
        );

        // Encoding and a pure remux (H.264/AAC into MKV) both go through FFmpeg.
        let webm = run(
            Action::VideoConvert(VideoFormat::WebM),
            &dir.join("clip.mp4"),
        );
        assert!(webm.is_file());
        let mkv = run(
            Action::VideoConvert(VideoFormat::Mkv),
            &dir.join("clip.mp4"),
        );
        assert!(mkv.is_file());

        let frame = run(
            Action::VideoExtract(VideoExtract::Frame),
            &dir.join("clip.mp4"),
        );
        let bytes = std::fs::read(&frame).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");

        let mp3 = run(
            Action::VideoExtract(VideoExtract::Audio(AudioFormat::Mp3)),
            &dir.join("clip.mp4"),
        );
        assert_eq!(mp3.extension().unwrap(), "mp3");

        let flac = run(
            Action::AudioConvert(AudioFormat::Flac),
            &dir.join("tone.wav"),
        );
        assert_eq!(&std::fs::read(&flac).unwrap()[..4], b"fLaC");

        let html = run(
            Action::DocumentConvert(DocumentFormat::Html),
            &dir.join("notes.md"),
        );
        let text = std::fs::read_to_string(&html).unwrap();
        assert!(text.contains("<strong>engine</strong>"), "{text}");
        let plain = run(
            Action::DocumentExtract(DocumentExtract::PlainText),
            &dir.join("notes.md"),
        );
        assert!(std::fs::read_to_string(&plain)
            .unwrap()
            .contains("Hello engine."));

        // No tessdata on this machine means OCR options stay hidden, not broken.
        let tesseract = caps.get(EngineId::Tesseract);
        println!(
            "tesseract installed={} languages={:?}",
            tesseract.installed, tesseract.ocr_languages
        );
    }

    /// §6 test 9: equal-weight progress across the steps (the marks read
    /// 1/3, 2/3, 1.0 and the delivery reports the final 1.0), and a cancel
    /// raised inside step 1 stops the run before `finish` while the workspace
    /// — the whole temp dir — is dropped by RAII on both paths.
    #[test]
    fn pipeline_progress_sums_to_one_and_cancel_drops_the_temp_dir() {
        use crate::engines::output::Workspace;

        // Success: three steps, delivery runs, temp dir gone afterwards.
        let workspace = Workspace::new().unwrap();
        let dir = workspace.dir().to_path_buf();
        let cancel = CancelToken::new();
        let mut marks: Vec<Option<f32>> = Vec::new();
        let steps = vec![
            Step::new("one", |_, _| Ok(())),
            Step::new("two", |_, _| Ok(())),
            Step::new("three", |ws, _| {
                std::fs::write(ws.join("output.bin"), b"done")
                    .map_err(|error| EngineError::Failed(error.to_string()))
            }),
        ];
        let delivered = run_pipeline(
            workspace,
            steps,
            &mut |progress| marks.push(progress.fraction),
            &cancel,
            |ws| {
                assert!(ws.join("output.bin").is_file());
                Ok(7usize)
            },
        )
        .unwrap();
        assert_eq!(delivered, 7);
        let fractions: Vec<f32> = marks.iter().flatten().copied().collect();
        assert_eq!(
            fractions.len(),
            4,
            "three step marks + the done mark: {marks:?}"
        );
        for (index, (got, want)) in fractions
            .iter()
            .zip([1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0])
            .enumerate()
        {
            assert!(
                (got - want).abs() < 1e-6,
                "progress mark {index} is {got}, want {want}"
            );
        }
        assert!(!dir.exists(), "the workspace must be gone after success");

        // Cancel raised while step 1 runs: step 2 and finish never happen.
        let workspace = Workspace::new().unwrap();
        let dir = workspace.dir().to_path_buf();
        let cancel = CancelToken::new();
        let mut delivered = false;
        let steps = vec![
            Step::new("one", {
                let cancel = cancel.clone();
                move |_, _| {
                    cancel.cancel();
                    Ok(())
                }
            }),
            Step::new("two", |_, _| panic!("step two must not run after cancel")),
        ];
        let error = run_pipeline(workspace, steps, &mut |_| {}, &cancel, |_| {
            delivered = true;
            Ok(())
        })
        .unwrap_err();
        assert!(matches!(error, EngineError::Cancelled));
        assert!(!delivered, "finish must never run after cancel");
        assert!(!dir.exists(), "cancel must leave no temp dir behind");
    }
}
