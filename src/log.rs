//! Diagnostic logging, gated behind the `TBI_LOG` env var (local file only —
//! no network, no telemetry). Used to trace renderer startup failures; can be
//! removed once Phase 1 verification is done.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("TBI_LOG").is_ok())
}

pub fn log(msg: &str) {
    if !enabled() {
        return;
    }
    let path = std::env::temp_dir().join("taskbar-island.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{msg}");
    }
}

pub fn log_err(ctx: &str, e: &windows::core::Error) {
    log(&format!("{ctx}: {e} (0x{:08X})", e.code().0 as u32));
}
