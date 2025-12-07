# PrintHelper 0.1.0 — initial release draft

Prepare multi-page print layouts from PDF and image inputs in an independent Windows desktop app.

## Added

- Page thumbnails, reorder/delete, clockwise rotation, individual edge cropping, bulk crop and undo/redo.
- 1/2/4-up sheet layouts on A4, A5 or Letter, with orientation, margins, gaps and grayscale.
- Output-sheet preview and losslessly compressed raster PDF export at 150 or 300 DPI.
- Saved layout recipes with source hashes to reproduce edits and detect changed sources.
- An explicit Windows print action, with paper/orientation and hard-margin checks before spooling.
- Settings with installed version, optional startup checks, manual update checks and a user-triggered verified download/install/restart path.

## Fixed during local review

- Avoid recalculating output sheets for repeated clicks or selection within the same sheet. Reuse cached thumbnails and separate missing-thumbnail rendering from sheet rendering. Background renders keep foreground controls/status stable, while one explicit action can queue behind them. Headless regressions cover dispatch and texture reuse; visible flicker still needs native-window validation.
- Embed the original printer icon in the standalone EXE and use it for the window and taskbar. All nine resolutions are verified in the packaged resources; native appearance remains pending GUI validation.
- Keep preview loading/retry status in one stable row and bottom progress in a fixed-height panel, preventing state changes from resizing or moving the sheet in headless layout tests. Replace the unsupported arrow glyph with readable page/sheet counts.
- Open Add files and the other file pickers on an isolated STA thread instead of the PDF worker's MTA, eliminating the confirmed COM-mode conflict that silently cancelled picker startup. Noninteractive apartment regressions cover this fix; actual picker interaction remains pending.
- Crop original image pixels before scaling so discarded borders do not contaminate previews or exported pages.
- Reject invalid PDF page/image dimensions and guard temporary-file destinations, including Windows filename case variants.
- Preserve a flushed executable backup and restore it when replacement or restart fails; close staging handles before Windows file replacement.
- Relaunch only a verified original after parent exit, and retain early helper failures in an adjacent error report.
- Compare SemVer precedence without build metadata, reject malformed executable headers/sections and enforce HTTPS throughout update redirects.
- Consolidate the toolbar, give PDF export primary emphasis, group crop controls and replace technical paragraphs with contextual help. Settings shows download progress and a download-specific cancel action; failed previews offer retry.

## Software rendering

- Replace eframe/glow/OpenGL with a CPU egui rasterizer and Windows GDI bitmap presentation.
- Retain the last bitmap for unchanged OS exposures, and suspend UI drawing while hidden/minimized/occluded while still handling background results and shutdown.
- Preserve pending texture/font updates across suppressed frames. Add headless raster and scheduling tests, a rendering diagnostics flag, and CI checks against GPU/OpenGL dependency/import regressions.

## Platform and download

Planned asset: `PrintHelper-v0.1.0.exe`, Windows 10/11 x64. Uses the built-in Windows PDF renderer; no PDF companion DLL, printer driver or service. Include `SHA256SUMS.txt` and third-party license notices with the executable.

## Known limitations

PDF output is rasterized; original vectors, searchable text, forms and links are not retained. Only the first frame of multi-frame image inputs is imported. Recipes reference original source locations. Existing output files are never overwritten; choose a new name. Office files, booklet mode and duplex management are not included.

Native PDF fixture rendering/export has automated coverage. GUI behavior, actual printing, graphics/idle performance, update download/self-replacement/rollback and real screenshots remain pending. No public release has been created; these notes describe the initial local implementation, not a published or device-certified release.
