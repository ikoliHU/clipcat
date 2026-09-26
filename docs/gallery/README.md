# ClipCat feature gallery

Real ClipCat React UI captured in **English US**, with the same linked, two-column layout as the GitCat gallery. Click any image to open the full-resolution PNG.

The main views use a 1180×900 viewport, saved at 2× resolution (2360×1800). Notifications use the app’s 380×96 window size, also saved at 2× resolution.

These captures use a browser fixture with simulated clips, devices, recording status, and update responses. Alpine Drift, Orbital Run, and Pine Valley are original geometric sample scenes, not footage from commercial games. Recording and update installation were not run. The interface is the unmodified ClipCat v0.6.0 UI at commit `5813bd2`.

[Gallery](#gallery) · [Playback & clip actions](#playback--clip-actions) · [Recording & replay](#recording--replay) · [Capture settings](#capture-settings) · [Audio & keyboard shortcuts](#audio--keyboard-shortcuts) · [System & app info](#system--app-info) · [Updates](#updates) · [Notifications](#notifications)

## Gallery

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="gallery/overview.png"><img src="gallery/overview.png" alt="ClipCat gallery overview" width="100%"></a><br>
      <strong>Gallery overview</strong><br>
      Game filters, thumbnail previews, clip durations, timestamps, file sizes, and replay controls in one window.
    </td>
    <td width="50%" valign="top">
      <a href="gallery/game-filter.png"><img src="gallery/game-filter.png" alt="ClipCat filter by game" width="100%"></a><br>
      <strong>Filter by game</strong><br>
      Focus on one game's clips while keeping the matching clip count and total size visible.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <a href="gallery/pagination.png"><img src="gallery/pagination.png" alt="ClipCat large libraries" width="100%"></a><br>
      <strong>Large libraries</strong><br>
      Browse a library of 65 sample clips with Previous and Next controls; this is the second page.
    </td>
    <td width="50%" valign="top">
      <a href="gallery/empty-state.png"><img src="gallery/empty-state.png" alt="ClipCat first clip" width="100%"></a><br>
      <strong>First clip</strong><br>
      The empty gallery points to the configured save shortcut.
    </td>
  </tr>
</table>

## Playback & clip actions

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="player/playback.png"><img src="player/playback.png" alt="ClipCat built-in player" width="100%"></a><br>
      <strong>Built-in player</strong><br>
      Play, pause, seek, adjust volume, or enter full screen; reveal the clip in its folder or open an external player.
    </td>
    <td width="50%" valign="top">
      <a href="player/delete-confirmation.png"><img src="player/delete-confirmation.png" alt="ClipCat delete confirmation" width="100%"></a><br>
      <strong>Delete confirmation</strong><br>
      The Delete button changes to an explicit confirmation before a clip is removed.
    </td>
  </tr>
</table>

## Recording & replay

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="recording/manual-recording.png"><img src="recording/manual-recording.png" alt="ClipCat manual recording" width="100%"></a><br>
      <strong>Manual recording</strong><br>
      The red Stop button shows elapsed recording time while the replay buffer remains available.
    </td>
    <td width="50%" valign="top">
      <a href="recording/buffer-filling.png"><img src="recording/buffer-filling.png" alt="ClipCat replay buffer progress" width="100%"></a><br>
      <strong>Replay buffer progress</strong><br>
      A live progress bar and duration show how much recent footage is available to save.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <a href="recording/replay-paused.png"><img src="recording/replay-paused.png" alt="ClipCat pause replay capture" width="100%"></a><br>
      <strong>Pause replay capture</strong><br>
      The replay switch pauses buffering and disables Save now while manual recording stays available.
    </td>
    <td width="50%" valign="top">
      <a href="recording/cpu-encoding.png"><img src="recording/cpu-encoding.png" alt="ClipCat encoder status" width="100%"></a><br>
      <strong>Encoder status</strong><br>
      The status card identifies the encoder and shows the extra-load notice for CPU encoding.
    </td>
  </tr>
</table>

## Capture settings

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="settings/capture.png"><img src="settings/capture.png" alt="ClipCat capture overview" width="100%"></a><br>
      <strong>Capture overview</strong><br>
      Save folder, replay length, RAM budget, resolution, frame rate, bitrate, codec, and desktop capture.
    </td>
    <td width="50%" valign="top">
      <a href="settings/disk-buffer.png"><img src="settings/disk-buffer.png" alt="ClipCat disk buffering" width="100%"></a><br>
      <strong>Disk buffering</strong><br>
      A ten-minute replay buffer with a separate buffer folder, estimated clip size, and continuous-write guidance.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <a href="settings/high-quality.png"><img src="settings/high-quality.png" alt="ClipCat 1440p at 120 fps" width="100%"></a><br>
      <strong>1440p at 120 FPS</strong><br>
      HEVC capture with an automatically recommended bitrate and the available RAM replay length.
    </td>
    <td width="50%" valign="top">
      <a href="settings/compact-quality.png"><img src="settings/compact-quality.png" alt="ClipCat 720p at 30 fps" width="100%"></a><br>
      <strong>720p at 30 FPS</strong><br>
      A lower-resolution H.264 configuration with a smaller estimated clip size; the Save bar indicates pending changes.
    </td>
  </tr>
</table>

## Audio & keyboard shortcuts

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="settings/push-to-talk.png"><img src="settings/push-to-talk.png" alt="ClipCat push-to-talk" width="100%"></a><br>
      <strong>Push-to-talk</strong><br>
      Choose a microphone and a talk key alongside the four configurable application shortcuts.
    </td>
    <td width="50%" valign="top">
      <a href="settings/push-to-talk-key.png"><img src="settings/push-to-talk-key.png" alt="ClipCat capture a talk key" width="100%"></a><br>
      <strong>Capture a talk key</strong><br>
      Assign a keyboard key or a supported middle or side mouse button; Escape cancels capture.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <a href="settings/microphone-device.png"><img src="settings/microphone-device.png" alt="ClipCat always-on microphone" width="100%"></a><br>
      <strong>Always-on microphone</strong><br>
      Select a headset microphone for continuous voice capture instead of push-to-talk.
    </td>
    <td width="50%" valign="top">
      <a href="settings/keyboard-shortcuts.png"><img src="settings/keyboard-shortcuts.png" alt="ClipCat keyboard shortcuts" width="100%"></a><br>
      <strong>Keyboard shortcuts</strong><br>
      Customize Save clip, Start / stop recording, Last clip folder, and Open gallery.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <a href="settings/shortcut-capture.png"><img src="settings/shortcut-capture.png" alt="ClipCat capture a shortcut" width="100%"></a><br>
      <strong>Capture a shortcut</strong><br>
      The shortcut button waits for a key combination and displays the cancellation hint.
    </td>
    <td></td>
  </tr>
</table>

## System & app info

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="settings/system-and-info.png"><img src="settings/system-and-info.png" alt="ClipCat system preferences" width="100%"></a><br>
      <strong>System preferences</strong><br>
      English US, save notifications, notification sounds, start at sign-in, capture recovery, version, license, and GitHub.
    </td>
    <td width="50%" valign="top">
      <a href="settings/saved.png"><img src="settings/saved.png" alt="ClipCat save confirmation" width="100%"></a><br>
      <strong>Save confirmation</strong><br>
      The floating save bar confirms that the edited settings were saved.
    </td>
  </tr>
</table>

## Updates

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="updates/available.png"><img src="updates/available.png" alt="ClipCat update available" width="100%"></a><br>
      <strong>Update available</strong><br>
      A simulated release displays sample notes, the sidebar update entry, and Install and restart.
    </td>
    <td width="50%" valign="top">
      <a href="updates/download-progress.png"><img src="updates/download-progress.png" alt="ClipCat download progress" width="100%"></a><br>
      <strong>Download progress</strong><br>
      The simulated download percentage appears in both the sidebar and the Info section.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <a href="updates/checking.png"><img src="updates/checking.png" alt="ClipCat update check in progress" width="100%"></a><br>
      <strong>Checking for updates</strong><br>
      The Info section displays the current version and disables the check button while a check is in progress.
    </td>
    <td></td>
  </tr>
</table>

## Notifications

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="notifications/clip-saved.png"><img src="notifications/clip-saved.png" alt="ClipCat clip saved" width="100%"></a><br>
      <strong>Clip saved</strong><br>
      A success notification includes the game and the shortcut for opening the clip folder.
    </td>
    <td width="50%" valign="top">
      <a href="notifications/saving-clip.png"><img src="notifications/saving-clip.png" alt="ClipCat saving a clip" width="100%"></a><br>
      <strong>Saving a clip</strong><br>
      A pending notification identifies the game while the clip is being saved.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <a href="notifications/recording-started.png"><img src="notifications/recording-started.png" alt="ClipCat recording started" width="100%"></a><br>
      <strong>Recording started</strong><br>
      A recording notification includes the shortcut used to stop the recording.
    </td>
    <td width="50%" valign="top">
      <a href="notifications/capture-error.png"><img src="notifications/capture-error.png" alt="ClipCat capture recovery" width="100%"></a><br>
      <strong>Capture recovery</strong><br>
      An error notification reports stopped capture and the restart status.
    </td>
  </tr>
</table>

[Back to the project README](../../README.md)
