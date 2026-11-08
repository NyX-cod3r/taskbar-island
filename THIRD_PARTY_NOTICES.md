# Third-party notices

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
  but it is the strictest license in this list — see `coordination/from-B.md`
  before deciding to keep it.
- **Tesseract language data** (`engines\tesseract\tessdata\*.traineddata`) is
  separately licensed per language (mostly Apache-2.0 or the respective
  dataset's terms — for example `eng.traineddata` from `tessdata_fast` is
  Apache-2.0). Only the languages selected at install time are shipped.
- Poppler's license depends on how the Windows build was configured
  (GPL-2.0 or GPL-3.0); the exact version shipped will be confirmed from the
  build's own license file when the engine is added.
- All trademarks and brand names belong to their respective owners.
