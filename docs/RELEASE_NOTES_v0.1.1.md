# PrintHelper 0.1.1 — local verification build

This version distinguishes the current executable from earlier local 0.1.0 builds. The latest 0.1.0 artifact already contained the fixes below; 0.1.1 preserves that behavior and corrects version tracking and packaging. No public release or upload has been created.

## Version tracking

- Show 0.1.1 through the Cargo-derived installed version and executable metadata, with the standalone name `PrintHelper-v0.1.1.exe`.
- Validate product/file versions and original filename during packaging. CI now inspects the version-derived artifact rather than a hardcoded 0.1.0 path.
- Increment the patch version for each changed build delivered to the user, including unpublished local builds.

## Included fixes from local review

- Open Add files and other native file pickers on an STA thread, separate from the PDF worker's MTA; apartment regressions reproduce and cover the old COM conflict.
- Use CPU rasterization and Windows GDI presentation without an OpenGL/GPU UI backend. Keep hidden rendering suppressed and retain unchanged frames.
- Embed the printer icon for the EXE, title bar and taskbar; keep loading/progress layout stable.
- Avoid recalculating sheets on repeated or same-sheet page selection, retain cached thumbnails and keep foreground controls stable during background renders. Reserve one bounded slot for an explicit action behind a render.
- Crop original pixels before scaling, validate PDF dimensions and protect source/output files during cancellation and failed saves.
- Preserve an executable backup, restore it after replacement/restart failure, validate update payloads and compare SemVer precedence correctly.

## Verification and limits

Formatting, Clippy and 35 noninteractive tests cover document/export behavior, update transactions, software rendering, layout, selection and dispatch. Packaging checks executable metadata, all nine embedded icon resolutions, and the absence of OpenGL/GPU presentation and external MSVC runtime imports.

Windows 10/11 x64; PDF export remains rasterized. GUI interaction, native icon appearance, visible flicker, runtime load, actual printing and live self-update remain unverified under the no-GUI/no-device instruction. No GUI or updater helper was launched. The initial capability draft remains in [0.1.0 notes](RELEASE_NOTES_v0.1.0.md).
