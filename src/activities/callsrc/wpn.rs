//! Source D: the Windows notification database, read-only (spec §3).
//!
//! `wpndatabase.db` is written by the notification platform. This source
//! opens it with `SQLITE_OPEN_READONLY` only — this module contains no
//! write statement — using the SQLite that ships with Windows
//! (`System32\winsqlite3.dll`, loaded dynamically so the island still runs
//! where it is absent).
//!
//! The trigger is a filesystem change notification on the Notifications
//! folder: event driven, with a 500 ms settle window so one burst of
//! platform writes costs exactly one query. Nothing polls.

use std::collections::HashSet;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCSTR};
use windows::Win32::Foundation::{FARPROC, HMODULE, WAIT_EVENT, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Storage::FileSystem::{
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};

use super::toast::{classify_toast, ToastCall};
use super::uiakit::DumpNode;
use super::{CallSource, Emit, Signal, SignalKind, SourceCtx, SourceId};

/// Platform writes arrive in bursts (payload + expiry + index); one query
/// per burst instead of one per write.
const SETTLE: Duration = Duration::from_millis(500);

/// Newest rows considered per scan. Mirrors the `LIMIT` in [`SQL_CALLS`].
const ROW_LIMIT: usize = 64;

/// Phone Link's handler rows. The platform stores the package family name
/// (or the app id) in `NotificationHandler.PrimaryId`.
const SQL_CALLS: &str = "SELECT n.[Id], n.[Payload] FROM [Notification] AS n \
    JOIN [NotificationHandler] AS h ON h.[RecordId] = n.[HandlerId] \
    WHERE h.[PrimaryId] LIKE '%YourPhone%' \
    OR h.[PrimaryId] LIKE '%PhoneExperience%' \
    ORDER BY n.[ArrivalTime] DESC LIMIT 64";

const SQLITE_OK: i32 = 0;
const SQLITE_ROW: i32 = 100;
const SQLITE_OPEN_READONLY: i32 = 0x0000_0001;

type OpenV2Fn = unsafe extern "system" fn(PCSTR, *mut *mut c_void, i32, PCSTR) -> i32;
type PrepareV2Fn =
    unsafe extern "system" fn(*mut c_void, PCSTR, i32, *mut *mut c_void, *mut PCSTR) -> i32;
type StepFn = unsafe extern "system" fn(*mut c_void) -> i32;
type ColumnInt64Fn = unsafe extern "system" fn(*mut c_void, i32) -> i64;
type ColumnBlobFn = unsafe extern "system" fn(*mut c_void, i32) -> *const u8;
type ColumnBytesFn = unsafe extern "system" fn(*mut c_void, i32) -> i32;
type FinalizeFn = unsafe extern "system" fn(*mut c_void) -> i32;
type CloseV2Fn = unsafe extern "system" fn(*mut c_void) -> i32;
type ErrmsgFn = unsafe extern "system" fn(*mut c_void) -> PCSTR;

/// One resolved export of the system SQLite.
fn load_fn<T: Copy>(module: HMODULE, name: &str) -> Option<T> {
    let mut bytes = name.as_bytes().to_vec();
    bytes.push(0);
    // SAFETY: the module came from LoadLibraryW and the name is NUL terminated.
    let far: FARPROC = unsafe { GetProcAddress(module, PCSTR(bytes.as_ptr())) };
    let func = far?;
    // SAFETY: every `T` here is a function pointer, the same size as the
    // export we just resolved by name.
    Some(unsafe { std::mem::transmute_copy(&func) })
}

/// A phone-link row: its unique id and the raw toast payload.
struct Row {
    id: i64,
    payload: Vec<u8>,
}

/// A read-only connection to `wpndatabase.db`.
struct Reader {
    db: *mut c_void,
    prepare_v2: PrepareV2Fn,
    step: StepFn,
    column_int64: ColumnInt64Fn,
    column_blob: ColumnBlobFn,
    column_bytes: ColumnBytesFn,
    finalize: FinalizeFn,
    close_v2: CloseV2Fn,
    errmsg: ErrmsgFn,
}

impl Reader {
    /// Open the database read-only, or `None` when the system SQLite or the
    /// file is unavailable (the source then says so and stops).
    fn open(path: &Path) -> Option<Reader> {
        let dll = HSTRING::from("winsqlite3.dll");
        // SAFETY: loading a known system DLL by name.
        let module: HMODULE = unsafe { LoadLibraryW(&dll) }.ok()?;
        let open_v2: OpenV2Fn = load_fn(module, "sqlite3_open_v2")?;
        let prepare_v2: PrepareV2Fn = load_fn(module, "sqlite3_prepare_v2")?;
        let step: StepFn = load_fn(module, "sqlite3_step")?;
        let column_int64: ColumnInt64Fn = load_fn(module, "sqlite3_column_int64")?;
        let column_blob: ColumnBlobFn = load_fn(module, "sqlite3_column_blob")?;
        let column_bytes: ColumnBytesFn = load_fn(module, "sqlite3_column_bytes")?;
        let finalize: FinalizeFn = load_fn(module, "sqlite3_finalize")?;
        let close_v2: CloseV2Fn = load_fn(module, "sqlite3_close_v2")?;
        let errmsg: ErrmsgFn = load_fn(module, "sqlite3_errmsg")?;

        let mut db: *mut c_void = ptr::null_mut();
        let name = cstring(path.to_string_lossy().as_ref());
        // SAFETY: `db` is a valid out pointer, the path is NUL terminated,
        // and READONLY is the only open flag passed.
        let rc = unsafe {
            open_v2(
                PCSTR(name.as_ptr()),
                &mut db,
                SQLITE_OPEN_READONLY,
                PCSTR(ptr::null()),
            )
        };
        if rc != SQLITE_OK || db.is_null() {
            return None;
        }
        Some(Reader {
            db,
            prepare_v2,
            step,
            column_int64,
            column_blob,
            column_bytes,
            finalize,
            close_v2,
            errmsg,
        })
    }

    /// The newest Phone Link rows, newest first.
    fn call_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let sql = cstring(SQL_CALLS);
        let mut stmt: *mut c_void = ptr::null_mut();
        // SAFETY: valid connection, NUL-terminated statement, out pointer.
        let rc = unsafe {
            (self.prepare_v2)(self.db, PCSTR(sql.as_ptr()), -1, &mut stmt, ptr::null_mut())
        };
        if rc != SQLITE_OK || stmt.is_null() {
            return rows;
        }
        loop {
            // SAFETY: the statement is live until finalized below.
            let rc = unsafe { (self.step)(stmt) };
            if rc != SQLITE_ROW {
                break;
            }
            // Column 0 = Notification.Id, column 1 = Notification.Payload.
            let id = unsafe { (self.column_int64)(stmt, 0) };
            // SAFETY: valid statement and column index.
            let len = unsafe { (self.column_bytes)(stmt, 1) }.max(0) as usize;
            let payload = if len == 0 {
                Vec::new()
            } else {
                // SAFETY: SQLite returns `len` readable bytes for the column.
                unsafe { std::slice::from_raw_parts((self.column_blob)(stmt, 1), len) }.to_vec()
            };
            rows.push(Row { id, payload });
            if rows.len() >= ROW_LIMIT {
                break;
            }
        }
        // SAFETY: the statement came from prepare_v2 and is finished with.
        unsafe { (self.finalize)(stmt) };
        rows
    }

    /// Last error text, for an honest log line.
    fn error(&self) -> String {
        // SAFETY: the connection is open.
        let text = unsafe { (self.errmsg)(self.db) };
        if text.0.is_null() {
            return "unknown".into();
        }
        // SAFETY: SQLite returns a NUL-terminated UTF-8 string.
        unsafe { std::ffi::CStr::from_ptr(text.0.cast::<std::ffi::c_char>()) }
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: the connection came from open_v2 and is closed once.
        unsafe { (self.close_v2)(self.db) };
    }
}

