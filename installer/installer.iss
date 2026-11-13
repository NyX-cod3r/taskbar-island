; TaskbarIsland - core per-user installer (Inno Setup 6.7+).
; Build with: .\tools\build.ps1 Installer
;
; This script ships ONLY TaskbarIsland's own files: the executable plus the
; license notices. The third-party conversion engines live in the separate,
; optional converters pack (installer\converters.iss) and are never packed
; here, so the artifact that gets code-signed contains nothing but our own
; binaries (docs/signing.md).
;
; The product name and version are not written in this file: build.ps1 reads
; them from Cargo.toml (the single source of truth) and passes them as
; /DMyAppName and /DMyAppVersion. CI fails a release tag that disagrees with
; Cargo.toml (.github/workflows/release.yml).

#ifndef MyAppName
  #error Pass /DMyAppName=<product name> to ISCC (tools\build.ps1 reads it from Cargo.toml).
#endif
#ifndef MyAppVersion
  #error Pass /DMyAppVersion=<x.y.z> to ISCC (tools\build.ps1 reads it from Cargo.toml).
#endif

#define MyAppExeName "taskbar-island.exe"

; Where this setup looks for a newer build, plus the local name the payload
; gets while it sits in {tmp}. See update-config.iss and update.iss.
#include "update-config.iss"

; The exe path is a define so CI can point at target\<triple>\release\ when it
; builds with an explicit --target (CARGO_BUILD_TARGET). The default is the
; plain host build tools\build.ps1 produces.
#ifndef AppExePath
  #define AppExePath "..\target\release\{#MyAppExeName}"
#endif

[Setup]
AppId={{B1E4F6D3-3C13-4CB8-8A9B-6D7F2B1F9D0A}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppName}
AppCopyright=Copyright (C) 2026 {#MyAppName}
VersionInfoCompany={#MyAppName}
VersionInfoDescription={#MyAppName} Setup
VersionInfoProductName={#MyAppName}
VersionInfoVersion={#MyAppVersion}
DefaultDirName={localappdata}\Programs\TaskbarIsland
DefaultGroupName={#MyAppName}
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableWelcomePage=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
AppMutex=TaskbarIsland.SingleInstance
CloseApplications=yes
RestartApplications=no
ChangesAssociations=no
ChangesEnvironment=no
UsePreviousAppDir=yes
OutputDir=..\dist
OutputBaseFilename=TaskbarIsland-Setup
SetupIconFile=..\assets\app.ico
WizardStyle=modern dynamic
WizardImageFile=..\assets\wizard.png
WizardImageFileDynamicDark=..\assets\wizard.png
WizardSmallImageFile=..\assets\wizard-small.png
WizardSmallImageFileDynamicDark=..\assets\wizard-small.png
WizardImageStretch=yes
Compression=lzma2/max
SolidCompression=yes
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startup"; Description: "Start {#MyAppName} when I sign in to Windows"; GroupDescription: "Startup:"; Flags: checkedonce
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

; Without a [Types] section Inno selects NOTHING that carries a Types/Components
; filter (the [Files] entries and the startup task were silently skipped), so the
; core component must belong to a type explicitly.
[Types]
Name: "full"; Description: "Full installation"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

; One fixed component: there is nothing to choose in the core installer.
; The engines are components of the converters pack only.
[Components]
Name: "core"; Description: "{#MyAppName} (required)"; Types: full custom; Flags: fixed

[Files]
; The AfterInstall hook re-copies the freshly downloaded GitHub payload over
; this file once it lands, so the newest build is on disk before [Run starts
; the app. With no update pending the hook is a no-op (see update.iss).
Source: "{#AppExePath}"; DestDir: "{app}"; Flags: ignoreversion; Components: core; AfterInstall: ApplyUpdatePayload
; GPL-3.0 requires the license text to travel with the binary. The third-party
; notices travel with the converters pack, which is where the engines are.
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion; Components: core
Source: "..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion; Components: core

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Comment: "Dynamic Island-style companion for the Windows taskbar"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "TaskbarIsland"; ValueData: """{app}\{#MyAppExeName}"""; Tasks: startup
; Kept for symmetry with the converters pack: an engine selection recorded by
; an old install must not outlive the app that reads it.
Root: HKCU; Subkey: "Software\TaskbarIsland\Engines"; Flags: uninsdeletekey

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Messages]
WelcomeLabel2=This will install [name/ver] for Windows 11.%n%nTaskbarIsland lives beside your system tray as a compact pill - no main window, no services, no network. Conversions use optional local engines; you can add them later with the separate Converters pack from the releases page.
FinishedLabel={#MyAppName} is ready. Look for the island beside your system tray, and right-click its tray icon for startup options and Exit.
FinishedLabelNoIcons={#MyAppName} is ready. Look for the island beside your system tray.

[Code]
#include "update.iss"

{ App-level events only: the engine components, their file lists and the
  "add or remove engines later" flow live in the converters pack
  (installer\converters.iss + installer\engines.iss). }

procedure InitializeWizard;
begin
  { An upgrade where autostart is already on should present it that way. }
  if RegValueExists(HKEY_CURRENT_USER,
      'Software\Microsoft\Windows\CurrentVersion\Run', 'TaskbarIsland') then
    WizardSelectTasks('startup');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    { An upgrade with the task unchecked must not keep an old autostart entry. }
    if not WizardIsTaskSelected('startup') then
      RegDeleteValue(HKEY_CURRENT_USER,
        'Software\Microsoft\Windows\CurrentVersion\Run', 'TaskbarIsland');
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RegDeleteValue(HKEY_CURRENT_USER,
      'Software\Microsoft\Windows\CurrentVersion\Run', 'TaskbarIsland');
end;
