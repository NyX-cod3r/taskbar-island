; Isolated harness for scripts\update.iss.
;
; It installs a dummy payload into a throwaway directory under its own AppId,
; so the real TaskbarIsland install, its registry entries and a running app
; are never touched. Driven by scripts\tests\update-smoke.ps1, which stands up
; a local HTTP server for the manifest and the payload.
;
; Build (the manifest URL is supplied by the runner):
;   ISCC.exe /DUpdateManifestUrl=http://127.0.0.1:18742/manifest.txt update-smoke.iss

#define MyAppName "TaskbarIslandUpdateSmoke"
#define MyAppVersion "1.0.0"
#define MyAppPublisher "TaskbarIsland"
#define MyAppExeName "app.txt"
#include "..\update-config.iss"

[Setup]
AppId={{4E7C6A5B-0E22-4A5C-9C4E-7A1B2D3E4F50}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
VersionInfoProductName={#MyAppName}
VersionInfoVersion={#MyAppVersion}
DefaultDirName={localappdata}\TaskbarIslandUpdateSmoke
DisableDirPage=no
DisableProgramGroupPage=yes
DisableWelcomePage=yes
PrivilegesRequired=lowest
OutputDir=..\..\dist
OutputBaseFilename=update-smoke
Compression=none
SolidCompression=no
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Types]
Name: "full"; Description: "Full installation"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

[Components]
Name: "core"; Description: "core"; Types: full custom; Flags: fixed

[Files]
; Mirrors the real installer: the embedded build is copied first, then the
; AfterInstall hook overwrites it with the downloaded payload when one exists.
Source: "embedded.txt"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion; Components: core; AfterInstall: ApplyUpdatePayload

[Code]
#include "..\update.iss"
