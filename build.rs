//! Compiles the Win32 icon + version resource into the executable.
//!
//! The host toolchain is `x86_64-pc-windows-gnu`, so `windres` emits a COFF
//! object that is handed straight to the linker. The icon must live in the
//! executable (not a side file) so the Start menu shortcut, Apps & Features
//! and the notification-area icon all get it for free.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=build.rs");

    let Some(windres) = find_windres() else {
        println!(
            "cargo:warning=windres was not found; taskbar-island.exe will be built without its icon"
        );
        return;
    };
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let object = out.join("app_icon.o");
    let status = Command::new(&windres)
        .args(["-O", "coff", "-o"])
        .arg(&object)
        .arg("assets/app.rc")
        .status();
    match status {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg={}", object.display());
        }
        Ok(status) => println!("cargo:warning=windres failed with {status}"),
        Err(error) => println!("cargo:warning=unable to run windres: {error}"),
    }
}

fn find_windres() -> Option<PathBuf> {
    if let Some(path) = env::var_os("WINDRES") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    // scripts/build.ps1 prepends this WinLibs toolchain to PATH; cargo run
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
