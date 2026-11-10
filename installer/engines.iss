; TaskbarIsland — engine installer include fragment (spec §6, agent B).
;
; scripts\installer.iss pulls this file in once, from its [Components]
; section. The [Files] and [Code] headers below switch sections on
; purpose: ISCC processes includes inline, so a section opened here is
; continued (and reopened later by the main script) without conflict —
; verified against Inno Setup 6.7.3.
;
; Everything engine-related lives here: components, payload file lists,
; per-engine disk sizes and the "add or remove engines later" modify
; flow. The main script owns Core only.
;
; Which engines exist is decided at build time from the payload folders
; under {#EngineRoot}: scripts\engine-includes.ps1 writes
; scripts\generated\engine-sizes.iss (sizes for all seven engines plus a
; Have* define for each of the four new ones it finds, probing by
; executable name anywhere in the engine folder so a portable zip with
; an extra wrapper directory still counts). An engine whose payload is
; missing is not offered, not packed, and never touched by the modify
; flow — so a build made without it can't delete an earlier install of
; it either.
;
; Owner decisions baked in (2026-11-10): LibreOffice is the largest
; engine and starts UNTICKED on every fresh install (opt-in checkbox,
; clearly sized); Ghostscript ships despite its AGPL-3.0 license —
; see THIRD_PARTY_NOTICES.md.

#include "..\scripts\generated\engine-sizes.iss"

; ---------------------------------------------------------- [Components]
; The full type selects every engine except LibreOffice, which has no
; Types entry at all and therefore starts unticked in every type. The
; modify flow restores or keeps the user's own choice from the registry.
Name: "ffmpeg"; Description: "FFmpeg — video, audio and AVIF conversion ({#EngineSizeFfmpeg})"; Types: full custom
Name: "pandoc"; Description: "Pandoc — document conversion ({#EngineSizePandoc})"; Types: full custom
Name: "tesseract"; Description: "Tesseract — text from images ({#EngineSizeTesseract})"; Types: full custom
#ifdef HavePoppler
Name: "poppler"; Description: "Poppler — PDF text, pages and images ({#EngineSizePoppler})"; Types: full custom
#endif
#ifdef HaveQpdf
Name: "qpdf"; Description: "qpdf — rotate, optimize and merge PDFs ({#EngineSizeQpdf})"; Types: full custom
#endif
#ifdef HaveGhostscript
Name: "ghostscript"; Description: "Ghostscript — PDF compression ({#EngineSizeGhostscript})"; Types: full custom
#endif
#ifdef HaveLibreOffice
Name: "libreoffice"; Description: "LibreOffice — Office and PDF document conversion ({#EngineSizeLibreOffice}, largest engine)"
#endif
; OCR language data: English ticked, every other language opt-in
; (generated from tesseract\tessdata, not assumed).
#include "..\scripts\generated\engine-langs.iss"

[Files]
; The whole engine folder ships as-is, so the on-disk layout the resolver
; probed at build time is exactly what {app}\engines\<name> gets.
; THIRD_PARTY_NOTICES.md always travels with the install: it names every
; engine, its license and where its source is (incl. Ghostscript's AGPL).
Source: "..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#EngineRoot}\ffmpeg\*"; DestDir: "{app}\engines\ffmpeg"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: ffmpeg
Source: "{#EngineRoot}\pandoc\*"; DestDir: "{app}\engines\pandoc"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: pandoc
Source: "{#EngineRoot}\tesseract\*"; DestDir: "{app}\engines\tesseract"; Excludes: "tessdata\*"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: tesseract
#include "..\scripts\generated\engine-lang-files.iss"
#ifdef HavePoppler
Source: "{#EngineRoot}\poppler\*"; DestDir: "{app}\engines\poppler"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: poppler
#endif
#ifdef HaveQpdf
Source: "{#EngineRoot}\qpdf\*"; DestDir: "{app}\engines\qpdf"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: qpdf
#endif
#ifdef HaveGhostscript
Source: "{#EngineRoot}\ghostscript\*"; DestDir: "{app}\engines\ghostscript"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: ghostscript
#endif
#ifdef HaveLibreOffice
Source: "{#EngineRoot}\libreoffice\*"; DestDir: "{app}\engines\libreoffice"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: libreoffice
#endif

[Code]
{ ENGINES_KEY first: the generated language code below reads and writes
  it. }
const
  ENGINES_KEY = 'Software\TaskbarIsland\Engines';

#include "..\scripts\generated\engine-lang-code.iss"

{ A fresh install keeps the type defaults above (LibreOffice off). A
  modify run — the tray's "Manage engines..." entry reruns this setup
  from its copy beside the app — restores exactly what the user picked
  last time. Engines this build does not ship are left alone: no
  selection is recorded and no folder is removed for them. }

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
  if not RegValueExists(HKEY_CURRENT_USER, ENGINES_KEY, 'seeded') then
    PreselectLanguages(False)
  else
  begin
    SelectEngine('ffmpeg');
    SelectEngine('pandoc');
    SelectEngine('tesseract');
    if WizardIsComponentSelected('tesseract') then
      PreselectLanguages(True);
#ifdef HavePoppler
    SelectEngine('poppler');
#endif
#ifdef HaveQpdf
    SelectEngine('qpdf');
#endif
#ifdef HaveGhostscript
    SelectEngine('ghostscript');
#endif
#ifdef HaveLibreOffice
    SelectEngine('libreoffice');
#endif
  end;
end;

procedure RecordEngines;
begin
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'seeded', 1);
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'ffmpeg', Ord(WizardIsComponentSelected('ffmpeg')));
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'pandoc', Ord(WizardIsComponentSelected('pandoc')));
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'tesseract', Ord(WizardIsComponentSelected('tesseract')));
  RecordLanguageSelection;
#ifdef HavePoppler
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'poppler', Ord(WizardIsComponentSelected('poppler')));
#endif
#ifdef HaveQpdf
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'qpdf', Ord(WizardIsComponentSelected('qpdf')));
#endif
#ifdef HaveGhostscript
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'ghostscript', Ord(WizardIsComponentSelected('ghostscript')));
#endif
#ifdef HaveLibreOffice
  RegWriteDWordValue(HKEY_CURRENT_USER, ENGINES_KEY, 'libreoffice', Ord(WizardIsComponentSelected('libreoffice')));
#endif
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
#ifdef HavePoppler
  if not WizardIsComponentSelected('poppler') then
    DelTree(Root + '\poppler', True, True, True);
#endif
#ifdef HaveQpdf
  if not WizardIsComponentSelected('qpdf') then
    DelTree(Root + '\qpdf', True, True, True);
#endif
#ifdef HaveGhostscript
  if not WizardIsComponentSelected('ghostscript') then
    DelTree(Root + '\ghostscript', True, True, True);
#endif
#ifdef HaveLibreOffice
  if not WizardIsComponentSelected('libreoffice') then
    DelTree(Root + '\libreoffice', True, True, True);
#endif
  { Empty folder after removing the last engine; a no-op otherwise. }
  RemoveDir(Root);
end;

{ Kept next to the app so the tray's "Manage engines..." entry can rerun
  this setup in modify mode. Running from that copy skips the self-copy. }
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
