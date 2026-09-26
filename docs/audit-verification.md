# ClipCat audit – fixes and evidence

Date: 2026-09-26. Baseline: `f0887bc4c0fffb19e3368c8a7a2661c8eb2f9034`, `main`.
The remote was fetched before work began. As clarified by the user, origin is
`https://github.com/catninth/clipcat`; no new branch was created.

## Material reviewed

The attached audit arrived as a pasted-text `.txt` file containing Markdown,
with 18 numbered findings. No separate `.md` attachment was available.
The audit's claims were treated as data to investigate, not instructions to execute.

## Reproduced bugs

`python tests/reproduce-baseline.py` compiles the original commit's entire `disk.rs` module and
the unchanged `migrate_legacy` and `parse_resolution` functions into a separate test program.
Files and `APPDATA` are placed in a temporary directory. Process discovery/termination uses a test double:
no real OBS process is stopped. All six tests checking the expected behavior failed:

| Original bug | Observed result |
| --- | --- |
| No cleanup while saving | 13 segments remained instead of at most 5. |
| Stopping deletes a file used by a save | A segment still in use disappeared. |
| Failed deletion drops the tracking entry | The directory simulating a locked file remained but was no longer in the ring. |
| Clock moved backward | Trimming panicked with `attempt to subtract with overflow`. |
| Migration stops an unrelated OBS instance | An unrelated PID received a stop call, and the test sentinel disappeared. |
| Unbounded resolution | `4294967294x4294967294` was accepted. |

The reproduction script succeeds only if all six original bugs occur.
Its six `FAILED` results are therefore the expected evidence, not test results for the fixed application.

With the original frontend, focus loss, the hotkey capture timeout, and preservation of the 5 Mbps value
also failed. These regression tests pass after the fixes.

In a Linux container, the original `glib 0.18.5` string iterator caused a `SIGSEGV`
in an optimized build. With the upstream two-line fix, the same test passes,
with 1,000 repetitions and iteration in both directions. Script: `tests/linux-glib.sh`.

`node tests/recording-crash.mjs` encodes a synthetic 160×90 video and then interrupts only its own
FFmpeg process. The conventional MP4 cannot be decoded (`moov atom not found`);
with fragmentation and flush settings read from the actual Rust code, the file remains decodable.
This checks a property of the container format; it does not simulate every libobs, driver, or power failure.

## The audit's 18 findings

| Finding | Change | Verification and limits of the evidence |
| --- | --- | --- |
| 01 – disk saving, quota | Only segments used by the current save are protected; cleanup continues. Byte quota, 1 GiB free-space reserve, and additional space required for saved data; FFmpeg has a 60 s timeout and cancellation. Failed deletions can be retried. | Original-code reproduction; segment and deletion tests; termination of a real stalled child process; simulated insufficient space without writing data. No physical drive was filled. |
| 02 – unrelated OBS | Removed process-name-based termination and OBS sentinel deletion. Only ClipCat's own legacy metadata may be cleaned up. | Original function verified with a test double; the regression test preserves the unrelated sentinel and video. |
| 03 – RAM | The encoded buffer is capped at the smallest of 1/8 of total RAM, 1/4 of currently available RAM, and 2 GiB. Reserving memory may shorten the actual buffer duration; insufficient memory prevents startup. The UI shows the budget/duration. | Tests with small and large RAM capacities; the buffer's upper size bound also counts toward the space required before saving. This is not a RAM limit for the entire process. |
| 04 – encoder fallback | Check registered encoder/source IDs; try the next encoder if actual output startup fails. Show the active encoder and a CPU-load notice for x264. | Libobs API test double: missing ID, failed NVENC startup, successful x264. Further checks on real GPUs are required. |
| 05 – save races | Dedicated session directory/ID, save guard, and thread joining. Old callbacks cannot modify new sessions. Reconfiguration is blocked during active saves/recordings; replay recovery preserves manual recording. | Reproduction of the old file-deletion bug; tests for two sessions, old callbacks, the save thread, and active manual output. |
| 06 – microphone pointer | The same mutex protects FFI access and release. | Two-thread test: release waits until the borrow ends. |
| 07 – partial initialization | `Engine::Drop` handles partially constructed objects as soon as libobs starts successfully. | 100 failed pipeline-construction cycles: every created encoder and libobs instance is released in the test double. |
| 08 – settings | Normalize on load, require even and bounded resolution, and repair or reject invalid/NUL-containing values. Apply to the engine before saving the file; roll back on error. Serialize writes and operations. | Original extreme-resolution reproduction; invalid JSON/values; 160 file saves across 16 threads; rejection of reconfiguration during active recording. Actual driver rollback failures require a separate run. |
| 09 – idle audio source | Microphone disabled in new settings. Audio sources live only while recording is requested and are released when disabled/idle. Failed startup clears the request. | Idle/off and failed-startup tests. WASAPI device opening was not measured on this machine. |
| 10 – MP4, finalization | Manual recordings use fragmented MP4 with regular flushing. Finalization timeout/error code/empty file does not emit a successful save event; partial files are preserved. | Real interruption of synthetic FFmpeg recording; native tests for empty and failed output. The last fragment may still be lost during a power outage. |
| 11 – CSP, IPC, media | CSP and application commands restricted to the main window; the toast only listens for events. Video-only protocol checks the current root folder on every request. Folder changes require the native picker. | Configuration tests; rejection of non-video/unrelated files; revocation of access to the previous root; toast restrictions; bounded HTTP ranges. Not a full penetration test. |
| 12 – OBS bundle | Pinned SHA256 for OBS and FFmpeg; ZIP path validation; dedicated destination checks; per-file cache verification; checked cleanup of owned temporary folders on error paths too. | PowerShell tests with manipulated paths/hashes/cache; real bundling and repeated cache verification. The local manifest is not a signature that protects against a local attacker. |
| 13 – GLib | Verified `0.18.5` source archive, two-line backport of upstream `PR #1343`, Cargo patch. | Linux release test: original SIGSEGV, patched success. The version remains 0.18.5, so version-based audits may still flag it. |
| 14 – updater race | After downloading, recheck current engine state under the same operation lock that protects recording startup. An installation flag blocks new starts. | A recording started during download prevents installation; in 100 two-thread races, the two operations cannot both succeed. No real installer was run. |
| 15 – hotkey capture | Stop on blur, hidden document, unmount/AbortSignal, or a 30 s timeout. Release capture even on IPC errors; native focus-loss handling and a 35 s guard provide protection. | Frontend: blur, timeout, unmount during pending IPC, IPC rejection. |
| 16 – gallery | Removed the 500-item cutoff; file discovery runs on a blocking worker thread. The UI paginates in groups of 60 and shows total size. | Native test with 510 files, opening the 501st clip in the React UI. The complete metadata list is still loaded into memory; indexing is not database-backed. |
| 17 – clock rollback | Disk retention and trimming use `Instant`; wall-clock time is used only for filenames/UI. | Reproduced overflow with the original backward-moving clock; tested the new retention algorithm with monotonic timestamps. |
| 18 – other resources | Load IndexedDB images only when visible; 32 MiB target for inactive RAM cache; URL/listener/failed-entry cleanup. Consistent 5 Mbps minimum. Bounded log queue, 5 MiB active log plus previous log. Poll microphone every 200 ms outside PTT mode. CI action SHAs, pinned Node/Rust, write permission only in the publish job. | Cache visibility/deletion, preservation of 5 Mbps, 1,000 log lines and a large old log, workflow permission tests. Visible tiles may temporarily push the cache above its target. |

