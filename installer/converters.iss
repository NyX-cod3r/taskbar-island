; TaskbarIsland Converters pack - optional installer for the third-party
; conversion engines (spec 1: "core installer + optional Converters pack").
; Build with: .\tools\build.ps1 Converters [-EnginesDir <dir>]
;
; This pack redistributes other people's programs (FFmpeg, Pandoc, Tesseract,
; ...), each under its own license - see THIRD_PARTY_LICENSES.md, which ships
; with it. It is deliberately a SEPARATE artifact from the core installer and
; is never code-signed under the TaskbarIsland name (docs/signing.md): only
; our own binaries go into the signed core installer.
;
; It installs into the same {app} folder as the app so engines\ sits next to
; taskbar-island.exe (src/engines probes that folder at startup), keeps a copy
; of itself there for the tray's "Manage engines..." entry, and records the
; user's choices under HKCU\Software\TaskbarIsland\Engines so a modify run
; restores them.

#ifndef EngineRoot
  #error Pass /DEngineRoot=<engines dir> to ISCC (tools\build.ps1 Converters does this).
#endif
#ifndef MyAppName
  #error Pass /DMyAppName=<product name> to ISCC (tools\build.ps1 reads it from Cargo.toml).
#endif
#ifndef MyAppVersion
  #error Pass /DMyAppVersion=<x.y.z> to ISCC (tools\build.ps1 reads it from Cargo.toml).
#endif

#define MyAppExeName "taskbar-island.exe"
; The name the tray's "Manage engines..." entry launches (src/tray.rs).
#define SetupCopyName "TaskbarIsland-Converters-Setup.exe"

; The engines never re-fetch themselves over the network (update.iss applies
; to the app payload inside the core installer only).

[Setup]
; Deliberately a different AppId from the core installer: two independent
; entries in Apps & Features, each uninstalling only what it installed.
AppId={{6E1F0C7B-4A52-4D9E-9C31-8B7A2D5E4F06}
AppName={#MyAppName} Converters
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} Converters {#MyAppVersion}
AppPublisher={#MyAppName}
AppCopyright=Copyright (C) 2026 {#MyAppName}
VersionInfoCompany={#MyAppName}
VersionInfoDescription={#MyAppName} Converters pack Setup
VersionInfoProductName={#MyAppName} Converters
VersionInfoVersion={#MyAppVersion}
; Fixed to the app's folder: engines anywhere else would not be found by the
; app's own resolver (see docs/engines.md), so there is no directory page.
DefaultDirName={localappdata}\Programs\TaskbarIsland
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableWelcomePage=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
UsePreviousAppDir=yes
OutputDir=..\dist
OutputBaseFilename=TaskbarIsland-Converters-Setup
SetupIconFile=..\assets\app.ico
WizardStyle=modern dynamic
WizardImageFile=..\assets\wizard.png
WizardImageFileDynamicDark=..\assets\wizard.png
WizardSmallImageFile=..\assets\wizard-small.png
WizardSmallImageFileDynamicDark=..\assets\wizard-small.png
WizardImageStretch=yes
Compression=lzma2/max
SolidCompression=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Types]
Name: "full"; Description: "Full installation"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

; Everything engine-related lives in installer\engines.iss: components, file
; lists, per-engine sizes, OCR languages and the modify flow. This script
; owns the pack's shell around it.
[Components]
#include "engines.iss"

[Registry]
Root: HKCU; Subkey: "Software\TaskbarIsland\Engines"; Flags: uninsdeletekey

[UninstallDelete]
; The engine payload and the self-copy are not all in the uninstall log
; (the copy is made at runtime), so sweep them explicitly.
Type: filesandordirs; Name: "{app}\engines"
Type: files; Name: "{app}\{#SetupCopyName}"

[Messages]
WelcomeLabel2=This installs the optional conversion engines for [name/ver].%n%nThe engines are separate local programs (FFmpeg, Pandoc, Tesseract and others), each under its own license - see THIRD_PARTY_LICENSES.md. They never connect to the network. TaskbarIsland itself works fully without them.
FinishedLabel=The engines are installed next to TaskbarIsland. Conversions are available right away.
FinishedLabelNoIcons=The engines are installed next to TaskbarIsland.

[Code]
procedure InitializeWizard;
begin
  PreselectEngines;
end;

{ A pack without the app cannot convert anything: offer to stop rather than
  leave engines nobody will run. }
function InitializeSetup: Boolean;
begin
  Result := True;
  if not FileExists(ExpandConstant('{localappdata}\Programs\TaskbarIsland\{#MyAppExeName}')) then
    if MsgBox('{#MyAppName} is not installed yet.' + #13#10 + #13#10 +
        'Install the app first, then run this pack again. Continue anyway?',
        mbConfirmation, MB_YESNO) = IDNO then
      Result := False;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    { Keep a copy beside the app for the tray's "Manage engines..." entry,
      record the selection for the next modify run, and drop engines the user
      unchecked (see engines.iss). }
    KeepSetupCopy;
    RecordEngines;
    RemoveDeselectedEngines;
  end;
end;
