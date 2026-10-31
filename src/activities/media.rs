//! Global media-session subscription. All WinRT work lives off the UI thread.

use std::sync::mpsc::Sender;

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaUpdate {
    pub title: String,
    pub artist: String,
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
                publish_current(&manager, &sender);

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
                            publish_current(manager, &session_sender);
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

fn publish_current(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
    sender: &Sender<MediaUpdate>,
) {
    let Ok(session) = manager.GetCurrentSession() else {
        return;
    };
    publish_session(&session, sender);
}

fn publish_session(
    session: &GlobalSystemMediaTransportControlsSession,
    sender: &Sender<MediaUpdate>,
) {
    let Ok(operation) = session.TryGetMediaPropertiesAsync() else {
        return;
    };
    let sender = sender.clone();
    let _ = operation.when(move |properties| {
        let Ok(properties) = properties else { return };
        let title = properties
            .Title()
            .map(|value| value.to_string())
            .unwrap_or_default();
        let artist = properties
            .Artist()
            .map(|value| value.to_string())
            .unwrap_or_default();
        if !title.is_empty() {
            let _ = sender.send(MediaUpdate { title, artist });
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
}
