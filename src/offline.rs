//! Offline guard. The v0.5 spec forbids network code at runtime, so the
//! dependency graph and our own sources are audited by the test suite.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    const DENIED_PACKAGES: &[&str] = &[
        "reqwest",
        "hyper",
        "ureq",
        "curl",
        "isahc",
        "surf",
        "attohttpc",
        "native-tls",
        "rustls",
        "openssl",
        "openssl-sys",
        "tokio",
        "winhttp",
        "wininet",
    ];

    const DENIED_WINDOWS_FEATURES: &[&str] = &[
        "Win32_Networking_WinHttp",
        "Win32_Networking_WinInet",
        "Win32_Networking_WinSock",
    ];

    fn manifest_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn source_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                source_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn cargo_lock_has_no_network_crates() {
        let lock = std::fs::read_to_string(manifest_dir().join("Cargo.lock"))
            .expect("Cargo.lock must exist for the offline audit");
        for line in lock.lines() {
            let Some(name) = line
                .trim()
                .strip_prefix("name = \"")
                .and_then(|rest| rest.strip_suffix('"'))
            else {
                continue;
            };
            assert!(
                !DENIED_PACKAGES.contains(&name.to_ascii_lowercase().as_str()),
                "Cargo.lock contains network-capable crate `{name}`; the v0.5 build must stay offline"
            );
        }
    }

    #[test]
    fn windows_features_do_not_include_networking() {
        let manifest = std::fs::read_to_string(manifest_dir().join("Cargo.toml"))
            .expect("Cargo.toml must exist");
        for feature in DENIED_WINDOWS_FEATURES {
            assert!(
                !manifest.contains(feature),
                "Cargo.toml enables `{feature}`; runtime networking is forbidden"
            );
        }
    }

    #[test]
    fn sources_do_not_open_sockets_or_http() {
        let mut files = Vec::new();
        source_files(&manifest_dir().join("src"), &mut files);
        files.push(manifest_dir().join("build.rs"));
        for file in files {
            if file.file_name().and_then(|n| n.to_str()) == Some("offline.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            for forbidden in [
                "std::net::",
                "TcpStream",
                "TcpListener",
                "UdpSocket",
                "getaddrinfo",
                "InternetOpen",
                "WinHttp",
                "WinInet",
                "http://",
                "https://",
            ] {
                assert!(
                    !text.contains(forbidden),
                    "{} contains `{forbidden}`; the converter must never touch the network",
                    file.display()
                );
            }
        }
    }
}
