# PrintHelper 0.1.2 — workspace redesign, local build

Changes from local 0.1.1. No public release or upload has been created.

## Interface

- Replace the gray/teal treatment with neutral light surfaces, dark readable text, one blue action color and pale-blue selection. Increase body text and control sizes; match the printer icon to the palette.
- Start with one clear Add files action. Once populated, show compact page cards with Add files and Move up/down beside the list.
- Separate Sheet setup from Page edits. Selecting a page opens its editing controls; add left/right rotation and keep crop, reset and removal together.
- Center the sheet preview and move Previous/Next navigation below it. Preserve the stable loading/error row and background-render behavior.
- Make Save PDF the main output action. Keep saved layout recipes in File and clarify what the unsaved-layout status means.
- Simplify Settings around version and updates, with optional supported-file and printing help. Bound long error details and center the settings/error/unsaved windows.

## Verification

- Formatting, Clippy and all 39 noninteractive tests passed for 0.1.2.
- Add regressions for readable palette contrast, stable/clickable page cards with long filenames, selected-page reorder/removal behavior, and whole-workspace bounds in empty, Sheet, Page, Settings, error and unsaved states at 960×640 and 1220×850 with 100/150/200% native scaling.
- Preserve the document/export, updater transaction and CPU rendering checks. Packaging verifies the 0.1.2 metadata, embedded icon images and absence of GPU/OpenGL UI imports.

All interface tests run in memory without a native window. They do not certify native appearance, keyboard/screen-reader behavior, visual clipping in every state, flicker, idle load, actual printing or live updating. Real application/Settings screenshots and native interaction remain pending under the user's no-GUI/no-computer-use instruction. PDF output remains rasterized, with unchanged source files.
