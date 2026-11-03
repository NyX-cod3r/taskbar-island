//! Global media-session subscription. All WinRT work lives off the UI thread.

use std::sync::mpsc::{Receiver, Sender};

use windows::Foundation::TypedEventHandler;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus, MediaPropertiesChangedEventArgs,
    PlaybackInfoChangedEventArgs,
};

use crate::state::{Activity, ActivityId, Priority};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaUpdate {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub position_secs: u32,
    pub duration_secs: u32,
}

impl MediaUpdate {
    /// The "no session" sentinel: the pill has nothing to show.
    pub fn none() -> Self {
        Self {
            title: String::new(),
            artist: String::new(),
            playing: false,
            position_secs: 0,
            duration_secs: 0,
        }
    }

}

/// Commands sent to the GSMTC worker. The UI never calls WinRT directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    TogglePlayPause,
    Previous,
    Next,
    /// Re-read metadata/timeline without changing playback.
    Refresh,
    /// Internal: the system's current media session changed.
    SessionChanged,
}

pub fn update(
    title: String,
    artist: String,
    playing: bool,
    position_secs: u32,
    duration_secs: u32,
) -> MediaUpdate {
    MediaUpdate {
        title,
        artist,
        playing,
        position_secs,
        duration_secs,
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct MediaActivity {
    pub current: Option<MediaUpdate>,
}

impl Activity for MediaActivity {
    fn id(&self) -> ActivityId {
        ActivityId::Media
    }
    fn priority(&self) -> Priority {
        Priority::Media
    }
    fn active(&self) -> bool {
        self.current.is_some()
    }
}

/// Starts the GSMTC listener without polling. The callback posts a compact
/// snapshot to the caller's channel, which can then post a UI message.
pub fn subscribe(
    sender: Sender<MediaUpdate>,
    command_rx: Receiver<MediaCommand>,
    command_tx: Sender<MediaCommand>,
) -> windows::core::Result<()> {
    std::thread::Builder::new()
        .name("taskbar-island-media".into())
        .spawn(move || {
            // WinRT needs a COM apartment on this worker before RequestAsync.
            if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                crate::log::log("media.worker: COM init failed");
                return;
            }
            let Ok(operation) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            else {
                crate::log::log("media.worker: manager request failed");
                return;
            };

            let _ = operation.when(move |manager| {
                let Ok(manager) = manager else {
                    crate::log::log("media.worker: manager request errored");
                    return;
                };
                // The completion callback can run on a WinRT thread-pool
                // thread. Give that thread a COM apartment before querying
                // the manager or its sessions.
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

                let session_event_tx = command_tx.clone();
                let session_handler: TypedEventHandler<
                    GlobalSystemMediaTransportControlsSessionManager,
                    CurrentSessionChangedEventArgs,
                > = TypedEventHandler::new(move |_manager, _| {
                    let _ = session_event_tx.send(MediaCommand::SessionChanged);
                    Ok(())
                });
                let _ = manager.CurrentSessionChanged(&session_handler);

                let mut session_handlers: Option<SessionHandlers> = None;
                refresh_current(&manager, &sender, &command_tx, &mut session_handlers, true);

                while let Ok(command) = command_rx.recv() {
                    match command {
                        MediaCommand::TogglePlayPause
                        | MediaCommand::Previous
                        | MediaCommand::Next => {
                            if let Ok(session) = manager.GetCurrentSession() {
                                match command {
                                    MediaCommand::TogglePlayPause => {
                                        let _ = session.TryTogglePlayPauseAsync();
                                    }
                                    MediaCommand::Previous => {
                                        let _ = session.TrySkipPreviousAsync();
                                    }
                                    MediaCommand::Next => {
                                        let _ = session.TrySkipNextAsync();
                                    }
                                    _ => {}
                                }
                            }
                        }
                        MediaCommand::Refresh => {
                            refresh_current(
                                &manager,
                                &sender,
                                &command_tx,
                                &mut session_handlers,
                                false,
                            );
                        }
                        MediaCommand::SessionChanged => {
                            refresh_current(
                                &manager,
                                &sender,
                                &command_tx,
                                &mut session_handlers,
                                true,
                            );
                        }
                    }
                }
            });
        })
        .map(|_| ())
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))
}

