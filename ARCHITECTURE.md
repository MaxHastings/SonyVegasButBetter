# Architecture

The native UI depends on plain project data and atomic commands. Media and persistence
adapters consume that data. FFmpeg, SQLite and egui types never enter the project model.

* `model`: integer time, media metadata, four tracks, clips and explicit A/V links.
* `edit`: copy/validate/commit commands, linked operations and bounded undo/redo.
* `persistence`: SQLite schema 5, explicit migrations and atomic replacement saves.
* `media`: source probes, progressive waveform/thumbnail indexing and loudness jobs.
* `video` and `native/video.cpp`: persistent FFmpeg decoding and RGBA conversion.
* `audio`: seekable, on-demand stereo float timeline mixing on mpv's demux thread.
* `playback`: narrow libmpv ABI, device output, buffering, limiter and output meter.
* `render`: offline FFmpeg graph planning, encoder probing, cancellation and validation.
* `ui`: native eframe panels, direct timeline/crop manipulation and raw-input GUI tests.

## Crops and bounded-size export

A clip has an optional static source-pixel crop and an optional sorted list of
`FramePoint { at, rect }` values. Point times refer to source ticks, so splits copy
the curve unchanged and trimming/moving does not retime it. Each split can then
be edited independently. There are at most 256 points per event; bounds, time,
uniqueness and video-only ownership are validated. Evaluation holds before/after
the points and linearly interpolates even source-pixel coordinates between them.
Turning animation off freezes the evaluated current rectangle. SQLite schema 5
stores points as JSON and migrates schema 4 with an empty curve and a backup.

Direct preview gestures edit an unsaved drag draft and commit once on release.
Escape cancels only that gesture. Framing controls reserve vertical space to avoid
moving the image beneath a new drag. Result view is restored on release; Adjust
frame reveals the full source. Undo/redo invalidates source-preview overrides.
Preview uses a non-owning AVFrame crop before scaling; the retained source is intact.

Animated export first resolves the project frame cadence and source offsets, then
uses interval `sendcmd` expressions to update a uniquely named crop at every frame.
Origin and dimensions reset to legal values before applying each rectangle. Scaling
and padding evaluate per frame; scale expressions explicitly depend on `t` to
avoid FFmpeg's constant-expression cache. No giant upscaled source or per-frame
command file is generated: graph size follows point count. The same graph feeds
CPU and hardware encoders, including two-pass and bounded-size exports.

An optional byte ceiling is an export constraint, separate from the project format.
The budget reserves container overhead and AAC priming, selects AAC 64/96/128/192
kbps, and assigns remaining bits to H.264. Optional format fitting reduces frame
rate to 30 when needed and scales the canvas while preserving its aspect ratio.
The 240p floor avoids silently shrinking to an unreadable thumbnail; very low
budgets are warned about or rejected before rendering.

Bounded-size exports use GPU bitrate control or CPU x264 two-pass encoding. Auto
probes the actual mode and can fall back to CPU. Each job owns its graph and
pass logs in a unique cache subdirectory, retained across both passes and removed
on success/failure/cancellation. Progress is weighted across analysis and encoding.
An oversized result retries with a lower video bitrate, within three total attempts
including any GPU fallback. Actual file size is
checked again during validation before atomic destination replacement. A successful
export therefore fits its ceiling; a failed job does not publish an oversized file.
Ordinary quality-driven exports retain hardware encoding and CPU fallback.

## Time and source decoding

Authoritative time is integer ticks at 282,240,000 Hz. This represents common broadcast
frame rates and 44.1/48 kHz samples exactly. Rational stream timebases, original start
PTS and offsets from the container origin remain available. Conversions use i128;
floating point is confined to UI and native API boundaries.

Video decoding retains the frame whose presentation timestamp covers the requested
source time plus one lookahead frame. Sequential requests reuse the decoder. Random
requests seek backward to a keyframe and decode forward. A dedicated worker coalesces
requests, cancels obsolete scrubs and publishes through a single-frame slot. Ordinary
forward playback can finish its current frame, avoiding cancellation starvation.
Source changes reopen the decoder. Scaling fits into 960x540 RGBA with letterboxing;
fades multiply RGB toward black. Video follows the buffered audio clock, interpolated
between mpv's clock updates. No frame seek launches a process or transcodes a source.

FFmpeg structs live only in a small C++ translation unit compiled against vendored,
pinned FFmpeg 9 headers. Rust loads ABI-major-matched libraries, keeps them alive until
decoder destruction, and transfers only opaque handles, byte arrays and scalar time.

## Audio and bounded buffering

