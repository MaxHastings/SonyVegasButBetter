# 0.1.2 verification

Windows x64 synthetic-media verification. No personal recordings are included.

* 22 ordinary core/UI tests and 8 real-media tests; formatting and strict Clippy.
* `size-limit-report.json`: 30/60 seconds under 10,000,000 bytes, 6 seconds under
  150,000 bytes; full decode, audio RMS, hard-ceiling validation and cancellation.
* `gui-report-qol.json`: 51 native editing steps, including linked A/V, split,
  edge/fade drags, unlink/move/undo, real playback, save and GPU export.
* `gui-report-crop-size.json`: 70 native steps, including drawing/moving/resizing,
  aspect mode, original/result preview, per-cut independence, reset/cancel,
  undo/redo, save/reopen and a real two-pass export under 200,000 bytes.
* `crop-tool.png`, `crop-result.png`, `size-limit-settings.png`: actual native
  framebuffer captures from that run. Coordinates in reports refer to synthetic
  local fixture paths; render timings depend on machine load.

The red/green crop integration test independently checks native decoder pixels
and exported pixels for two differently cropped cuts, then resets the retained
source frame at the same timestamp. The transport regression test issues play
immediately after reload/seek and checks rapid toggle intent.

Headless CI uses libmpv's real-time null audio device for transport tests; PCM
captures independently verify the mix. Desktop GUI runs still use the real device.
Each Windows test command has an explicit exit-code gate, so a later packaging
success cannot conceal an earlier failed test.

The 0.1.1 complete-session/recovery evidence remains in the adjacent version
directory. This update reruns the editing QoL stage and adds crop/size coverage;
it does not claim a new 202-step full-session run. Personal footage and physical
device-loss/long-session listening remain manual playtest work.
