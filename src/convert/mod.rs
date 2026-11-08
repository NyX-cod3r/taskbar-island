//! convert — the offline Shelf converter.
//!
//! Everything in this module is local: files are read, decoded, re-encoded and
//! written next to the original. There is no updater, no remote codec and no
//! cloud call. `detect` classifies inputs by magic bytes before any decoder is
//! allowed to look at them.

pub mod detect;
pub mod jobs;
pub mod options;
pub mod shell;

pub use detect::{probe_all, Probe, ProbeKind};

/// Hard ceiling before a file is even opened for conversion.
pub const MAX_FILE_BYTES: u64 = 1 << 30; // 1 GiB
/// Any single decoded raster above this is refused.
pub const MAX_PIXELS: u64 = 150_000_000;
/// Upper bound for all decoder allocations for one image.
pub const MAX_DECODE_BYTES: u64 = 3 * (1 << 29); // 1.5 GiB
/// A batch never grows beyond this many dragged files.
pub const MAX_INPUTS: usize = 24;

/// Size in bytes as "2.4 MB" without pulling the view layer in.
pub fn size_label(bytes: u64) -> String {
    crate::view::fmt_size(bytes)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// The conversion path must never delete a file, not even its own failed
    /// temp output. Only production code is scanned; test fixtures clean up
    /// their own scratch directories.
    #[test]
    fn conversion_path_never_deletes_files() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("convert");
        for name in ["mod.rs", "detect.rs", "jobs.rs"] {
            let text = std::fs::read_to_string(dir.join(name)).unwrap();
            let production = text.split("#[cfg(test)]").next().unwrap_or("");
            for forbidden in [
                "remove_file(",
                "remove_dir(",
                "remove_dir_all(",
                "DeleteFileW(",
                "DeleteFileA(",
            ] {
                assert!(
                    !production.contains(forbidden),
                    "{name} contains `{forbidden}`; conversion may never delete a file"
                );
            }
        }
    }
}
