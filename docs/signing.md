# Code signing plan (TaskbarIsland → SignPath Foundation)

**Nothing here is implemented yet. Every step below is marked
`[AFTER APPROVAL]` and stays a TODO until SignPath accepts the project and the
owner approves it. No certificate, no key and no signing request exists today.**

## Why SignPath

TaskbarIsland is GPL-3.0 open source with no budget for a commercial
certificate. The [SignPath Foundation](https://signpath.org/) issues free
code-signing certificates for open-source projects; signing happens on
SignPath's infrastructure against an artifact we upload, so **no private key
ever touches this repository, this machine or the CI runner's disk**.

## What will be signed — and what will not

| Artifact | Signed | Reason |
| --- | --- | --- |
| `target\…\taskbar-island.exe` (and therefore the copy inside the installer and the self-update payload `taskbar-island-<ver>.exe`) | **Yes** | Our own binary; SmartScreen and the self-update both see it directly. |
| `dist\TaskbarIsland-Setup.exe` (core installer) | **Yes** | The file a user downloads. |
| `dist\TaskbarIsland-Converters-Setup.exe` (engines pack) | **No** | Redistributes other people's programs (FFmpeg, Pandoc, Tesseract, …). It is a separate download and is never signed under the TaskbarIsland name. |
| Anything else in the repository | **No** | Scripts and documents are not binaries. |

Rules that follow from that table:

- **Only our own binaries go into the signed core installer.** The core
  installer packs `taskbar-island.exe`, `LICENSE` and `NOTICE` and nothing
  else; third-party engines exist only in the unsigned converters pack.
- **No secrets in the repository.** The API token lives in GitHub's
  repository *settings* (Secrets and variables → Actions), added by the owner,
  and is read as `${{ secrets.… }}`. This file, `release.yml` and every script
  in `tools\` contain no token, no certificate and no key.
- **The SHA-256 is computed after signing.** Signing changes the bytes, so the
  `.sha256` asset must be written from the signed file — never from the
  pre-signing build.

## Roles (see README → Code signing policy)

- **Author** — writes the change, supplies the evidence, cannot approve it.
- **Reviewer** — checks the change and the release build before tagging.
- **Approver — Satyam (owner)** — the only person who can publish a release
  or approve a signing request. CI stops at a draft; SignPath stops at
  "pending approval".

## The flow once it exists

```
tag v0.9.0 pushed
  └─ CI: build exe → test → (TODO) sign exe → package installer
        → (TODO) sign installer → SHA-256 → DRAFT release + assets
             └─ SignPath: signing request PENDING  ← owner approves manually
                  └─ signed bytes + fresh SHA-256 on the draft
                       └─ owner reviews and PUBLISHES the release
```

Two approvals are always manual and always the owner's: **the SignPath
signing request** and **the GitHub release publish**.

## TODO steps — all `[AFTER APPROVAL]`

- **`[AFTER APPROVAL]` TODO-1 — Owner prerequisites (manual, no code).**
  Enable two-factor authentication on the GitHub account, publish the first
  release (`docs/release-process.md`), then apply at
  <https://signpath.org/apply> with the repository URL, the GPL-3.0 licence and
  a short description. Nothing in the repository changes for this step.

- **`[AFTER APPROVAL]` TODO-2 — SignPath project setup (manual, no code).**
  Create the project in the SignPath portal, choose the open-source
  certificate, and record the **Project ID** and the signing policy that
  requires *manual approval*. Confirm the current signing-request API and the
  official GitHub Action name from <https://help.signpath.org/> at
  implementation time — this document deliberately does not guess action
  versions.

- **`[AFTER APPROVAL]` TODO-3 — Store the token (owner, GitHub settings).**
  Add the SignPath API token as a repository secret (e.g.
  `SIGNPATH_API_TOKEN`) in *Settings → Secrets and variables → Actions*. It is
  never written into a file in this repository.

- **`[AFTER APPROVAL]` TODO-4 — Split the package step.**
  Add a `-Mode Package` to `tools\build.ps1` that runs only ISCC against an
  already-built (and already-signed) `taskbar-island.exe`, so signing can sit
  between `cargo build` and packaging. `-Mode Installer` keeps working as the
  local, unsigned path.

- **`[AFTER APPROVAL]` TODO-5 — Add the signing steps to
  `.github/workflows/release.yml`.** After *Test* and after `cargo build`, and
  again after ISCC:

  1. Submit a **signing request** for the build artifact
     (`taskbar-island.exe`, then `TaskbarIsland-Setup.exe`) with the Project ID
     and a description that includes the tag, e.g. `TaskbarIsland v0.9.0`.
  2. **Wait for the request to be approved** — it stays *Pending* until the
     approver (Satyam) approves it in the SignPath portal. The workflow polls
     for completion; it cannot approve its own request.
  3. Download the signed artifact back over the unsigned one, then re-run the
     SHA-256 step so `.sha256` matches the signed bytes.
  4. Upload everything to the **draft** release as today.

- **`[AFTER APPROVAL]` TODO-6 — Verify before publishing.**
  On the draft's assets: `Get-AuthenticodeSignature` reports `Valid`, the
  signer is the SignPath-issued certificate for TaskbarIsland, a timestamp is
  present, and a clean Windows 11 VM installs and launches the signed installer
  with no SmartScreen prompt. Record the evidence in `docs/testing.md`.

- **`[AFTER APPROVAL]` TODO-7 — Publish the SignPath line.** Replace the
  placeholder in `README.md → Code signing policy` with **"Code signing
  provided by the SignPath Foundation."**, and note the first signed release in
  `CHANGELOG.md`. Until then the README keeps the unsigned/SmartScreen
  explanation.

- **`[AFTER APPROVAL]` TODO-8 — Keep the unsigned fallback honest.** If a
  signing request is declined or times out, CI must leave the draft **unsigned
  but clearly labelled** (the SHA-256 still matches what is uploaded), never
  fail silently and never substitute a different artifact.

## Current state (2026-11-13)

- `release.yml` builds the core installer from source on a version tag, runs
  the tests, checks the tag against `Cargo.toml`, hashes the installer and
  uploads installer + SHA-256 + payload + manifest to a **draft** release. It
  contains no signing step and no secret.
- The exe's VERSIONINFO (`ProductName`, `ProductVersion`) and the installer's
  metadata are generated from `Cargo.toml`, and `version_tests` fails the build
  if a literal copy reappears anywhere.
- Nothing is signed. No certificate has been requested or issued.
