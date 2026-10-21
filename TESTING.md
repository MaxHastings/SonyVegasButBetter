# Verification

Windows x64 is the only locally executed platform. macOS, Linux/X11 and Wayland
GUI behavior remains unverified locally. GitHub CI builds and tests all three OSes;
its actual run status is visible in the public repository.

Hosted runners have no physical audio device. Transport integration tests use
libmpv's real-time null audio device; separate PCM captures still verify the actual
decoded mix against exported audio. Native Windows GUI runs use the real output
device. Windows CI explicitly checks every native test command's exit code.

## Reproduce

PE import auditing rejects separate Visual C++ runtime dependencies. The app-local
Vulkan loader ships with its pinned source and notices, covering the transitive
libmpv dependency on machines without a system Vulkan loader.

Public Windows setup is also checked in 64-bit Windows PowerShell: fresh upstream
downloads/extraction in a path with spaces, offline repeat startup, retry after an
interrupted setup, missing-runtime repair, CMD launcher module isolation, GUI launch
and a cropped CLI export/full decode using only that installed runtime. CI exercises
the public package setup with the pinned archive cache. The public ZIP contains no
media binaries; the fully bundled local test archive is separate.

```
cargo test --release --locked
cargo test --release --test media_pipeline -- --ignored --nocapture --test-threads=1
cargo test --release --test media_edges -- --ignored --nocapture --test-threads=1
cargo test --release --test size_limit -- --ignored --nocapture --test-threads=1
cargo test --release --test crop_pipeline -- --ignored --nocapture --test-threads=1
cargo clippy --all-targets --all-features -- -D warnings
powershell -File scripts/run-gui-acceptance.ps1
powershell -File scripts/test-recovery.ps1
cargo run --release --example benchmark
```

The GUI scripts build with the opt-in gui-test feature. Its driver sends raw egui
pointer/keyboard/drop events into the actual native window, targeting rectangles
observed from the previous UI pass. It uses real controls, commands and workers.
The distributed release excludes this driver.

## Current 0.1.8 checks

Version 0.1.8 passes 36 core/UI/budget checks, eight native preview/media edge
tests, and 283 native GUI steps across trim-preview, framing, seek-resize and
responsive preview (records in `docs/verification/0.1.8`). Formatting and strict
Clippy checks pass. Unchanged render pipelines retain the additional 0.1.7 tests
described below.

The new `trim-preview` native GUI stage holds and shortens both ends of video
and linked audio events, changes the boundary while holding, and checks a
presented video frame at each expected source time before release. It starts with
framing controls open, checks that no model edit commits during the gesture,
undoes each completed trim, then saves and exports. The test fails on 0.1.7
with no displayed boundary frame during the held trim.

## Previous 0.1.7 checks

36 core/UI/budget checks and 13 real-media tests pass locally, plus 389 native
GUI steps (records in `docs/verification/0.1.7`). Six GUI stages cover direct framing/animation, crop-size, general QoL, placement, seek-resize
and responsive preview. The new framing stage uses a direct preview drag without
opening a tool, checks the actual source rectangle under the pointer, one-step
undo/redo, explicit Animate, seek-and-adjust motion, point navigation/removal,
Escape during a held drag, splits, save and a GPU size-limited export.

The animated crop regression generates a changing-color VFR source (30 → 15 fps),
trims its source in, splits midway, animates position/zoom/aspect, and exports with
CPU two-pass under 200,000 bytes. At six positions it compares live Player output
pixel-for-pixel against an independent native decoder using the model rectangle,
and compares sampled export RGB values against that preview (mean channel error
below 8 to allow encoding/scaling/chroma differences). Project tests cover source
time continuity through moves/trims/splits, static freeze, replacement/deletion,
invalid rectangles/times, locked/audio tracks, undo and SQLite round trips/migration.

No automatic tracking, subject detection or easing curve editor is present.
Real-gameplay composition and physical mouse/DPI variants still need user playtesting.

## Previous 0.1.5 checks

33 core/UI/budget checks, 12 real-media tests, a long-recording benchmark and
318 native GUI steps passed locally. The interleaved warm-cache comparison
recorded revisited frames at about 2.5–3 ms versus 40–99 ms in 0.1.4; see
`docs/verification/0.1.5/preview-comparison.json` for all cases and limitations.

`cargo test --release --locked --test preview_requests -- --ignored --nocapture --test-threads=1`
checks rapid source/crop switches, restoration to the timeline, decoder reuse,
frame identity, bounded cache memory and audio reloads at different durations/positions.
Windows CI runs these with the real pinned media runtime.

