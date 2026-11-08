//! Global media session subscription (GSMTC). All WinRT/WIC work stays on a
//! worker thread; the UI only receives plain snapshots.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus, MediaPropertiesChangedEventArgs,
    PlaybackInfoChangedEventArgs, TimelinePropertiesChangedEventArgs,
};
use windows::Media::MediaPlaybackAutoRepeatMode;
use windows::Storage::Streams::DataReader;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory2, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::design::{self, Color};
use crate::log;
use crate::view::{AlbumArt, MediaControls, RepeatMode};

#[derive(Debug, Clone)]
pub struct MediaSnapshot {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub source_app: String,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    /// True when the session exposes a real start/end timeline.
    pub live: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub controls: MediaControls,
    pub art: Option<Arc<AlbumArt>>,
    pub accent: Color,
    pub fetched_at: Instant,
}

impl MediaSnapshot {
    pub fn effective_position(&self, now: Instant) -> f64 {
        if !self.playing {
            return self.position;
        }
        let elapsed = now.saturating_duration_since(self.fetched_at).as_secs_f64();
        (self.position + elapsed).min(self.duration.max(self.position))
    }

    pub fn fraction(&self, now: Instant) -> f32 {
        if self.duration <= 0.0 {
            return 0.0;
        }
        (self.effective_position(now) / self.duration).clamp(0.0, 1.0) as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaCommand {
    TogglePlayPause,
    Previous,
    Next,
    ToggleShuffle,
    CycleRepeat,
    Seek(f64),
    Refresh,
    /// The current GSMTC session object itself changed; re-hook its handlers.
    SessionChanged,
}

/// Start the event-driven GSMTC worker. Snapshots arrive on `sender`.
/// Posted to the island window whenever a media snapshot (or its absence)
/// is ready. The UI never polls the media worker.
pub const WM_APP_MEDIA: u32 = WM_APP + 8;

/// Start the event-driven GSMTC worker. Snapshots arrive on `sender` and the
/// worker posts `WM_APP_MEDIA` to `hwnd` so the UI redraws without a timer.
pub fn subscribe(
    sender: Sender<Option<MediaSnapshot>>,
    command_rx: Receiver<MediaCommand>,
    command_tx: Sender<MediaCommand>,
    hwnd: HWND,
) -> windows::core::Result<()> {
    let notify = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("taskbar-island-media".into())
        .spawn(move || {
            // SAFETY: COM initialization is per-thread and balanced by the thread exit.
            if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                log::log("media.worker: COM init failed");
                return;
            }
            let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .and_then(|op| op.join())
            else {
                log::log("media.worker: manager request failed");
                return;
            };
            let event_tx = command_tx.clone();
            let session_handler: TypedEventHandler<
                GlobalSystemMediaTransportControlsSessionManager,
                CurrentSessionChangedEventArgs,
            > = TypedEventHandler::new(move |_manager, _| {
                let _ = event_tx.send(MediaCommand::SessionChanged);
                Ok(())
            });
            let _ = manager.CurrentSessionChanged(&session_handler);

            let mut handlers: Option<SessionHandlers> = None;
            let mut art_cache = ArtCache::default();
            deliver(
                notify,
                &sender,
                publish(&manager, &command_tx, &mut handlers, &mut art_cache),
            );

            while let Ok(command) = command_rx.recv() {
                if command == MediaCommand::SessionChanged {
                    // The old session's delegates belong to a dead session.
                    handlers = None;
                } else {
                    apply_command(&manager, command);
                }
                deliver(
                    notify,
                    &sender,
                    publish(&manager, &command_tx, &mut handlers, &mut art_cache),
                );
            }
        })
        .map(|_| ())
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))
}

/// Publish one snapshot (or `None` when there is no live session) and wake
/// the UI thread. Runs on the media worker only.
fn deliver(notify: isize, sender: &Sender<Option<MediaSnapshot>>, snapshot: Option<MediaSnapshot>) {
    if sender.send(snapshot).is_ok() && notify != 0 {
        // SAFETY: the HWND lives for the process lifetime; posting a message is
        // safe even if the window is in the middle of being destroyed.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(notify as *mut _)),
                WM_APP_MEDIA,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }
}

