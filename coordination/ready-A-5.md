# ready — A5 (release)

Phase: **A5 (prompt §7)** — releasing.
Branch: `release/v0.8.0` (fast-forwarded from `agent-b/v0.8.0`).

## Built once, published unchanged

| Asset | Size | SHA-256 |
| --- | --- | --- |
| `TaskbarIsland-Setup.exe` | 136,260,210 bytes | `092fb592f88cd56dcc35a2d7e544796919b880849394dd1ea9509b0cacd380ba` |
| `taskbar-island-0.8.0.exe` | 3,695,104 bytes | `202045f52850672a225bc060317c23c42b8efa084fd41c9b2a4ff11d3246bef2` |

`manifest.txt` points at the payload URL and the payload hash above.

## Checks before publish

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`: clean, **272 passed / 0 failed / 1 ignored**.
- Installer harness on these exact bytes: **44/44, `INSTALLER-TEST-PASS`**.
- Tag: delete the stale local+origin `v0.8.0` (which points at the unreleased
  intermediate `3727c41`), recreate annotated on the final release commit.
- Publish `TaskbarIsland-Setup.exe`, `taskbar-island-0.8.0.exe` and
  `manifest.txt` as a stable (not draft/prerelease) release, then download the
  installer and compare SHA-256 against the build.
- PR `release/v0.8.0 -> main` (fast-forwardable). The repo default branch is
  `master`; `main` is the integration branch for this release.

## Known limits stated in the notes

- Phone Link real incoming-call validation not run (A2 spike NO-GO).
- Windows Sandbox clean-machine install not run (Sandbox absent).
- 22 engine golden rows skipped (LibreOffice/Poppler/qpdf/Ghostscript/tessdata
  payloads absent from `ENGINES_DIR`).
- Minute-30 soak spike disclosed; not reproduced.
- Installer unsigned; Setup's opt-in update check is the only network access.
- Ghostscript AGPL-3.0 restated in `THIRD_PARTY_NOTICES.md`.
