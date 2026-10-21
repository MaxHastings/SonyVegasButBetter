# Changelog

## 0.1.9 — A workspace built around Preview

- Give Preview more room with narrower sidebars and a compact timeline by default.
- Add Expand / Restore layout and Ctrl+Shift+F for a preview focus view; playback
  and framing remain available. Escape closes framing first, then exits focus.
- Add Media / Properties sidebar toggles, remembered panel sizes/visibility, a
  scrollable inspector and View → Reset workspace layout.
- Fit Preview within the available width and height; compact its controls while
  keeping the image stable when a framing gesture reveals contextual controls.
- Keep compact audio waveforms below their labels and track gain/mute/lock handy.
- Test normal, minimum and ultrawide windows, focus/restore, direct framing and
  preferences across a restart; rerun existing editing GUI stages.
- Refresh the expired libmpv download pin to the 20261018 build; add its upstream
  SourceForge mirror with the same required hash and retry-safe bootstrap downloads.

## 0.1.8 — Reliable live trimming preview

- Keep the preview updating while holding either clip edge: coalesce mouse motion
  and let each boundary frame reach the display before requesting the latest one.
- Stop restarting unchanged boundary seeks on every UI repaint.
- Give timeline drags preview priority over an open framing tool.
- Use the linked video event’s framing when trimming from its audio event.
- Show the source timecode beside the preview during a trim.
- Add a native GUI regression for both edges, linked audio, held-pointer changes,
  undo, save and export; it reproduces the failure on the previous build.

## 0.1.7 — Direct framing and manual pan/zoom

- Draw directly in Preview and release to commit; remove Apply/Cancel steps.
- Adjust frame reveals the source, with move/corner resize and Alt+drag redraw.
- Make every gesture one undo; Escape cancels an active drag.
- Add explicit Animate, editable source-time motion points, point navigation/removal,
  selected-clip timeline diamonds and freeze-at-current-view when disabled.
- Preserve motion through splits, trims, moves, undo, schema-5 saves and migration.
- Evaluate the same animated framing in preview, trim feedback and CPU/GPU exports,
  including size limits. Static framing remains the default; tracking is deferred.
- Allow in-flight animated playback frames to finish instead of cancelling on every moving crop.
- Add native GUI and real-media pixel regressions for framing, animation and playback.

## 0.1.5 — Responsive preview and timeline navigation

- Send visual seeks directly to a latest-request mailbox, independently of audio
  loading. Serialize/coalesce audio reloads and retain the latest play intent.
- Reject obsolete frames, measure first-frame/audio readiness and texture-upload
  latency, and reuse up to 32 MiB of frames and three source decoders.
- Reuse original source pixels across timeline edits; crop and fade changes remain
  correct. Background waveform/thumbnail work yields CPU priority on Windows.
- Add Alt+Left/Right to jump between cuts on the selected track (video by default),
  Shift+F / Selection to zoom to a clip, and F to restore the prior view.
- Auto-scroll left/right while moving or trimming at timeline edges; retain fixed
  opposite trim edges, linked audio/video and one-step undo.
- Navigation reveals an off-screen playhead, including Home/End and exact seeks.
- Keep repeated clip clicks at the pointer position instead of jumping to the clip start.
- Show delayed seek feedback and add measured long-recording/native UI regressions.

## 0.1.4 — Preview seek fix

- Fix “Seek failed” errors when moving, trimming or deleting clips shortens the
  timeline while the playhead is beyond the new end.
- Bound reloads and seeks to the current edit, discard queued seeks for older
  edits, and handle end-of-file probes without reporting a media failure.
- Save bounded local playback logs and expose their path in error dialogs with
  a Copy error details button. No telemetry or automatic log uploads.
- Add real-runtime and native GUI regressions for shortening edits and resuming
  playback, plus virtual stream and log retention checks.

## 0.1.3 — First Windows preview

- Edit long recordings immediately using on-demand playback and progressive waveforms.
- Separate linked audio/video; one-click selection/playhead positioning and S to split.
- Trim with live source preview and fixed opposite edges; drag top corners to fade.
- Move clips flush against neighbours with a live outline and synchronized sound.
- Zoom at the mouse with Ctrl+wheel, pan with the wheel, and fit/restore with F.
- Crop each video section independently; splitting inherits the crop for each half.
- Fit exports under a custom byte ceiling, with GPU bitrate encoding or CPU two-pass.
- Automatic audio bitrate and optional resolution/frame-rate reduction for tight limits.
- GPU probing, CPU fallback in Auto, progress/ETA, cancellation and output validation.
- Auto-scaled quiet waveforms, loudness matching, clip/track gains and peak protection.
- SQLite projects, undo/redo, recovery saves and missing-media relinking.

The public Windows ZIP includes a first-run launcher: it downloads checksum-pinned
media tools directly from upstream once, then runs offline. Static compiler runtime and app-local Vulkan loader remove separate runtime
installer requirements. No admin access, Rust,
or separate archive tool is needed. Windows 10/11 x64; unsigned early preview.

Verified locally: 28 model/UI/budget tests, nine real-media tests, 170 native GUI
steps and actual NVIDIA size-limited exports. Editor CI passes on Windows, Linux
and macOS. Other GPU hardware and non-Windows desktop interaction remain untested.

Project format is schema 4, as in 0.1.2. Older executables cannot read schema-4
projects. Source media remains external to project files.

## Earlier local builds

0.1.2 added per-clip cropping and CPU size-limited exports. 0.1.1 introduced
on-demand playback, separate linked audio/video and direct trim/fade controls.
0.1.0 established the edit model, native UI, save/recovery and export pipeline.
Those builds were local playtests; v0.1.3 is the first GitHub Windows preview.
