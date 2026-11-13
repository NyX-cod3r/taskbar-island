# Third-party notices

## What ships where (license audit, 2026-11-13)

- **The core installer (`TaskbarIsland-Setup.exe`) contains no third-party
  code at all.** It packs `taskbar-island.exe`, `LICENSE` and `NOTICE` only.
  Every crate linked into `taskbar-island.exe` is OSI-approved open source
  (Apache-2.0 / MIT / BSD-2-Clause — windows-rs, image, zip, pdf-writer,
  kamadak-exif, webp, zopfli, flate2, and their transitive dependencies); the
  exact set and version for a build is `Cargo.lock`. Nothing in the core
  installer needs a license file beyond `LICENSE` (GPL-3.0) and `NOTICE`.
- **Everything below lives in the optional converters pack
  (`TaskbarIsland-Converters-Setup.exe`)**, a separate download that is never
  code-signed under the TaskbarIsland name (see `docs/signing.md`). All of the
  engines are OSI-approved open source (GPL, MPL, Apache-2.0, AGPL).
- **The one item whose terms are not fully verified** is the per-language
  Tesseract language data (`tessdata\*.traineddata`): each file carries its own
  dataset terms rather than a single OSI license. Because it cannot be
  certified as OSI-approved from the file alone, it is pack-only and never
  part of the signed core installer. Core functionality (the pill, clipboard
  history, shelf, calls) runs without any engine.

TaskbarIsland launches the programs below as **separate local processes** (argv
arrays, no shell). They are not linked into `taskbar-island.exe`, not modified,
and never make a network connection on the app's behalf. Each is distributed
under its own license; the full license text ships next to the engine in
`{app}\engines\<name>\`.

Engines are optional installer components. Core TaskbarIsland functionality
(the pill, clipboard history, shelf) runs without any of them.

| Engine | Version | License | Source | Shipped files |
| --- | --- | --- | --- | --- |
| Pandoc | 3.12 | GPL-2.0 | <https://github.com/jgm/pandoc> | `engines\pandoc\` (`COPYING.rtf`, `COPYRIGHT.txt`) |
| FFmpeg | 9.0.2 (gyan.dev full build) | **GPL-3.0** for this build | <https://www.gyan.dev/ffmpeg/builds/> · <https://ffmpeg.org/> | `engines\ffmpeg\` (`LICENSE`, `README.txt`) |
| Tesseract OCR | 5.4.0.20240606 | Apache-2.0 | <https://github.com/tesseract-ocr/tesseract> | `engines\tesseract\` (+ `tessdata\`, each language file its own license, see below) |
| LibreOffice | *(pending drop-in)* | MPL-2.0 | <https://www.libreoffice.org/> | `engines\libreoffice\` |
| Poppler | *(pending drop-in)* | GPL-2.0 / GPL-3.0 | <https://poppler.freedesktop.org/> | `engines\poppler\` |
| qpdf | *(pending drop-in)* | Apache-2.0 | <https://github.com/qpdf/qpdf> | `engines\qpdf\` |
| Ghostscript | *(pending drop-in)* | **AGPL-3.0** | <https://www.ghostscript.com/> · <https://git.ghostscript.com/> | `engines\ghostscript\` |

## Notes

- **FFmpeg licensing depends on the build.** The bundled binary reports
  `--enable-gpl --enable-version3`, which makes this build a GPL-3.0 work
  (libx264/x265, `--enable-version3` components). The `ffmpeg -version`
  configuration line is the authority; a future LGPL build would be listed
  differently.
- **Ghostscript is AGPL-3.0**, not GPL. AGPL adds the network-use clause: if
  you distribute Ghostscript (or a modified version) and let users interact
  with it remotely over a network, you must offer them the corresponding
  source. Bundling it as an unmodified local process in a desktop installer
  does not by itself trigger source disclosure for TaskbarIsland's own code,
  but it is the strictest license in this list. **Owner decision (2026-11-10):
  keep it** — it ships as an optional installer component when its payload is
  present, and this file travels with the installer.
- **Tesseract language data** (`engines\tesseract\tessdata\*.traineddata`) is
  separately licensed per language (mostly Apache-2.0 or the respective
  dataset's terms — for example `eng.traineddata` from `tessdata_fast` is
  Apache-2.0). Only the languages selected at install time are shipped.
- Poppler's license depends on how the Windows build was configured
  (GPL-2.0 or GPL-3.0); the exact version shipped will be confirmed from the
  build's own license file when the engine is added.
- All trademarks and brand names belong to their respective owners.
