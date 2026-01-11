# PrintHelper 0.1.3 — drag and drop, local build

Changes from local 0.1.2. No release or upload has been published.

- Drop local PDFs and images anywhere in the window to append them, with a preview-area drop hint. Files use the same validated, bounded background import as Add files and keep their received order.
- Reorder page cards by dragging to the blue insertion line. The list scrolls near its edges; release commits one Undo step and follows the moved page's sheet. Escape or release outside cancels. Same-position drops leave document order and Undo unchanged.
- Cancel stale page drags after a revision change, and block drops during incompatible foreground actions or the unsaved-layout choice. Keep Move up/down as an alternative.
- Preserve the current catalog/layout on unsupported, oversized or cancelled file batches, including cancellation after the final thumbnail. Source bytes remain unchanged.

Formatting, Clippy and 45 noninteractive tests passed, including synthetic pointer drags, cancellation/no-op/stale cases, direct-path batch rollback, auto-scroll limits and a dropped-file input consumed once across repeated layout passes. The tests create no native window or file picker. Packaging verifies version 0.1.3, icons and the CPU/GDI executable imports.

Real File Explorer/OLE drops, multi-file native routing, edge-scroll feel, lost-focus behavior and appearance remain pending native GUI validation. No GUI, computer use, printer or live updater was run. Virtual files without a local path and recursive folder imports are unsupported; PDF output remains rasterized.
