# Phone Link spike — 2026-11-08

## Scope and safety

This is a read-only feasibility spike. It made no notification, default-device,
or Phone Link changes. Caller names and numbers were neither captured nor
written to disk.

## Environment evidence

- Installed package: `Microsoft.YourPhone_1.26092.119.0_x64__8wekyb3d8bbwe`
  (status `Ok`).
- AUMID settings key exists at
  `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings\\Microsoft.YourPhone_8wekyb3d8bbwe!App`.
  It contained `LastNotificationAddedTime`; no `ShowBanner`,
  `ShowInActionCenter`, or `Enabled` value was present at inspection time.
- The notification-store file exists at
  `%LOCALAPPDATA%\\Microsoft\\Windows\\Notifications\\wpndatabase.db`
  (1 MiB). It was not copied or read because no real call was available to
  establish a schema, locking behavior, or privacy-safe query.

## Results

| Item | Decision | Evidence and implementation boundary |
| --- | --- | --- |
| Incoming-call detection | **NO-GO** | No package identity/sparse package is configured, so `UserNotificationListener` consent cannot be requested by this unpackaged app. UI Automation and the undocumented database have not been validated against a real incoming call. No release detection UI is enabled. |
| Keep information while hiding Phone Link banner | **NO-GO** | The settings key exists but the relevant values and their semantics were not proven. The application does not write this key. Any future setting needs an explicit setup-card action and restore path. |
| Answer / Decline / End | **NO-GO** | No stable UI Automation IDs were obtained or exercised while minimized, locked, closed, or after an update. The app must not advertise call control. |
| Audio | **PARTIAL** | Existing code reads the communications/default output friendly name and endpoint mute state and can toggle endpoint mute. It does not enumerate paired Bluetooth devices, switch defaults through `IPolicyConfig`, or prove the Phone Link role; those features remain disabled. |

## Provider policy

`activities::call_state::CallProvider` only updates UI state after a provider
reports an action success. The current Phone Link provider is deliberately not
connected: unsupported actions return `Unsupported`; no fake Answer, Decline,
End, caller, or toast state is exposed in release builds.

## What is required for a future GO

1. Link a real phone and collect masked samples for incoming, active, missed,
   and ended calls under a user-approved, ephemeral test run.
2. Prototype `UserNotificationListener` with package identity and document its
   consent flow and installer impact. If it cannot be packaged, validate UIA
   structures by automation IDs (never localized text).
3. Verify the banner settings values by toggling them manually in Windows,
   then implement an opt-in/restore flow only after confirming the notification
   remains observable.
4. Validate call controls and the communications role across Phone Link
   restart/minimized/locked scenarios. Until then the only safe fallback is to
   open Phone Link, not to claim a control action succeeded.