fn cstring(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// Source D itself.
pub struct WpnDbSource;

impl CallSource for WpnDbSource {
    fn id(&self) -> SourceId {
        SourceId::WpnDb
    }

    fn label(&self) -> &'static str {
        "D: wpndatabase.db (read-only)"
    }

    fn run(self: Box<Self>, ctx: SourceCtx, emit: Emit) {
        run(ctx, emit);
    }
}

/// What this source is responsible for ending.
#[derive(Default)]
struct Tracker {
    /// Call rows that already existed before we started: never emitted, so
    /// a stale row cannot invent a call this source did not watch begin.
    known: HashSet<i64>,
    /// The row whose disappearance (or replacement by a "missed call")
    /// ends the call this source reported.
    tracked: Option<i64>,
}

fn run(ctx: SourceCtx, emit: Emit) {
    let diag = ctx.diag.clone();
    let note = |line: String| match &diag {
        Some(diag) => diag.log(&line),
        None => crate::log::log(&line),
    };

    let Some(dir) = notifications_dir() else {
        note("source D: %LOCALAPPDATA% is not available".into());
        return;
    };
    let path = dir.join("wpndatabase.db");
    let reader = match Reader::open(&path) {
        Some(reader) => reader,
        None => {
            note(format!(
                "source D: cannot open {} read-only (winsqlite3.dll missing or file unavailable)",
                path.display()
            ));
            return;
        }
    };

    // Arm the watcher before the first scan so a write during the scan is
    // not lost.
    let filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE;
    let wide_dir = HSTRING::from(dir.as_os_str());
    // SAFETY: `dir` exists; we only ever wait on the returned handle.
    let change = match unsafe { FindFirstChangeNotificationW(&wide_dir, false, filter) } {
        Ok(handle) => handle,
        Err(error) => {
            note(format!("source D: change notification failed: {error}"));
            return;
        }
    };

    let mut tracker = Tracker::default();
    scan(&reader, &mut tracker, &ctx, &emit, true);
    note(format!(
        "source D ready: read-only {} ({})",
        path.display(),
        reader.error()
    ));

    // The only wait in this source: the filesystem tells us when the
    // platform wrote, and the settle window collapses a burst into one scan.
    let mut settle: Option<Instant> = None;
    loop {
        let timeout = match settle {
            None => INFINITE,
            Some(due) => due
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(u128::from(u32::MAX)) as u32,
        };
        // SAFETY: the handle came from FindFirstChangeNotificationW.
        let event: WAIT_EVENT = unsafe { WaitForSingleObject(change, timeout) };
        if event == WAIT_OBJECT_0 {
            // SAFETY: re-arming after a signaled notification.
            if let Err(error) = unsafe { FindNextChangeNotification(change) } {
                note(format!("source D: re-arm failed: {error}"));
                break;
            }
            settle.get_or_insert_with(|| Instant::now() + SETTLE);
        } else if event == WAIT_TIMEOUT {
            if settle.is_some_and(|due| Instant::now() >= due) {
                settle = None;
                scan(&reader, &mut tracker, &ctx, &emit, false);
            }
        } else {
            note("source D: wait failed, stopping".into());
            break;
        }
    }
    // SAFETY: closing the notification handle we own.
    let _ = unsafe { FindCloseChangeNotification(change) };
}

