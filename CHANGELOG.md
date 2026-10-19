# Changelog

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
