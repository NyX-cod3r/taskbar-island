// --- TaskbarIsland installer: self-update ---------------------------------
// The setup doubles as the updater. On the Ready page it fetches a tiny text
// manifest published as an asset on the newest GitHub release and, when that
// manifest is newer than the version compiled into this file, downloads the
// matching application payload into {tmp} and verifies its SHA-256. The core
// file entry then copies that payload over {app} from its AfterInstall hook,
// so the newest build is on disk before Setup ever launches the app.
//
// Only the application's own files travel over the network. The bundled engines
// under {app}\engines are never re-fetched, and user data lives outside {app}
// entirely (%APPDATA%\TaskbarIsland and %LOCALAPPDATA%\TaskbarIsland), so
// settings, the shelf and the clipboard history survive every upgrade.
//
// Every failure path - offline, no releases published yet, a declined prompt, a
// hash mismatch - falls back to installing the payload embedded in this file,
// exactly as the installer behaved before the updater existed. The network is
// strictly best-effort, which is also why none of this ships inside the app
// binary: TaskbarIsland itself must never open a socket (AGENTS.md).
//
// Host scripts must define MyAppVersion and MyAppExeName and must include
// update-config.iss before this file.

#ifndef UpdateManifestUrl
  #error Include scripts\update-config.iss before scripts\update.iss
#endif
#ifndef MyAppVersion
  #error scripts\update.iss expects MyAppVersion to be defined by the host
#endif
#ifndef MyAppExeName
  #error scripts\update.iss expects MyAppExeName to be defined by the host
#endif

const
  MANIFEST_URL = '{#UpdateManifestUrl}';
  MANIFEST_BASENAME = 'tbi-manifest.txt';
  PAYLOAD_BASENAME = '{#UpdatePayloadBaseName}';
  { CRLF for the message boxes below. It has to hide behind a name: a source
    line whose first non-blank character is a hash is read as a preprocessor
    directive, so a literal character-code newline could never start one. }
  NL = #13#10;

var
  UpdatePage: TDownloadWizardPage;
  UpdateUrl: String;
  UpdateSha256: String;
  UpdateVersion: String;

