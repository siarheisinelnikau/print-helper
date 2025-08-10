# PrintHelper 0.1.0 — initial release draft

Prepare multi-page print layouts from PDF and image inputs in an independent Windows desktop app.

## Added

- Page thumbnails, reorder/delete, clockwise rotation, individual edge cropping, bulk crop and undo/redo.
- 1/2/4-up sheet layouts on A4, A5 or Letter, with orientation, margins, gaps and grayscale.
- Output-sheet preview and losslessly compressed raster PDF export at 150 or 300 DPI.
- Saved layout recipes with source hashes to reproduce edits and detect changed sources.
- An explicit Windows print action, with paper/orientation and hard-margin checks before spooling.
- Settings with installed version, optional startup checks, manual update checks and a user-triggered verified download/install/restart path.

## Platform and download

Planned asset: `PrintHelper-v0.1.0.exe`, Windows 10/11 x64. Uses the built-in Windows PDF renderer; no PDF companion DLL, printer driver or service. Include `SHA256SUMS.txt` and third-party license notices with the executable.

## Known limitations

PDF output is rasterized; original vectors, searchable text, forms and links are not retained. Only the first frame of multi-frame image inputs is imported. Recipes reference original source locations. Existing output files are never overwritten; choose a new name. Office files, booklet mode and duplex management are not included.

Native PDF fixture rendering/export has automated coverage. GUI behavior, actual printing, graphics/idle performance, update download/self-replacement/rollback and real screenshots remain pending. No public release has been created; these notes describe the initial local implementation, not a published or device-certified release.
