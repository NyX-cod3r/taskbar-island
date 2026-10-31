//! Global media-session subscription. All WinRT work lives off the UI thread.

use std::sync::mpsc::{Receiver, Sender};

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus, PlaybackInfoChangedEventArgs,
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

    /// Design spec demo track: "Deewana Kar Raha Hai", "Now playing", 1:24 / -2:24
    pub fn demo() -> Self {
        Self {
            title: "Deewana Kar Raha Hai".into(),
            artist: "Now playing".into(),
            playing: true,
            position_secs: 84,   // 1:24
            duration_secs: 228,  // 3:48 (remaining: -2:24)
        }
    }
}

pub fn update(title: String, artist: String, playing: bool) -> MediaUpdate {
    MediaUpdate {
        title,
        artist,
        playing,
        position_secs: 84,
        duration_secs: 228,
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
    toggle_rx: Receiver<()>,
) -> windows::core::Result<()> {
    std::thread::Builder::new()
        .name("taskbar-island-media".into())
        .spawn(move || {
            let Ok(operation) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            else {
                return;
            };
            let _ = operation.when(move |manager| {
                let Ok(manager) = manager else { return };
                if let Ok(session) = manager.GetCurrentSession() {
                    publish_session(&session, &sender);
                    hook_playback_info(session, &sender);
                }

                let session_sender = sender.clone();
                let handler: TypedEventHandler<
                    GlobalSystemMediaTransportControlsSessionManager,
                    CurrentSessionChangedEventArgs,
                > = TypedEventHandler::new(
                    move |manager: windows::core::Ref<
                        '_,
                        GlobalSystemMediaTransportControlsSessionManager,
                    >,
                          _| {
                        if let Some(manager) = manager.as_ref() {
                            match manager.GetCurrentSession() {
                                Ok(session) => {
                                    publish_session(&session, &session_sender);
                                    hook_playback_info(session, &session_sender);
                                }
                                Err(_) => {
                                    let _ = session_sender.send(MediaUpdate::none());
                                }
                            }
                        }
                        Ok(())
                    },
                );
                let _ = manager.CurrentSessionChanged(&handler);

                while let Ok(()) = toggle_rx.recv() {
                    if let Ok(session) = manager.GetCurrentSession() {
                        let _ = session.TryTogglePlayPauseAsync();
                    }
                }
            });
        })
        .map(|_| ())
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))
}

fn hook_playback_info(
    session: GlobalSystemMediaTransportControlsSession,
    sender: &Sender<MediaUpdate>,
) {
    let session_sender = sender.clone();
    let handler: TypedEventHandler<
        GlobalSystemMediaTransportControlsSession,
        PlaybackInfoChangedEventArgs,
    > = TypedEventHandler::new(
        move |session: windows::core::Ref<'_, GlobalSystemMediaTransportControlsSession>, _| {
            if let Some(session) = session.as_ref() {
                publish_session(session, &session_sender);
            }
            Ok(())
        },
    );
    let _ = session.PlaybackInfoChanged(&handler);
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
        if !title.is_empty() {
            let _ = sender.send(update(title, artist, playing));
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
        let update = update("Track".into(), "Artist".into(), true);
        assert_eq!(update.title, "Track");
        assert_eq!(update.artist, "Artist");
        assert!(update.playing);
    }

    #[test]
    fn sentinel_is_the_empty_title() {
        let none = MediaUpdate::none();
        assert!(none.title.is_empty() && none.artist.is_empty() && !none.playing);
    }

    #[test]
    fn paused_track_keeps_its_title() {
        let paused = update("Track".into(), "Artist".into(), false);
        assert!(!paused.playing);
        assert_eq!(paused.title, "Track");
    }
}
