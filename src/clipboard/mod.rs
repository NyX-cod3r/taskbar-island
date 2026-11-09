//! clipboard — offline clipboard history (v0.7 Phase 2).
//!
//! No UI lives here. The runtime loads the encrypted store at startup, keeps
//! the message-only capture listener alive, applies worker results and runs the
//! hourly retention sweep.

pub mod capture;
pub mod dpapi;
pub mod secret;
pub mod store;
pub mod write;
pub mod writer;

use std::sync::mpsc::Receiver;

use windows::Win32::Foundation::{HINSTANCE, HWND};

use crate::json;

use capture::Capture;
pub use capture::{CaptureResult, WM_APP_CLIPBOARD};
use store::{AddOutcome, ClipItem, ClipKind, ClipStore, PinError};
use writer::StoreWriter;

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

pub struct ClipboardRuntime {
    pub store: ClipStore,
    capture: Capture,
    results: Receiver<CaptureResult>,
    writer: StoreWriter,
}

pub struct Applied {
    /// The store accepted the copy (pinned-full stores refuse it).
    pub stored: bool,
    pub id: Option<u64>,
}

impl ClipboardRuntime {
    pub fn start(instance: HINSTANCE, owner: HWND, paused: bool) -> Option<Self> {
        let writer = StoreWriter::start()?;
        let mut store = ClipStore::load();
        let expired = store.expire(now_secs());
        if !expired.is_empty() {
            let deletes: Vec<String> = expired.into_iter().map(|item| item.digest).collect();
            store::persist_json(&json::to_string_pretty(&store.to_json()), &deletes);
        }
        store::cleanup_orphans(store.items());
        let (capture, results) = Capture::start(instance, owner, paused)?;
        Some(Self {
            store,
            capture,
            results,
            writer,
        })
    }

    pub fn is_paused(&self) -> bool {
        self.capture.is_paused()
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.capture.set_paused(paused);
    }

    pub fn drain(&self) -> Vec<CaptureResult> {
        let mut results = Vec::new();
        while let Ok(result) = self.results.try_recv() {
            results.push(result);
        }
        results
    }

    /// Expire old rows in the same transaction as the new copy, then apply the
    /// capacity and duplicate rules and persist everything off-thread.
    pub fn apply(&mut self, item: ClipItem, now: i64) -> Applied {
        let digest = item.digest.clone();
        let is_image = item.kind == ClipKind::Image;
        let mut deletes: Vec<String> = self
            .store
            .expire(now)
            .into_iter()
            .map(|item| item.digest)
            .collect();
        let outcome = self.store.add(
            item.kind,
            &digest,
            item.text,
            item.paths,
            item.image,
            now,
            item.source_app,
        );
        if let AddOutcome::Added { evicted, .. } = &outcome {
            deletes.extend(evicted.iter().cloned());
        }
        if !outcome.accepted() && is_image {
            // The worker already wrote this blob; nothing references it.
            deletes.push(digest);
        }
        self.writer.save(&self.store, deletes);
        Applied {
            stored: outcome.accepted(),
            id: outcome.id(),
        }
    }

    /// Hourly sweep and the check run whenever the card opens.
    pub fn sweep(&mut self, now: i64) -> usize {
        let removed = self.store.expire(now);
        if removed.is_empty() {
            return 0;
        }
        let deletes: Vec<String> = removed.iter().map(|item| item.digest.clone()).collect();
        self.writer.save(&self.store, deletes);
        removed.len()
    }

    pub fn clear_unpinned(&mut self) -> usize {
        let removed = self.store.clear_unpinned();
        if removed.is_empty() {
            return 0;
        }
        let deletes: Vec<String> = removed.iter().map(|item| item.digest.clone()).collect();
        self.writer.save(&self.store, deletes);
        removed.len()
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> Result<(), PinError> {
        self.store.set_pinned(id, pinned)?;
        self.writer.save(&self.store, Vec::new());
        Ok(())
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let Some(item) = self.store.remove(id) else {
            return false;
        };
        let deletes = if item.kind == ClipKind::Image {
            vec![item.digest]
        } else {
            Vec::new()
        };
        self.writer.save(&self.store, deletes);
        true
    }

    /// Puts a history row back on the system clipboard, then arms the
    /// own-write guard so the `WM_CLIPBOARDUPDATE` this write causes is
    /// dropped by the capture gate instead of recorded as a duplicate
    /// (spec 2.3 and 5.7).
    pub fn copy_back(&self, item: &ClipItem, owner: HWND) -> Result<(), String> {
        write::write(item, owner)?;
        self.capture.note_own_write();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::store::ImageMeta;

    #[test]
    fn expiry_upserts_do_not_leak_blob_names() {
        // Exercise the pure store path the runtime uses on every copy.
        let mut store = ClipStore::new();
        let now = 1_760_000_000;
        let old = store.add(
            ClipKind::Image,
            "old",
            String::new(),
            Vec::new(),
            Some(ImageMeta {
                width: 2,
                height: 2,
                bytes: 10,
                thumb: true,
            }),
            now - store::RETENTION_SECS - 1,
            None,
        );
        assert!(old.accepted());
        let removed = store.expire(now);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].digest, "old");
    }
}