/// Read the newest rows and fold them into the tracker.
///
/// `prime` is the startup pass: it only records what already exists. Every
/// later pass may emit — a *new* call row starts a call, and the loss of the
/// row this source is holding (or its replacement by a missed-call entry)
/// ends it.
fn scan(reader: &Reader, tracker: &mut Tracker, ctx: &SourceCtx, emit: &Emit, prime: bool) {
    let rows = reader.call_rows();
    let mut present: HashSet<i64> = HashSet::new();
    let mut calls: Vec<(i64, ToastCall)> = Vec::new();
    for row in &rows {
        present.insert(row.id);
        let Some(nodes) = payload_nodes(&row.payload) else {
            continue;
        };
        if let Some(diag) = &ctx.diag {
            let joined = nodes
                .iter()
                .map(|node| node.name.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            diag.log(&format!(
                "source D: row {} {}",
                row.id,
                crate::activities::call_state::mask_number(&joined)
            ));
        }
        if let Some(call) = classify_toast(&nodes, &[]) {
            calls.push((row.id, call));
        }
    }

    if prime {
        for (id, _) in &calls {
            tracker.known.insert(*id);
        }
        return;
    }

    // A row we are holding went away, or was replaced by a missed/ended
    // entry: this source reports the end it witnessed.
    if let Some(held) = tracker.tracked {
        if !present.contains(&held) {
            end_held(ctx, emit, tracker, held, "row removed");
        } else if calls
            .iter()
            .any(|(id, call)| *id == held && matches!(call, ToastCall::Ended { .. }))
        {
            end_held(ctx, emit, tracker, held, "row now says the call is over");
        }
    }

    // A call row we have never seen: a call starting right now.
    let fresh = calls
        .iter()
        .find(|(id, call)| {
            !tracker.known.contains(id)
                && matches!(call, ToastCall::Incoming { .. } | ToastCall::Dialing { .. })
        })
        .cloned();
    if let Some((id, call)) = fresh {
        tracker.known.insert(id);
        tracker.tracked = Some(id);
        let (kind, caller) = match call {
            ToastCall::Incoming { caller } => (SignalKind::Incoming, caller),
            ToastCall::Dialing { caller } => (SignalKind::Dialing, caller),
            ToastCall::Ended { caller } => (SignalKind::Ended, caller),
        };
        if let Some(diag) = &ctx.diag {
            let shown = caller
                .as_deref()
                .map(crate::activities::call_state::mask_number);
            diag.log(&format!("source D: new row {id} {kind:?} caller={shown:?}"));
        }
        emit.send(Signal::new(SourceId::WpnDb, kind, caller));
    }
}

/// Report the end of the row this source is holding.
fn end_held(ctx: &SourceCtx, emit: &Emit, tracker: &mut Tracker, held: i64, why: &str) {
    tracker.tracked = None;
    if let Some(diag) = &ctx.diag {
        diag.log(&format!("source D: row {held} {why} -> Ended"));
    }
    emit.send(Signal::new(SourceId::WpnDb, SignalKind::Ended, None));
}

/// `%LOCALAPPDATA%\Microsoft\Windows\Notifications`.
fn notifications_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local)
            .join("Microsoft")
            .join("Windows")
            .join("Notifications"),
    )
}

