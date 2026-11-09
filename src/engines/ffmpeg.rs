//! ffmpeg.rs — FFmpeg and FFprobe jobs.
//!
//! Every argument is built as an argv item. The dropped file only ever appears
//! after `-i`; its name is never concatenated into a command line. The engine
//! writes into a private workspace, then `output::deliver` moves the finished
//! file beside the original without replacing anything.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::convert::detect::{self, SourceFormat};

use super::exec::{self, Spec};
use super::output::{self, TargetFormat, Workspace};
use super::{
    argv_path, Action, AudioFormat, CancelToken, EngineError, ImageFormat, ImageOptimize, Job,
    JobOutput, Progress, ProgressSink, Stage, VideoEdit, VideoExtract, VideoFormat, VideoOptimize,
    IMAGE_TIMEOUT, MEDIA_TIMEOUT,
};

/// What FFprobe could tell us about the input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaInfo {
    pub duration: Option<Duration>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

/// The complete argv for one FFmpeg run, kept separate from process handling
/// so the plan can be tested without spawning anything.
#[derive(Debug, Clone)]
pub struct Plan {
    pub args: Vec<OsString>,
    pub target: TargetFormat,
    pub duration: Option<Duration>,
    pub timeout: Duration,
    /// A stream copy was planned; if it fails the runner re-encodes.
    pub remux: bool,
}

pub fn run(
    exe: &Path,
    job: &Job,
    progress: ProgressSink<'_>,
    cancel: &CancelToken,
) -> Result<JobOutput, EngineError> {
    let input_text = job.input.to_string_lossy().into_owned();
    let source = detect::quick_format(&input_text).ok_or(EngineError::Unsupported)?;
    let target = output::target_for(job.action, source).ok_or(EngineError::Unsupported)?;

    progress(Progress {
        stage: Stage::Reading,
        fraction: None,
    });

    let media = match target {
        TargetFormat::Image(_) => MediaInfo::default(),
        _ => probe_media(exe, &job.input, cancel).unwrap_or_default(),
    };
    let workspace = Workspace::new()?;
    let output_path = workspace.join(&format!("out.{}", target.extension()));
    let first_plan = plan(job.action, source, &job.input, &output_path, &media)
        .ok_or(EngineError::Unsupported)?;

    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    progress(Progress {
        stage: Stage::Converting,
        fraction: first_plan.remux.then_some(0.0),
    });

    let result = execute(exe, &first_plan, workspace.dir(), cancel, progress, &media);
    let captured = match result {
        Err(EngineError::Failed(_)) if first_plan.remux => {
            // A stream copy is only a shortcut; the container may reject a
            // stream the probe considered compatible. Fall back to a full
            // encode once, with a fresh output name.
            let encoded_path =
                workspace.join(&format!("encoded.{}", first_plan.target.extension()));
            let encoded = plan(job.action, source, &job.input, &encoded_path, &media)
                .map(|mut plan| {
                    plan.remux = false;
                    strip_copy(&mut plan.args);
                    plan
                })
                .ok_or(EngineError::Unsupported)?;
            execute(exe, &encoded, workspace.dir(), cancel, progress, &media)?
        }
        other => other?,
    };
    let _ = captured;

    progress(Progress {
        stage: Stage::Saving,
        fraction: None,
    });
    let engine_output = if first_plan.remux {
        output_path.clone()
    } else {
        workspace.join(&format!("out.{}", first_plan.target.extension()))
    };
    let engine_output = if engine_output.is_file() {
        engine_output
    } else {
        workspace.join(&format!("encoded.{}", first_plan.target.extension()))
    };
    output::verify(&engine_output, target).map_err(|reason| {
        let _ = reason;
        EngineError::Failed("FFmpeg produced an unreadable file.".into())
    })?;
    let (definite, _fallback) = output::deliver_in(
        job.output.parent(),
        &engine_output,
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
        output: definite,
        format: Some(target),
        text: None,
    })
}

