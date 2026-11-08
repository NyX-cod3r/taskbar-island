//! writer.rs — off-thread persistence for the clipboard history.
//!
//! DPAPI encryption and disk writes run on one worker thread so a capture
//! never blocks the island message loop.

use std::sync::mpsc::{self, Sender};
use std::thread;

use crate::json;
use crate::log;

use super::store::{self, ClipStore};

pub struct StoreWriter {
    tx: Sender<Job>,
}

struct Job {
    text: String,
    deletes: Vec<String>,
}

impl StoreWriter {
    pub fn start() -> Option<Self> {
        let (tx, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("clipboard-store".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    if !store::persist_json(&job.text, &job.deletes) {
                        log::log("clipboard.store: persistence failed");
                    }
                }
            })
            .ok()?;
        Some(Self { tx })
    }

    pub fn save(&self, store: &ClipStore, deletes: Vec<String>) {
        let text = json::to_string_pretty(&store.to_json());
        let _ = self.tx.send(Job { text, deletes });
    }
}
