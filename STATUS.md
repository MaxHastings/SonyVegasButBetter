# SVBB 0.1.1 — Windows editing update

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

Windows verification is recorded in TESTING.md and docs/verification/0.1.1.
The personal real-footage playtest gate remains open. macOS/Linux have not been run
locally. This is a personal Windows playtest package, not a public release.

Run artifacts/SVBB-0.1.1-windows-x64/SonyVegasButBetter.exe with its runtime folder.
The 0.1.0 package is retained separately. Projects saved by 0.1.1 use schema 3 and
are not readable by the older executable; migration backups preserve older files.

Scope remains one video lane, three audio lanes, SDR, straight cuts/fades and the
first source video/audio streams. Same-lane overlaps are rejected. No titles,
layer compositing, denoising or plugin hosting. Export still uses CPU decoding and
filters with optional hardware encoding; it is not an entirely GPU-resident pipeline.

Manual checks still include real speech/gameplay listening, physical mouse/DPI
variants, audio-device loss, OS file-drop delivery and long unique-source workloads.
Public redistribution retains the native-runtime corresponding-source/notice and
codec distribution review items described in LICENSES.md.