Generate acceptance media with `scripts/make_acceptance_media.py`, then run:

```
cargo test --release --locked --test preview_responsiveness -- --ignored --nocapture --test-threads=1
```

The measured workload opens a ten-minute 1080p source, seeks across it and back,
issues 151 rapid requests and resumes audio playback. Expected image fingerprints
and request IDs are checked. `SVBB_BENCH_REPORT` selects a JSON output file.
The native `SVBB_GUI_TEST=responsive` stage measures real texture uploads while
exercising cut navigation, selection zoom, moving/trimming with scrolling in both
directions, linked tracks, undo, playback, save and export.

Comparisons must use the same source and similar cache/load conditions. The acceptance
file repeats six seconds of test pattern; results do not establish performance for
all codecs, keyframe spacings, storage devices, real gameplay or physical displays.
The OS file cache is not flushed. First-frame time starts at an adapter request;
probe/import time is separate. Reports are in docs/verification/0.1.5.

## Previous 0.1.4 checks

30 core/UI/budget checks and ten real-media tests cover the current editor.

The seek regression reproduces the 0.1.3 error with a 12-second source, a playhead
at 9.8 seconds and a shortened 5.633-second mix (2,163,280 bytes). It checks bounds
on reload and direct seeks, EOF recovery, audible playback and queued rapid reloads.
The `seek-resize` native GUI stage moves a linked clip left to shorten the edit,
trims through the old playhead, deletes the final event, undoes and resumes playback.
Virtual-stream tests cover seeking beyond EOF and back. Log tests check the
1 MiB/session and eight-session bounds while preserving unrelated files.
The reproduction and native GUI evidence are in docs/verification/0.1.4.

Run `SVBB_GUI_TEST=seek-resize` with the `gui-test` build and acceptance media as
for the existing native GUI stages. The distributed executable excludes this driver.

## Previous 0.1.3 checks

28 core/UI/budget tests include nearest-legal drag placement, exact-fit gaps,
off-frame audio boundaries, linked-lane obstacles, locked/no-space atomicity,
undo, both-edge snapping and anchored zoom. A property test compares placement
against a brute-force nearest legal slot.

The native placement GUI stage tests linked collisions and a held-drag outline
before commit, Ctrl+wheel anchoring, wheel panning, and Fit/restore. The existing
QoL and crop-size GUI stages are rerun. The size-goal dialog exposes the encoder
choice in both modes; the hardware test uses the real NVIDIA encoder at 60 seconds
under 10 MB and 6 seconds under 150 KB, fully decoding both outputs. GPU coverage
is conditional on a supported device; hosted CI normally exercises CPU fallback.
No local hardware result is claimed for Intel, AMD or Apple.

Reports and synthetic screenshots are in docs/verification/0.1.3.

## Previous 0.1.2 checks

22 core/UI tests plus 8 real-media integration tests cover the previous editing
workflow and the new export/crop features. Size tests encode 30 and 60 seconds
under 10,000,000 bytes, and 6 seconds under 150,000 bytes, decode full outputs,
measure mix gain, reject an oversized validation candidate, and cancel during pass
two while preserving a sentinel destination and removing temporary files.
Budget tests cover decimal units, container/audio reserve, invalid limits/durations,
overflow bounds, portrait/landscape aspect and automatic format selection.

Crop tests cover per-event split inheritance/independence, undo/redo, locked tracks,
invalid rectangles, schema-3 migration/backup and save/reopen. A real red/green
reference checks independent native preview and two-pass export crops and verifies
that returning to the full frame at the same timestamp restores unmodified pixels.
The native GUI crop-size stage exercises the actual crop and size-goal controls.
It also resizes crop corners and toggles the framed result. The existing QoL stage
passes alongside it. A regression found during this run is covered by a real-media
test: play intent must survive an immediate reload, seek and rapid pause toggles.
The worker now toggles desired transport intent instead of a delayed device snapshot,
and defers play until the pending load/seek handshake has completed.
Reports and screenshots are under docs/verification/0.1.2.

## Previous 0.1.1 checks

**19 core/UI tests and all 5 real-media integration tests pass.** They cover
insertion, split, trim, order/move, locks, snap, all ripple intersections, exact
frame/sample conversions, atomic failure, undo/redo, source overwrite refusal,
gain limits, save/reopen/backup equivalence, populated migrations and shortcut
precedence. Property tests split random frame boundaries and undo exactly.