struct SessionHandlers {
    playback:
        TypedEventHandler<GlobalSystemMediaTransportControlsSession, PlaybackInfoChangedEventArgs>,
    media: TypedEventHandler<
        GlobalSystemMediaTransportControlsSession,
        MediaPropertiesChangedEventArgs,
    >,
    timeline: TypedEventHandler<
        GlobalSystemMediaTransportControlsSession,
        TimelinePropertiesChangedEventArgs,
    >,
}

#[derive(Default)]
struct ArtCache {
    key: String,
    art: Option<Arc<AlbumArt>>,
    accent: Option<Color>,
}

fn apply_command(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
    command: MediaCommand,
) {
    let Ok(session) = manager.GetCurrentSession() else {
        return;
    };
    match command {
        MediaCommand::TogglePlayPause => {
            let _ = session.TryTogglePlayPauseAsync().and_then(|op| op.join());
        }
        MediaCommand::Previous => {
            let _ = session.TrySkipPreviousAsync().and_then(|op| op.join());
        }
        MediaCommand::Next => {
            let _ = session.TrySkipNextAsync().and_then(|op| op.join());
        }
        MediaCommand::ToggleShuffle => {
            let active = session
                .GetPlaybackInfo()
                .ok()
                .and_then(|info| info.IsShuffleActive().ok())
                .and_then(|r| r.Value().ok())
                .unwrap_or(false);
            let _ = session
                .TryChangeShuffleActiveAsync(!active)
                .and_then(|op| op.join());
        }
        MediaCommand::CycleRepeat => {
            let current = session
                .GetPlaybackInfo()
                .ok()
                .and_then(|info| info.AutoRepeatMode().ok())
                .and_then(|r| r.Value().ok())
                .unwrap_or(MediaPlaybackAutoRepeatMode::None);
            let next = if current == MediaPlaybackAutoRepeatMode::None {
                MediaPlaybackAutoRepeatMode::List
            } else if current == MediaPlaybackAutoRepeatMode::List {
                MediaPlaybackAutoRepeatMode::Track
            } else {
                MediaPlaybackAutoRepeatMode::None
            };
            let _ = session
                .TryChangeAutoRepeatModeAsync(next)
                .and_then(|op| op.join());
        }
        MediaCommand::Seek(seconds) => {
            let ticks = (seconds.max(0.0) * 10_000_000.0) as i64;
            let _ = session
                .TryChangePlaybackPositionAsync(ticks)
                .and_then(|op| op.join());
        }
        MediaCommand::Refresh | MediaCommand::SessionChanged => {}
    }
}

