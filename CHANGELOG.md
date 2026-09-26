# Changelog

## 0.6.0 – 2026-09-26

### Updates

- Use the shared `catninth-updater` Rust library for stable GitHub release checks, patch notes, signed downloads, and installation.
- Preserve automatic checks every six hours, localized notifications, and user-triggered installation.
- Recheck recording and clip-saving activity after downloading and reserve installation under the capture operation lock.
- Stop the capture engine and release the single-instance lock before handing off to the Windows installer or restarting after installation.
- Add updater state, progress, version precedence, and capture/installation regression tests.

## 0.5.0 – 2026-09-26

### Added

- Hungarian and English US translations, with the initial selection based on the Windows display language.
- Info section at the bottom of Settings: update check, license link, and GitHub icon.

### Fixed

- Disk save timeout, segment protection, session identification, storage and RAM limits.
- Recording engine and microphone lifecycle; working fallback after hardware encoder failures.
- Settings validation and serialized saving; protection for active recordings during reconfiguration and updates.
- Fragmented MP4 for manual recordings; finalization error reporting and preservation of partial files.
- Restricted IPC/CSP/file access, verified OBS packaging, and a GLib security backport.
- Hotkey capture cancellation, paginated gallery, bounded thumbnail cache and logging.
- CI and release workflows build the embedded UI before running native tests.

### Verification

- Regression tests and audit evidence: `docs/audit-verification.md`.

## 0.4.1 – 2026-09-21

### Fixed

- Hotkeys now work while games that swallow Windows global hotkey events have focus, such as League of Legends in borderless mode.
- Prevent duplicate actions between native hotkey events and fallback key polling.