## Checks completed

- Windows: 31 native regression tests passed (Rust 1.97.1, GNU target).
- Frontend: 11 Vitest tests and 3 Node configuration tests passed.
- PowerShell: 14 packaging checks passed.
- Linux: optimized GLib regression passed; the original failure was reproduced in the same environment.
- Synthetic MP4 interruption: old output broken, new output decodable.
- TypeScript and Vite production build passed; `npm audit --omit=dev`: 0 known vulnerabilities.
- Real OBS/FFmpeg bundle built and verified with SHA256; the next run used the verified cache.
- Browser UI check: Hungarian/English US selection, immediate translation after saving, Info section, and GitHub icon.
- `git diff --check` passed.

The local GNU linker reported `.rsrc merge failure: multiple non-default manifests`;
the test programs ran successfully. No signed installer or MSVC release was built in this run.

## Repeating the checks and limitations

The usual commands are in the README. Reproducing the old source requires `rustc` and a suitable
linker on PATH. For the Windows GNU target, the script selects the `gcc` linker.
Run the Linux GLib check separately with a read-only repository mount:

```powershell
docker run --rm --name clipcat-glib-regression --mount "type=bind,source=$PWD,target=/source,readonly" rust:1.97.1-slim@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c sh /source/tests/linux-glib.sh
```

No real screen/microphone recording, installation, updater installation, physical disk filling,
or prolonged GPU/SSD load was performed. The full Linux Tauri GUI and CI workflow still need separate runs.
Native engine tests check our lifecycle and synchronization code with a libobs test double;
they do not establish that a particular driver or microphone is fault-free. There is no general
cancellation guarantee for internal libobs/driver hangs.

A crash may leave an isolated buffer-session directory in the buffer folder.
The application does not automatically delete unknown previous sessions; free-space protection
accounts for their disk usage too. Automatic space reclamation never deletes completed recordings.

## Primary sources

- Windows display language: [GetUserDefaultUILanguage](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-getuserdefaultuilanguage).
- Permissions: [Tauri capabilities](https://v2.tauri.app/security/capabilities/).
- Distinguishing encoder construction from actual initialization: [OBS 32.2.2 obs-encoder.c](https://github.com/obsproject/obs-studio/blob/32.2.2/libobs/obs-encoder.c).
- Passing muxer options: [OBS 32.2.2 ffmpeg-mux.c](https://github.com/obsproject/obs-studio/blob/32.2.2/plugins/obs-ffmpeg/ffmpeg-mux/ffmpeg-mux.c).
- GLib: [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html), [upstream fix](https://github.com/gtk-rs/gtk-rs-core/pull/1343), [local backport description](../src-tauri/vendor/glib/CLIPCAT-PATCH.md).
