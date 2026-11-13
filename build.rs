//! Compiles the Win32 icon, fusion manifest and version resource into the
//! executable.
//!
//! The host toolchain is `x86_64-pc-windows-gnu`, so `windres` emits a COFF
//! object that is handed straight to the linker. The icon must live in the
//! executable (not a side file) so the Start menu shortcut, Apps & Features
//! and the notification-area icon all get it for free.
//!
//! `assets/app.rc` is a *template*: the product name and version come from
//! `Cargo.toml` (`version` and `[package.metadata] product-name`), the single
//! source of truth. There is deliberately no literal version anywhere else -
//! `src/main.rs::version_tests` fails the test suite if one reappears.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");

    let Some(windres) = find_windres() else {
        // A debug build may run without resources, but a release build is a
        // shippable artifact: without windres it would carry no icon and no
        // product name/version in its VERSIONINFO, which the release gate
        // (.github/workflows/release.yml) and the signing policy require.
        if env::var("PROFILE").as_deref() == Ok("release") {
            panic!(
                "windres was not found; a release build must carry the icon, \
                 VERSIONINFO and fusion manifest resources"
            );
        }
        println!(
            "cargo:warning=windres was not found; taskbar-island.exe will be built without its icon"
        );
        return;
    };

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let object = out.join("app_icon.o");
    let rc = out.join("app.rc");
    if let Err(error) = render_rc(&rc) {
        if env::var("PROFILE").as_deref() == Ok("release") {
            panic!("could not render assets/app.rc: {error}");
        }
        println!("cargo:warning=could not render assets/app.rc: {error}");
        return;
    }
    let status = Command::new(&windres)
        .args(["-O", "coff", "-o"])
        .arg(&object)
        .arg(&rc)
        .status();
    match status {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg={}", object.display());
        }
        Ok(status) => println!("cargo:warning=windres failed with {status}"),
        Err(error) => println!("cargo:warning=unable to run windres: {error}"),
    }
}

/// Writes `assets/app.rc` with the Cargo identity substituted, so `windres`
/// compiles a concrete resource script while the checked-in file stays a
/// token template.
fn render_rc(dest: &Path) -> std::io::Result<()> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let template = fs::read_to_string(manifest_dir.join("assets/app.rc"))?;
    let version = env!("CARGO_PKG_VERSION");
    let product_name = product_name()?;
    let (major, minor, patch) = version_triplet(version);
    let rendered = template
        .replace("@VERSION@", version)
        .replace("@VERSION_CSV@", &format!("{major},{minor},{patch},0"))
        .replace("@PRODUCT_NAME@", &product_name);
    fs::write(dest, &rendered)?;
    Ok(())
}

/// `[package.metadata] product-name = "..."` from Cargo.toml.
fn product_name() -> std::io::Result<String> {
    let text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))?;
    let mut in_metadata = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_metadata = line == "[package.metadata]";
            continue;
        }
        if !in_metadata {
            continue;
        }
        if let Some(rest) = line.strip_prefix("product-name") {
            let rest = rest.trim_start().strip_prefix('=').map(str::trim_start);
            if let Some(value) = rest.and_then(|r| r.strip_prefix('"')) {
                if let Some(name) = value.strip_suffix('"') {
                    if !name.is_empty() {
                        return Ok(name.to_string());
                    }
                }
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "product-name not found in Cargo.toml [package.metadata]",
    ))
}

/// `0.9.0` -> `(0, 9, 0)`; the fourth FILEVERSION field is always 0 (Cargo
/// versions are x.y.z and Inno's VersionInfoVersion pads the same way).
fn version_triplet(version: &str) -> (u64, u64, u64) {
    let mut parts = version.split('.').map(|part| part.parse::<u64>());
    let major = parts.next().and_then(Result::ok);
    let minor = parts.next().and_then(Result::ok);
    let patch = parts.next().and_then(Result::ok);
    match (major, minor, patch) {
        (Some(major), Some(minor), Some(patch)) => (major, minor, patch),
        _ => panic!("version '{version}' is not x.y.z"),
    }
}

fn find_windres() -> Option<PathBuf> {
    if let Some(path) = env::var_os("WINDRES") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    // tools/build.ps1 prepends this WinLibs toolchain to PATH; cargo run
    // directly from a shell does not, so look it up explicitly first.
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        let packages = Path::new(&local).join("Microsoft/WinGet/Packages");
        if let Ok(entries) = fs::read_dir(packages) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if name.starts_with("brechtsanders.winlibs") {
                    let candidate = entry.path().join("mingw64/bin/windres.exe");
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    if Command::new("windres").arg("--version").output().is_ok() {
        return Some(PathBuf::from("windres"));
    }
    for candidate in [
        r"C:\msys64\mingw64\bin\windres.exe",
        r"C:\MinGW\bin\windres.exe",
    ] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}
