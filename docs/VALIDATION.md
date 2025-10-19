# Validation record

Date: 2026-10-31. Scope: code-only development on Windows x64 with rustc 1.96.1 / Cargo 1.96.1. No application window, file dialog, print dialog, real printer, updater helper, browser UI or screenshots were opened. All document inputs were generated inside repository-local `.test-output` directories. No real user documents or settings were modified.

## Automated checks

Current software-rendering result: formatting and Clippy passed; all **27 tests** passed (3 file-transaction unit tests, 12 document/export integration tests, 5 update integration tests and 7 software-rendering tests). Checks use static MSVC runtime linking. The active Windows dependency graph contains egui, egui-winit, egui_software_backend, softbuffer and winit, and excludes eframe/glow/glutin/wgpu and other GPU backends. The first implementation had 10 tests and the subsequent code review had 20; the software migration adds seven headless regressions.

Artifact: `dist/PrintHelper-v0.1.0.exe`, 8,840,704 bytes. SHA-256: `0857c3b7b13aa5038dd1e05339a1397fd4e7e834b8f3c44491ae61a0f00784cd`. The release build completed successfully. Read-only inspection of normal and delay imports passed: GDI is present; OpenGL/GPU presentation and external VCRUNTIME/MSVCP imports are absent. The packaged license notices match the regenerated notices for 157 dependency packages. The EXE was never executed.

- `cargo fmt --all -- --check`: formatting.
- `cargo clippy --offline --all-targets -- -D warnings`: all application/library/test targets.
- `cargo test --offline --locked`: twenty-seven synthetic tests, including Windows PDF rendering, file transactions with fake restart callbacks, and CPU rasterization in memory.
- `scripts/package.ps1 -Offline`: optimized Windows x64 GUI executable, versioned name, SHA256SUMS and license notices. Static MSVC runtime is selected in the repository configuration.
- `python scripts/check-software-backend.py`: inspect the active Windows dependency tree and reject OpenGL/GPU backend packages. Also runs through scripts/check.ps1 and CI.
- `python scripts/inspect-pe.py dist/PrintHelper-v0.1.0.exe`: read-only architecture/subsystem/direct-and-delay-import check; rejects VCRUNTIME/MSVCP and OpenGL/GPU presentation imports and requires GDI. It never executes the application.
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

## Software migration evidence

The CPU path is `egui -> egui_software_backend 0.0.2 -> retained BGRA bitmap -> softbuffer 0.4.8 Win32 DIB/BitBlt`. The exact pinned crate sources were inspected, including software rasterization, texture handling and the Windows GDI presenter. eframe, glow and glutin have been removed rather than selecting a software adapter behind a GPU API. Windows compositor behavior and internals of Windows PDF rendering are outside this UI-backend claim.

Seven in-memory tests establish: idle/timed invalidation settles; queued redraws do not retain expired timers; hidden requests wait for restore; exposure can reuse the retained frame; independent worker wakeups coalesce but continue without GUI frames; texture partial updates/frees, resizing and BGRA colors work; a font-atlas upload from a suppressed frame survives; invalid/oversized framebuffers fail without losing the last bitmap. These cases are grouped in seven tests. No winit event loop, native window, GDI surface, application instance or screenshot is created by these tests.

An initial texture test reproduced stale pixels in the renderer library's mesh cache after a partial texture update. That cache is disabled; the whole-window retained bitmap still avoids rasterization for unchanged OS exposures. Independent background wakeups avoid relying on an already-pending egui repaint while hidden. UI layout is separated from result/close processing. The native maximum client size is 4000×4000 physical pixels, within a 16-megapixel framebuffer budget.

## Code-review findings and fixes

| Priority | Finding | Fix and evidence |
| --- | --- | --- |
| P1 | A successful EXE replacement followed by process-creation failure left the new installation in place without rollback | A flushed independent backup precedes Windows ReplaceFile; replacement/restart errors restore the original bytes. Tests explicitly verify that the fake restart callback was reached, failed, and left the old contents plus backup intact. No executable is launched by these tests. |
| P2 | Main's generic helper-error handler launched an unchecked CLI target, including argument/wait failures that could leave the original running | Relaunch is inside the validated helper path, after parent exit and verification of restored original bytes. Earlier failures only record an adjacent helper error report when possible. Process waiting/relaunch still require runtime validation. |
| P2 | Resampling before cropping blended discarded image borders into lower-resolution preview/export | Crop original pixels before filtering; snap only floating-point noise at integer pixel boundaries. The formerly failing 1×1 border fixture now passes for 29/40/43% crops, four rotations and three output sizes. |
| P2 | Output staging could select the destination itself as a temporary name, including Windows case variants | Skip matching destination filenames with Windows case rules; tests confirm the saved file survives cleanup. Temporary handles are flushed and closed before publication/replacement. |
| P2 | SemVer's total ordering treated build metadata as an upgrade | Compare version precedence, with explicit equal-precedence build-metadata regression coverage. Reject repeated tag prefixes. |
| P2 | EXE validation accepted only a six-byte signature/machine marker, and read before enforcing its size budget | Bound actual reads; validate PE32+ GUI image flags, optional-header bounds, directory count, section ranges and executable entry point. Matching hashes on malformed fixtures still fail. Update redirects require HTTPS. These are structural checks, not a guarantee of OS/runtime compatibility. |
| P2 | PDF writer accepted nonfinite/invalid page or image dimensions; public native rendering lacked a pixel-budget guard | Reject invalid dimensions before writing a page or invoking WinRT. Tests cover bad dimensions without contaminating the next valid page. Source-document reads are bounded even if the source grows. |