Actual media fixtures cover H.264/AAC MP4, mixed-rate MKV, 44.1 kHz mono voice,
odd-sized 4:4:4 video, delayed PCM audio, MP3 and HEVC MOV imports. Tests verify:

* Real output codecs/dimensions/duration/audio format and decoded cut pixels.
* Actual audio, fade attenuation, measured LUFS matching and silence refusal.
* Original audio PTS offsets, resampling, meters and finite bounded samples.
* Reopen/edit/export again and cancellation preserving an existing destination.
* Persistent preview frame colors, exact seeks and playback advancement.
* Captured preview PCM versus decoded export: sampled RMS ratios approximately
  1.0026, 1.0004, 0.9981, 0.9983; the AAC comparison tolerance is 3%.
* VFR frames at PTS 0, .2 and .7 cut at .5: preview/export show the covering .2
  frame until .7. The persistent native decoder preserves this rule without source caches.
* Two one-frame 60 fps clips exported at 30 fps produce one video frame.
* Black video gaps display correctly while independent audio keeps playing.

## Native integrated acceptance

Three ten-minute videos include 1080p60 and mixed resolution/FPS. Four microphone
files have mixed levels and sample rates. Videos loop six-second moving patterns;
voices are modulated tonal signals. These are not actual gameplay or speech.

The 0.1.0 baseline passed **193 GUI steps**: import seven recordings, assemble 240 seconds,
remove ten one-second segments, reorder three sections, position four distinct
microphones, match LUFS, lower gameplay, add fades, exercise drag import/audio
lane moves/edge trims/video reorder and undo restoration, preview, save, and
render a validated **230-second 1080p60 H.264/AAC MP4**. About **166 seconds**,
including first-use source caches. Session 2 passed **14 steps** in a new process:
reopen all 17 clips, verify edits, change a cut, save and export again; about
**98 seconds**. Both complete outputs independently decoded without errors.

Recovery passed in two more processes: deliberate exit 86 after a real autosave,
recover through the GUI dialog, verify changed gain -13 dB while the manual save
remains 0 dB, and verify recovered work is dirty with no manual save path. This
intentional termination test is separate from crash-free editing acceptance.
Reports are preserved in docs/verification; generated media/logs are test-output.

The 0.1.1 driver additionally verifies separate linked A/V, one-click split, ordinary
trim extension without moving the clip, fixed-end left trim, top-corner video/audio
fades, unlinking, independent lane movement, undo, playback, saving and GPU export.
The full acceptance/recovery scripts now operate on four lanes and 30 explicit clips.
Current runs passed 202 + 14 + 51 GUI steps, both normal exports used NVIDIA NVENC,
and both recovery phases passed. The first full session took about 89 seconds,
including editing and export, without creating video/PCM preparation caches.
RF64 capture duration checks detect duplicated output during load/seek; delayed
mono resampling is checked in both preview and export. Video mute exports black.

## Native OS checks and remaining manual work

The computer-use plugin verified launch, Ctrl+I and multiple files through the
Windows dialog. Its mouse input reported a 14x14 window boundary despite complete
screenshots; reselecting did not fix it. Internal native raw-input tests cover
editor drags. Actual Explorer-to-window OS drop, physical mouse feel, DPI variants,
audio-device loss and a real-recording human listen/edit session remain manual
checks in PLAYTEST.md. Normal saves and deliberate interruption are tested;
actual power-loss and full-disk fault injection are not.

The window capture helper also returned blank images for this OpenGL surface.
SVBB's own rendered framebuffer was captured and visually reviewed successfully;
the workspace, actual preview, timeline and waveforms are present. For local
diagnostics, set `SVBB_UI_SNAPSHOT` to an absolute PNG path before launching; one
framebuffer capture is saved after the workspace settles. It is local and opt-in.

## Performance observations

AMD Ryzen 7 7800X3D, 8 cores/16 threads, 127 GiB RAM, Windows x64. The adapter
benchmark seeks a ten-second tail from a synthetic ten-minute 1080p60 source and
renders 960x540, excluding UI texture upload. The new adapter needs no video/PCM
cache preparation. The latest on-demand run measured roughly **59 fps**, **27-120 ms**
seeks and less than 0.1 ms preview snapshot preparation. First-frame startup including adapter/device initialization was
about 0.52 s in the final run. Results use a synthetic ten-minute recording and are observations,
not a guarantee for all codecs, GOP structures, disks or actual gameplay. Detailed
0.1.1 reports are under docs/verification/0.1.1. The earlier 0.1.0 warm-cache benchmark
and full GUI reports remain as historical evidence in the parent folder.
