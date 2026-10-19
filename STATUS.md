# SVBB 0.1.2 — Size limits and per-clip cropping

The source repository is public at https://github.com/MaxHastings/SonyVegasButBetter.
No public binary release has been published. This remains a personal playtest build.

Render MP4 now offers a maximum file size in decimal MB. CPU two-pass H.264
allocates the video budget while AAC bitrate adapts automatically. Optional automatic
format adjustment lowers resolution/frame rate when needed. Mix volume and timing
stay intact. The finished file is measured before acceptance; an overrun retries
once, and failed/cancelled jobs preserve the destination and clean their pass logs.

Each video event has an optional non-destructive source-pixel crop. Draw, move and
resize in the existing preview, with project aspect by default, Free mode, result
preview, reset, Apply and Cancel. Splits inherit crops; each half is independently
editable. Undo and schema-4 project saves preserve the result. Preview, edge-trim
feedback and export all use the same source crop.

The long-recording preview bottleneck is removed. Persistent FFmpeg decoders read
originals on demand; the audio mix is a bounded virtual float stream rather than
prepared PCM files. Waveform indexing is progressive and independent of playback.

Implemented: one-click playhead placement/selection and S split; live source preview
while trimming; fixed opposite-edge trims clamped to source/neighbour bounds; corner
fade handles; independent video/audio lanes linked for positional edits, split and
delete; unlink/relink and undo; readable auto-scaled waveforms; manual gain to +60 dB,
loudness matching to +48 dB subject to true peak; actual GPU encoder probing, CPU
fallback, render ETA and validation. Schema 3 migrates older implicit source audio
onto its own linked track and preserves a backup.

Windows verification is recorded in TESTING.md and docs/verification/0.1.2.
The personal real-footage playtest gate remains open. macOS/Linux have not been run
locally. This is a personal Windows playtest package, not a public release.

Run artifacts/SVBB-0.1.2-windows-x64/SonyVegasButBetter.exe with its runtime folder.
Older packages are retained separately. Projects saved by 0.1.2 use schema 4 and
are not readable by older executables; migration backups preserve older files.

Scope remains one video lane, three audio lanes, SDR, straight cuts/fades and the
first source video/audio streams. Same-lane overlaps are rejected. No titles,
layer compositing, denoising or plugin hosting. Export still uses CPU decoding and
filters with optional hardware encoding; it is not an entirely GPU-resident pipeline.

Manual checks still include real speech/gameplay listening, physical mouse/DPI
variants, audio-device loss, OS file-drop delivery and long unique-source workloads.
Public redistribution retains the native-runtime corresponding-source/notice and
codec distribution review items described in LICENSES.md.
