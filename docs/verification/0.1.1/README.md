# SVBB 0.1.1 verification, Windows x64

18 model/UI tests and 5 real-media tests passed, including audio preview/export
comparison, RF64 capture duration, VFR covering-frame seeks, positive audio offsets,
black gaps, video mute, GPU export and explicit CPU export. Clippy with warnings as
errors and rustfmt checks passed.

The native raw-input runs passed all 202 + 14 + 48 steps. They use real controls,
commands, source decoders, save/reopen, fades, linked A/V, loudness measurement and
validated 230-second H.264/AAC exports. The encoder reported by both full sessions
was NVIDIA NVENC. The recovery runs deliberately terminated after an autosave and
verified recovery without overwriting the manual save.

Benchmark: persistent native 960x540 video with a ten-second tail from a ten-minute
1080p60 synthetic source. No video or PCM preparation files are generated. Device
initialization, first-frame latency, seeks and playback rate are separate metrics.
These are machine-specific synthetic results; real-footage listening/manual playtest
and other desktop platforms remain separate checks.
