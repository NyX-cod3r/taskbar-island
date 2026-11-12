# ready — A4 (manual smoke, auto-hide, soak, installer test)

Phase: **A4 (prompt §6)** — complete.
Branch: `agent-b/v0.8.0`.

## Evidence

- **125 % smoke: PASS.** Pill 280 x 50 px, card 500 x 240 px (400 x 192 DIP),
  6 DIP gap, centered, no clipping. Rail tiles, exact wheel cycle, tooltips
  460 ms, Shelf scroll/Clear/Convert, Clipboard pop/card, timer controls.
  Idle CPU 16 ms / 10 s.
- **100 % and 150 %: PASS.** Card 400 x 192 DIP at 100 % (gap 6 px) and
  600 x 288 px at 150 % (gap 9 px); rail-all passes at both scales.
- **Auto-hide (125 %): PASS (`AUTOHIDE-PASS`).** Pill and window region
  identical hidden/revealed, card 500 x 239 px = 400 x 191 DIP in the region,
  no UIA clipping, tooltip 473 ms, wheel cycle 0 failures, all five rail
  tiles usable while the bar is hidden. Screens 26/27/28/28b.
- **30-minute soak.** 29/30 samples flat (WS 70.1-70.7 MB, handles 441-451,
  idle CPU 31-312 ms/min). The minute-30 sample spiked to 82.8 MB / 594
  handles / 20 threads and was not reproduced by a targeted 90 s probe
  (+5-6 transient handles). Reported, not claimed leak-free.
- **Installer: PASS (44/44).** Custom `/COMPONENTS=core,ffmpeg` removes
  Pandoc/Tesseract, uninstall keeps user data, default reinstall restores all
  engines, shortcut/Run key/registry, single instance, no panics. Clean-machine
  Windows Sandbox install **not run** (Sandbox absent). Real Phone Link call
  **not run** (no linked phone; A2 NO-GO).

## Artifacts

`docs/release/v0.8.0/test-report.md`, `screens/` (40 captures),
`C:\Users\satya\AppData\Local\Temp\opencode\soak-0.8.0.csv`.
