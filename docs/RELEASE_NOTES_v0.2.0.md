# PrintHelper 0.2.0

Prepare print layouts in your preferred interface language and appearance.

## Added

- English, Russian, French, German, Spanish and Italian interface catalogs covering document and sheet controls, Settings, update actions, file filters, help, dialogs, actionable validation errors and status/progress messages (206 messages in each catalog).
- A saved Language choice and Light/Dark theme in Settings > Appearance, applied without restarting.
- A complete dark palette for panels, page cards, controls, selections, drag-and-drop hints and the preview surround. Output paper stays white.

## Improved

- Existing settings keep the saved startup-update preference and gain English/Light defaults.
- Status messages retain their parameters and retranslate after a language change; paths and original document contents remain unchanged.
- Dependency notices include the shared updater's canonical repository and workspace license.

## Verification and limits

Automated checks cover catalog coverage and placeholders, safe parameter substitution, old/new settings round trips, both palettes' text contrast, translated workspace/settings bounds and existing document/update/software-renderer regressions. No application window, file dialog, print dialog or real printer was opened. Complete application/settings screenshots and runtime idle/hidden/restore measurements remain pending because this repository's current task instructions prohibit GUI launches. Native Windows dialog buttons use the OS language; technical diagnostic details retain their original text.

Windows 10/11 x64. Portable asset: `PrintHelper-v0.2.0.exe`, with `SHA256SUMS.txt` and license notices. Settings and source documents are preserved during application updates.
