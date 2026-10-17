# Verification

Windows x64 is the only locally executed platform. macOS, Linux/X11 and Wayland
are unverified. CI configuration is supplied; it has not run in this repository.

## Reproduce

```
cargo test --release --locked
cargo test --release --test media_pipeline -- --ignored --nocapture --test-threads=1
cargo test --release --test media_edges -- --ignored --nocapture --test-threads=1
cargo clippy --all-targets --all-features -- -D warnings
powershell -File scripts/run-gui-acceptance.ps1
powershell -File scripts/test-recovery.ps1
cargo run --release --example benchmark
```

The GUI scripts build with the opt-in gui-test feature. Its driver sends raw egui
pointer/keyboard/drop events into the actual native window, targeting rectangles
observed from the previous UI pass. It uses real controls, commands and workers.
The distributed release excludes this driver.

## Passed automated checks

**16 core/UI tests and all 5 real-media integration tests pass.** They cover
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
  1.0026, 1.0004, 1.0006, 1.0006; the AAC comparison tolerance is 3%.
* VFR frames at PTS 0, .2 and .7 cut at .5: preview/export show the covering .2
  frame until .7. Source caches preserve this rule.
* Two one-frame 60 fps clips exported at 30 fps produce one video frame.
* Black video gaps display correctly while independent audio keeps playing.

## Native integrated acceptance

Three ten-minute videos include 1080p60 and mixed resolution/FPS. Four microphone
files have mixed levels and sample rates. Videos loop six-second moving patterns;
voices are modulated tonal signals. These are not actual gameplay or speech.

Session 1 passed **193 GUI steps**: import seven recordings, assemble 240 seconds,
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
renders 960x540, excluding UI texture upload. Warm caches measured **59.25 fps**,
**13-69 ms** seeks, **43 ms** adapter initialize/load and **0.83 ms** warm plan
preparation. Initial source caching is a separate lengthy job with real progress;
the 30-minute dataset used about 1.75 GB of video caches. Cold native startup and
worst-case unique gameplay performance have not been benchmarked. These results
are observations on this hardware, not general performance promises.
