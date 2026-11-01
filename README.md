# PrintHelper

Prepare clear, economical print layouts from PDFs and images. PrintHelper is an independent Windows desktop application written in Rust, with its own interface and implementation. It is not affiliated with FinePrint.

Version **0.2.0**. Source and release assets are prepared for the [canonical repository](https://github.com/siarheisinelnikau/print-helper). See the repository Releases page for published availability.

## What works in the implementation

- Import PDF, PNG, JPEG, BMP, TIFF and WebP files using Add files or by dropping local files onto the window. Dropped batches append in received order.
- Reorder, delete, rotate left or right, crop each original edge by percentage, and apply one crop to every page. Undo and redo retain 64 editing steps.
- Place 1, 2 or 4 pages on A4, A5 or Letter paper, with portrait/landscape, margins, gaps and grayscale.
- Preview each output sheet using the same geometry and rendering path as export and printing.
- Export a new PDF at 150 or 300 DPI. Existing output files and source files are never overwritten.
- Save a `.phlayout` recipe with source paths, SHA-256 hashes, source page numbers, dimensions and every transform. Reopening verifies the sources before accepting the recipe.
- Deliberately open the Windows print dialog, validate paper size/orientation and printer margins, and send rasterized sheets through GDI. Printing has **not been device-tested**.
- Use **Settings > Appearance** to choose English, Русский, Français, Deutsch, Español or Italiano and a **Light** or **Dark** theme. Both choices apply immediately and persist alongside existing preferences.
- Settings show the installed version, startup update toggle, manual **Check for updates**, download progress, SHA-256 verification, and **Download and install** followed by **Install and restart**.

## Run and use

Windows 10/11 x64 is the supported target. PDF rendering uses the Windows PDF API already provided by the OS; no PDF DLL, virtual printer, driver or service is installed. The MSVC runtime is linked statically. The egui interface is rasterized on the CPU by `egui_software_backend`, then presented as a bitmap through softbuffer's Windows GDI backend. The application has no OpenGL or GPU UI backend. Windows desktop composition remains an OS responsibility. Native window behavior and actual idle/hidden load remain unverified because the GUI was not launched.

1. Run `dist/PrintHelper-v0.2.0.exe` from a writable folder.
2. Choose **Add files...** or drag local PDFs/images onto the window. Later, add more from the Pages panel or drop another batch. Click a page to open its **Page** controls: rotate left/right, crop, reset edits or remove it. Drag page cards to the blue insertion line to reorder; the list scrolls at its edges. Release to commit one Undo step, or press Escape/release outside the list to cancel. **Move up** and **Move down** remain available.
3. Open the **Sheet** tab for paper size, orientation, 1/2/4 pages per sheet and color mode. Expand **Margins and spacing** or **Output quality** when needed. Two-up uses top/bottom cells in portrait and left/right cells in landscape; four-up reads left to right, then top to bottom.
4. Inspect the centered sheet preview. Use **Previous** and **Next** below it to review every sheet. Blank cells on a final partial sheet remain white.
5. Choose **Save PDF...** to create a new PDF, or **File > Save layout as...** to keep the arrangement as a reusable recipe. Choose a new filename for every save/export; replacement is intentionally refused even if a file dialog offers overwrite confirmation. Recipes reference the source files and do not embed them; keep those files at their original locations.
6. To print, choose **Print...**, then set matching paper/orientation in printer Properties. Use 100% scale and one driver page per sheet to avoid applying N-up twice. The app rejects paper mismatches and margins smaller than the driver's printable area before starting a print job. Driver copies/collation are delegated to Windows. A successful spool operation does not confirm physical output.

Settings are stored at `%LOCALAPPDATA%/PrintHelper/settings.json`. Existing settings gain English and Light defaults without changing the startup-update choice. Application labels, file-filter labels and status messages use the selected language; native Windows dialog controls follow the Windows display language. File names, source documents and technical diagnostic details keep their original text. Source documents are held as immutable loaded snapshots during a session. Project recipes and exports go only to destinations chosen by the user. Closing with unsaved layout changes asks whether to keep editing or discard them. Changes are not autosaved. A dropped batch is imported as one transaction: unsupported/cancelled inputs leave the current layout unchanged. Folders are not expanded, and virtual files without local paths are unsupported. Finish a foreground action or unsaved-layout choice before dropping another batch. **Layout not saved** refers to the reusable arrangement; saving a PDF does not save that recipe.

## Fidelity and limits

Output is a **raster PDF**, compressed losslessly in RGB at the selected DPI. Searchable text, links, forms, original vectors, accessibility structure, layers and original color profiles are not retained. Preview has lower resolution, so it verifies geometry and placement, not fine text quality at print DPI. Inspect exported PDFs before printing important documents. Grayscale removes color; it does not guarantee lower ink usage on every printer.

Windows decides supported PDF rendering features. Password-protected/unsupported documents report an error. Complex PDFs, unusual fonts, transparency, annotation/form appearance, color management and third-party PDF viewer compatibility need broader validation. Office files, virtual-printer capture, booklet imposition, automatic duplex, OCR and printer management are outside this version.

Image orientation metadata is applied, transparency is composited on white, and image dimensions use a fixed 96 DPI rather than embedded print-DPI metadata. Only the first image frame/page is imported for TIFF and other multi-frame inputs. Image crops operate on original pixels before filtering and clockwise rotation, so discarded borders cannot bleed into smaller previews. Aspect ratio is preserved; there is no automatic rotation or stretching to fill a cell.

Limits: 500 project pages, 64 MiB per input, 256 MiB combined source cost (compressed PDF bytes or decoded RGB image bytes), and 32 megapixels per image/full-page raster. Large crops at 300 DPI may exceed the render budget; use 150 DPI or a smaller crop. These limits bound application work, not Windows PDF renderer internals. Cancellation is checked between pages/sheets or download chunks; an active native rendering operation or print dialog cannot be interrupted immediately. A cancelled print job cannot recall already printed sheets.

Source thumbnails are color previews. Edited thumbnails refresh on selection; after a bulk crop, unvisited pages can show a refresh notice until selected. The output-sheet preview always uses all current transforms.

The software window framebuffer is capped at 16 megapixels, with a 4000×4000 physical-pixel maximum client size. UI layout/rasterization is suspended for hidden, minimized or occluded windows. Background document/update results continue to be processed, preserving texture changes for the next visible frame. Unchanged OS repaint requests reuse the retained bitmap. These scheduling properties have headless policy tests; real Windows integration and performance still need manual validation.

## Updates

Startup checks are enabled by default, run off the UI thread, and make a public HTTPS request to the canonical repository's latest stable release. Turn off **Check for updates on startup** to prevent that startup request; manual checks remain available. No-update and transient startup errors appear only in Settings. An unpublished repository, missing release, API limit or network error is reported as unavailable, never as up to date.

Only a newer stable `PrintHelper-vVERSION.exe` asset at the canonical release URL is accepted; SemVer build metadata does not trigger an update. Shared-core verification checks size, SHA-256, PE architecture/GUI headers, section ranges and entry point. A supplied checksum file is verified; GitHub's asset digest remains accepted for historical releases without one. After downloading, save the layout and choose **Install and restart**. The helper waits for the old process to exit, replaces the executable with a backup and restarts. Settings, source documents and layout recipes stay separate. A first-frame startup acknowledgement enables cleanup; if startup fails before acknowledgement, the helper rolls back. Staging uses a narrowly validated `.printhelper-update-*` directory beside the executable. The older flat helper protocol remains accepted for upgrades from existing installations.

The updater requires a writable application folder; it never requests elevation. Backup/replacement and restart-failure rollback have tests using artificial files and fake restart callbacks. Live network, actual process self-replacement and restart remain **pending manual verification**. No updater helper was executed and no update was downloaded or installed in this session. The updater's public HTTPS requests are application runtime behavior; development does not use them for remote repository operations.

## Build and verify

Use a Windows x64 MSVC Rust toolchain (Rust 1.95 or later) and the Windows SDK/linker, including `rc.exe` for the embedded icon and version resources. Python 3.10 or later is used by the dependency/PE checks, icon generator and license-notice generator. Dependencies are pinned in `Cargo.lock`. Builds use at most two Cargo jobs and a repository-local `target` directory.

```powershell
Set-Location E:\work\github\print-helper
./scripts/check.ps1 -Offline
./scripts/package.ps1 -Offline
python ./scripts/inspect-pe.py ./dist/PrintHelper-v0.2.0.exe
```

Omit `-Offline` on a machine that needs to resolve dependencies. Packaging produces `dist/PrintHelper-v0.2.0.exe` and `dist/SHA256SUMS.txt`, with license notices. It verifies that embedded product/file versions and original filename match the Cargo version, then inspects PE imports and icons without starting the application. CI uses this same version-derived packaging and inspection path.

Increment the minor version for new capabilities and the patch version for fixes before delivering a changed build. `Cargo.toml` is the source for the installed-version UI, update client, executable resources and package name; update the lockfile, current documentation and release notes together. Packaging validates the version but does not increment it automatically. Local commits track source changes independently of application versions.

The original printer icon is defined in `assets/app-icon.svg`. After editing it, run `python scripts/generate-icon.py` to regenerate the PNG and multi-resolution ICO using the script's supported rectangle/circle subset. The normal check script verifies these assets are current. All icon sizes are embedded in the executable; the title bar and taskbar load the same resource without external asset files.

For a later permitted runtime check, `PrintHelper-v0.2.0.exe --render-diagnostics <new-file.json>` runs the normal application and saves aggregate UI/raster/presentation counts and maximum durations when it exits. It is not a capture or benchmark mode and was not invoked during this no-GUI session. Measure process CPU and observe idle/minimized/restored behavior separately; diagnostic counters alone do not certify responsiveness.

See [architecture](docs/ARCHITECTURE.md), [validation and remaining checks](docs/VALIDATION.md), and the [current release notes](docs/RELEASE_NOTES_v0.2.0.md).

## Screenshots

Real application and Settings screenshots of the six-language, Light/Dark interface are **pending**. The user prohibited launching or controlling GUI applications and capturing screenshots during this session. No mock screenshots or broken image links are supplied. Add real screenshots after the permitted interactive check.

## License and references

MIT; dependency notices are in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The task reference was [FinePrint's public product page](https://fineprint.com/fp/); no competitor code or assets were used. Platform behavior follows the [Windows PDF rendering API](https://learn.microsoft.com/en-us/uwp/api/windows.data.pdf.pdfpage.rendertostreamasync) and [GitHub release API](https://docs.github.com/en/rest/releases/releases).


## Shared updater source dependency

Keep the `rust-auto-updator` workspace beside this checkout at `../rust-auto-updator` when building locally or in CI. This is a local path dependency and has not been published by this change. `src/updater.rs` contains product configuration and compatibility routing; release selection, HTTPS downloads, verification, staging, replacement, rollback and restart use the shared core. Startup checks retain the saved opt-out; manual checks and user-initiated download/install remain in Settings. Download cancellation is cooperative. The older `--finish-update` protocol is translated into the shared validated transaction. Rollback files are cleaned only after the new application presents its first successful frame.

Rust 1.95 or newer is required by the shared updater. PrintHelper has no tray workflow; its embedded application icon is also assigned to the native window and taskbar. A [native resource proof](docs/icon-resources.png) shows the shipped icon at its native sizes on light/dark backgrounds; it is not an OS screenshot. The CPU software rasterizer/GDI host retains unchanged pixels for OS exposure and suppresses layout/raster/presentation while hidden, minimized or occluded.
