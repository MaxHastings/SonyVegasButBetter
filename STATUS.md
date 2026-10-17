# SVBB implementation status

Current milestone: A — establish the native application and real media path.

Release gate: **not yet satisfied**. The acceptance session has not run.

Plan, in dependency order:
1. Native Rust shell, precise model, persistent FFmpeg-backed video, trim and validated export.
2. Deterministic timeline commands, undo/redo, drag editing and snapping.
3. Three-lane audio, shared processing, waveforms and measured loudness matching.
4. SQLite persistence, recovery, relinking and failure handling.
5. Integration fixtures, native GUI playtest, portable Windows package and platform CI.

Environment: Windows x64; Rust 1.93.1. macOS and Linux are unverified.
