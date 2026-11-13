//! Verbose call diagnostics (spec §3).
//!
//! `--diag-call` runs every source for five minutes and writes one
//! timestamped line per event to
//! `%LOCALAPPDATA%\TaskbarIsland\diag\call-YYYYMMDD-HHMMSS.log`.
//! `--dump-uia` adds full UIA tree dumps to the same sink (or to its own
//! `uia-*.log` file when used alone).

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::activities::call_state::mask_number;

/// Append-only diagnostics file. Cloneable so every source thread shares it.
#[derive(Clone)]
pub struct Diag {
    inner: Arc<Mutex<Option<File>>>,
}

impl Diag {
    /// Open (creating parents) the file at `path`.
    pub fn open(path: &PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Some(file))),
        })
    }

    pub fn log(&self, message: &str) {
        let line = format!("[{}] {message}\n", stamp());
        if let Ok(mut guard) = self.inner.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.write_all(line.as_bytes());
                let _ = file.flush();
            }
        }
        // Mirror into the normal app log when TBI_LOG=1.
        crate::log::log(&format!("call.diag: {message}"));
    }

    /// Dump a pre-formatted UIA tree, one indented line per element, with a
    /// header. Numbers are masked before they reach the file.
    pub fn dump_uia(&self, header: &str, tree: &str) {
        self.log(&format!("--- {header} ---"));
        for line in tree.lines() {
            self.log(&format!("  {}", mask_numbers(line)));
        }
        self.log("--- end ---");
    }
}

/// `GetCurrentPackageFullName` for this process: the honest answer to "does
/// the notification listener see us as a packaged app?". Source B cannot
/// attach without identity (`none`), which is what the sparse package
/// registration is for.
pub fn package_identity() -> String {
    use windows::core::PWSTR;
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

    const APPMODEL_ERROR_NO_PACKAGE: u32 = 15700;
    const ERROR_MORE_DATA: u32 = 122;
    let mut len = 0u32;
    // SAFETY: size query — no buffer is passed, `len` is the out parameter.
    let code = unsafe { GetCurrentPackageFullName(&mut len, None) }.0;
    if code == APPMODEL_ERROR_NO_PACKAGE {
        return "none (no package identity)".to_string();
    }
    if code != 0 && code != ERROR_MORE_DATA {
        return format!("error {code}");
    }
    let mut buffer = vec![0u16; len as usize];
    // SAFETY: `buffer` holds `len` UTF-16 units, exactly what the API asked
    // for on the size query above.
    let code = unsafe { GetCurrentPackageFullName(&mut len, Some(PWSTR(buffer.as_mut_ptr()))) }.0;
    if code != 0 {
        return format!("error {code}");
    }
    let end = len.saturating_sub(1) as usize;
    String::from_utf16_lossy(&buffer[..end.min(buffer.len())])
}

/// Mask digit runs of six or more characters in diagnostic dumps (phone
/// numbers, long ids) while keeping clocks and sizes readable.
fn mask_numbers(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut digits = String::new();
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            flush_digits(&mut out, &mut digits);
            out.push(ch);
        }
    }
    flush_digits(&mut out, &mut digits);
    out
}

fn flush_digits(out: &mut String, digits: &mut String) {
    if digits.len() >= 6 {
        out.push_str(&mask_number(digits));
    } else {
        out.push_str(digits);
    }
    digits.clear();
}

/// `%LOCALAPPDATA%\TaskbarIsland\diag`.
pub fn diag_dir() -> PathBuf {
    crate::settings::data_dir().join("diag")
}

/// `diag/<prefix>-YYYYMMDD-HHMMSS.log`.
pub fn log_path(prefix: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, m, d, hh, mm, ss) = civil_from_unix(now as i64);
    diag_dir().join(format!(
        "{prefix}-{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}.log"
    ))
}

/// Seconds since the Unix epoch -> local-looking civil date. Windows file
/// times are local, but the diag name only needs to be sortable and unique.
fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    // Howard Hinnant's civil_from_days, good until 2100 — diagnostic labels.
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Line timestamp `HH:MM:SS.mmm`.
fn stamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let rem = now.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        now.subsec_millis()
    )
}

/// Per-source event counters for the end-of-run summary line.
#[derive(Default)]
pub struct Counters {
    counts: Mutex<HashMap<&'static str, u64>>,
}

impl Counters {
    pub fn bump(&self, source: &'static str) {
        if let Ok(mut guard) = self.counts.lock() {
            *guard.entry(source).or_default() += 1;
        }
    }

    pub fn summary(&self) -> String {
        let guard = match self.counts.lock() {
            Ok(guard) => guard,
            Err(_) => return String::new(),
        };
        let mut parts: Vec<String> = guard.iter().map(|(k, v)| format!("{k}={v}")).collect();
        parts.sort();
        if parts.is_empty() {
            "no source events".into()
        } else {
            parts.join(" ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_paths_are_named_and_sorted() {
        let path = log_path("call");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("call-20"), "got {name}");
        assert!(name.ends_with(".log"));
        assert_eq!(name.len(), "call-YYYYMMDD-HHMMSS.log".len());
    }

    #[test]
    fn long_digit_runs_are_masked_but_clocks_stay() {
        assert_eq!(mask_numbers("12:34 keeps"), "12:34 keeps");
        assert_eq!(mask_numbers("caller +919876543210"), "caller +•••••••••210");
    }

    #[test]
    fn counters_summarize() {
        let counters = Counters::default();
        counters.bump("toast");
        counters.bump("toast");
        counters.bump("window");
        assert_eq!(counters.summary(), "toast=2 window=1");
        assert_eq!(Counters::default().summary(), "no source events");
    }
}
