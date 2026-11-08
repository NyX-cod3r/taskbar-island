; TaskbarIsland 0.7.0 — per-user installer (Inno Setup 6.7+).
; Build with: .\scripts\build.ps1 Installer [-EnginesDir <dir>]
;
; The engine payloads are shipped inside this setup and copied under
; {app}\engines only for the components the user selects. The app probes
; that folder at startup (see engines::engines_dir), so no extra registry
; state is ever needed to make conversions work.

#ifndef EngineRoot
  #error Pass /DEngineRoot=<engines dir> to ISCC (build.ps1 does this).
#endif

#define MyAppName "TaskbarIsland"
#define MyAppVersion "0.7.0"
#define MyAppPublisher "TaskbarIsland"
#define MyAppExeName "taskbar-island.exe"
#define SetupCopyName "TaskbarIsland-Setup.exe"

; Where this setup looks for a newer build, plus the local name the payload
; gets while it sits in {tmp}. See update-config.iss and update.iss.
#include "update-config.iss"

[Setup]
AppId={{B1E4F6D3-3C13-4CB8-8A9B-6D7F2B1F9D0A}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppCopyright=Copyright (C) 2026 {#MyAppPublisher}
VersionInfoCompany={#MyAppPublisher}
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

[Types]
Name: "full"; Description: "Full installation"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

; The full installation selects every engine. Modify mode deselects the ones
; the user removed last time from [Code]; sizes come from [Files].
[Components]
Name: "core"; Description: "{#MyAppName} (required)"; Types: full custom; Flags: fixed
Name: "ffmpeg"; Description: "FFmpeg — video, audio and AVIF conversion"; Types: full custom
Name: "pandoc"; Description: "Pandoc — document conversion"; Types: full custom
Name: "tesseract"; Description: "Tesseract — text from images"; Types: full custom
#include "generated\engine-langs.iss"

[Files]
; The AfterInstall hook re-copies the freshly downloaded GitHub payload over
; this file once it lands, so the newest build is on disk before [Run starts
; the app. With no update pending the hook is a no-op (see update.iss).
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion; Components: core; AfterInstall: ApplyUpdatePayload
Source: "{#EngineRoot}\ffmpeg\*"; DestDir: "{app}\engines\ffmpeg"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: ffmpeg
Source: "{#EngineRoot}\pandoc\*"; DestDir: "{app}\engines\pandoc"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: pandoc
; OCR language data is added per component by the generated include.
Source: "{#EngineRoot}\tesseract\*"; DestDir: "{app}\engines\tesseract"; Excludes: "tessdata\*"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: tesseract
#include "generated\engine-lang-files.iss"

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Comment: "Dynamic Island-style companion for the Windows taskbar"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "TaskbarIsland"; ValueData: """{app}\{#MyAppExeName}"""; Tasks: startup
Root: HKCU; Subkey: "Software\TaskbarIsland\Engines"; Flags: uninsdeletekey

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Messages]
WelcomeLabel2=This will install [name/ver] for Windows 11.%n%nTaskbarIsland lives beside your system tray as a compact pill — no main window, no services, no network. Pick the local engines you want for conversions below; you can add or remove them later from the tray menu.
FinishedLabel={#MyAppName} is ready. Look for the island beside your system tray, and right-click its tray icon for startup options and Exit.
FinishedLabelNoIcons={#MyAppName} is ready. Look for the island beside your system tray.

[Code]
const
  ENGINES_KEY = 'Software\TaskbarIsland\Engines';

#include "generated\engine-lang-code.iss"
#include "update.iss"

{ Kept next to the app so the tray's "Manage engines" entry can rerun this
  setup in modify mode. Running from that copy skips the self-copy. }
procedure KeepSetupCopy;
var
  Source, Target: String;
begin
  Source := ExpandConstant('{srcexe}');
  Target := ExpandConstant('{app}\{#SetupCopyName}');
  if CompareText(Source, Target) <> 0 then
    if not CopyFile(Source, Target, False) then
      Log('Manage engines: could not keep a setup copy at ' + Target);
end;

procedure SelectEngine(Name: String);
var
  V: Cardinal;
begin
  if RegQueryDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, Name, V) and (V <> 0) then
    WizardSelectComponents(Name)
  else
    WizardSelectComponents('!' + Name);
end;

procedure PreselectEngines;
begin
  { Fresh install: every engine is already on. Modify: restore the last choice. }
  if not RegValueExists(HKEY_CURRENT_USER, ENGINES_KEY, 'seeded') then
    PreselectLanguages(False)
  else
  begin
    SelectEngine('ffmpeg');
    SelectEngine('pandoc');
    SelectEngine('tesseract');
    if WizardIsComponentSelected('tesseract') then
      PreselectLanguages(True);
  end;
end;

procedure RecordEngines;
begin
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'seeded', 1);
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'ffmpeg', Ord(WizardIsComponentSelected('ffmpeg')));
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'pandoc', Ord(WizardIsComponentSelected('pandoc')));
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'tesseract', Ord(WizardIsComponentSelected('tesseract')));
  RecordLanguageSelection;
end;

procedure RemoveDeselectedEngines;
var
  Root: String;
begin
  Root := ExpandConstant('{app}\engines');
  if not WizardIsComponentSelected('ffmpeg') then
    DelTree(Root + '\ffmpeg', True, True, True);
  if not WizardIsComponentSelected('pandoc') then
    DelTree(Root + '\pandoc', True, True, True);
  if WizardIsComponentSelected('tesseract') then
    RemoveDeselectedLanguages
  else
    DelTree(Root + '\tesseract', True, True, True);
  { Empty folder after removing the last engine; a no-op otherwise. }
  RemoveDir(Root);
end;

procedure InitializeWizard;
begin
  PreselectEngines;
  { An upgrade where autostart is already on should present it that way. }
  if RegValueExists(HKEY_CURRENT_USER,
      'Software\Microsoft\Windows\CurrentVersion\Run', 'TaskbarIsland') then
    WizardSelectTasks('startup');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    KeepSetupCopy;
    RecordEngines;
    RemoveDeselectedEngines;
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