function PayloadPath: String;
begin
  Result := ExpandConstant('{tmp}\') + PAYLOAD_BASENAME;
end;

// Progress callback. The wizard page draws its own bar and the manifest is a
// couple of hundred bytes, so there is nothing worth reporting; True simply
// lets the transfer carry on.
function UpdateDownloadProgress(const Url, FileName: String;
  const Progress, ProgressMax: Int64): Boolean;
begin
  Result := True;
end;

{ /NOUPDATE skips the network entirely, for unattended installs. }
function UpdateDisabled: Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if (CompareText(ParamStr(I), '/NOUPDATE') = 0) or
       (CompareText(ParamStr(I), '-NOUPDATE') = 0) or
       (CompareText(ParamStr(I), 'NOUPDATE') = 0) then
    begin
      Result := True;
      Exit;
    end;
end;

{ "key=value" -> the value behind Key, or '' when the key is absent. }
function ManifestValue(const Lines: TArrayOfString; const Key: String): String;
var
  I, P: Integer;
begin
  Result := '';
  for I := 0 to GetArrayLength(Lines) - 1 do
  begin
    P := Pos('=', Lines[I]);
    if (P > 1) and (CompareText(Copy(Lines[I], 1, P - 1), Key) = 0) then
    begin
      Result := Trim(Copy(Lines[I], P + 1, Length(Lines[I]) - P));
      Exit;
    end;
  end;
end;

{ StrToVersion only understands "a.b.c.d", so a short tag such as "0.7" is
  padded out first. }
function PackedVersion(const S: String; var V: Int64): Boolean;
var
  T: String;
  Dots, I: Integer;
begin
  T := Trim(S);
  Dots := 0;
  for I := 1 to Length(T) do
    if T[I] = '.' then
      Dots := Dots + 1;
  while Dots < 3 do
  begin
    T := T + '.0';
    Dots := Dots + 1;
  end;
  Result := StrToVersion(T, V);
end;

function IsNewerVersion(const Candidate, Current: String): Boolean;
var
  VCandidate, VCurrent: Int64;
begin
  Result := False;
  if not PackedVersion(Candidate, VCandidate) then
  begin
    Log('update: cannot parse version "' + Candidate + '"');
    Exit;
  end;
  if not PackedVersion(Current, VCurrent) then
  begin
    Log('update: cannot parse bundled version "' + Current + '"');
    Exit;
  end;
  Result := ComparePackedVersion(VCandidate, VCurrent) > 0;
end;

{ Fetches the release manifest. Returns False whenever the check cannot be
  completed - offline, no releases published yet, malformed manifest - and in
  every one of those cases Setup installs the embedded payload instead. }
function CheckForUpdate: Boolean;
var
  Lines: TArrayOfString;
  ManifestPath: String;
begin
  Result := False;
  UpdateUrl := '';
  UpdateSha256 := '';
  UpdateVersion := '';

  if UpdateDisabled then
  begin
    Log('update: skipped (/NOUPDATE)');
    Exit;
  end;

  try
    // Returns the byte count; the file itself lands at {tmp}\tbi-manifest.txt.
    DownloadTemporaryFile(MANIFEST_URL, MANIFEST_BASENAME, '',
      @UpdateDownloadProgress);
  except
    Log('update: manifest fetch failed: ' + GetExceptionMessage);
    Exit;
  end;
  ManifestPath := ExpandConstant('{tmp}\') + MANIFEST_BASENAME;

  if not LoadStringsFromFile(ManifestPath, Lines) then
  begin
    Log('update: manifest unreadable: ' + ManifestPath);
    Exit;
  end;

  UpdateVersion := ManifestValue(Lines, 'version');
  UpdateUrl := ManifestValue(Lines, 'url');
  UpdateSha256 := Lowercase(ManifestValue(Lines, 'sha256'));

  if UpdateVersion = '' then
  begin
    Log('update: manifest carries no version');
    Exit;
  end;
  { A SHA-256 is exactly 64 hex digits; anything else cannot be verified. }
  if (UpdateUrl = '') or (Length(UpdateSha256) <> 64) then
  begin
    Log('update: manifest has no verifiable payload for ' + UpdateVersion);
    UpdateUrl := '';
    Exit;
  end;
  if not IsNewerVersion(UpdateVersion, '{#MyAppVersion}') then
  begin
    Log('update: manifest ' + UpdateVersion + ' is not newer than {#MyAppVersion}');
    UpdateUrl := '';
    Exit;
  end;

  Log('update: ' + UpdateVersion + ' available (bundled: {#MyAppVersion})');
  Result := True;
end;

{ Fetches the payload into the temp folder and verifies its SHA-256. Returns
  True only when a complete copy is sitting there for ApplyUpdatePayload. }
function DownloadPayload: Boolean;
var
  Err: String;
begin
  Result := False;
  Err := '';

  if WizardSilent then
  begin
    { Silent installs have no wizard form to own a progress page. }
    try
      DownloadTemporaryFile(UpdateUrl, PAYLOAD_BASENAME, UpdateSha256,
        @UpdateDownloadProgress);
      Result := True;
    except
      Err := GetExceptionMessage;
    end;
  end
  else
  begin
    UpdatePage.Clear;
    UpdatePage.Add(UpdateUrl, PAYLOAD_BASENAME, UpdateSha256);
    UpdatePage.Show;
    try
      try
        UpdatePage.Download;
        Result := True;
      except
        if UpdatePage.AbortedByUser then
          Err := 'cancelled'
        else
          Err := Format('%s: %s', [UpdatePage.LastBaseNameOrUrl, GetExceptionMessage]);
      end;
    finally
      UpdatePage.Hide;
    end;
  end;

  if Result then
  begin
    Log('update: payload verified (sha256 ' + UpdateSha256 + ')');
    Exit;
  end;

  { A half-written file must never reach the install folder: delete it so only
    a complete, hash-verified payload can survive into the install. }
  DeleteFile(PayloadPath);
  UpdateUrl := '';
  Log('update: payload download failed: ' + Err);
  if Err <> 'cancelled' then
    SuppressibleMsgBox(
      'TaskbarIsland ' + UpdateVersion + ' could not be downloaded.' + NL + NL +
      'Setup will install the version bundled with this file instead.' + NL + NL +
      Err, mbInformation, MB_OK, IDOK);
end;

{ Installed as the core exe entry's AfterInstall hook, so it runs once the
  embedded build has been copied and always before Setup launches the app. }
procedure ApplyUpdatePayload;
var
  Dest: String;
begin
  if not FileExists(PayloadPath) then
    Exit; { offline, declined or already current: keep the embedded build }

  Dest := ExpandConstant('{app}\') + '{#MyAppExeName}';
  if not CopyFile(PayloadPath, Dest, False) then
  begin
    Log('update: could not copy ' + PayloadPath + ' over ' + Dest);
    SuppressibleMsgBox(
      'TaskbarIsland ' + UpdateVersion + ' was downloaded but could not be' +
      ' installed.' + NL + NL +
      'Setup will leave the existing files in place.', mbError, MB_OK, IDOK);
    Exit;
  end;
  Log('update: installed ' + UpdateVersion + ' over ' + Dest);
end;

<event('InitializeWizard')>
procedure UpdateInitializeWizard;
begin
  UpdatePage := CreateDownloadPage(SetupMessage(msgWizardPreparing),
    SetupMessage(msgPreparingDesc), @UpdateDownloadProgress);
  UpdatePage.ShowBaseNameInsteadOfUrl := True;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID <> wpReady then
    Exit;

  { Never reuse a payload left behind by an earlier pass over this page. }
  DeleteFile(PayloadPath);

  if not CheckForUpdate then
    Exit;

  if SuppressibleMsgBox(
       'TaskbarIsland ' + UpdateVersion + ' is available.' + NL + NL +
       'You have {#MyAppVersion}. Download and install the new version now?' + NL + NL +
       'Your settings and shelf are kept either way.',
       mbConfirmation, MB_YESNO, IDYES) <> IDYES then
  begin
    Log('update: declined; installing the bundled {#MyAppVersion}');
    Exit;
  end;

  DownloadPayload;
end;
