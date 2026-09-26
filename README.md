# clipcat

ShadowPlay-style replay recorder with an embedded OBS capture engine, a clip gallery, and configurable recording controls.

## Screenshots

<p align="center">
  <a href="docs/gallery/gallery/overview.png">
    <img src="docs/gallery/gallery/overview.png" alt="ClipCat gallery with game filters, clip previews, and replay controls" width="100%">
  </a>
</p>

<p align="center">
  <a href="docs/gallery/player/playback.png">
    <img src="docs/gallery/player/playback.png" alt="ClipCat built-in video player and clip actions" width="49%">
  </a>
  <a href="docs/gallery/settings/capture.png">
    <img src="docs/gallery/settings/capture.png" alt="ClipCat capture settings for replay length, quality, and desktop recording" width="49%">
  </a>
</p>

<p align="center">
  <a href="docs/gallery/settings/push-to-talk.png">
    <img src="docs/gallery/settings/push-to-talk.png" alt="ClipCat microphone, push-to-talk, and keyboard shortcuts" width="49%">
  </a>
  <a href="docs/gallery/settings/disk-buffer.png">
    <img src="docs/gallery/settings/disk-buffer.png" alt="ClipCat disk buffer settings for longer replays" width="49%">
  </a>
</p>

<p align="center">
  <a href="docs/gallery/README.md"><strong>Browse the full feature gallery →</strong></a>
</p>

Screenshots show the English UI with sample clips and simulated capture state. See the gallery for all 28 images and capture details.

## Language and Info

The **Language** field in Settings switches between Hungarian and English US. On first launch,
the Windows display language determines the default: Hungarian for Hungarian Windows, English US otherwise.
A saved manual selection is retained for subsequent launches. After saving, the UI, tray menu, and notifications
switch languages without restarting.

The **Info** section at the bottom of Settings contains the version, update check, license link,
and GitHub icon. The repository is [catninth/clipcat](https://github.com/catninth/clipcat).
The License button opens the specified [MPL-2.0 license file](https://github.com/catninth/cutcat/blob/main/LICENSE).

## Releases and updates

ClipCat uses the shared [`catninth-updater`](https://github.com/catninth/updater)
Rust library. It checks stable releases from `catninth/clipcat` after 20 seconds,
then every six hours, and displays the GitHub release body as patch notes.
Installation is user-triggered and uses the selected tag's signed `latest.json`:
NSIS on Windows, AppImage replacement or `.deb`/`.rpm` installation through `pkexec`
on Linux. Recording and clip-saving guards remain in ClipCat.
See [the updater integration guide](docs/updater.md) for configuration and lifecycle details.

To publish a new version:

1. Bump the version in `src-tauri/Cargo.toml` and the `clipcat` entry in `src-tauri/Cargo.lock`
   (`tauri.conf.json` uses the Cargo version), then update `CHANGELOG.md`.
2. After committing and pushing, open Actions → **release** → *Run workflow*, enable `publish`,
   and provide English release notes following the format of previous releases.
   This creates a `v<version>` release with installers, signatures, and `latest.json`.

The `release` workflow also creates a `nightly` prerelease every night; the updater ignores these.

The updater verifies packages with a minisign key. The public key is in `tauri.conf.json`;
the private key and its password are stored in the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. If the private key is lost, existing installations
can no longer receive updates.

## Development

The UI uses React + Tailwind CSS (Vite), with source files in `ui/`; translations live in
`ui/locales/` and are also loaded by Rust.

```sh
npm install
npx tauri dev    # Vite dev server + application
npx tauri build  # installer package
```

## Verification

```sh
npm ci
npm test
npm run build
# Windows: generate the configured resources for native tests as well.
powershell -NoProfile -ExecutionPolicy Bypass -File bundle-obs.ps1
cargo test --locked --manifest-path src-tauri/Cargo.toml -- --test-threads=1
powershell -NoProfile -ExecutionPolicy Bypass -File tests/bundle.test.ps1
node tests/recording-crash.mjs
```

`npm run preview:ui` starts a separate browser test UI at `127.0.0.1:5174`.
It uses the real React components with simulated native responses; it does not start recording,
run an installer, or overwrite the application's saved settings.

For reproductions of the original audit findings, fixes, and test limitations, see
[audit-verification.md](docs/audit-verification.md).