/// Decode the stored toast payload into classification nodes. `None` means
/// the payload is not something this source can honestly read.
fn payload_nodes(payload: &[u8]) -> Option<Vec<DumpNode>> {
    let xml = decode_payload(payload)?;
    Some(toast_nodes(&xml))
}

/// UTF-16LE when the second byte is 0 (a `<` in UTF-16LE), UTF-8 otherwise.
/// `None` when the result does not look like markup at all.
fn decode_payload(payload: &[u8]) -> Option<String> {
    if payload.is_empty() {
        return None;
    }
    let text = if payload.len() >= 2 && payload.len().is_multiple_of(2) && payload[1] == 0 {
        let (units, _rest) = payload.as_chunks::<2>();
        let units: Vec<u16> = units.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(payload).into_owned()
    };
    if text.trim_start().starts_with('<') {
        Some(text)
    } else {
        None
    }
}

/// Pull `<text>` contents and `<action content="…">` labels out of toast
/// markup. A deliberately small scanner: the payloads are one document and
/// the classifier only needs the human-visible strings.
pub fn toast_nodes(xml: &str) -> Vec<DumpNode> {
    let mut nodes = Vec::new();
    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('>') else {
            break;
        };
        let tag = after[..close].trim();
        rest = &after[close + 1..];
        if tag.starts_with('/') || tag.starts_with('!') || tag.starts_with('?') {
            continue;
        }
        let (name, attrs) = match tag.split_once(char::is_whitespace) {
            Some((name, attrs)) => (name, attrs),
            None => (tag, ""),
        };
        if name.eq_ignore_ascii_case("text") {
            let end = rest.find("</").unwrap_or(rest.len());
            let content = unescape(&rest[..end]).trim().to_string();
            if !content.is_empty() {
                nodes.push(DumpNode {
                    ctrl: "Text".into(),
                    name: content,
                });
            }
            rest = &rest[end..];
        } else if name.eq_ignore_ascii_case("action") {
            if let Some(content) = attr_value(attrs, "content") {
                let content = unescape(&content).trim().to_string();
                if !content.is_empty() {
                    nodes.push(DumpNode {
                        ctrl: "Button".into(),
                        name: content,
                    });
                }
            }
        }
    }
    nodes
}

