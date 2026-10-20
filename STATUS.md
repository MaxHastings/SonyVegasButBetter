# SVBB 0.1.7 — Direct framing and manual pan/zoom

The source repository is public at https://github.com/MaxHastings/SonyVegasButBetter.
The first Windows preview was v0.1.3; v0.1.4 fixed seeks after shortening edits.
Version 0.1.5 adds independent visual requests, bounded frame/source reuse, measured
latency, cut navigation, selection zoom and edge auto-scroll. The public first-run ZIP contains
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

Each video event has direct non-destructive framing: draw in Preview, release to
commit, adjust the existing frame or Alt+drag to redraw. Each gesture is one undo;
Escape cancels a drag. Animate is explicitly enabled and uses source-time motion
points for manual pan/zoom, with point navigation/removal and timeline diamonds.
Disable it to freeze the current view. Splits/trims retain the source curve, and
schema-5 saves preserve it. Preview, edge-trim feedback and CPU/GPU exports use the
same evaluated rectangle. Automatic subject tracking remains deferred.

Preview no longer requires processing a whole recording before editing. Exact uncached seeks still depend on codec/keyframe spacing and storage speed. Persistent FFmpeg decoders read
originals on demand; the audio mix is a bounded virtual float stream rather than
prepared PCM files. Waveform indexing is progressive and independent of playback.

Implemented: one-click playhead placement/selection and S split; live source preview
while trimming; fixed opposite-edge trims clamped to source/neighbour bounds; corner
fade handles; independent video/audio lanes linked for positional edits, split and
delete; unlink/relink and undo; readable auto-scaled waveforms; manual gain to +60 dB,
loudness matching to +48 dB subject to true peak; actual GPU encoder probing, CPU
fallback, render ETA and validation. Schema 3 migrates older implicit source audio
onto its own linked track and preserves a backup.

Windows verification is recorded in TESTING.md and docs/verification. Version
0.1.7 passes 49 core/native-media checks and 389 GUI steps, adding source-time
model/persistence tests, VFR motion preview/export pixel checks, and a
native direct-framing workflow with real save and GPU size-limited export. Existing
placement, trim, preview and QoL regression stages are rerun. The real-footage
playtest gate remains open; macOS/Linux GUI behavior is unverified.

Run artifacts/SVBB-0.1.7-windows-x64-first-run/Start SVBB.cmd.
For the public download, extract the ZIP and run Start SVBB.cmd; first setup needs
internet, subsequent launches work offline. Old generated builds are removed.
Projects saved by 0.1.7 use schema 5 and need 0.1.7 or newer. Schema-4 projects
from 0.1.2–0.1.5 migrate with a backup. Original media is never changed.

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
