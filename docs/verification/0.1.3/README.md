# 0.1.3 Windows verification

Native GUI raw-input stages passed: placement 49 steps, existing editing QoL 51,
and crop/size export 70. These use the actual window, widgets, edit commands,
playback, project saves and export workers. The production binary excludes the
driver. Fixtures and screenshots contain synthetic bars, motion and sine audio.

The placement stage deliberately drags into an occupied lane, moves away and
returns against the neighbour, verifies the held-drag outline before commit,
and confirms linked sound follows. Ctrl+wheel keeps the timeline time under the
mouse fixed; ordinary wheel pans without changing scale. F fits/restores the
previous zoom and scroll. See the placement report and two timeline screenshots.

The size-goal screenshot shows the encoder selector available with a byte ceiling.
All three GUI exports used NVIDIA NVENC. The separate hardware integration test
encoded 60 seconds under 10,000,000 bytes (9,974,461 bytes) and 6 seconds under
150,000 bytes (137,599 bytes), then decoded both complete outputs. CPU two-pass
size exports, audio gain, cancellation and temporary-file cleanup are covered
separately. Hardware coverage is NVIDIA only on this machine; no Intel, AMD or
Apple device execution is claimed. CI without a supported GPU exercises fallback.

28 core/UI/budget tests and nine real-media tests pass locally. The model/property
tests include nearest legal placement, exact-fit gaps, off-frame audio boundaries,
locked/no-space drops, both linked lanes, undo and magnetic alignment of either
edge. Strict all-target/all-feature Clippy and formatting checks pass.

Manual real-footage listening, physical pointer/DPI variants and non-Windows GUI
checks remain the playtest gates described in TESTING.md.
