//! store.rs — clipboard history model, retention, capacity and persistence.
//!
//! The store is pure data: capture and the UI call into it with an explicit
//! wall-clock timestamp so the retention rules are testable. Expiry uses a
//! persisted high-water mark, so a backwards clock jump never ages items early
//! while a forwards jump expires them normally.

use std::path::{Path, PathBuf};

use crate::design::{CLIP_MAX_ITEMS, CLIP_RETENTION_DAYS};
use crate::json::{self, Json};
use crate::settings;

use super::dpapi;

pub const RETENTION_SECS: i64 = CLIP_RETENTION_DAYS * 24 * 60 * 60;
/// Items inside their final day render amber.
pub const EXPIRING_SECS: i64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClipKind {
    Text,
    Link,
    Image,
    Files,
}

impl ClipKind {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Link => "link",
            Self::Image => "image",
            Self::Files => "files",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "text" => Some(Self::Text),
            "link" => Some(Self::Link),
            "image" => Some(Self::Image),
            "files" => Some(Self::Files),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    /// Size of the stored PNG, for the row tooltip.
    pub bytes: u64,
    pub thumb: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipItem {
    pub id: u64,
    pub kind: ClipKind,
    /// Content hash; also the blob file name for images.
    pub digest: String,
    /// UTC seconds since the Unix epoch.
    pub created_at: i64,
    pub pinned: bool,
    pub text: String,
    pub paths: Vec<String>,
    pub image: Option<ImageMeta>,
}

impl ClipItem {
    pub fn age_secs(&self, now: i64) -> i64 {
        now.saturating_sub(self.created_at).max(0)
    }

    /// Older than six days: the card shows the age in amber.
    pub fn expires_soon(&self, now: i64) -> bool {
        !self.pinned && self.age_secs(now) >= RETENTION_SECS - EXPIRING_SECS
    }