fn execute(
    exe: &Path,
    plan: &Plan,
    dir: &Path,
    cancel: &CancelToken,
    progress: ProgressSink<'_>,
    media: &MediaInfo,
) -> Result<exec::Captured, EngineError> {
    let total = plan.duration.or(media.duration);
    let mut last_bucket = 0u32;
    exec::run(
        &Spec {
            exe,
            args: &plan.args,
            dir,
            timeout: plan.timeout,
        },
        cancel,
        |line| {
            let Some(total) = total else {
                return;
            };
            let Some(seconds) = progress_seconds(line) else {
                return;
            };
            let fraction = (seconds / total.as_secs_f64()).clamp(0.0, 1.0) as f32;
            let bucket = (fraction * 100.0) as u32;
            if bucket > last_bucket {
                last_bucket = bucket;
                progress(Progress {
                    stage: Stage::Converting,
                    fraction: Some(fraction),
                });
            }
        },
    )
}

/// `-progress pipe:1` emits `out_time_us` on current builds and `out_time_ms`
/// (also microseconds) on older ones.
fn progress_seconds(line: &str) -> Option<f64> {
    let value = line
        .strip_prefix("out_time_us=")
        .or_else(|| line.strip_prefix("out_time_ms="))?;
    let micros: f64 = value.trim().parse().ok()?;
    (micros >= 0.0).then_some(micros / 1_000_000.0)
}

fn probe_media(ffmpeg: &Path, input: &Path, cancel: &CancelToken) -> Option<MediaInfo> {
    let ffprobe = ffprobe_beside(ffmpeg)?;
    let args = vec![
        OsString::from("-v"),
        OsString::from("error"),
        OsString::from("-show_entries"),
        OsString::from("format=duration:stream=codec_type,codec_name"),
        OsString::from("-of"),
        OsString::from("default=noprint_wrappers=1"),
        argv_path(input),
    ];
    let captured = exec::run(
        &Spec {
            exe: &ffprobe,
            args: &args,
            dir: &std::env::temp_dir(),
            timeout: Duration::from_secs(20),
        },
        cancel,
        |_| {},
    )
    .ok()?;
    Some(parse_media_info(&captured.stdout))
}

fn ffprobe_beside(ffmpeg: &Path) -> Option<PathBuf> {
    let candidate = ffmpeg.with_file_name("ffprobe.exe");
    candidate.is_file().then_some(candidate)
}

pub fn parse_media_info(text: &str) -> MediaInfo {
    let mut info = MediaInfo::default();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("codec_name=") {
            pending = Some(value.to_ascii_lowercase());
        } else if let Some(kind) = line.strip_prefix("codec_type=") {
            let codec = pending.take();
            match kind.trim() {
                "video" if info.video_codec.is_none() => info.video_codec = codec,
                "audio" if info.audio_codec.is_none() => info.audio_codec = codec,
                _ => {}
            }
        } else if let Some(value) = line.strip_prefix("duration=") {
            if let Ok(seconds) = value.trim().parse::<f64>() {
                if seconds.is_finite() && seconds > 0.0 {
                    info.duration = Some(Duration::from_secs_f64(seconds));
                }
            }
        }
    }
    info
}

// -------------------------------------------------------------------- plan

