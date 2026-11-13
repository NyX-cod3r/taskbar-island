- Split the installer into the **core** setup (`TaskbarIsland-Setup.exe`: our
  executable plus `LICENSE`/`NOTICE`, nothing else) and the optional
  **Converters pack** (`TaskbarIsland-Converters-Setup.exe`, third-party
  engines only), so the artifact that gets code-signed contains only our own
  binaries; the tray's "Manage engines..." now prefers the pack and still
  falls back to the pre-0.9 setup copy.
- Single-source the product identity: `version` and `[package.metadata]
  product-name` in `Cargo.toml` feed the exe's VERSIONINFO (`build.rs` renders
  `assets/app.rc` from a template) and Inno Setup (`/DMyAppVersion`,
  `/DMyAppName`), with `version_tests` failing the suite if a literal copy
  reappears. Bumped to 0.9.0.
- Added `.github/workflows/release.yml`: on a `v<x.y.z>` tag it builds the core
  installer from source on a clean runner, runs the tests, **fails when the tag
  disagrees with `Cargo.toml`**, hashes the installer and uploads the setup,
  its SHA-256, the update payload and `manifest.txt` to a **draft** release —
  never publishing and never using a stored secret.
- Added `README.md` (with the **Code signing policy**: author / reviewer /
  approver roles, the no-network privacy statement, SmartScreen explanation and
  a SignPath Foundation line kept as a pending placeholder), `docs/signing.md`
  (the SignPath plan, every step marked `[AFTER APPROVAL]`) and
  `docs/release-process.md` (bump → test → merge → tag → CI draft → sign →
  owner publishes).
- License audit: the core installer ships no third-party code and every Rust
  dependency is OSI-approved; the only item whose terms are not verifiable from
  the file alone (per-language Tesseract `tessdata`) stays pack-only. Recorded
  in `THIRD_PARTY_LICENSES.md`.
- Worked around the GCC 16 / binutils manifest conflict (sourceware 34362):
  `tools\build.ps1` moves the toolchain's `default-manifest.o` aside and gates
  every release build on the exe carrying exactly one `<assembly>` root
  including our `<msix>` element (sparse-package identity, spec 3 Source B).
