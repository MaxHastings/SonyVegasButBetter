# Decisions

## Native shell
Use eframe/egui 0.31, a native Rust window with a custom timeline, no browser engine.
Qt Widgets would require an additional C++ build and binding layer; Slint's custom
timeline and media integration still require native adapters. Egui keeps the first
implementation in Rust, with a replaceable shell and a framework-independent core.
Its immediate-mode controls and native file dialogs meet the compact workstation
layout. This trades native OS menus for consistent in-window menus.

## Media adapter
Use pinned libmpv (FFmpeg libraries internally) for persistent decoding, accurate
timestamp seeking, audio output and synchronized presentation. The C ABI is isolated
in playback.rs. Offline jobs use FFmpeg and ffprobe processes with argument arrays.
Preview compiles disposable EDLs; EDL is never authoritative project storage.

Audio clips are cached as stereo float PCM using the same resample/gain/fade filters
as export. Only changed clips need rebuilding. Three audio EDL lanes are mixed by
libavfilter in mpv using the same track gain, summation and peak protection as export.
Playback waits for a changed mix to prepare; the UI and video seeking remain usable.
This avoids disk or decode work in an application audio callback. Cache latency is a
known tradeoff to measure with the acceptance workload.

## Timing and persistence
Integer ticks at 282,240,000 Hz represent ordinary broadcast frame rates and 44.1/48
kHz sample positions exactly. Media retains rational stream timebases and start PTS.
Use checked i128 conversions, with no accumulated floating-point seconds. SQLite
transactions, explicit migrations and stable UUIDs preserve edits independently of UI.

## Licensing
The application is GPL-3.0-or-later, compatible with the selected GPL libmpv and x264
runtime. Never redistribute the installed Kdenlive FFmpeg: its build enables nonfree.
Acquire a separate GPL runtime and record its configuration and hashes. Proprietary
distribution would require a different audited runtime strategy and legal review.