    pub fn expired(&self, effective_now: i64) -> bool {
        !self.pinned && effective_now.saturating_sub(self.created_at) > RETENTION_SECS
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddOutcome {
    /// All slots are pinned; the new copy is not stored.
    Refused,
    Added {
        id: u64,
        evicted: Vec<String>,
    },
    /// Same content; the row moved to the top and its 7 days restarted.
    Updated {
        id: u64,
    },
}

impl AddOutcome {
    pub fn accepted(&self) -> bool {
        !matches!(self, Self::Refused)
    }

    pub fn id(&self) -> Option<u64> {
        match self {
            Self::Added { id, .. } | Self::Updated { id } => Some(*id),
            Self::Refused => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinError {
    /// 29 rows are already pinned; keep one slot free for new copies.
    NoSlot,
    Missing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClipCounts {
    pub all: usize,
    pub text: usize,
    pub link: usize,
    pub image: usize,
    pub files: usize,
    pub pinned: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClipStore {
    /// Newest first.
    items: Vec<ClipItem>,
    /// Highest wall-clock second observed; guards against clock rollback.
    last_seen: i64,
    next_id: u64,
}

impl ClipStore {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            last_seen: 0,
            next_id: 1,
        }
    }

    pub fn items(&self) -> &[ClipItem] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn pinned_count(&self) -> usize {
        self.items.iter().filter(|item| item.pinned).count()
    }

    pub fn last_seen(&self) -> i64 {
        self.last_seen
    }

    pub fn counts(&self) -> ClipCounts {
        let mut counts = ClipCounts {
            all: self.items.len(),
            pinned: self.pinned_count(),
            ..ClipCounts::default()
        };
        for item in &self.items {
            match item.kind {
                ClipKind::Text => counts.text += 1,
                ClipKind::Link => counts.link += 1,
                ClipKind::Image => counts.image += 1,
                ClipKind::Files => counts.files += 1,
            }
        }
        counts
    }

    pub fn get(&self, id: u64) -> Option<&ClipItem> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Add a captured item. Duplicates move to the top instead of creating a
    /// second row; a full store evicts its oldest unpinned row in the same
    /// call, and an all-pinned store refuses the copy.
    pub fn add(
        &mut self,
        kind: ClipKind,
        digest: &str,
        text: String,
        paths: Vec<String>,
        image: Option<ImageMeta>,
        now: i64,
    ) -> AddOutcome {
        let stamp = now.max(self.last_seen);
        self.last_seen = stamp;

        if let Some(pos) = self
            .items
            .iter()
            .position(|item| item.kind == kind && item.digest == digest)
        {
            let mut item = self.items.remove(pos);
            item.created_at = stamp;
            let id = item.id;
            self.items.insert(0, item);
            return AddOutcome::Updated { id };
        }

        let mut evicted = Vec::new();
        if self.items.len() >= CLIP_MAX_ITEMS {
            let Some(pos) = self.oldest_unpinned() else {
                return AddOutcome::Refused;
            };
            evicted.push(self.items.remove(pos).digest);
        }

        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.items.insert(
            0,
            ClipItem {
                id,
                kind,
                digest: digest.to_owned(),
                created_at: stamp,
                pinned: false,
                text,
                paths,
                image,
            },
        );
        AddOutcome::Added { id, evicted }
    }

    fn oldest_unpinned(&self) -> Option<usize> {
        let mut best: Option<(usize, i64)> = None;
        for (index, item) in self.items.iter().enumerate() {
            if item.pinned {
                continue;
            }
            if best.is_none_or(|(_, stamp)| item.created_at <= stamp) {
                best = Some((index, item.created_at));
            }
        }
        best.map(|(index, _)| index)
    }

    /// Remove every unpinned item older than the retention window. Returns the
    /// removed rows so their blobs can be deleted. Uses `max(now, last_seen)`
    /// so a clock moving backwards never deletes anything early.
    pub fn expire(&mut self, now: i64) -> Vec<ClipItem> {
        let effective = now.max(self.last_seen);
        self.last_seen = effective;
        let mut removed = Vec::new();
        let mut kept = Vec::with_capacity(self.items.len());
        for item in self.items.drain(..) {
            if item.expired(effective) {
                removed.push(item);
            } else {
                kept.push(item);
            }
        }
        self.items = kept;
        removed
    }

    pub fn remove(&mut self, id: u64) -> Option<ClipItem> {
        let pos = self.items.iter().position(|item| item.id == id)?;
        Some(self.items.remove(pos))
    }

    /// Clear history keeps pinned rows (spec 2.4).
    pub fn clear_unpinned(&mut self) -> Vec<ClipItem> {
        let mut removed = Vec::new();
        let mut kept = Vec::with_capacity(self.items.len());
        for item in self.items.drain(..) {
            if item.pinned {
                kept.push(item);
            } else {
                removed.push(item);
            }
        }
        self.items = kept;
        removed
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> Result<(), PinError> {
        let pos = self
            .items
            .iter()
            .position(|item| item.id == id)
            .ok_or(PinError::Missing)?;
        if pinned && !self.items[pos].pinned && self.pinned_count() >= CLIP_MAX_ITEMS - 1 {
            return Err(PinError::NoSlot);
        }
        self.items[pos].pinned = pinned;
        Ok(())
    }

    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("version".into(), Json::Num(1.0)),
            ("lastSeen".into(), Json::Num(self.last_seen as f64)),
            ("nextId".into(), Json::Num(self.next_id as f64)),
            (
                "items".into(),
                Json::Arr(self.items.iter().map(item_to_json).collect()),
            ),
        ])
    }

    /// Lenient reader: a corrupt store starts empty and never panics.
    pub fn from_json(value: &Json) -> Self {
        let mut store = Self::new();
        let Some(items) = value.get("items").and_then(Json::as_array) else {
            return store;
        };
        for raw in items {
            let Some(kind) = raw
                .get("kind")
                .and_then(Json::as_str)
                .and_then(ClipKind::from_tag)
            else {
                continue;
            };
            let Some(digest) = raw.get("digest").and_then(Json::as_str) else {
                continue;
            };
            if digest.is_empty()
                || store
                    .items
                    .iter()
                    .any(|item| item.kind == kind && item.digest == digest)
            {
                continue;
            }
            let Some(created_at) = raw.get("createdAt").and_then(Json::as_f64) else {
                continue;
            };
            if !created_at.is_finite() || created_at < 0.0 {
                continue;
            }
            let created_at = created_at as i64;
            let id = raw
                .get("id")
                .and_then(Json::as_f64)
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(|n| n as u64)
                .unwrap_or(0);
            let paths = raw
                .get("paths")
                .and_then(Json::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|path| path.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let image = raw.get("image").and_then(|image| {
                let width = image.get("width").and_then(Json::as_f64)?;
                let height = image.get("height").and_then(Json::as_f64)?;
                if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
                    return None;
                }
                Some(ImageMeta {
                    width: width as u32,
                    height: height as u32,
                    bytes: image
                        .get("bytes")
                        .and_then(Json::as_f64)
                        .filter(|n| n.is_finite() && *n >= 0.0)
                        .unwrap_or(0.0) as u64,
                    thumb: image.get("thumb").and_then(Json::as_bool).unwrap_or(false),
                })
            });
            store.items.push(ClipItem {
                id,
                kind,
                digest: digest.to_owned(),
                created_at,
                pinned: raw.get("pinned").and_then(Json::as_bool).unwrap_or(false),
                text: raw
                    .get("text")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                paths,
                image,
            });
            if store.items.len() >= CLIP_MAX_ITEMS {
                break;
            }
        }
        let highest = store.items.iter().map(|item| item.id).max().unwrap_or(0);
        let next_from_json = value
            .get("nextId")
            .and_then(Json::as_f64)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64)
            .unwrap_or(0);
        store.next_id = next_from_json.max(highest + 1).max(1);
        // Rows without a persisted id all share id 0; give them stable ids.
        let mut next = store.next_id;
        for item in &mut store.items {
            if item.id == 0 {
                item.id = next;
                next += 1;
            }
        }
        store.next_id = next;
        let newest = store
            .items
            .iter()
            .map(|item| item.created_at)
            .max()
            .unwrap_or(0);
        let last_seen = value
            .get("lastSeen")
            .and_then(Json::as_f64)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as i64)
            .unwrap_or(0);
        store.last_seen = last_seen.max(newest);
        store
    }

    pub fn load() -> Self {
        let Ok(sealed) = std::fs::read(store_path()) else {
            return Self::new();
        };
        let Some(plain) = dpapi::unprotect(&sealed) else {
            return Self::new();
        };
        let Ok(text) = String::from_utf8(plain) else {
            return Self::new();
        };
        let Ok(value) = json::parse(&text) else {
            return Self::new();
        };
        Self::from_json(&value)
    }

    /// Synchronous save, used at startup and by tests. Capture goes through
    /// the writer thread instead.
    pub fn save(&self) -> bool {
        persist_json(&json::to_string_pretty(&self.to_json()), &[])
    }
}

fn item_to_json(item: &ClipItem) -> Json {
    let image = item.image.map_or(Json::Null, |meta| {
        Json::Obj(vec![
            ("width".into(), Json::Num(meta.width as f64)),
            ("height".into(), Json::Num(meta.height as f64)),
            ("bytes".into(), Json::Num(meta.bytes as f64)),
            ("thumb".into(), Json::Bool(meta.thumb)),
        ])
    });
    Json::Obj(vec![
        ("id".into(), Json::Num(item.id as f64)),
        ("kind".into(), Json::Str(item.kind.tag().into())),
        ("digest".into(), Json::Str(item.digest.clone())),
        ("createdAt".into(), Json::Num(item.created_at as f64)),
        ("pinned".into(), Json::Bool(item.pinned)),
        ("text".into(), Json::Str(item.text.clone())),
        (
            "paths".into(),
            Json::Arr(item.paths.iter().map(|p| Json::Str(p.clone())).collect()),
        ),
        ("image".into(), image),
    ])
}

pub fn clipboard_dir() -> PathBuf {
    settings::data_dir().join("clipboard")
}

pub fn store_path() -> PathBuf {
    clipboard_dir().join("store.dat")
}

pub fn blobs_dir() -> PathBuf {
    clipboard_dir().join("blobs")
}

pub fn blob_path(digest: &str) -> PathBuf {
    blobs_dir().join(format!("{digest}.png"))
}

pub fn thumb_path(digest: &str) -> PathBuf {
    blobs_dir().join(format!("{digest}.thumb.png"))
}

/// Encrypt and atomically replace the store, then delete blobs that are no
/// longer referenced. Returns whether the store reached the disk.
pub fn persist_json(text: &str, deletes: &[String]) -> bool {
    let Some(sealed) = dpapi::protect(text.as_bytes()) else {
        return false;
    };
    if write_atomic(&store_path(), &sealed).is_err() {
        return false;
    }
    delete_blobs(deletes);
    true
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub fn write_blob(digest: &str, png: &[u8], thumb: Option<&[u8]>) -> bool {
    if write_atomic(&blob_path(digest), png).is_err() {
        return false;
    }
    if let Some(thumb) = thumb {
        if write_atomic(&thumb_path(digest), thumb).is_err() {
            let _ = std::fs::remove_file(blob_path(digest));
            return false;
        }
    }
    true
}

pub fn delete_blobs(digests: &[String]) {
    for digest in digests {
        let _ = std::fs::remove_file(blob_path(digest));
        let _ = std::fs::remove_file(thumb_path(digest));
    }
}

/// Remove blob files no live image row references. Called once at startup.
pub fn cleanup_orphans(items: &[ClipItem]) {
    let live: std::collections::HashSet<&str> = items
        .iter()
        .filter(|item| item.kind == ClipKind::Image)
        .map(|item| item.digest.as_str())
        .collect();
    let Ok(entries) = std::fs::read_dir(blobs_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let digest = name
            .strip_suffix(".thumb.png")
            .or_else(|| name.strip_suffix(".png"));
        if digest.is_none_or(|digest| !live.contains(digest)) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60;
    const T0: i64 = 1_760_000_000;

    fn text_item(store: &mut ClipStore, content: &str, now: i64) -> AddOutcome {
        store.add(
            ClipKind::Text,
            &format!("digest-{content}"),
            content.into(),
            Vec::new(),
            None,
            now,
        )
    }

    /// Insert a row with a historical timestamp, bypassing the high-water
    /// stamping that `add` applies to live captures.
    fn raw_item(store: &mut ClipStore, content: &str, created_at: i64) {
        let id = store.next_id.max(1);
        store.next_id = id + 1;
        store.last_seen = store.last_seen.max(created_at);
        store.items.push(ClipItem {
            id,
            kind: ClipKind::Text,
            digest: format!("digest-{content}"),
            created_at,
            pinned: false,
            text: content.into(),
            paths: Vec::new(),
            image: None,
        });
    }

    #[test]
    fn retention_boundaries() {
        let mut store = ClipStore::new();
        raw_item(
            &mut store,
            "seven-days-minus-a-second",
            T0 - RETENTION_SECS + 1,
        );
        raw_item(
            &mut store,
            "seven-days-plus-a-second",
            T0 - RETENTION_SECS - 1,
        );
        let removed = store.expire(T0);
        assert_eq!(store.len(), 1);
        assert_eq!(store.items()[0].text, "seven-days-minus-a-second");
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].text, "seven-days-plus-a-second");
    }

    #[test]
    fn clock_backwards_never_expires_early() {
        let mut store = ClipStore::new();
        text_item(&mut store, "old", T0);
        // Two days earlier: the high-water mark keeps the age at zero.
        assert!(store.expire(T0 - 2 * DAY).is_empty());
        assert_eq!(store.len(), 1);
        // The next copy is stamped at the high-water mark, not the rolled-back
        // clock, so it still gets its full seven days.
        text_item(&mut store, "new", T0 - 2 * DAY);
        assert_eq!(store.items()[0].created_at, T0);
        assert!(store.expire(T0 + RETENTION_SECS).is_empty());
        assert_eq!(store.expire(T0 + RETENTION_SECS + 1).len(), 2);
    }

    #[test]
    fn pinned_rows_outlive_retention() {
        let mut store = ClipStore::new();
        let outcome = text_item(&mut store, "keep", T0 - 10 * DAY);
        store.set_pinned(outcome.id().unwrap(), true).unwrap();
        assert!(store.expire(T0).is_empty());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn thirty_first_evicts_oldest_unpinned() {
        let mut store = ClipStore::new();
        for i in 0..CLIP_MAX_ITEMS {
            text_item(&mut store, &format!("row-{i}"), T0 + i as i64);
        }
        assert_eq!(store.len(), CLIP_MAX_ITEMS);
        // Pin the oldest row; eviction must skip it and take the next oldest.
        store
            .set_pinned(store.items()[CLIP_MAX_ITEMS - 1].id, true)
            .unwrap();
        let outcome = text_item(&mut store, "row-31", T0 + 31);
        match outcome {
            AddOutcome::Added { evicted, .. } => {
                assert_eq!(evicted.len(), 1);
                assert_eq!(evicted[0], "digest-row-1");
            }
            other => panic!("expected Added, got {other:?}"),
        }
        assert_eq!(store.len(), CLIP_MAX_ITEMS);
        assert!(store.items().iter().any(|item| item.text == "row-0"));
        assert!(!store.items().iter().any(|item| item.text == "row-1"));
        assert!(store.items().iter().any(|item| item.text == "row-2"));
    }

    #[test]
    fn all_pinned_store_refuses_new_copies() {
        let mut store = ClipStore::new();
        for i in 0..CLIP_MAX_ITEMS {
            text_item(&mut store, &format!("row-{i}"), T0 + i as i64);
            store.items[0].pinned = true;
        }
        assert_eq!(store.pinned_count(), CLIP_MAX_ITEMS);
        assert_eq!(
            text_item(&mut store, "row-31", T0 + 31),
            AddOutcome::Refused
        );
        assert_eq!(store.len(), CLIP_MAX_ITEMS);
    }

    #[test]
    fn pin_is_refused_at_twenty_nine() {
        let mut store = ClipStore::new();
        let mut last = 0;
        for i in 0..CLIP_MAX_ITEMS {
            let outcome = text_item(&mut store, &format!("row-{i}"), T0 + i as i64);
            last = outcome.id().unwrap();
            if i < CLIP_MAX_ITEMS - 1 {
                store.set_pinned(last, true).unwrap();
            }
        }
        assert_eq!(store.pinned_count(), CLIP_MAX_ITEMS - 1);
        assert_eq!(store.set_pinned(last, true), Err(PinError::NoSlot));
        assert_eq!(store.pinned_count(), CLIP_MAX_ITEMS - 1);
        // Freeing one pinned slot re-allows the refused pin.
        let freed = store.items()[1].id;
        assert_eq!(store.set_pinned(freed, false), Ok(()));
        assert_eq!(store.set_pinned(last, true), Ok(()));
    }

    #[test]
    fn duplicates_move_to_top_and_reset_age() {
        let mut store = ClipStore::new();
        text_item(&mut store, "alpha", T0);
        text_item(&mut store, "beta", T0 + 10);
        let id = store.items()[1].id;
        let outcome = text_item(&mut store, "alpha", T0 + DAY);
        assert_eq!(outcome, AddOutcome::Updated { id });
        assert_eq!(store.len(), 2);
        assert_eq!(store.items()[0].text, "alpha");
        assert_eq!(store.items()[0].created_at, T0 + DAY);
        assert!(store.expire(T0 + RETENTION_SECS).is_empty());
    }

    #[test]
    fn clear_keeps_pinned_rows() {
        let mut store = ClipStore::new();
        text_item(&mut store, "a", T0);
        let pinned = text_item(&mut store, "b", T0 + 1).id().unwrap();
        text_item(&mut store, "c", T0 + 2);
        store.set_pinned(pinned, true).unwrap();
        let removed = store.clear_unpinned();
        assert_eq!(removed.len(), 2);
        assert_eq!(store.len(), 1);
        assert_eq!(store.items()[0].text, "b");
    }

    #[test]
    fn json_round_trips_items_and_high_water_mark() {
        let mut store = ClipStore::new();
        text_item(&mut store, "alpha", T0);
        let image = store
            .add(
                ClipKind::Image,
                "img-digest",
                String::new(),
                Vec::new(),
                Some(ImageMeta {
                    width: 640,
                    height: 480,
                    bytes: 1234,
                    thumb: true,
                }),
                T0 + 5,
            )
            .id()
            .unwrap();
        store.set_pinned(image, true).unwrap();
        text_item(&mut store, "beta", T0 + 10);

        let text = json::to_string_pretty(&store.to_json());
        let parsed = json::parse(&text).unwrap();
        let again = ClipStore::from_json(&parsed);
        assert_eq!(again.items(), store.items());
        assert_eq!(again.last_seen(), store.last_seen());
        assert_eq!(again.next_id, store.next_id);
    }

    #[test]
    fn corrupt_json_starts_empty() {
        let empty = ClipStore::from_json(&Json::Null);
        assert!(empty.is_empty());
        let broken = ClipStore::from_json(&json::parse(r#"{"items":[{"kind":"bogus"}]}"#).unwrap());
        assert!(broken.is_empty());
        // A row with a missing id gets a fresh stable one.
        let legacy = ClipStore::from_json(
            &json::parse(
                r#"{"items":[{"kind":"text","digest":"d","createdAt":1760000000,"text":"x"}]}"#,
            )
            .unwrap(),
        );
        assert_eq!(legacy.len(), 1);
        assert!(legacy.items()[0].id >= 1);
    }

    #[test]
    fn load_dedupes_and_caps_rows() {
        let mut rows = Vec::new();
        for i in 0..(CLIP_MAX_ITEMS + 5) {
            rows.push(Json::Obj(vec![
                ("kind".into(), Json::Str("text".into())),
                ("digest".into(), Json::Str(format!("d{i}"))),
                ("createdAt".into(), Json::Num(T0 as f64 + i as f64)),
                ("text".into(), Json::Str(format!("row {i}"))),
            ]));
        }
        rows.push(Json::Obj(vec![
            ("kind".into(), Json::Str("text".into())),
            ("digest".into(), Json::Str("d0".into())),
            ("createdAt".into(), Json::Num(T0 as f64)),
        ]));
        let store = ClipStore::from_json(&Json::Obj(vec![("items".into(), Json::Arr(rows))]));
        assert_eq!(store.len(), CLIP_MAX_ITEMS);
        let unique: std::collections::HashSet<_> = store
            .items()
            .iter()
            .map(|item| item.digest.as_str())
            .collect();
        assert_eq!(unique.len(), CLIP_MAX_ITEMS);
    }

    #[test]
    fn expiring_soon_tracks_the_last_day() {
        let mut store = ClipStore::new();
        raw_item(&mut store, "fresh", T0);
        raw_item(&mut store, "old", T0 - RETENTION_SECS + EXPIRING_SECS);
        let now = T0 + EXPIRING_SECS;
        let fresh = store
            .items()
            .iter()
            .find(|item| item.text == "fresh")
            .unwrap();
        let old = store
            .items()
            .iter()
            .find(|item| item.text == "old")
            .unwrap();
        assert!(!fresh.expires_soon(now));
        assert!(old.expires_soon(now));
    }
}
