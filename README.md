# PrintHelper

Prepare clear, economical print layouts from PDFs and images. PrintHelper is an independent Windows desktop application written in Rust, with its own interface and implementation. It is not affiliated with FinePrint.

Version **0.1.0**, local first-version build. The planned [canonical repository](https://github.com/siarheisinelnikau/print-helper) and release have not been published by this development session.

## What works in the implementation

- Import PDF, PNG, JPEG, BMP, TIFF and WebP files into one page list, with page thumbnails.
- Reorder, delete, rotate clockwise, crop each original edge by percentage, and apply one crop to every page. Undo and redo retain 64 editing steps.
- Place 1, 2 or 4 pages on A4, A5 or Letter paper, with portrait/landscape, margins, gaps and grayscale.
- Preview each output sheet using the same geometry and rendering path as export and printing.
- Export a new PDF at 150 or 300 DPI. Existing output files and source files are never overwritten.
- Save a `.phlayout` recipe with source paths, SHA-256 hashes, source page numbers, dimensions and every transform. Reopening verifies the sources before accepting the recipe.
- Deliberately open the Windows print dialog, validate paper size/orientation and printer margins, and send rasterized sheets through GDI. Printing has **not been device-tested**.
- Settings show the installed version, startup update toggle, manual **Check for updates**, download progress, SHA-256 verification, and **Download and install** followed by **Install and restart**.

## Run and use

Windows 10/11 x64 is the supported target. PDF rendering uses the Windows PDF API already provided by the OS; no PDF DLL, virtual printer, driver or service is installed. The MSVC runtime is linked statically. The UI uses eframe/egui with an OpenGL presenter; compatibility on the user's graphics hardware is pending.

1. Run `dist/PrintHelper-v0.1.0.exe` from a writable folder.
2. Choose **Add files…**. Click a page thumbnail, then use Up/Down/Delete and the Selected page controls.
3. Choose paper, orientation, pages per sheet, margins, gap, grayscale and output DPI. Two-up uses top/bottom cells in portrait and left/right cells in landscape. Four-up reads left to right, then top to bottom.
4. Inspect every output sheet with Previous/Next. Blank cells on a final partial sheet remain white.
5. Choose **Save layout…** to preserve a recipe, or **Export PDF…** to create a new document. Choose a new filename for every save/export; replacement is intentionally refused even if a file dialog offers overwrite confirmation. Recipes reference the source files and do not embed them; keep those files at their original locations.
6. To print, choose **Print…**, then set matching paper/orientation in printer Properties. Use 100% scale and one driver page per sheet to avoid applying N-up twice. The app rejects paper mismatches and margins smaller than the driver's printable area before starting a print job. Driver copies/collation are delegated to Windows. A successful spool operation does not confirm physical output.

Settings are stored at `%LOCALAPPDATA%/PrintHelper/settings.json`. Source documents are held as immutable loaded snapshots during a session. Project recipes and exports go only to destinations chosen by the user. Closing with unsaved layout changes asks whether to keep editing or discard them. Changes are not autosaved.

## Fidelity and limits

Output is a **raster PDF**, compressed losslessly in RGB at the selected DPI. Searchable text, links, forms, original vectors, accessibility structure, layers and original color profiles are not retained. Preview has lower resolution, so it verifies geometry and placement, not fine text quality at print DPI. Inspect exported PDFs before printing important documents. Grayscale removes color; it does not guarantee lower ink usage on every printer.

Windows decides supported PDF rendering features. Password-protected/unsupported documents report an error. Complex PDFs, unusual fonts, transparency, annotation/form appearance, color management and third-party PDF viewer compatibility need broader validation. Office files, virtual-printer capture, booklet imposition, automatic duplex, OCR and printer management are outside this version.

Image orientation metadata is applied, transparency is composited on white, and image dimensions use a fixed 96 DPI rather than embedded print-DPI metadata. Only the first image frame/page is imported for TIFF and other multi-frame inputs. Crop removes original edges **before** clockwise rotation. Aspect ratio is preserved; there is no automatic rotation or stretching to fill a cell.

Limits: 500 project pages, 64 MiB per input, 256 MiB combined source cost (compressed PDF bytes or decoded RGB image bytes), and 32 megapixels per image/full-page raster. Large crops at 300 DPI may exceed the render budget; use 150 DPI or a smaller crop. These limits bound application work, not Windows PDF renderer internals. Cancellation is checked between pages/sheets or download chunks; an active native rendering operation or print dialog cannot be interrupted immediately. A cancelled print job cannot recall already printed sheets.

Source thumbnails are color previews. Edited thumbnails refresh on selection; after a bulk crop, unvisited pages can show a refresh notice until selected. The output-sheet preview always uses all current transforms.

## Updates

Startup checks are enabled by default, run off the UI thread, and make a public HTTPS request to the canonical repository's latest stable release. Turn off **Check for updates on startup** to prevent that startup request; manual checks remain available. No-update and transient startup errors appear only in Settings. An unpublished repository, missing release, API limit or network error is reported as unavailable, never as up to date.

Only a newer stable `PrintHelper-vVERSION.exe` Windows x64 asset at the canonical release URL is accepted. Installation requires a GitHub-provided SHA-256 asset digest, the declared size and an x64 PE header; it stops on missing/mismatched metadata. Downloads require user action. After verification, save the layout and choose **Install and restart**. The verified new executable waits for the old process to exit, stages a replacement, keeps a uniquely named `.exe.previous-PID` backup, replaces the executable and restarts it. Settings/data remain separate. If replacement fails, it attempts rollback and relaunches with an error. Backup files can be retained for manual recovery. A crash can leave `.exe.next`, `.printhelper-update-*.exe` or a temporary output; cleanup/recovery is manual.

The updater requires a writable application folder; it never requests elevation. Live network, self-replacement, rollback and restart behavior are **pending manual verification**. No update was downloaded or installed in this session. The updater's public HTTPS requests are application runtime behavior; development does not use them for remote repository operations.

## Build and verify

Use a Windows x64 MSVC Rust toolchain (Rust 1.92 or later) and the Windows SDK/linker. Dependencies are pinned in `Cargo.lock`. Builds use at most two Cargo jobs and a repository-local `target` directory.

```powershell
Set-Location E:\work\github\print-helper
./scripts/check.ps1 -Offline
./scripts/package.ps1 -Offline
```

Omit `-Offline` on a machine that needs to resolve dependencies. Packaging produces `dist/PrintHelper-v0.1.0.exe` and `dist/SHA256SUMS.txt`, with license notices. It does not start the application or publish anything. CI runs the same synthetic checks on Windows and retains an artifact without creating a release.

See [architecture](docs/ARCHITECTURE.md), [validation and remaining checks](docs/VALIDATION.md), and the [initial release-note draft](docs/RELEASE_NOTES_v0.1.0.md).

## Screenshots

Real application and Settings screenshots are **pending**. The user prohibited launching or controlling GUI applications and capturing screenshots during this session. No mock screenshots or broken image links are supplied. Add real screenshots after the permitted interactive check.

## License and references

MIT; dependency notices are in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The task reference was [FinePrint's public product page](https://fineprint.com/fp/); no competitor code or assets were used. Platform behavior follows the [Windows PDF rendering API](https://learn.microsoft.com/en-us/uwp/api/windows.data.pdf.pdfpage.rendertostreamasync) and [GitHub release API](https://docs.github.com/en/rest/releases/releases).
