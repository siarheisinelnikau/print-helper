# Validation record

Date: 2026-10-31. Scope: code-only development on Windows x64 with rustc 1.96.1 / Cargo 1.96.1. No application window, file dialog, print dialog, real printer, updater helper, browser UI or screenshots were opened. All document inputs were generated inside repository-local `.test-output` directories. No real user documents or settings were modified.

## Automated checks

Final result: formatting passed, Clippy passed with warnings denied for application code, and all 10 integration tests passed. The same checks passed again with static MSVC runtime linking enabled. The release/package script completed successfully. Read-only PE inspection confirmed x64, Windows GUI subsystem and only Windows system DLL imports, without VCRUNTIME/MSVCP. The packaged dependency notices match the committed notice document.

Artifact: `dist/PrintHelper-v0.1.0.exe`, 9,137,664 bytes. SHA-256: `e183365051b82371a4e66c78280424168ec243423224057356488dadfca5c400`. The EXE was inspected without execution.

- `cargo fmt --all -- --check`: formatting.
- `cargo clippy --offline --all-targets -- -D warnings`: all application/library/test targets.
- `cargo test --offline --locked`: ten synthetic integration tests, including the built-in Windows PDF renderer.
- `scripts/package.ps1 -Offline`: optimized Windows x64 GUI executable, versioned name, SHA256SUMS and license notices. Static MSVC runtime is selected in the repository configuration.
- `python scripts/inspect-pe.py dist/PrintHelper-v0.1.0.exe`: read-only architecture/subsystem/import-table check; the script rejects external VCRUNTIME/MSVCP imports. It never executes the application.
- `python scripts/generate-notices.py`: collect license/notice texts for the locked Windows dependency graph, including font licenses. Standard license texts supplement crates that do not package their declared license file.

Dependency network retrieval initially failed with `getaddrinfo() thread failed to start`. Implementation uses cached dependencies and the OS PDF renderer, and the Cargo lockfile resolves offline. Cargo reported that incremental-cache hard links were unavailable and copied cache files instead. This environment limitation did not block compilation or tests. Application output publication uses Windows MoveFileEx without replacement and does not depend on hard links.

## What the tests establish

| Test | Evidence |
| --- | --- |
| Cell geometry | A4/A5/Letter, portrait/landscape, 1/2/4-up, margins and partial-sheet counts; invalid layout values are rejected |
| Actual sheet pixels | Reordered red/blue fixture pages occupy the expected cells; margins and unused final cells remain white |
| Page transforms | Asymmetric crop precedes clockwise rotation; resulting color regions match; grayscale channels are equal |
| Alpha handling | Fully transparent image regions become white paper |
| Recipe reproducibility | Project settings/transforms serialize and deserialize identically; changed source hashes fail validation |
| Output safety | Cancellation publishes no document; existing source/output destinations survive failed saves unchanged |
| Native PDF round trip | Generated two-page PDF imports with correct native dimensions, reorders/rotates, exports to one two-up sheet, reopens in Windows PDF, and has correct paper dimensions and expected top/bottom colors; source bytes stay unchanged |
| Update selection | Numeric semver order; older/equal, draft and prerelease exclusion |
| Update metadata | Missing/incompatible asset, wrong repository URL, missing/malformed digest and excessive size are rejected |
| Update payload | Synthetic x64 PE header plus matching SHA-256 accepted; wrong checksum or architecture rejected; no executable launched |

These tests do not establish full PDF fidelity, actual printer output, a working self-update cycle, or the visual quality/responsiveness of the GUI.

## Required manual checks before publication

1. Launch the packaged EXE on Windows 10 and 11 x64, including a machine without a separately installed Visual C++ redistributable. Verify first launch, missing OpenGL support errors, normal close, Unicode paths and a read-only executable folder.
2. Check page-list selection, keyboard/mouse focus, reorder/delete/rotate/crop, bulk crop, Undo/Redo, project open/save cancellation, unsaved-change prompts, preview sheet navigation, 100/150/200% display scaling and minimum window size. Capture real main-window and Settings screenshots, then embed them in README.
3. Review representative PDFs in an independent PDF viewer: embedded/nonembedded fonts, mixed page sizes, intrinsic rotation, CropBox/MediaBox differences, transparency, annotations/forms, grayscale photographs, TIFF first-frame behavior, EXIF orientation, fine lines and small text at both DPI settings. Confirm export dimensions/ordering after deleting pages and across multiple sheets.
4. Measure idle CPU and frame/presentation latency in an optimized packaged build with empty and populated layouts, minimized/restored and during interaction. No smoothness or low-CPU claim is made from source inspection.
5. Only with later user authorization, print to a test destination: confirm driver paper/orientation mismatch and hard-margin rejection, one-up driver scaling, copies/collation, grayscale, multiple/partial sheets, cancellation, spooler errors and physical measurements. Do not treat a spooler success as confirmed paper output.
6. With two controlled published test releases, verify disabled startup checks make no request, enabled checks are nonblocking, no-update/startup-error results stay unobtrusive, manual errors/rate limits are clear, and correct compatible assets are selected.
7. Exercise download progress/cancellation, digest/size mismatch, unwritable directory, process-exit timing, replacement, rollback, restart, preserved settings and recovery from leftover `.exe.next`/staged/backup files. Never test this against the sole working installation or real user data.
8. Review dependency notices, intended repository/version/asset names, release notes and the unsigned executable's distribution experience. Publish only under separate explicit authorization through the connected GitHub plugin.

Screenshots, UI/hardware validation, independent-viewer checks and live update checks remain deferred under the user's explicit no-GUI/no-device/no-publication constraints. There is no release in this session to compare against; the release-note document is an initial-capability draft.