/// Value of `key="…"` / `key='…'` inside a tag's attribute list.
fn attr_value(attrs: &str, key: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let at = lower.find(&format!("{key}="))?;
    let value = &attrs[at + key.len() + 1..];
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &value[1..];
    let end = value.find(quote)?;
    Some(value[..end].to_string())
}

/// The five XML entities plus numeric references.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..semi];
        rest = &rest[semi + 1..];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let code = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| entity.strip_prefix('#')?.parse::<u32>().ok());
                match code.and_then(char::from_u32) {
                    Some(ch) => out.push(ch),
                    None => {
                        out.push('&');
                        out.push_str(entity);
                        out.push(';');
                    }
                }
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INCOMING_TOAST: &str = r#"<toast><visual><binding template="ToastGeneric">
<text>Incoming call</text><text>Priya Sharma</text><text>Mobile</text>
</binding></visual><actions>
<action content="Answer" arguments="answer"/>
<action content="Decline" arguments="decline"/>
</actions></toast>"#;

    const MISSED_TOAST: &str = r#"<toast><visual><binding template="ToastGeneric">
<text>Missed call</text><text>Bo Chan</text>
</binding></visual></toast>"#;

    #[test]
    fn text_and_action_labels_are_extracted() {
        let nodes = toast_nodes(INCOMING_TOAST);
        assert_eq!(nodes.len(), 5);
        assert_eq!(nodes[0].ctrl, "Text");
        assert_eq!(nodes[0].name, "Incoming call");
        assert_eq!(nodes[1].name, "Priya Sharma");
        assert_eq!(nodes[3].ctrl, "Button");
        assert_eq!(nodes[3].name, "Answer");
        assert_eq!(nodes[4].name, "Decline");
    }

    #[test]
    fn extracted_nodes_classify_as_a_call() {
        let nodes = toast_nodes(INCOMING_TOAST);
        assert_eq!(
            classify_toast(&nodes, &[]),
            Some(ToastCall::Incoming {
                caller: Some("Priya Sharma".into())
            })
        );
        let missed = toast_nodes(MISSED_TOAST);
        assert_eq!(
            classify_toast(&missed, &[]),
            Some(ToastCall::Ended {
                caller: Some("Bo Chan".into())
            })
        );
    }

    #[test]
    fn utf16_payloads_and_entities_decode() {
        let xml = "<toast><text>A &amp; B &#x2713;</text></toast>";
        let mut bytes: Vec<u8> = Vec::new();
        for unit in xml.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(bytes[1], 0, "UTF-16LE is detected by its zero high byte");
        let decoded = decode_payload(&bytes).expect("utf-16 payload");
        assert_eq!(
            toast_nodes(&decoded),
            vec![DumpNode {
                ctrl: "Text".into(),
                name: "A & B ✓".into()
            }]
        );
        assert!(decode_payload(b"not markup at all").is_none());
        assert!(decode_payload(b"").is_none());
    }

    #[test]
    fn attribute_values_handle_both_quotes() {
        assert_eq!(
            attr_value(r#"content="Answer" arguments='x'"#, "content"),
            Some("Answer".into())
        );
        assert_eq!(
            attr_value(r#"arguments="x" content='Go'"#, "content"),
            Some("Go".into())
        );
        assert_eq!(attr_value(r#"arguments="x""#, "content"), None);
        assert_eq!(attr_value(r#"content = "spaced""#, "content"), None);
    }

    #[test]
    fn sql_is_select_only() {
        let upper = SQL_CALLS.to_ascii_uppercase();
        assert!(upper.starts_with("SELECT "), "read-only by construction");
        for forbidden in ["INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "PRAGMA"] {
            assert!(!upper.contains(forbidden), "{forbidden} must not appear");
        }
        assert!(upper.contains(&format!("LIMIT {ROW_LIMIT}")));
    }
}
