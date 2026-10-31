//! Global media-session subscription. All WinRT work lives off the UI thread.
//!
//! Event chain: session-manager request → publish current session, subscribe
//! `CurrentSessionChanged` + per-session `PlaybackInfoChanged`. Everything the
//! UI ends up drawing arrives as a `MediaUpdate` on the channel; nothing is
//! ever polled here. Updates are gated by `PlaybackStatus == Playing`: a
//! paused/stopped/missing session publishes the empty-title *sentinel*
//! (`MediaUpdate` with empty strings), which the UI path maps to "nothing
//! playing" and dedupes.

use std::sync::mpsc::Sender;

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus, PlaybackInfoChangedEventArgs,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaUpdate {
    pub title: String,
    pub artist: String,
}

impl MediaUpdate {
    /// The "nothing playing" sentinel.
    fn none() -> Self {
        Self {
            title: String::new(),
            artist: String::new(),
        }
    }
}

/// Starts the GSMTC listener without polling. The callback posts a compact
/// snapshot to the caller's channel, which can then post a UI message.
pub fn subscribe(sender: Sender<MediaUpdate>) -> windows::core::Result<()> {
    std::thread::Builder::new()
        .name("taskbar-island-media".into())
        .spawn(move || {
            let Ok(operation) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            else {
                return;
            };
            let _ = operation.when(move |manager| {
                let Ok(manager) = manager else { return };
                // Whatever is playing right now, publish once immediately.
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
                                // No current session at all → nothing playing.
                                Err(_) => {
                                    let _ = session_sender.send(MediaUpdate::none());
                                }
                            }
                        }
                        Ok(())
                    },
                );
                let _ = manager.CurrentSessionChanged(&handler);
                // Keep the manager and callback alive for the process.
                std::thread::park();
            });
        })
        .map(|_| ())
        .map_err(|e| {
            windows::core::Error::new(windows::core::HRESULT(0x80004005u32 as i32), e.to_string())
        })
}

/// Registers the per-session `PlaybackInfoChanged` event (play/pause/track
/// changes) so pausing and stopping reach the UI without polling, and keeps
/// the session object alive for the process.
fn hook_playback_info(
    session: GlobalSystemMediaTransportControlsSession,
    sender: &Sender<MediaUpdate>,
) {
    let sender = sender.clone();
    let handler: TypedEventHandler<
        GlobalSystemMediaTransportControlsSession,
        PlaybackInfoChangedEventArgs,
    > = TypedEventHandler::new(
        move |session: windows::core::Ref<'_, GlobalSystemMediaTransportControlsSession>, _| {
            if let Some(session) = session.as_ref() {
                publish_session(session, &sender);
            }
            Ok(())
        },
    );
    let _ = session.PlaybackInfoChanged(&handler);
    // Leaked on purpose: the handler chain lives inside the session object,
    // which must stay alive. Single-instance app; process exit frees the lot.
    Box::leak(Box::new(session));
}

/// Publishes the *visible* media state: the track only while the session is
/// actually playing, otherwise the nothing-playing sentinel. The status is
/// re-read inside the async callback so pause events race no track updates.
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
        if playing && !title.is_empty() {
            // Playing but empty title is transient (metadata not ready yet);
            // let the next event supply it instead of clearing the pill.
            let _ = sender.send(MediaUpdate { title, artist });
        } else if !playing {
            let _ = sender.send(MediaUpdate::none());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_keeps_track_metadata() {
        let update = MediaUpdate {
            title: "Track".into(),
            artist: "Artist".into(),
        };
        assert_eq!(update.title, "Track");
        assert_eq!(update.artist, "Artist");
    }

    #[test]
    fn sentinel_is_the_empty_title() {
        let none = MediaUpdate::none();
        assert!(none.title.is_empty() && none.artist.is_empty());
    }
}
