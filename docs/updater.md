# Shared updater integration

`src-tauri/src/updater.rs` connects ClipCat to
[`catninth-updater`](https://github.com/catninth/updater). The Git dependency is
pinned in `src-tauri/Cargo.toml` and `Cargo.lock`; a sibling checkout is unnecessary.

## Configuration and release contract

- `REPOSITORY`: `catninth/clipcat`.
- `CHECK_INTERVAL_MINUTES`: 360, with a 20-second startup delay.
- Installed version: Tauri package metadata, sourced from `src-tauri/Cargo.toml`.
- Public key and passive NSIS mode: `plugins.updater` in `tauri.conf.json`.
- Request timeout: 120 seconds for metadata, manifests, and downloads.

The library checks the latest stable GitHub release, compares semantic versions,
and exposes its release body as patch notes. Pre-releases/nightlies are excluded.
On install, the Tauri adapter reads `latest.json` from the selected release tag and
verifies that its version matches. The existing release workflow publishes the
manifest, native bundles, and Minisign signatures. No signing-key rotation is needed.

The library's Tauri adapter supports Windows, Linux, and macOS. ClipCat's existing
release workflow still ships Windows NSIS and Linux AppImage, `.deb`, and `.rpm`;
this integration does not add macOS capture support or a macOS release job.

## Application hooks

1. Before downloading, reject installation while a recording or clip save is active.
2. After downloading and signature verification, recheck the engine under the
   capture operation lock and set the installation flag. Capture starts and
   settings changes use this same lock and flag.
3. Retain the installation reservation until installation and restart preparation
   finish. On failure, release the flag so capture can resume.
4. On Windows, stop the engine and release the single-instance lock immediately
   before the installer takes over. On Linux/macOS, perform that cleanup after
   installation and before restarting.

ClipCat retains the polling handle for its lifetime. The library serializes checks
and installs and emits update-available notifications once per version. ClipCat
maps those to its existing localized toast, log, and `update` event.

## Frontend contract and tests

The existing commands (`get_update_state`, `check_update`, `install_update`) and
UI payload are preserved. Only newer, unapplied versions are offered for install;
failed attempts retain their version and patch notes for retry. Download events
are forwarded only when the visible state or percentage changes.

Run `npm test`, `npm run build`, then
`cargo test --locked --manifest-path src-tauri/Cargo.toml -- --test-threads=1`.
Native regression tests cover version/phase mapping, progress, recording started
during download, shutdown rejection, and the race between capture and installation.