pub fn plan(
    action: Action,
    source: SourceFormat,
    input: &Path,
    output: &Path,
    media: &MediaInfo,
) -> Option<Plan> {
    let target = output::target_for(action, source)?;
    if matches!(
        target,
        TargetFormat::Image(ImageFormat::Ico | ImageFormat::Pdf) | TargetFormat::Document(_)
    ) {
        return None;
    }
    let mut args: Vec<OsString> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-y".into(),
        "-i".into(),
        argv_path(input),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
    ];

    let (timeout, remux) = match target {
        TargetFormat::Image(_) => (IMAGE_TIMEOUT, false),
        TargetFormat::Video(VideoFormat::Gif) => (MEDIA_TIMEOUT, false),
        TargetFormat::Video(_) | TargetFormat::Audio(_) => (
            MEDIA_TIMEOUT,
            (matches!(action, Action::VideoConvert(_)) && canvas_remux(source, target, media))
                // "Remux" is the stream-copy job itself: always copy.
                || matches!(action, Action::VideoOptimize(VideoOptimize::Remux)),
        ),
        // Office formats are LibreOffice's job, never FFmpeg's.
        TargetFormat::Document(_) | TargetFormat::Sheet(_) | TargetFormat::Present(_) => {
            return None
        }
    };

    if remux {
        args.push("-c".into());
        args.push("copy".into());
        if matches!(
            target,
            TargetFormat::Video(VideoFormat::Mp4 | VideoFormat::Mov)
        ) {
            args.push("-movflags".into());
            args.push("+faststart".into());
        }
    } else {
        match target {
            TargetFormat::Image(format) => {
                image_args(&mut args, action, source, format, media.duration)
            }
            TargetFormat::Video(format) => video_args(&mut args, action, format, media),
            TargetFormat::Audio(format) => audio_args(&mut args, action, format),
            TargetFormat::Document(_) | TargetFormat::Sheet(_) | TargetFormat::Present(_) => {
                return None
            }
        }
    }

    args.push(argv_path(output));
    Some(Plan {
        args,
        target,
        duration: media.duration,
        timeout,
        remux,
    })
}

/// A remux is only safe when the target container accepts both streams
/// unchanged. When in doubt the job re-encodes.
fn canvas_remux(source: SourceFormat, target: TargetFormat, media: &MediaInfo) -> bool {
    let TargetFormat::Video(format) = target else {
        return false;
    };
    if source.video_format() == Some(format) {
        return false;
    }
    let video = media.video_codec.as_deref();
    let audio = media.audio_codec.as_deref();
    match format {
        VideoFormat::Mkv => video.is_some(),
        VideoFormat::Mp4 | VideoFormat::Mov => {
            matches!(video, Some("h264" | "hevc" | "av1" | "mpeg4" | "vp9"))
                && audio.is_none_or(|codec| {
                    matches!(codec, "aac" | "mp3" | "ac3" | "eac3" | "opus" | "flac")
                })
        }
        VideoFormat::WebM => {
            matches!(video, Some("vp8" | "vp9" | "av1"))
                && audio.is_none_or(|codec| matches!(codec, "opus" | "vorbis"))
        }
        VideoFormat::Avi | VideoFormat::Wmv | VideoFormat::Gif => false,
    }
}

fn strip_copy(args: &mut Vec<OsString>) {
    if let Some(index) = args.iter().position(|arg| arg == "-c") {
        args.drain(index..(index + 2).min(args.len()));
    }
    if let Some(index) = args
        .iter()
        .position(|arg| arg == "-movflags")
        .filter(|index| args.get(index + 1).is_some_and(|arg| arg == "+faststart"))
    {
        args.drain(index..(index + 2).min(args.len()));
    }
}

