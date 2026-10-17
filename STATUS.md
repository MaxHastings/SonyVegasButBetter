# SVBB 0.1 - Windows personal playtest candidate

Milestones A-D are implemented: native Rust editor, import/metadata, waveforms/
thumbnails, persistent synchronized preview, precise editing, drag/trim/reorder,
snap/locks/ripple/undo, three audio lanes, gain/mute/fades, measured voice matching,
meters, SQLite projects/migrations, atomic saves/backups, per-session recovery,
relinking and validated H.264/AAC export with progress and cancellation.

Milestone E: **Windows synthetic automated workflow passes; final manual release
gate remains open.** There are no known failing automated core-workflow tests.
This is not a public release or a claim that every manual acceptance check passed.

Verified locally on Windows x64:

* 16 core/UI tests and 5 real-media integration tests pass.
* Native GUI: 193 steps, 30 minutes of sources, four distinct microphones, ten
  removals, three reorders, mix/fades, save and 3:50 export.
* New process: 14 steps, reopen/edit/save/export again; both files decode fully.
* Intentional interruption/autosave/native recovery preserves the manual save.
* Native launch and multi-file Windows dialog import.
* Release compilation, runtime checksums, portable package and launch checks as
  recorded in docs/verification and TESTING.md.

Artifacts: artifacts/SVBB-0.1.0-windows-x64/SonyVegasButBetter.exe and the adjacent
portable ZIP. Run directly and keep its runtime folder. PLAYTEST.md is the guide.

Limits: initial 960x540 video cache preparation takes time and disk space. Later
edits reuse it; export reads originals. SDR only, one video/two external audio
lanes, no same-lane overlaps, first source video/audio streams only, no titles,
compositing, advanced effects or plugin hosting.

Unverified: actual Explorer OS drop delivery, physical/high-DPI variants, device
loss, audible speech quality and the complete personal session with real gameplay.
The external mouse helper's boundary failure limits OS automation. macOS/Linux
were neither compiled nor run locally; CI and packaging configurations are ready
without a success claim. See PLAYTEST.md for remaining checks.

Public redistribution needs native runtime corresponding-source/notice audit
and codec patent/distribution review. The application is GPL-3.0-or-later; no
proprietary distribution clearance is claimed.
