# TaskbarIsland

A Dynamic-Island-style pill that lives on the Windows 11 taskbar: the current
media session, an active call, a file shelf, a countdown timer and the
clipboard history — one compact, always-identical pill beside the tray, with a
flyout for the details.

Native Rust + `windows-rs`. DirectComposition, Direct2D, DirectWrite. No
webview, no Electron, no WinUI, no services, no background processes.

- **Event-driven.** No polling loops shorter than a second; when nothing is
  live the pill is hidden and the process sits at ~0 % CPU.
- **Offline by design.** No telemetry, no update check, no network calls of
  any kind. See [PRIVACY.md](PRIVACY.md).
- **Same shell everywhere.** Every activity uses the same pill and the same
  flyout size; only the inner layout changes.

## Install

Download `TaskbarIsland-Setup.exe` from the
[releases page](https://github.com/NyX-cod3r/taskbar-island/releases) and run
it. It is a per-user installer (no admin prompt) that installs to
`%LOCALAPPDATA%\Programs\TaskbarIsland`.

Two separate downloads:

| Artifact | Contents | Signed |
| --- | --- | --- |
| `TaskbarIsland-Setup.exe` | TaskbarIsland's own executable + license notices. Nothing else. | Yes — this is the artifact code signing applies to |
| `TaskbarIsland-Converters-Setup.exe` | The optional conversion engines (FFmpeg, Pandoc, Tesseract, …), each under its own license. | No — third-party engines are never signed under our name |

The app works fully without the converters pack; it only adds document/image
conversion. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

### About the SmartScreen warning

Until the release is code-signed (see below), Windows SmartScreen may show
"Windows protected your PC" when you run the downloaded installer. That
dialog means *this file is not yet known to Microsoft*, not that anything is
wrong with it: check the SHA-256 on the release page if you want to verify the
file, then choose **More info → Run anyway**. The warning disappears for
signed builds.

## Build from source

Prerequisites on Windows:

- Rust (`rustup`), host target `x86_64-pc-windows-gnu`
- A MinGW-w64 toolchain providing `gcc`, `windres` and `dlltool`
  (WinLibs works; `build.ps1` finds it locally or uses whatever is on `PATH`)
- [Inno Setup 6](https://jrsoftware.org/isinfo.php) for the installers

```powershell
.\tools\build.ps1 -Mode Debug        # debug build
.\tools\build.ps1 -Mode Release      # release build + resource/manifest gate
.\tools\build.ps1 -Mode Test         # unit tests
.\tools\build.ps1 -Mode Installer    # core installer  -> dist\TaskbarIsland-Setup.exe
.\tools\build.ps1 -Mode Converters   # engines pack    -> dist\TaskbarIsland-Converters-Setup.exe
```

`-Mode Installer` and `-Mode Release` fail unless the built executable carries
exactly one Windows manifest and it is *our* fusion manifest (this guards
against a toolchain manifest conflict, see `docs/signing.md`).

Verify on the real install after every change:

```powershell
powershell -File tools\tests\install-smoke.ps1
```

It stops the running island, installs `dist\TaskbarIsland-Setup.exe` over the
per-user install, byte-matches the installed executable against
`target\release`, launches it and checks the pill/tray windows and
single-instance behaviour. `cargo test` alone is not evidence that a change
works.

The product name and version live in exactly one place — `Cargo.toml`
(`version`, `[package.metadata] product-name`). `build.rs` renders them into
the executable's VERSIONINFO, `tools\build.ps1` passes them to Inno Setup, and
CI refuses a tag that disagrees with `Cargo.toml`.

## Code signing policy

**Status: preparing. Nothing in this repository is signed yet, and no
certificate has been issued.**

### Roles

| Role | Who | What they do |
| --- | --- | --- |
| **Author** | the contributor (an agent or a human) who writes the change | Opens the change with evidence: tests, `install-smoke.ps1`, measurements. Never signs, never approves their own work. |
| **Reviewer** | a second pair of eyes | Checks the change against `AGENTS.md`, the tests and the install smoke run. Reviews the release build before it is tagged. |
| **Approver** | **Satyam** (project owner) | Sole approver of a release and of anything that changes what gets signed. Approves the signing request itself in SignPath after the CI build exists. |

Only the approver can publish a release or approve a signing request. CI never
holds a signing key and never publishes on its own: it produces a **draft**
release and stops.

### Privacy statement

TaskbarIsland does not send any data over the network. There is no telemetry,
no analytics, no update check and no remote logging — the application makes no
outbound connections at all (see [PRIVACY.md](PRIVACY.md)). Code signing exists
solely so Windows can verify *who built the file*; it does not add, enable or
license any network behaviour.

### What gets signed

- The **core installer** and the executable inside it — our own binaries only.
- The optional **converters pack is not signed under our name**: it
  redistributes other people's programs (see
  [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)).

### SignPath Foundation

> *Pending approval — this line is added only after SignPath accepts the
> project and the first release has been signed:*
>
> **Code signing provided by the SignPath Foundation.**

Until that sentence replaces this placeholder, releases are unsigned and
SmartScreen may warn (see above). The full plan, including the exact TODO
steps that are gated on approval, is in [docs/signing.md](docs/signing.md);
the end-to-end release checklist is in
[docs/release-process.md](docs/release-process.md).

## Documentation

| Document | Contents |
| --- | --- |
| [AGENTS.md](AGENTS.md) | Architecture rules, build/verify rules, status log |
| [docs/release-process.md](docs/release-process.md) | Bump → test → merge → tag → CI draft → approve → publish |
| [docs/signing.md](docs/signing.md) | Code signing plan and the SignPath steps (TODO, after approval) |
| [docs/testing.md](docs/testing.md) | What was tested, with the evidence |
| [docs/engines.md](docs/engines.md) | The optional conversion engines |
| [PRIVACY.md](PRIVACY.md) · [LICENSE](LICENSE) | Privacy policy · GPL-3.0-or-later |

## License

GPL-3.0-or-later — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