mpv opens a custom read-only virtual RF64 stream. Its demux/cache thread requests small
blocks of stereo float PCM. Each block evaluates active clips, seeks persistent source
decoders as needed, resamples to 48 kHz stereo and mixes the requested range. Original
source PTS preserve delayed audio and gaps. At most eight clip decoder contexts are
retained; requests and buffers are bounded. RF64 supports projects exceeding 4 GiB of
logical PCM without creating a PCM file. Read-ahead is one second with explicit byte
limits. Device audio callbacks perform no application decoding, file reads or locks.

All source audio lives on audio lanes, never implicitly on V1. Preview and export use
this processing order: source range and PTS offsets, 48 kHz stereo resampling, clip
gain, linear in/out fades, track gain, timeline placement, summation, optional limiter.
Gain is 10^(dB/20). Edges have a minimum 3 ms safety ramp capped at half the clip;
overlapping explicit fades multiply. Native sample evaluation and FFmpeg filter
compilation are checked against independently decoded preview/export reference audio.
The master filter is shared: limit 0.98, 5 ms attack, 50 ms release, no makeup gain,
latency compensation. Metering reads actual post-filter astats metadata.

Loudness matching measures integrated LUFS and true peak over the selected source range.
It targets -16 LUFS, limits predicted true peak to -1 dBTP and boost to +48 dB.
Nonfinite/silent material below -70 LUFS is refused. Manual gain ranges -60 to +60 dB.
Automatic waveform scaling is a labelled display setting, independent of audible gain.
Waveform indexing is progressive, cancellable and never gates playback.

## Editing and persistence

V1 is video only; A1 is the default source-audio lane, A2 voice, A3 music. Inserting an
A/V source creates two clips sharing a link ID. Move, split, trim, reorder and deletion
operate on both. Gain, mute and fades are independent. Splitting assigns the right pair
a new link ID. Unlink/relink and all compound changes are undoable. Link validation
requires matching source, source range and timeline placement; a locked partner blocks
a compound edit atomically. Audio-only sources create one clip on the chosen audio lane.

Clicking a clip selects it and sets the playhead. Ordinary edge trims keep the opposite
edge fixed and clamp against source bounds and neighbours on both linked lanes. They
never ripple implicitly. A drag previews the exposed source frame, including beyond the
old trim. Top-corner handles edit fades. Body drags move linked clips; overlaps are
rejected. Earlier/Later swaps neighbours with linked sound. Delete leaves a gap;
Shift+Delete explicitly removes the selected interval across unlocked tracks, trimming
or splitting intersections. Linked locks cannot silently break synchronization.

Edits pause playback and rebuild only a small immutable preview snapshot after 40 ms.
Revision checks discard stale jobs. Export uses its own snapshot and originals; further
editing cannot mutate an active render. Auto encoding probes real device initialization
(NVENC, QSV, AMF or VideoToolbox), falls back to x264, and can retry on CPU after a GPU
render failure. ETA is estimated from elapsed time and completed output duration.
Output is written to a sibling temporary file, decoded/validated and atomically replaced.

Windows is locally tested. Linux/macOS source and packaging paths are maintained but
have not been run locally. See BUILDING.md and TESTING.md for runtime requirements.


## Preview request ownership and responsiveness

Each load, seek or source-boundary preview receives a monotonically increasing
interaction ID. The client invalidates the old output and submits directly to a
video mailbox; libmpv loading and synchronous property queries do not gate that
submission. A bounded latest-only mailbox coalesces requests. Cancellation and
publication both check identity; output publication and client invalidation use
the same mutex. Continuous playback can finish nearby frames within one interaction.

The video worker retains three source decoders and an LRU of unfaded 960x540 source
frames capped at 32 MiB. Keys include source identity/path/size/stream, source time
and crop. Timeline position, request identity and opacity are applied on reuse;
changing a fade never changes the cached original pixels. Decoder memory is
additional to the bitmap cache budget. There is no full-resolution proxy generation.

Audio reloads are serialized: an edit arriving during a load replaces the pending
project snapshot. The previous FILE_LOADED completion cannot seek/resume the newer
mix. Seeks and play/pause intent continue to coalesce while loading; the latest
snapshot opens next. Already-open audio streams own their immutable mix snapshots.

Snapshot diagnostics expose first correct frame, UI texture-upload and audio-ready
latencies for the current interaction, plus frame-cache size/hits and decoder opens.
Texture upload is a useful UI timing boundary, not a measurement of physical monitor
presentation. Local bounded logs record completed request metrics. Windows asset
processes run below normal priority; thumbnails and waveforms use bounded threading.
