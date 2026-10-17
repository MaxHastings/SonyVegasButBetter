# Architecture

Dependency direction: native UI → commands/project model → plain owned data.
Persistence and media adapters depend on that data; the model never imports egui,
SQLite or FFmpeg types.

* `model`: version-independent project, assets, stream metadata, clips and tracks.
* `edit`: atomic copy/validate/commit commands and bounded snapshot undo/redo.
* `persistence`: normalized SQLite tables, migrations and atomic replacement saves.
* `media`: runtime discovery, probing, decode validation, waveform/thumbnail cache,
  and integrated LUFS/true-peak measurements on worker threads.
* `render`: deterministic timeline evaluation and shared audio filters; cancellable
  FFmpeg jobs and independent ffprobe/output decode validation.
* `playback`: isolated libmpv C ABI, persistent decoders, EDL compiler products,
  separate command and rendering threads, synchronized audio and video.
* `ui`: native eframe window, resizable panels and bespoke timeline. Opt-in GUI
  regression code feeds raw events to this same window and widgets.

## Time

Authoritative time is signed integer ticks at 282,240,000 ticks/second. This
represents common frame rates (including 30000/1001 and 60000/1001) and 44.1/48 kHz
sample positions exactly. Rational stream timebases and original start PTS are
retained. Arithmetic conversions use i128 intermediates. Decimal seconds occur
only at UI/native media boundaries; they are never accumulated to form an edit.

Source timestamps determine decoding, including VFR. Project FPS determines edit
boundaries; export FPS is a separate choice. Source video/audio offsets relative
to container start are inspected. Positive audio offsets are restored after
resampling/trimming. Invalid/missing timestamps are delegated to the demuxer only
when it supplies a finite readable timeline; otherwise import fails explicitly.

## Playback and mixing

Persistent libmpv handles actual seeking and presentation timestamps. A dedicated
software-render worker sends the latest frame through a bounded single-frame
slot; UI work never decodes media. Normal libmpv commands run on another worker,
avoiding render-thread/command-thread wait cycles. Shutdown joins render work and
frees the render context before destroying the core/library.

Video sources first normalize to disposable 960×540 H.264 preview files at the
project frame rate. FFmpeg samples the actual original PTS with round-up timestamp
mapping, preserving the frame covering a VFR cut. The caches have exact tick-based
MP4 timestamps. EDL source offsets then evaluate against that normalized timeline.
The same sampling rule is used for export, which reads originals. Cache identity
includes source path, file metadata and project FPS; trims/splits do not invalidate
a source cache. Preparation runs on a cancellable worker with real progress.
No FFmpeg process launches on seek. Blank segments use the same dimensions,
pixel format and project frame cadence as the video caches.

For preview, changed audio clips render to disposable float PCM caches. Unchanged
clips reuse those caches. Three stereo 48 kHz EDL lanes are mixed by libavfilter,
with the same gain/fade/master semantics as offline export. All lane segments,
including silence, use the same PCM format to avoid graph reinitialization stalls.
No application realtime audio callback performs decoding, file reads or locks;
libmpv owns audio-device buffering. Playback can use its fallback clock when there
is no audio device; export never depends on a device.

The UI pauses after edits and debounces background mix preparation by 300 ms.
Export works from a project snapshot, so further edits do not mutate a running
render. Async preview results are revision-checked before being installed.

## Audio semantics

Source range → resample 48 kHz/stereo float → restore initial source offset → pad
and trim exact clip duration → clip gain → linear in/out fades → track gain →
sample-aligned timeline delay → sum without automatic attenuation → optional
master limiter. Gain uses `10^(dB/20)`. Every audio edge has at least a 3 ms safety
ramp, capped at half the clip. Explicit fades replace the corresponding ramp.
Overlapping in/out fades multiply.

Voice matching measures integrated LUFS and true peak on the selected source
range. Target −16 LUFS, maximum boost +18 dB, predicted peak ceiling −1 dBTP.
Silence/nonfinite results and levels below −50 LUFS are refused. This is fixed
loudness matching, not dynamic compression or arbitrary peak normalization.

The optional master limiter uses limit 0.98, attack 5 ms, release 50 ms, no makeup
gain, latency compensation. It protects sample peaks; lossy AAC reconstruction can
overshoot slightly. The output meter reads actual post-mix astats metadata.

## Editing semantics

All mutations validate before committing. Locked lanes protect clip edits and
positions. Primary clips cannot overlap. Audio overlaps on the same lane are
rejected atomically. Delete leaves a gap. Ripple-delete removes the selected time
interval from all unlocked lanes; intersecting clips are trimmed or split, fully
covered clips are removed, and later clips shift left. Locked lanes stay fixed.

Primary edge trims hold the clip's timeline start and ripple later material whose
start is at or beyond the old end. Audio spanning that boundary stays anchored.
Numeric property edits do not ripple. A primary drag into an occupied position
reorders a gapless video sequence; independent audio remains positioned. An
unoccupied drop moves the clip without shifting other clips. Reorder buttons swap
neighbors while retaining the gap between them.

Split retains outer fades, clears the new interior fades, and clamps fades to the
new duration. This rule is stored as actual clip settings and is undoable.

Cross-platform adapters use eframe, rfd, dynamically loaded libmpv, and child
process argument arrays. Windows is the only locally executed platform; CI and
manual platform procedures cover the others without claiming they have run.
