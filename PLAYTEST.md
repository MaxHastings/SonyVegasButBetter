# Personal playtest

For the GitHub first-run ZIP, extract everything and run **Start SVBB.cmd**.
It downloads verified media tools once, then launches offline. For a locally
bundled package, launch **SonyVegasButBetter.exe**. Keep
`runtime` beside it. Open directly into an empty workspace. Start with copies of
recordings you can use for testing, and save a project before a long session.

For the crop workflow, crop different timeline sections independently with Preview's
Crop tool. Draw, move/resize, switch Free / Match project aspect, preview the result,
and Apply. Split the event and change just one half; undo and save/reopen to verify
the two framings. Escape cancels a draft; Reset + Apply clears one clip's crop.

Render a 30- or 60-second edited timeline with **Fit under a size limit → 10 MB**.
Leave automatic format adjustment enabled, then check the finished file's displayed
size and listen to the mix. Audio bitrate should adjust, while mix gain and timing
stay intact. Try an impractically small limit for a long edit: the app should explain
the constraint before starting. Auto uses GPU bitrate encoding when available;
choose CPU for whole-edit two-pass encoding. Both measure the final size and
retry overruns. Cancel during export and verify an existing
destination is preserved. Upload testing against your actual account is manual.

1. Import all recordings with Ctrl+I (multi-select) or drop files from Explorer.
   The left panel shows metadata and thumbnails; indexing runs in the background.
   Preview reads originals on demand. You can cut and seek while waveform
   indexing is still running; no source-wide video or PCM cache is required.
2. Double-click video files to append. Alternatively select a file and click
   Append selected, press Ctrl+Enter, or drag it onto the primary lane. Choose
   audio destination A1/A2/A3 for audio-only files. Alt+Up/Down changes the selected
   media item; Ctrl+Left/Right selects timeline clips.
3. Click the ruler to seek; Space plays/pauses, Left/Right steps one project frame.
   Ctrl+G opens exact timeline position in seconds. Ctrl+wheel zooms at the mouse;
   ordinary wheel and scrollbars pan. +/− and the Zoom slider change scale around
   the visible playhead. F / Fit fits the edit; press again to restore your view.
   Shift+F / Selection zooms to the selected clip; F restores the previous view.
   Alt+Left/Right jumps between cuts on the selected track, or video when no clip is selected.
4. Click inside a clip, then press S. Linked sound splits with it. Delete leaves a gap; Shift+Delete removes
   that interval and shifts unlocked lanes. Close gaps removes primary gaps.
   Undo/redo with Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y).
5. Drag edges to trim with live source preview and the opposite edge fixed.
   Hold a move or trim near either timeline edge to scroll without releasing the mouse.
   Top corners adjust fades. Body drags move linked A/V together and stop flush
   against neighbours. A live outline shows the legal drop; use
   Earlier/Later for neighbor swaps. Numeric Source in, Duration and Position
   fields provide precise alternatives. Click a numeric value to type it.
6. Append microphone clips to A2/A3 and set their Position fields. Set gain/fades
   in Clip Properties. Match voice loudness measures LUFS, targets −16 LUFS and
   limits unsafe boosts; Ctrl+L runs it for the selected clip. Lower gameplay
   with A1's dB control. Watch the actual stereo output meter.
7. Lock independent audio lanes before ripple operations if their positions must
   remain fixed. Ripple-delete trims/splits intersecting material on unlocked
   lanes. Ordinary numeric edits and reorder do not ripple other lanes.
8. Ctrl+S saves; Ctrl+Shift+S saves another version. Close the app and reopen the
   `.svbb` using Ctrl+O. Missing files appear in red; right-click and Relink source.
9. Render MP4 (Ctrl+R), choose Match project / 1080p30 / 1080p60, choose quality,
   choose Auto GPU/CPU/an explicit encoder, browse for a destination, and Render.
   The dialog shows the actual encoder, progress and estimated remaining time. Cancel leaves
   an existing destination intact. Success requires output probe/decode checks.
10. Open finished video from the dialog or your usual player. Check speech level,
    gameplay level, fade edges, every cut, end duration and audiovisual alignment.
    Reopen the project, change a cut, and export a second file.

On macOS use Command in place of Ctrl for command shortcuts.

## Acceptance workload

Use three gameplay files totaling 30–60 minutes, four microphones at mixed levels,
and at least one 1080p60 recording. Build a 3–8-minute result. Remove at least ten
segments, reorder three sections, position all four microphones, match loudness,
lower gameplay and add fades. Preview, save, close, reopen, export, independently
inspect the result, edit again and export again.

The provided synthetic acceptance dataset is reproducible with
`python scripts/make_acceptance_media.py`; it loops moving patterns and uses
tonal voice-like signals. It cannot establish intelligibility or performance on
every real gameplay recording. See TESTING.md for executed results.

## Troubleshooting and feedback

* Missing runtime: restore the package's runtime folder. Launch diagnostics are
  also written to `%TEMP%/svbb-launch-error.txt` if startup fails.
* Waveforms still building: playback and editing are already available. Auto waveform
  scale affects visibility only; disable it for an amplitude view. Manual gain reaches
  +60 dB; matching permits +48 dB subject to the true-peak ceiling.
* Disposable thumbnails/waveforms live in `%LOCALAPPDATA%/SVBB/cache-v1`. Old 0.1.0
  video/PCM caches are unused by this build. Recovery files remain outside that folder.
* Linked A/V shows a link arrow. Unlink in Clip Properties or the context menu for
  independent edits. Relinking requires aligned clips from the same source with
  matching source in/duration. Older projects separate audio automatically and retain
  a backup before migration.
* Codec/import error: the message includes the media tool failure. Try another
  recording; do not assume a recognized extension guarantees a working decoder.
* Quiet/silent source rejected by matching: listen first; adjust gain manually
  only if useful signal is present. Matching is not noise removal.
* Recovery prompt: recover the interrupted session and Save As. Your last manual
  save and its `.bak` are independent.

Report crashes, lost changes, wrong cuts, shifted audio, stalls, unexpected ripple
behavior and failed exports first. Include source codec/resolution/FPS, project
settings, the exact action and whether the issue reproduces after reopening.
