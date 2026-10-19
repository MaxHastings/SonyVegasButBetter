# SVBB 0.1.3 — Timeline placement, navigation and GPU size limits

The source repository is public at https://github.com/MaxHastings/SonyVegasButBetter.
The first Windows preview is published as v0.1.3. The public first-run ZIP contains
the editor and setup scripts; pinned media tools download directly from upstream.

Dragging timeline clips resolves to the nearest available placement, flush with
neighbouring clips. Linked video/audio obey every linked lane together; a live
outline previews the resolved position before drop. Snap aligns either edge and
excludes linked partners. Numeric position fields remain exact. Locked/no-space
drops leave the project intact and report inline.

Ctrl+wheel zooms around the mouse; ordinary wheel pans horizontally. + / − and
a logarithmic slider zoom around the visible playhead or viewport centre. F / Fit
fits the whole edit and restores the previous zoom and scroll on the next press.

Size-limited exports now honor Auto / explicit GPU / CPU encoder choices. Hardware
uses bitrate control; CPU retains whole-edit two-pass H.264. Auto probes actual
bitrate settings and can fall back to CPU. AAC bitrate adjusts automatically;
optional format adjustment lowers resolution/frame rate. Completed files must
satisfy the byte ceiling; overruns trigger bounded lower-bitrate retries. Failed
and cancelled jobs preserve the destination and clean temporary files.

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

Windows verification is recorded in TESTING.md and docs/verification/0.1.3.
37 local tests and 170 native GUI steps passed, including actual NVIDIA size-limit
exports. Windows, macOS and Linux CI passed for the editor implementation. The
real-footage playtest gate remains open; macOS/Linux GUI behavior is unverified.

Run artifacts/SVBB-0.1.3-windows-x64-first-run/Start SVBB.cmd.
For the public download, extract the ZIP and run Start SVBB.cmd; first setup needs
internet, subsequent launches work offline. Old generated builds are removed.
Projects saved by 0.1.2 and 0.1.3 use schema 4 and
are not readable by older executables; migration backups preserve older files.

Scope remains one video lane, three audio lanes, SDR, straight cuts/fades and the
first source video/audio streams. Same-lane overlaps are prevented. No titles,
layer compositing, denoising or plugin hosting. Export still uses CPU decoding and
filters with optional hardware encoding; it is not an entirely GPU-resident pipeline.

Manual checks still include real speech/gameplay listening, physical mouse/DPI
variants, audio-device loss, OS file-drop delivery and long unique-source workloads.
Public redistribution retains the native-runtime corresponding-source/notice and
codec distribution review items described in LICENSES.md.

Windows release builds statically link the MSVC/C++ runtime. The public archive
includes an app-local Vulkan loader built from pinned source, with source archives
and notices. No Visual C++ or Vulkan system installer is required.
