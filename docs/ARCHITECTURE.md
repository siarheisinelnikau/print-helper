# Architecture

## Document model

`layout.rs` contains a serializable project recipe: explicit format version, paper settings and ordered page records. Source records contain canonical paths, SHA-256, source page index and native dimensions in points. Crops are fractions of original edges, followed by clockwise quarter turns. N-up cells are row-major. A single `fit` calculation centers each page inside a cell without changing aspect ratio. All margins and gaps are in millimeters, converted through 72 PDF points per inch.

`document.rs` owns immutable image/Windows PDF sources. It rejects oversized files/images, applies EXIF orientation, composites alpha over white and validates saved recipes against source hashes/dimensions. PDFs are loaded from in-memory streams; sources are never opened for writing. Windows PDF sizes are in 96-DPI device-independent units and are converted to points with a 0.75 factor. Render requests account for the crop before selecting the full-source bitmap dimensions. The native engine's default page background is white.

`output.rs` creates sheets through one common rasterizer. Preview requests a roughly 1000-pixel longest edge; export/print select 150/300 DPI. Each sheet is generated independently. A minimal PDF 1.4 writer stores sheets as losslessly deflated RGB Image XObjects, includes explicit MediaBox dimensions and a complete xref table, and retains at most one sheet's image/compression data. Files are written to unique adjacent temporary files and flushed before Windows MoveFileEx without REPLACE_EXISTING publishes them. Existing source/output files survive save failures and cancellation. The non-Windows library fallback uses no-overwrite hard links; the application is supported on Windows only.

`printing.rs` opens PrintDlgW only for an explicit Print action. It checks physical dimensions against the layout (six-point tolerance) and driver hard margins before StartDocW. It draws top-down 32-bit DIBs to the physical paper coordinate system, offsetting the printable-area origin, and releases HDC/global dialog resources through RAII. Failures/cancellation call AbortDoc for the current job. Driver copies/collation use PD_USEDEVMODECOPIESANDCOLLATE. No printer or system defaults are changed by the app directly. Driver-specific dialog and spooler effects require manual testing.

## UI and work lifecycle

`main.rs` uses eframe/egui with English UI and an original page-list/settings/preview arrangement. The page list is virtualized. Two persistent workers handle document and update operations separately. Each has a one-command bounded queue and a four-event bounded queue; progress is coalesced through try_send, while final results are retained. Workers block on receive while idle and request repaint only after results. The UI requests a short repaint deadline for pending debounced preview work, without a continuous application repaint loop. Preview revisions discard stale results, and undo/redo keep up to 64 recipe snapshots. Thumbnail cache size is capped at 600 textures.

File dialogs, PDF/image reads, thumbnails, sheet rendering, export, printing, settings writes and update HTTP/download work execute outside the UI thread. Native PDF operations run in an initialized MTA. Import/open is transactional: a failed/cancelled batch does not replace the active catalog/project. Catalog source cost is limited to 256 MiB, excluding opaque native renderer allocations. Deleting pages retains loaded sources for undo; New/Open releases the previous catalog. Application close cancels worker work; there is no tray or hidden background service.

Framework-level idle CPU, presentation latency, DPI/graphics compatibility and close-during-work behavior require observation. The pinned eframe/winit implementation has not been launched or profiled here; reactive application code alone does not prove low idle CPU.

## Update boundary

`updater.rs` uses anonymous HTTPS to one canonical repository's latest published stable release, parsed semver, exact asset names/URLs, a 200 MB download limit, declared byte count, SHA-256 and x64 PE checks. Startup checks are optional/persisted. No installer helper starts before an explicit user action. The new executable doubles as the helper and uses process waiting and staged replacement with an old-executable backup. Installation does not modify settings, document sources or recipes. Tests validate selection/hash rejection only, never execute downloaded or replacement binaries.

The SHA-256 digest authenticates bytes relative to GitHub HTTPS metadata, not an independent publisher signature. Authenticode signing and release publication are not configured in this phase.