fn push_filter(args: &mut Vec<OsString>, filters: &[String]) {
    let joined = filters
        .iter()
        .filter(|filter| !filter.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(",");
    if !joined.is_empty() {
        args.push("-vf".into());
        args.push(joined.into());
    }
}

fn push_audio_filter(args: &mut Vec<OsString>, filters: &[String]) {
    let joined = filters
        .iter()
        .filter(|filter| !filter.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(",");
    if !joined.is_empty() {
        args.push("-af".into());
        args.push(joined.into());
    }
}

fn image_args(
    args: &mut Vec<OsString>,
    action: Action,
    source: SourceFormat,
    format: ImageFormat,
    duration: Option<std::time::Duration>,
) {
    let mut filters = Vec::new();
    let mut strip_metadata = false;
    if matches!(action, Action::VideoExtract(VideoExtract::Frame)) {
        // The representative frame: half the clip, like a thumbnail scrubber.
        if let Some(duration) = duration {
            args.push("-ss".into());
            args.push(format!("{:.3}", (duration / 2).as_secs_f64()).into());
        }
    }
    match action {
        Action::ImageOptimize(optimize) => match optimize {
            ImageOptimize::ResizePercent(percent) => filters.push(format!(
                "scale=iw*{percent}/100:ih*{percent}/100:flags=lanczos"
            )),
            ImageOptimize::Fit(px) | ImageOptimize::Thumbnail(px) => filters.push(fit_filter(px)),
            ImageOptimize::WebReady => {
                filters.push(fit_filter(1920));
                strip_metadata = true;
            }
            ImageOptimize::Compress => {}
            ImageOptimize::StripMetadata => strip_metadata = true,
        },
        Action::ImageEdit(edit) => filters.extend(edit_filters(edit)),
        _ => {}
    }
    push_filter(args, &filters);
    if strip_metadata {
        args.push("-map_metadata".into());
        args.push("-1".into());
    }

    match format {
        ImageFormat::Jpeg => {
            args.push("-frames:v".into());
            args.push("1".into());
            args.push("-c:v".into());
            args.push("mjpeg".into());
            args.push("-q:v".into());
            args.push("2".into());
        }
        ImageFormat::WebP => {
            args.push("-frames:v".into());
            args.push("1".into());
            args.push("-c:v".into());
            args.push("libwebp".into());
            args.push("-quality".into());
            args.push(
                if matches!(action, Action::ImageOptimize(ImageOptimize::WebReady)) {
                    "78".into()
                } else {
                    "85".into()
                },
            );
        }
        ImageFormat::Gif => {
            args.push("-loop".into());
            args.push("0".into());
            // An animated GIF stays animated; a still image becomes one frame.
            if source != SourceFormat::Gif {
                args.push("-frames:v".into());
                args.push("1".into());
            }
        }
        ImageFormat::Avif => {
            args.push("-frames:v".into());
            args.push("1".into());
            args.push("-c:v".into());
            args.push("libaom-av1".into());
            args.push("-crf".into());
            args.push("30".into());
            args.push("-b:v".into());
            args.push("0".into());
            args.push("-still-picture".into());
            args.push("1".into());
        }
        ImageFormat::Png | ImageFormat::Bmp | ImageFormat::Tiff => {
            args.push("-frames:v".into());
            args.push("1".into());
        }
        ImageFormat::Ico | ImageFormat::Pdf => {}
    }
}

fn fit_filter(px: u32) -> String {
    format!(
        "scale='min(iw,{px})':'min(ih,{px})':force_original_aspect_ratio=decrease:flags=lanczos"
    )
}

fn edit_filters(edit: crate::engines::ImageEdit) -> Vec<String> {
    use crate::engines::ImageEdit;
    match edit {
        ImageEdit::RotateLeft => vec!["transpose=2".into()],
        ImageEdit::RotateRight => vec!["transpose=1".into()],
        ImageEdit::FlipHorizontal => vec!["hflip".into()],
        ImageEdit::FlipVertical => vec!["vflip".into()],
        ImageEdit::Grayscale => vec!["format=gray".into()],
        ImageEdit::BlackWhite => {
            vec![
                "format=gray".into(),
                "lut=y='if(gte(val,128),255,0)'".into(),
            ]
        }
        ImageEdit::CropSquare => vec!["crop='min(iw,ih)':'min(iw,ih)'".into()],
    }
}

fn video_args(args: &mut Vec<OsString>, action: Action, format: VideoFormat, media: &MediaInfo) {
    let mut filters: Vec<String> = Vec::new();
    let mut audio_filters: Vec<String> = Vec::new();
    let mut mute = false;
    let mut frame_at: Option<Duration> = None;

    match action {
        Action::VideoOptimize(optimize) => match optimize {
            VideoOptimize::Compress => {}
            VideoOptimize::Height(height) => {
                filters.push(format!("scale=-2:min(ih,{height}):flags=lanczos"));
            }
            VideoOptimize::Mute => mute = true,
            // Remux takes the `-c copy` path in `plan()` and never reaches
            // the filter/encoder branch.
            VideoOptimize::Remux => {}
        },
        Action::VideoEdit(edit) => match edit {
            VideoEdit::RotateLeft => filters.push("transpose=2".into()),
            VideoEdit::RotateRight => filters.push("transpose=1".into()),
            VideoEdit::Speed(speed) => {
                let (pts, tempo) = match speed {
                    crate::engines::Speed::Half => ("2.0*PTS", "0.5"),
                    crate::engines::Speed::OneAndHalf => ("PTS/1.5", "1.5"),
                    crate::engines::Speed::Double => ("0.5*PTS", "2.0"),
                };
                filters.push(format!("setpts={pts}"));
                audio_filters.push(format!("atempo={tempo}"));
            }
        },
        Action::VideoExtract(VideoExtract::Frame) => {
            if let Some(duration) = media.duration {
                frame_at = Some(duration / 2);
            }
        }
        Action::VideoExtract(VideoExtract::Gif) | Action::VideoConvert(VideoFormat::Gif) => {
            filters.push("fps=12".into());
            filters.push("scale=640:-2:flags=lanczos".into());
        }
        _ => {}
    }

    if let Some(at) = frame_at {
        args.push("-ss".into());
        args.push(format!("{:.3}", at.as_secs_f64()).into());
    }
    if matches!(action, Action::VideoExtract(VideoExtract::Gif)) {
        args.push("-t".into());
        args.push("5".into());
    }
    push_filter(args, &filters);

    let crf = if matches!(action, Action::VideoOptimize(VideoOptimize::Compress)) {
        "28"
    } else {
        "20"
    };
    match format {
        VideoFormat::Mp4 | VideoFormat::Mkv | VideoFormat::Mov => {
            args.push("-c:v".into());
            args.push("libx264".into());
            args.push("-preset".into());
            args.push("medium".into());
            args.push("-crf".into());
            args.push(crf.into());
            args.push("-pix_fmt".into());
            args.push("yuv420p".into());
        }
        VideoFormat::WebM => {
            args.push("-c:v".into());
            args.push("libvpx-vp9".into());
            args.push("-crf".into());
            args.push("32".into());
            args.push("-b:v".into());
            args.push("0".into());
        }
        VideoFormat::Avi => {
            args.push("-c:v".into());
            args.push("mpeg4".into());
            args.push("-q:v".into());
            args.push("4".into());
        }
        VideoFormat::Wmv => {
            args.push("-c:v".into());
            args.push("wmv2".into());
            args.push("-b:v".into());
            args.push("4M".into());
        }
        VideoFormat::Gif => {
            args.push("-loop".into());
            args.push("0".into());
        }
    }

    if matches!(format, VideoFormat::Gif | VideoFormat::Wmv) || mute {
        args.push("-an".into());
    } else if media.audio_codec.is_some() || media.video_codec.is_some() {
        push_audio_filter(args, &audio_filters);
        let (codec, bitrate) = match format {
            VideoFormat::WebM => ("libopus", "128k"),
            VideoFormat::Avi => ("libmp3lame", "192k"),
            _ => ("aac", "192k"),
        };
        args.push("-c:a".into());
        args.push(codec.into());
        args.push("-b:a".into());
        args.push(bitrate.into());
        if matches!(format, VideoFormat::Mp4 | VideoFormat::Mov) {
            args.push("-movflags".into());
            args.push("+faststart".into());
        }
    }
}

fn audio_args(args: &mut Vec<OsString>, action: Action, format: AudioFormat) {
    let mut filters: Vec<String> = Vec::new();
    let mut bitrate: Option<&str> = None;
    match action {
        Action::AudioOptimize(optimize) => match optimize {
            crate::engines::AudioOptimize::SmallerBitrate => bitrate = Some("128k"),
            crate::engines::AudioOptimize::Mono => {
                args.push("-ac".into());
                args.push("1".into());
            }
            crate::engines::AudioOptimize::Normalize => filters.push("loudnorm".into()),
            crate::engines::AudioOptimize::TrimSilence => filters.push(
                "silenceremove=start_periods=1:start_duration=0.1:start_threshold=-50dB".into(),
            ),
        },
        Action::VideoExtract(VideoExtract::Audio(_)) | Action::VideoConvert(_) => {}
        _ => {}
    }
    if action_is_extract_audio(action) {
        args.push("-vn".into());
    }
    push_audio_filter(args, &filters);
    let (codec, default_bitrate) = match format {
        AudioFormat::Mp3 => ("libmp3lame", "192k"),
        AudioFormat::Wav => ("pcm_s16le", ""),
        AudioFormat::Flac => ("flac", ""),
        AudioFormat::M4a => ("aac", "192k"),
        AudioFormat::Ogg => ("libvorbis", "160k"),
        AudioFormat::Opus => ("libopus", "128k"),
        AudioFormat::Aac => ("aac", "192k"),
        AudioFormat::Wma => ("wmav2", "192k"),
        AudioFormat::Aiff => ("pcm_s16be", ""),
    };
    args.push("-c:a".into());
    args.push(codec.into());
    if !default_bitrate.is_empty() {
        args.push("-b:a".into());
        args.push(bitrate.unwrap_or(default_bitrate).into());
    }
    if matches!(format, AudioFormat::M4a) {
        args.push("-movflags".into());
        args.push("+faststart".into());
    }
}

fn action_is_extract_audio(action: Action) -> bool {
    matches!(action, Action::VideoExtract(VideoExtract::Audio(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> PathBuf {
        PathBuf::from(r"C:\videos\clip.mkv")
    }

    fn output() -> PathBuf {
        PathBuf::from(r"C:\temp\out.mp4")
    }

    fn plan_for(action: Action, source: SourceFormat, media: MediaInfo) -> Plan {
        plan(action, source, &input(), &output(), &media).unwrap()
    }

    fn text(plan: &Plan) -> String {
        plan.args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_input_is_a_single_argv_item_after_dash_i() {
        let plan = plan_for(
            Action::VideoConvert(VideoFormat::Mp4),
            SourceFormat::Mkv,
            MediaInfo::default(),
        );
        let position = plan.args.iter().position(|arg| arg == "-i").unwrap();
        assert_eq!(
            plan.args[position + 1],
            OsString::from(r"C:\videos\clip.mkv")
        );
        assert_eq!(text(&plan).matches("-i").count(), 1);
    }

    #[test]
    fn video_conversion_encodes_or_remuxes() {
        let info = MediaInfo {
            video_codec: Some("h264".into()),
            audio_codec: Some("aac".into()),
            ..MediaInfo::default()
        };
        let remux = plan_for(
            Action::VideoConvert(VideoFormat::Mkv),
            SourceFormat::Mp4,
            info.clone(),
        );
        assert!(remux.remux);
        assert!(text(&remux).contains("-c copy"));

        let encode = plan_for(
            Action::VideoConvert(VideoFormat::WebM),
            SourceFormat::Mp4,
            info,
        );
        assert!(!encode.remux);
        assert!(text(&encode).contains("libvpx-vp9"));
    }

    #[test]
    fn convert_to_the_same_container_is_not_a_remux() {
        let info = MediaInfo {
            video_codec: Some("h264".into()),
            ..MediaInfo::default()
        };
        let plan = plan_for(
            Action::VideoConvert(VideoFormat::Mp4),
            SourceFormat::Mp4,
            info,
        );
        assert!(!plan.remux);
    }

    #[test]
    fn edit_and_optimize_plans_carry_their_filters() {
        let rotate = plan_for(
            Action::VideoEdit(VideoEdit::RotateLeft),
            SourceFormat::Mp4,
            MediaInfo::default(),
        );
        assert!(text(&rotate).contains("transpose=2"));

        let resize = plan_for(
            Action::VideoOptimize(VideoOptimize::Height(720)),
            SourceFormat::Mp4,
            MediaInfo::default(),
        );
        assert!(text(&resize).contains("scale=-2:min(ih,720)"));

        let mute = plan_for(
            Action::VideoOptimize(VideoOptimize::Mute),
            SourceFormat::Mp4,
            MediaInfo::default(),
        );
        assert!(text(&mute).contains("-an"));

        let half = plan_for(
            Action::VideoEdit(VideoEdit::Speed(crate::engines::Speed::Half)),
            SourceFormat::Mp4,
            MediaInfo {
                video_codec: Some("h264".into()),
                audio_codec: Some("aac".into()),
                ..MediaInfo::default()
            },
        );
        assert!(text(&half).contains("setpts=2.0*PTS"));
        assert!(text(&half).contains("atempo=0.5"));
    }

    #[test]
    fn frame_extraction_seeks_to_the_midpoint() {
        let info = MediaInfo {
            duration: Some(Duration::from_secs(10)),
            ..MediaInfo::default()
        };
        let plan = plan_for(
            Action::VideoExtract(VideoExtract::Frame),
            SourceFormat::Mp4,
            info,
        );
        assert!(text(&plan).contains("-ss 5.000"));
        assert_eq!(plan.target, TargetFormat::Image(ImageFormat::Png));
    }

    #[test]
    fn audio_extraction_drops_the_video_stream() {
        let plan = plan_for(
            Action::VideoExtract(VideoExtract::Audio(AudioFormat::Mp3)),
            SourceFormat::Mp4,
            MediaInfo::default(),
        );
        assert!(text(&plan).contains("-vn"));
        assert!(text(&plan).contains("libmp3lame"));
    }

    #[test]
    fn image_edits_become_filters() {
        use crate::engines::ImageEdit;
        let plan = plan_for(
            Action::ImageEdit(ImageEdit::Grayscale),
            SourceFormat::Jpeg,
            MediaInfo::default(),
        );
        assert!(text(&plan).contains("format=gray"));
        assert!(text(&plan).contains("-frames:v 1"));
        assert!(text(&plan).contains("mjpeg"));
    }

    #[test]
    fn audio_optimize_keeps_the_source_container() {
        let plan = plan_for(
            Action::AudioOptimize(crate::engines::AudioOptimize::Normalize),
            SourceFormat::Mp3,
            MediaInfo::default(),
        );
        assert!(text(&plan).contains("loudnorm"));
        assert_eq!(plan.target, TargetFormat::Audio(AudioFormat::Mp3));
    }

    #[test]
    fn ffprobe_output_is_parsed() {
        let info = parse_media_info(
            "codec_name=h264\ncodec_type=video\ncodec_name=aac\ncodec_type=audio\nduration=12.500000\n",
        );
        assert_eq!(info.video_codec.as_deref(), Some("h264"));
        assert_eq!(info.audio_codec.as_deref(), Some("aac"));
        assert_eq!(info.duration, Some(Duration::from_millis(12_500)));
    }

    #[test]
    fn progress_lines_are_microseconds() {
        assert_eq!(progress_seconds("out_time_us=2500000"), Some(2.5));
        assert_eq!(progress_seconds("out_time_ms=1000000"), Some(1.0));
        assert_eq!(progress_seconds("progress=continue"), None);
    }
}
