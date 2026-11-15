# Privacy policy

TaskbarIsland is free, open-source software (GPL-3.0). It runs entirely on your PC. It collects nothing and sends nothing to any server.

## What the app does not do

- No telemetry, no analytics, no crash reporting, no update check, no remote logging.
- The app itself makes no network connections. File conversion runs offline with local engines. The optional converters pack, if you choose to install it, is a separate download that you start yourself from the GitHub Releases page.

## What the app reads, and what it keeps

- **Clipboard history:** kept only on this PC, at most 30 items for at most 7 days. You can pause or clear it at any time. The app respects the flags that password managers set on copied items and does not store those items.
- **Media info:** title, artist and app name are read from Windows to show on the pill. They are not saved.
- **Calls:** the app reads Phone Link notification and window text on this PC only to show who is calling. Caller names and numbers stay in memory during the call and are not saved. Diagnostic logs, if you enable them, mask phone numbers (for example +00 00•• ••• 000).
- **File shelf:** the app keeps only a list of file paths. Your files stay where they are.

## Where data lives

Everything stays inside two folders for your Windows user, both named
`TaskbarIsland`:

- `%APPDATA%\TaskbarIsland` (Roaming) — `settings.json` (your preferences).
- `%LOCALAPPDATA%\TaskbarIsland` (Local) — `clipboard\` (the encrypted
  clipboard history), `shelf.json` (the file shelf's list of paths) and, only
  if you turn diagnostics on, `diag\` (call logs).

Nothing is written anywhere else, and nothing leaves the PC.

## Deleting your data

Uninstall TaskbarIsland from Windows Settings (Apps), then delete the two
folders above if you want to remove everything.

## Contact

Open an issue: https://github.com/NyX-cod3r/taskbar-island/issues
