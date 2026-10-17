# Personal playtest

Extract the Windows portable folder and launch **SonyVegasButBetter.exe**. Keep
`runtime` beside it. Open directly into an empty workspace. Start with copies of
recordings you can use for testing, and save a project before a long session.

1. Import all recordings with Ctrl+I (multi-select) or drop files from Explorer.
   The left panel shows metadata and thumbnails; indexing runs in the background.
   First timeline use prepares each video's playback cache with real progress.
   Wait for Preview ready before playback. Long recordings may take a minute or
   more initially; later cuts and reopens reuse the caches.
2. Double-click video files to append. Alternatively select a file and click
   Append selected, press Ctrl+Enter, or drag it onto the primary lane. Choose
   audio destination 1/2 for audio-only files. Alt+Up/Down changes the selected
   media item; Ctrl+Left/Right selects timeline clips.
3. Click the ruler to seek; Space plays/pauses, Left/Right steps one project frame.
   Ctrl+G opens exact timeline position in seconds. Fit and +/− adjust zoom;
   scrollbars pan long timelines. Ctrl+wheel zooms over the timeline.
4. Select a clip, seek inside it, press S. Delete leaves a gap; Shift+Delete removes
   that interval and shifts unlocked lanes. Close gaps removes primary gaps.
   Undo/redo with Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y).
5. Drag clip edges to trim. Drag a primary clip onto another to reorder; use
   Earlier/Later for neighbor swaps. Numeric Source in, Duration and Position
   fields provide precise alternatives. Click a numeric value to type it.
6. Append microphone clips to A1/A2 and set their Position fields. Set gain/fades
   in Clip Properties. Match voice loudness measures LUFS, targets −16 LUFS and
   limits unsafe boosts; Ctrl+L runs it for the selected clip. Lower gameplay
   with the first track's dB control. Watch the actual stereo output meter.
7. Lock independent audio lanes before ripple operations if their positions must
   remain fixed. Ripple-delete trims/splits intersecting material on unlocked
   lanes. Ordinary numeric edits and reorder do not ripple other lanes.
8. Ctrl+S saves; Ctrl+Shift+S saves another version. Close the app and reopen the
   `.svbb` using Ctrl+O. Missing files appear in red; right-click and Relink source.
9. Render MP4 (Ctrl+R), choose Match project / 1080p30 / 1080p60, choose quality,
   browse for a destination, and Render. Progress comes from FFmpeg. Cancel leaves
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
* Changed mix preparing: wait for Preview ready; video sources need an initial
  playback cache, and changed audio clips need PCM generation. The UI stays
  available. PCM uses about 23 MB/minute; compressed video cache size depends on
  content (the 30-minute synthetic dataset used about 1.75 GB). The common silence
  file has a 1.38 GB logical size. Disposable caches live in
  `%LOCALAPPDATA%/SVBB/cache-v1` on Windows and can be removed while the app is
  closed; they will regenerate. Recovery files are kept outside this subfolder.
* Codec/import error: the message includes the media tool failure. Try another
  recording; do not assume a recognized extension guarantees a working decoder.
* Quiet/silent source rejected by matching: listen first; adjust gain manually
  only if useful signal is present. Matching is not noise removal.
* Recovery prompt: recover the interrupted session and Save As. Your last manual
  save and its `.bak` are independent.

Report crashes, lost changes, wrong cuts, shifted audio, stalls, unexpected ripple
behavior and failed exports first. Include source codec/resolution/FPS, project
settings, the exact action and whether the issue reproduces after reopening.
