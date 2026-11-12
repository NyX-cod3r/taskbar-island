# Testing

What is tested in this repository, what is skipped, and why. Updated as the
v0.9 work lands; the gate evidence below was rescued from the scratch logs at
the workspace root before those were deleted by the v0.9 cleanup
(`docs/cleanup-plan.md`).

## Rust gates (current)

| Gate | Command | Last known result |
| --- | --- | --- |
| Unit/integration tests | `cargo test` | **272 passed, 0 failed, 1 ignored** — re-run green after the v0.9 cleanup (post-delete gate, branch `chore/cleanup`) |
| Release build | `cargo build --release` | Green after the cleanup (5 m 32 s from an empty `target/`) |
| Ignored test | `convert::jobs::live_external_engines_convert_real_files` | Needs live engines in `%LOCALAPPDATA%\Filee\engines`; run manually when engines are installed |
| Clippy | `cargo clippy --all-targets -- -D warnings` | Clean at the v0.8.0 release; re-run after v0.9 changes |
| Format | `cargo fmt --check` | Clean at the v0.8.0 release; re-run after v0.9 changes |

Toolchain note: builds must use the WinLibs gcc — a broken `C:\MinGW\bin\gcc.exe`
shadows it. `tools\build.ps1` and `tools\cargo-winlibs.cmd` prepend the WinLibs
`mingw64\bin` to `PATH` automatically.

## Engine golden suite

`TI_ENGINES_DIR=%LOCALAPPDATA%\Filee\engines cargo test convert::golden` —
**13 passed, 22 skipped, 0 failed** at the v0.8.0 release. The 22 skips are the
rows whose engine payload is absent from this machine; they print a SKIP line,
they are not silent. The per-row table (missing payload → skipped rows) lives in
`docs/release/v0.8.0/test-report.md` §3 and will be restated here per the v0.9
brief §1 ("List each skipped row with the reason in docs/testing.md").

## Release-gate evidence (v0.8.0, quoted from the deleted scratch logs)

From `rv8-*` (release worktree `D:\ti-release-v08`, 2026-11-08/09):

- `cargo fmt --check` → one diff found in `src/main.rs` (`installer_version`
  concat), fixed by `cargo fmt` (rv8-fmt-apply: clean).
- `cargo clippy --all-targets -- -D warnings` → `Finished dev profile in 4.03s`,
  no diagnostics (rv8-clippy-err.txt).
- `cargo build --release` → `Finished release profile in 39.73s`
  (rv8-release-err.txt).
- `cargo test` → **187 passed, 0 failed, 1 ignored** (rv8-test-out.txt), then
  **188 passed, 0 failed, 1 ignored** after the last fix (rv8-test2-out.txt).

From the main-tree logs (superseded by the current 272-test baseline):

- `build-err.txt` — v0.7.0 dev build with 139 warnings (pre-burn-down).
- `clippy-err.txt` — the pre-burn-down state: 182 clippy errors (debt later
  burned to zero in commit `393f52c`).
- `test-out.txt` / `final-test-out.txt` — 187 then **188 passed, 0 failed,
  1 ignored**.

These logs were deleted because every number in them is reproduced by running
the gates again; the quoted results above keep the historical record.

## Not tested (v0.8.0 release, carried into v0.9)

Stated in `docs/release/v0.8.0/test-report.md` and repeated here so it is not
lost:

- A real incoming/outgoing Phone Link call (no phone was linked; the spike
  returned NO-GO). v0.9 §3 replaces the detector — until a real call log exists,
  call support is **not claimed**.
- Clean-VM install (Windows Sandbox absent on this machine).
- DPI 100/125/150 light+dark and auto-hide were run for v0.8.0 and must be
  re-run after the v0.9 UI changes.