/// Event delegates for the current session. Holding them here keeps the
/// subscriptions alive for as long as the worker owns the manager.
#[allow(dead_code)]
struct SessionHandlers {
    playback: TypedEventHandler<GlobalSystemMediaTransportControlsSession, PlaybackInfoChangedEventArgs>,
    media: TypedEventHandler<GlobalSystemMediaTransportControlsSession, MediaPropertiesChangedEventArgs>,
}

/// Re-reads the current session on the COM worker. `hook` installs fresh
/// listeners when the current session itself changed.
fn refresh_current(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
    sender: &Sender<MediaUpdate>,
    command_tx: &Sender<MediaCommand>,
    session_handlers: &mut Option<SessionHandlers>,
    hook: bool,
) {
    match manager.GetCurrentSession() {
        Ok(session) => {
            publish_session(&session, sender);
            if hook || session_handlers.is_none() {
                *session_handlers = Some(hook_session(session, command_tx));
            }
        }
        Err(_) => {
            *session_handlers = None;
            let _ = sender.send(MediaUpdate::none());
        }
    }
}

fn hook_session(
    session: GlobalSystemMediaTransportControlsSession,
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

    SessionHandlers { playback, media }
}

fn publish_session(
    session: &GlobalSystemMediaTransportControlsSession,
    sender: &Sender<MediaUpdate>,
) {
    let Ok(operation) = session.TryGetMediaPropertiesAsync() else {
        return;
    };
    let state_session = session.clone();
    let sender = sender.clone();
    let _ = operation.when(move |properties| {
        let Ok(properties) = properties else { return };
        let playing = state_session.GetPlaybackInfo().is_ok_and(|info| {
            info.PlaybackStatus().is_ok_and(|status| {
                status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing
            })
        });
        let title = properties
            .Title()
            .map(|value| value.to_string())
            .unwrap_or_default();
        let artist = properties
            .Artist()
            .map(|value| value.to_string())
            .unwrap_or_default();

        // Timeline properties give the real position/duration (100 ns ticks).
        let (position_secs, duration_secs) = state_session
            .GetTimelineProperties()
            .map(|timeline| {
                let start = timeline.StartTime().map(|t| t.Duration).unwrap_or(0);
                let end = timeline.EndTime().map(|t| t.Duration).unwrap_or(0);
                let position = timeline.Position().map(|t| t.Duration).unwrap_or(0);
                let duration = (end - start).max(0) / 10_000_000;
                let position = ((position - start).max(0) / 10_000_000).min(duration);
                (position as u32, duration as u32)
            })
            .unwrap_or((0, 0));

        if !title.is_empty() {
            let _ = sender.send(update(title, artist, playing, position_secs, duration_secs));
        } else {
            let _ = sender.send(MediaUpdate::none());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_keeps_track_metadata() {
        let update = update("Track".into(), "Artist".into(), true, 30, 180);
        assert_eq!(update.title, "Track");
        assert_eq!(update.artist, "Artist");
        assert!(update.playing);
        assert_eq!(update.position_secs, 30);
        assert_eq!(update.duration_secs, 180);
    }

    #[test]
    fn sentinel_is_the_empty_title() {
        let none = MediaUpdate::none();
        assert!(none.title.is_empty() && none.artist.is_empty() && !none.playing);
    }

    #[test]
    fn paused_track_keeps_its_title() {
        let paused = update("Track".into(), "Artist".into(), false, 10, 60);
        assert!(!paused.playing);
        assert_eq!(paused.title, "Track");
    }
}
