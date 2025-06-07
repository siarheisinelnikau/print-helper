# PrintHelper

Repository: `print-helper`
Priority: 12 of 15 (user-requested launch order)
Product: PrintHelper
Purpose: Prepare clear, economical print layouts
Inspiration: https://fineprint.com/fp/

Independent open-source implementation; no affiliation, competitor code or copied assets.

## Requested implementation

Implement a native print-preparation app for PDF and image inputs: page thumbnails, reorder/delete/rotate, crop margins, grayscale, 1/2/4 pages per sheet, paper size/orientation/margins and an accurate output preview. Include PDF export and a deliberate Windows print action; booklet/manual-duplex guidance can follow. Keep source documents unchanged and make every page transform reproducible.

Prefer a useful standalone app over a new virtual-printer driver. Test PDF page counts, dimensions, transforms, ordering and output using generated documents and fixtures. Do not use a real printer, invoke the system print UI, install printer drivers or change printers. If rendering or conversion needs a companion library, bundle/extract it deliberately and document why a strict single EXE is not possible. Do not pretend Office document support or print fidelity has been verified without evidence.

## Packaging and publication target

Windows standalone asset: `PrintHelper-vVERSION.exe` (for the initial release, `PrintHelper-v0.1.0.exe`). Append architecture only when distributing multiple executable architectures in the same release.
Planned canonical repository: https://github.com/siarheisinelnikau/print-helper (not yet created or published; keep availability/error reporting truthful).

## Working agreement

Follow AGENTS.md. Complete as much real implementation and safe noninteractive verification as possible in this unattended code-only session. Manual UI, device and hardware tests will happen later. Record required external components and their verification status. Keep changes committed locally in meaningful increments.

