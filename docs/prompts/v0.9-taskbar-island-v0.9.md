# TaskbarIsland v0.9: fixes, new UI, and repo cleanup

Repo: NyX-cod3r/taskbar-island (currently v0.8.0, PR #2 merged, tag v0.8.0).
Read this whole file before touching code. Work on a new branch v0.9. Do not push to main directly.

## 0. Ground rules (unchanged)

Native Rust + windows-rs. DirectComposition + Direct2D + DirectWrite. No webview, no Electron.
Super light: idle CPU near 0, no polling loops when nothing is live.
Never duplicate the taskbar: no clock, date, calendar, notifications list.
Nothing live means the pill is hidden. No "Nothing playing" text.
No hidden gestures. No hover scaling. Hover only lifts the background.
Every pill and every flyout has the same outer size for every activity.
Colors follow Windows (light/dark, accent). Corners are Windows-style, 8 / 6 / 4 px.
Outer sizes: the design below uses pill 280 × 34 and flyout 500 × 240, measured from v0.8.0 screenshots. If design.rs already has different values, KEEP the repo's values and scale the inner layout. Do not resize the shell.

## 1. Answer to the v0.8.0 release report

Verdict: GO for a v0.8.x tag, not yet "done". Required follow-ups, in order:

Real Phone Link call test is the biggest gap. The A2 spike returned NO-GO because no phone was linked, so call detection has never been proven. Section 3 replaces it with a multi-source detector plus diagnostics. The owner will run a real incoming call, an outgoing call and a call ended from each side, and send back the diag log. Do not claim call support until that log shows all three detected.

Minute-30 handle/memory spike. "Not reproducible" is not a fix. Run a 60-minute soak with a handle and private-bytes sample every 10 s (write CSV to target/soak/). If a spike repeats, find it (likely GSMTC session churn, registry watcher re-arm, or thumbnail cache). Report the numbers.

Installer size. The ~130 MB Setup.exe is almost all conversion engines (ffmpeg, pandoc, tesseract, LibreOffice, Poppler, qpdf, Ghostscript). The exe is 3.7 MB. Split: core installer (the island only, a few MB) and an optional "Converters pack" downloaded or installed on demand. The island must work fully without the pack.

Ghostscript is AGPL. Remove it from the default bundle, or ship it only in the optional pack with a clear license notice and source offer. Prefer replacing it with a permissive alternative if the feature allows. Add THIRD_PARTY_LICENSES.md listing every bundled engine and license.

Skipped golden rows (22). List each skipped row with the reason in docs/testing.md. Either fix, or mark as an explicit known gap. Do not leave silent skips.

Clean-machine install. Windows Sandbox is absent. Test the installer in a fresh Windows 11 VM (Hyper-V or VirtualBox), including uninstall and upgrade from v0.8.0. If no VM is possible, say so plainly.

Unsigned installer. Signing is optional. Document the SmartScreen warning in the README. Do not buy or fake a certificate.

Keep the DPI results (100, 125, 150 percent, light and dark, auto-hide). Re-run them after the UI changes below.

## 2. Bug: clipboard image missing from the Images filter

Symptom: Picture1.png copied as a file shows under All but not under Images. A screenshot shows in both.
Cause to check: items are classified by clipboard format. A screenshot arrives as bitmap (CF_DIB/CF_DIBV5/PNG). A copied file arrives as CF_HDROP and is typed as a file or text.
Fix:

Classify by content, not only by format. For CF_HDROP, check each path's extension (png, jpg, jpeg, gif, bmp, webp, tif, tiff, heic, ico) and magic bytes. If it is an image, the item type is Image with source = File.
Keep one item per clipboard change. It appears in All and in Images.
Load a real thumbnail with WIC (IWICImagingFactory) decoded to a 128 × 80 bitmap, cached on disk or in memory, off the UI thread. Fall back to a generic image glyph if decoding fails.
Row layout: 50 px high, 64 × 40 thumbnail (radius 4, 1 px white 10% border), title, meta line PNG · 1280 × 720 · 214 KB. Screenshots are titled with their size (615 × 322) and meta PNG · 47 KB · screenshot. File-sourced images show their file name and a small FILE badge at the right.
The counter stays n / 30. Filters: All, Text, Images, Links, Pinned. The Images count must include file images.
Tests: copy a PNG file, copy a screenshot, copy a JPG file, copy a text file named a.png.txt (must stay text), copy a corrupt .png (shows glyph, no crash).

## 3. Bug: Phone Link calls not shown

Symptom: the Windows toast for an incoming call appears, but the island shows nothing. Outgoing calls made in Phone Link are not shown either, even after opening Phone Link from the island button.
The v0.8 detector (mic consent-store watcher) only fires when the microphone is open, and Phone Link routes call audio through the phone and a Bluetooth HFP link, so it often never opens the PC microphone. Replace it with several sources feeding one CallState machine. Each source is a module with a trait CallSource; all send events to the call state machine.

State machine: None -> Incoming -> Active -> Ended, and None -> Dialing -> Active -> Ended. Ended returns to None after 3 s. Active timer starts when Active begins. A call is the same call if caller text and source match within 5 s.

Source A: UI Automation toast listener (primary).

Register a UIA event handler with IUIAutomation::AddAutomationEventHandler for UIA_Window_WindowOpenedEventId on the root element (once, on a dedicated MTA thread).
Toasts are windows of class Windows.UI.Core.CoreWindow or XamlExplorerHostIslandWindow owned by ShellExperienceHost/explorer. When one opens, read its descendants' Name strings.
Detect a Phone Link call when text contains the app name Phone Link (also Your Phone, localized names read from the YourPhone app package display name at runtime) and call words (Incoming call, Calling, Answer, Decline) or has buttons named Answer/Decline. Take the caller name from the first non-chrome text line.
Wrap all UIA calls with a 500 ms timeout and never block the UI thread.

Source B: UserNotificationListener (secondary).

Windows.UI.Notifications.Management.UserNotificationListener needs package identity. Add a sparse MSIX package (AppxManifest.xml with uap3:Extension and userNotificationListener capability) registered at install time via Add-AppxPackage -Register with -ExternalLocation. Request access once; if the user denies, Source A still works.
Handle NotificationChanged. Filter on AppInfo.PackageFamilyName starting with Microsoft.YourPhone.

Source C: Phone Link window watcher.

Find the Phone Link top-level window (process PhoneExperienceHost.exe or YourPhone.exe). Using UIA, look for a button named End call or Hang up, a mute button, and a timer text \d+:\d\d. Their presence means Active. Read the caller name from the nearest large text.
This is also how outgoing calls are detected (the Dial UI turns into an in-call UI).

Source D (optional, off by default): notification database.

Read-only copy of %LOCALAPPDATA%\Microsoft\Windows\Notifications\wpndatabase.db. Only if A to C fail, behind a hidden setting call.use_wpn_db. Never write to it.

Actions (Answer, Decline, End, Mute):

No Windows API controls another app's call. Use UIA InvokePattern on the buttons of the toast (Answer, Decline) or the Phone Link window (End call, mute). If the button is not found, show a small inline message "Open Phone Link to do this" and bring Phone Link to the front. Never fake a success.

Open Phone Link button: launch with the shell URI ms-phonelink: or the AppUserModelID from shell:AppsFolder. After launch, the detector keeps watching; it must not need a restart.

Diagnostics (required, because this was never verified on a real call):

taskbar-island.exe --diag-call runs the detectors verbosely for 5 minutes and writes %LOCALAPPDATA%\TaskbarIsland\diag\call-YYYYMMDD-HHMMSS.log with each source's events and timestamps.
--dump-uia writes the UIA tree (names, class names, control types) of any new toast and of the Phone Link window, so the string matching can be fixed from real data.
--simulate-call incoming|dialing|active|end injects fake events for UI testing without a phone.
Add a unit test per source using recorded UIA dumps as fixtures.
In the final report, say exactly which sources were tested with a real call and which were not.

## 4. New: audio output picker (calls and music)

Button: an output chip with device icon, device name and a chevron. Music: 148 × 36, at the right end of the controls row. Call (active or dialing): 150 × 44, between mute and End. Clicking opens an output sheet: a panel that slides up inside the same flyout (240 ms, cubic-bezier(.2,.9,.25,1)), full flyout size, background rgb(38,38,38), padding 12 16, header (title 14 px semibold, close button 32 × 32), then 44 px rows (28 × 28 icon tile, name 13 px, sub-line 11.5 px, check mark on the current one). The flyout never changes size.

Titles: "Play on" for music, "Call audio" for calls.
Music list: every active render endpoint (IMMDeviceEnumerator, DEVICE_STATE_ACTIVE, eRender), then a final row "Cast to a device…".
Call list: the Bluetooth hands-free endpoint (name usually contains Hands-Free), PC speakers and microphone, and Phone (transfer the call back to the phone).

Implementation:

Set the default endpoint with IPolicyConfig::SetDefaultEndpoint (undocumented COM interface; define it in win/policy_config.rs with the known GUIDs CLSID_PolicyConfigClient and IPolicyConfigVista/IPolicyConfig). Set all three roles (console, multimedia, communications). Wrap it so a failure falls back to opening ms-settings:sound.
For calls, set the communications role and also the default role when the user chose the PC.
"Cast to a device…" opens ms-settings-connectabledevices:devicediscovery (Windows Cast UI). Say honestly in the UI sub-line "Phone, tablet or TV". Direct casting to a phone or tablet is not possible with a public API.
"Phone" (call transfer): UIA-invoke the Phone Link in-call button for switching audio to the phone (name varies; log it in diag). If not found, show "Open Phone Link to transfer".
Subscribe to IMMNotificationClient to refresh the list and the chip when devices appear or the default changes. No polling.
Show the chip text as the real current default device name, truncated with an ellipsis.

## 5. New: music text (two lines)

Line 1: media title, 16 px, semibold, ellipsis.
Line 2: artist, then a 12 px gap, then the app name, both 12.5 px. Artist at 78% white, app at 52% white. Single line, ellipsis.
App name: from GSMTC SourceAppUserModelId, mapped to a display name (resolve from the AppUserModelID via shell:AppsFolder, or the package display name; for browsers use the page's app, for example YouTube Music, Spotify, Chrome).
The freed space holds: the wavy progress bar, the controls row (shuffle, previous, play or pause, next, repeat), and the output chip. Art is 72 × 72, radius 8.

## 6. New: timer stepper

Remove the 5, 10 and 15 presets.
Set view: − button (56 × 56, radius 8), the big time (40 px, MM:SS, tabular numbers) with a caption N minutes · hold to repeat, and + button. Below: a full-width Start button (44 px high, amber #FFB547, dark text).
Step is 1 minute, range 1 to 180, default 15. Pressing and holding repeats: first repeat after 420 ms, then every 110 ms. Stop repeating on mouse up, leave, or capture loss (release SetCapture correctly).
Running view: ring tile (56 × 56) with progress, name and status, time left (22 px) at the right, the wavy bar with elapsed and total, then buttons: pause or resume (48 × 40, amber), −1 min, +1 min, stop (40 × 40). Pause flattens the wave in 320 ms.
The pill shows the time left. When it ends, the pill pulses amber and the flyout may open once. Remember the last duration.

## 7. Flyout layout (all tabs)

Flyout 500 × 240, radius 8, centered on the pill, bottom 60 px above the bottom edge. Fill rgb(32,32,32) at 96%, 1 px border white 10%, soft shadow.
Left rail 64 px wide with a 1 px right border (white 8%): five buttons 40 × 40 (radius 6), gap 8, top padding 10. Order: Music, Timer, Call, Shelf, Clipboard. Active: background white 10% and a 3 × 18 accent bar at the left edge. Inactive icon 62% white.
Content area: padding 16, fills the rest.
Open: morph from the pill rect, 460 ms, cubic-bezier(.2,1.1,.3,1). Close: 260 ms, cubic-bezier(.4,0,.2,1). Content fades in after 160 ms over 280 ms. Close on outside click, Esc, or 700 ms after the pointer leaves.
Pill 280 × 34, radius 8, fill white 9%, 1 px border white 10%. Contents: 26 × 26 icon tile (radius 6, accent color, dark icon) at left 4, label (12.5 px, 94% white, ellipsis), and a live value at right (12.5 px, accent color, tabular).
Accents: music album-art color (default #FF6B8A), call #4ADE80, timer #FFB547, shelf #60CDFF, clipboard #5EEAD4. Decline/End red #C42B1C.
Wavy progress: wavelength 20, amplitude 3, stroke 3, round cap, scroll 700 ms per wave, track 3 px at white 22%, thumb 4 × 18 white.

Call tab states

None: top row (56 tile, title "Call", sub "Phone Link", right "No call"), then "No active call" (20 px), then mute mic 44, mute speakers 44 and Open Phone Link (flex, green 16% fill, green text).
Incoming: the pill border turns green and pulses. Decline (red) and Answer (green), each 52 px high, flex 1, gap 10.
Dialing and Active: route line (Galaxy Buds2 Pro · Connected), then mute 44, output chip 150 × 44, End (red, flex 1). Dialing shows "Calling…".

Shelf tab: header row (56 tile, "File shelf", "3 files · 9.8 MB", Clear all), then three equal tiles 78 px high (badge, name, size), then a hint line. Drag out to move, click to open.
Clipboard tab: a 34 px header with five filter icons, a centered 9 / 30 counter, pause and clear. Rows below as in section 2. Pause stops capture; the icon switches to play.

## 8. Priority and visibility

Call > timer ending > volume HUD > music > timer running > shelf. Max two visible activities. If nothing is live, hide the pill. The rail always shows all five tabs even when empty (they are for browsing, not for the pill).

## 9. Repo cleanup and restructure

The repo has accumulated old prompts, an old design system, and many AI or testing files. Clean it, safely.

Inventory first. Create docs/cleanup-plan.md listing every top-level file and folder, its size, whether it is tracked by git, and a decision: Keep, Move, or Delete, with a one-line reason. Print the total repo size and target/ size before.

Safety rules.

Do it on branch chore/cleanup, in separate commits (move, then delete).
Never delete: source code, tests and fixtures that tests use, the LICENSE and third-party notices, installer scripts, CI config, user data folders.
If a file is ambiguous, move it to _to_delete/ at the repo root, list it in the plan, and ask the owner. Do not guess.
Keep git history. Use git mv.

Target tree.

```
/  src/                 Rust source (app, anchor.rs, ui, music, call, clipboard, shelf, timer, audio, win)
   tests/               integration tests and fixtures
   assets/              icons, sparse package manifest
   installer/           core installer and optional converters-pack scripts
   tools/               dev scripts (soak, dpi smoke, diag helpers)
   docs/
      design/            the current design spec (v0.9) and tokens
      prompts/           the current prompt only (this file)
      testing.md         what is tested, what is skipped and why
      cleanup-plan.md
      archive/           old prompts and the older design system, kept for reference
   THIRD_PARTY_LICENSES.md
   README.md
   LICENSE
   Cargo.toml
   .gitignore
```

Archive (do not delete): old prompts and the older design system go to docs/archive/ with a README line saying they are superseded.
Delete (unnecessary, free disk): target/ and other build output, bundled engine binaries that are downloaded at install time, caches, logs, soak and smoke screenshots older than the latest run, duplicate installers, temporary or scratch AI and agent files (for example agent scratch folders, session dumps, *.orig, *.rej, stray .bak), and anything generated that can be rebuilt. Add all of these patterns to .gitignore. If large binaries were committed, report them; do not rewrite history without asking.
Report before and after sizes (repo working tree, .git, target, total) and list everything removed in docs/cleanup-plan.md. After cleanup, cargo build --release and all tests must still pass.

## 10. Acceptance checklist

- [ ] Picture1.png (file) shows in All and Images with a thumbnail and a FILE badge; screenshot unchanged.
- [ ] --diag-call log exists; a real incoming call, a real outgoing call and an end are detected (owner verifies). The report states which sources were proven.
- [ ] Answer, Decline, End work through UIA, or show the honest fallback message.
- [ ] Output picker works on music and on calls; default device changes; list refreshes when a device connects.
- [ ] Music shows two lines: title, then artist, gap, app name.
- [ ] Timer uses the − and + stepper with hold-to-repeat; no fixed presets remain.
- [ ] Pill and flyout sizes identical across all activities; no hover scaling.
- [ ] Hidden pill when nothing is live.
- [ ] 60-minute soak CSV attached; no growth trend in handles or private bytes.
- [ ] Core installer is small; converters pack is optional; Ghostscript is gone from the default bundle or clearly licensed.
- [ ] DPI smoke at 100, 125, 150 percent in light and dark; auto-hide taskbar.
- [ ] Repo cleaned, tree matches section 9, sizes reported.
- [ ] Final report lists what was NOT tested.
