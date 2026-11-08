; Build-time knobs for the installer's self-update, shared by
; scripts\installer.iss and scripts\tests\update-smoke.iss.
;
; Override any of them on the ISCC command line, e.g.
;   ISCC.exe /DUpdateManifestUrl=https://staging.example/manifest.txt ...
;
; Nothing defined here reaches the shipped application: TaskbarIsland itself
; must stay fully offline (AGENTS.md), so the updater lives only in Setup.

#ifndef UpdateManifestUrl
  ; Small text manifest published as an asset on the newest GitHub release by
  ; scripts\release.ps1. The "latest/download" alias always resolves to that
  ; release, so no version number is baked into the URL. A repository with no
  ; releases yet answers 404, which the installer treats as "no update".
  #define UpdateManifestUrl "https://github.com/NyX-cod3r/taskbar-island/releases/latest/download/manifest.txt"
#endif

#ifndef UpdatePayloadBaseName
  ; Name the downloaded payload gets inside {tmp}. The installer reads it back
  ; from there, so the download and the AfterInstall hook must agree on it.
  #define UpdatePayloadBaseName "tbi-update.exe"
#endif