Additional regressions cover cancellation after rendering, a destination created during export, temporary-file cleanup, and native PDF round trips across three landscape sheets with crop, rotation, grayscale and a partial final sheet. The suite never opens a file/print dialog or sends a print job.

## Product-design and rendering review

Applied the product-design and rust-performance skills to source/configuration review. Centralized palette, type and spacing; simplified the toolbar and File menu; grouped sheet controls; moved crop and printer guidance into contextual sections; added download progress and a preview retry state. Numeric normal-state color checks give body/background contrast 14.58:1, secondary/background 6.08:1, primary text/button 7.70:1 and error/background 6.91:1. These calculations do not verify actual font rendering, disabled/focus states, scaling, clipping or keyboard access.

No capture mode, GUI or computer use was launched, respecting the user's prohibition. Main and Settings screenshots remain pending. The rendering/load gate is **incomplete**, not passed: source and headless tests now establish a CPU rasterizer and GDI presenter with Wait/WaitUntil scheduling, while real native-window behavior is unmeasured. The old eframe/OpenGL path from the earlier review is superseded by this migration. For a later permitted run, `--render-diagnostics <new-file.json>` records aggregate UI/raster/presentation counts, suppressed redraws and stage timings at shutdown. It runs the normal application and is not a self-capture or benchmark mode; it was not invoked here. Before future visual work, apply the current gate and measure visible idle, hidden/minimized, restore, and draw/present behavior. No idle-load, smoothness or complete-UI visual claim is made from these code-only checks.

## Required manual checks before publication

1. When GUI testing is permitted, launch the packaged EXE on Windows 10 and 11 x64, including a machine without a separately installed Visual C++ redistributable. Verify the CPU/GDI backend, native bitmap presentation, initialization failures, normal/unsaved close, Unicode paths and a read-only executable folder. Inspect runtime backend evidence; a PE import check alone does not measure running behavior.
2. Check page-list selection, keyboard/mouse focus, reorder/delete/rotate/crop, bulk crop, Undo/Redo, project open/save cancellation, unsaved-change prompts, preview sheet navigation, 100/150/200% display scaling and minimum window size. Capture real main-window and Settings screenshots, then embed them in README.
3. Review representative PDFs in an independent PDF viewer: embedded/nonembedded fonts, mixed page sizes, intrinsic rotation, CropBox/MediaBox differences, transparency, annotations/forms, grayscale photographs, TIFF first-frame behavior, EXIF orientation, fine lines and small text at both DPI settings. Confirm export dimensions/ordering after deleting pages and across multiple sheets.
4. Measure idle CPU and frame/presentation latency in an optimized packaged build with empty and populated layouts, minimized/restored and during interaction. No smoothness or low-CPU claim is made from source inspection.
5. Only with later user authorization, print to a test destination: confirm driver paper/orientation mismatch and hard-margin rejection, one-up driver scaling, copies/collation, grayscale, multiple/partial sheets, cancellation, spooler errors and physical measurements. Do not treat a spooler success as confirmed paper output.
6. With two controlled published test releases, verify disabled startup checks make no request, enabled checks are nonblocking, no-update/startup-error results stay unobtrusive, manual errors/rate limits are clear, and correct compatible assets are selected.
7. Exercise live download progress/cancellation, digest/size mismatch, unwritable directory, process-exit timing, actual executable replacement/rollback/restart, preserved settings and recovery from staged/backup/temporary files and helper error reports. File-transaction tests already cover synthetic replacement and failed-restart rollback; real process lifecycle remains unverified. Never test this against the sole working installation or real user data.
8. Review dependency notices, intended repository/version/asset names, release notes and the unsigned executable's distribution experience. Publish only under separate explicit authorization through the connected GitHub plugin.

Screenshots, UI/hardware validation, independent-viewer checks and live update checks remain deferred under the user's explicit no-GUI/no-device/no-publication constraints. There is no release in this session to compare against; the release-note document is an initial-capability draft.