fn publish(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
    command_tx: &Sender<MediaCommand>,
    handlers: &mut Option<SessionHandlers>,
    art_cache: &mut ArtCache,
) -> Option<MediaSnapshot> {
    let Ok(session) = manager.GetCurrentSession() else {
        *handlers = None;
        return None;
    };
    if handlers.is_none() {
        *handlers = Some(hook_session(&session, command_tx));
    }

    let Ok(playback) = session.GetPlaybackInfo() else {
        return None;
    };
    let playing = playback
        .PlaybackStatus()
        .map(|s| s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
        .unwrap_or(false);
    let repeat = playback
        .AutoRepeatMode()
        .ok()
        .and_then(|r| r.Value().ok())
        .map(map_repeat)
        .unwrap_or(RepeatMode::None);
    let shuffle = playback
        .IsShuffleActive()
        .ok()
        .and_then(|r| r.Value().ok())
        .unwrap_or(false);
    let controls = playback
        .Controls()
        .ok()
        .map(|c| MediaControls {
            toggle: c.IsPlayPauseToggleEnabled().unwrap_or(false),
            previous: c.IsPreviousEnabled().unwrap_or(false),
            next: c.IsNextEnabled().unwrap_or(false),
            shuffle: c.IsShuffleEnabled().unwrap_or(false),
            repeat: c.IsRepeatEnabled().unwrap_or(false),
            seek: c.IsPlaybackPositionEnabled().unwrap_or(false),
        })
        .unwrap_or_default();

    let mut live = false;
    let mut position = 0.0;
    let mut duration = 0.0;
    if let Ok(timeline) = session.GetTimelineProperties() {
        let start = timeline.StartTime().map(|t| t.Duration).unwrap_or(0);
        let end = timeline.EndTime().map(|t| t.Duration).unwrap_or(0);
        let pos = timeline.Position().map(|t| t.Duration).unwrap_or(0);
        let total_ticks = (end - start).max(0);
        duration = total_ticks as f64 / 10_000_000.0;
        position = ((pos - start).max(0) as f64 / 10_000_000.0).min(duration.max(0.0));
        live = total_ticks == 0;
    }

    let Ok(properties) = session
        .TryGetMediaPropertiesAsync()
        .and_then(|op| op.join())
    else {
        return None;
    };
    let title = properties
        .Title()
        .map(|s| s.to_string())
        .unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    let artist = properties
        .Artist()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let album = properties
        .AlbumTitle()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let source_aumid = session
        .SourceAppUserModelId()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let source_app = friendly_source(&source_aumid);

    let art_key = format!("{title}\u{1f}{artist}\u{1f}{album}");
    if art_cache.key != art_key {
        art_cache.key = art_key;
        let (art, accent) = properties
            .Thumbnail()
            .ok()
            .and_then(|thumb| decode_thumbnail(&thumb))
            .map(|(art, accent)| (Some(Arc::new(art)), accent))
            .unwrap_or((None, design::MUSIC_DEFAULT));
        art_cache.art = art;
        art_cache.accent = Some(accent);
    }

    let snapshot = MediaSnapshot {
        title,
        artist,
        album,
        source_app,
        playing,
        position,
        duration,
        live,
        shuffle,
        repeat,
        controls,
        art: art_cache.art.clone(),
        accent: art_cache.accent.unwrap_or(design::MUSIC_DEFAULT),
        fetched_at: Instant::now(),
    };
    Some(snapshot)
}

fn hook_session(
    session: &GlobalSystemMediaTransportControlsSession,
    command_tx: &Sender<MediaCommand>,
) -> SessionHandlers {
    let playback_tx = command_tx.clone();
    let playback = TypedEventHandler::new(move |_session, _| {
        let _ = playback_tx.send(MediaCommand::Refresh);
        Ok(())
    });
    let _ = session.PlaybackInfoChanged(&playback);

    let media_tx = command_tx.clone();
    let media = TypedEventHandler::new(move |_session, _| {
        let _ = media_tx.send(MediaCommand::Refresh);
        Ok(())
    });
    let _ = session.MediaPropertiesChanged(&media);

    let timeline_tx = command_tx.clone();
    let timeline = TypedEventHandler::new(move |_session, _| {
        let _ = timeline_tx.send(MediaCommand::Refresh);
        Ok(())
    });
    let _ = session.TimelinePropertiesChanged(&timeline);

    SessionHandlers {
        playback,
        media,
        timeline,
    }
}

fn map_repeat(mode: MediaPlaybackAutoRepeatMode) -> RepeatMode {
    if mode == MediaPlaybackAutoRepeatMode::Track {
        RepeatMode::Track
    } else if mode == MediaPlaybackAutoRepeatMode::List {
        RepeatMode::List
    } else {
        RepeatMode::None
    }
}

fn friendly_source(aumid: &str) -> String {
    let low = aumid.to_ascii_lowercase();
    let known = [
        ("spotify", "Spotify"),
        ("zunemusic", "Media Player"),
        ("media player", "Media Player"),
        ("youtube music", "YouTube Music"),
        ("music.youtube", "YouTube Music"),
        ("applemusic", "Apple Music"),
        ("itunes", "Apple Music"),
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("firefox", "Firefox"),
        ("vlc", "VLC"),
        ("foobar", "foobar2000"),
        ("tidal", "TIDAL"),
        ("soundcloud", "SoundCloud"),
    ];
    for (needle, name) in known {
        if low.contains(needle) {
            return name.into();
        }
    }
    let tail = aumid
        .rsplit('!')
        .next()
        .unwrap_or(aumid)
        .rsplit('\\')
        .next()
        .unwrap_or(aumid);
    let tail = tail.strip_suffix(".exe").unwrap_or(tail);
    if tail.is_empty() {
        "Unknown app".into()
    } else {
        tail.to_string()
    }
}

fn decode_thumbnail(
    reference: &windows::Storage::Streams::IRandomAccessStreamReference,
) -> Option<(AlbumArt, Color)> {
    // SAFETY: all WinRT/WIC calls are synchronous on the COM worker; buffers
    // are sized from the API's own length values.
    unsafe {
        let stream = reference.OpenReadAsync().ok()?.join().ok()?;
        let mut length = stream.Size().ok()? as u32;
        length = length.min(8 * 1024 * 1024);
        if length == 0 {
            return None;
        }
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        let loaded = reader.LoadAsync(length).ok()?.join().ok()? as u32;
        let mut bytes = vec![0u8; loaded as usize];
        reader.ReadBytes(&mut bytes).ok()?;

        let istream = SHCreateMemStream(Some(&bytes))?;
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory2, None, CLSCTX_INPROC_SERVER).ok()?;
        let decoder = factory
            .CreateDecoderFromStream(&istream, std::ptr::null(), WICDecodeMetadataCacheOnLoad)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let mut w = 0u32;
        let mut h = 0u32;
        converter.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        let scale = 192.0 / w.max(h) as f32;
        let tw = ((w as f32 * scale).round() as u32).max(1);
        let th = ((h as f32 * scale).round() as u32).max(1);
        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&converter, tw, th, WICBitmapInterpolationModeFant)
            .ok()?;
        let stride = tw * 4;
        let mut bgra = vec![0u8; (stride * th) as usize];
        scaler
            .CopyPixels(std::ptr::null(), stride, &mut bgra)
            .ok()?;
        let accent = dominant_color(&bgra, tw, th).unwrap_or(design::MUSIC_DEFAULT);
        let art = AlbumArt {
            id: fnv1a(&bytes),
            width: tw,
            height: th,
            bgra: Arc::new(bgra),
        };
        Some((art, accent))
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for b in bytes.iter().take(4096) {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Pick the most vibrant quantized hue, then clamp it into the readable range.
pub fn dominant_color(bgra: &[u8], width: u32, height: u32) -> Option<Color> {
    if width == 0 || height == 0 {
        return None;
    }
    let mut bins = vec![(0.0f32, 0.0f32, 0.0f32, 0.0f32); 36]; // weight, r, g, b
    let step = (width as usize * height as usize / 4096).max(1);
    let mut i = 0usize;
    let px = width as usize * height as usize;
    while i < px {
        let p = i * 4;
        if p + 3 >= bgra.len() {
            break;
        }
        let b = bgra[p] as f32 / 255.0;
        let g = bgra[p + 1] as f32 / 255.0;
        let r = bgra[p + 2] as f32 / 255.0;
        let a = bgra[p + 3] as f32 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        if a > 0.5 && sat > 0.18 && max > 0.12 {
            let (h, _, _) = rgb_to_hsl(r, g, b);
            let bin = ((h / 360.0 * 36.0) as usize).min(35);
            let weight = sat * max;
            bins[bin].0 += weight;
            bins[bin].1 += r * weight;
            bins[bin].2 += g * weight;
            bins[bin].3 += b * weight;
        }
        i += step;
    }
    let best = bins
        .iter()
        .enumerate()
        .max_by(|a, b| a.1 .0.total_cmp(&b.1 .0))?;
    if best.1 .0 <= 0.0 {
        return None;
    }
    let (w, r, g, b) = *best.1;
    let (mut h, mut s, mut l) = rgb_to_hsl(r / w, g / w, b / w);
    s = s.max(0.45);
    l = l.clamp(0.55, 0.72);
    let (r, g, b) = hsl_to_rgb(h, s, l);
    h = h.rem_euclid(360.0);
    let _ = h;
    Some(Color { r, g, b, a: 1.0 })
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d.abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6);
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (r1 + m, g1 + m, b1 + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_are_friendly() {
        assert_eq!(friendly_source("Spotify.exe"), "Spotify");
        assert_eq!(
            friendly_source("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "Media Player"
        );
        assert_eq!(
            friendly_source("MSEdge_8wekyb3d8bbwe!Microsoft.MSEdge"),
            "Edge"
        );
    }

    #[test]
    fn dominant_color_finds_red() {
        let mut bgra = Vec::new();
        for _ in 0..64 {
            bgra.extend_from_slice(&[30, 30, 220, 255]);
        }
        let color = dominant_color(&bgra, 8, 8).unwrap();
        assert!(color.r > 0.8 && color.g < 0.4 && color.b < 0.4);
    }

    #[test]
    fn repeat_mapping() {
        assert_eq!(
            map_repeat(MediaPlaybackAutoRepeatMode::None),
            RepeatMode::None
        );
        assert_eq!(
            map_repeat(MediaPlaybackAutoRepeatMode::List),
            RepeatMode::List
        );
        assert_eq!(
            map_repeat(MediaPlaybackAutoRepeatMode::Track),
            RepeatMode::Track
        );
    }
}
