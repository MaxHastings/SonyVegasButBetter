# Decisions

## Native shell
Use eframe/egui 0.31, a native Rust window with a custom timeline, no browser engine.
Qt Widgets would require an additional C++ build and binding layer; Slint's custom
timeline and media integration still require native adapters. Egui keeps the first
implementation in Rust, with a replaceable shell and a framework-independent core.
Its immediate-mode controls and native file dialogs meet the compact workstation
layout. This trades native OS menus for consistent in-window menus.

## Media adapter (revised after long-recording playtest)
The previous full-source video normalization and per-clip PCM preparation made long
recordings unusable until processing finished. Neither is on the interactive path now.
Persistent native FFmpeg video decoding retains the frame covering a VFR cut. Persistent
FFmpeg audio decoding feeds an on-demand virtual float stream to libmpv for buffering
and device output. This avoids EDL boundary frame loss and graph reinitialization when
sources change sample format, rate or channel count. No process launches per seek.

A small C++ shim owns FFmpeg structures; Rust owns project semantics, scheduling,
source requests and sample gain/fade evaluation. Vendored FFmpeg 9 headers and checked
ABI-major dynamic libraries make this boundary reviewable. Audio decodes on the demux
worker, not the device callback. Decoder contexts, decoded frames and read-ahead are
bounded independently of source duration.

## Direct manipulation and separate A/V
Audio and video are explicit separate clips. Link IDs provide synchronized positional
edits, while gain/fades remain independent. Legacy implicit audio is materialized by
schema migration. Normal trims follow the pointer and keep the opposite edge fixed;
ripple deletion is explicit. Overlapping body drops are rejected; reorder buttons
remain available. This removes the previous surprise ripple/reorder behavior.

## Hardware export
Auto performs a short real encoder probe. Listing an FFmpeg encoder alone does not
prove that a GPU/driver can initialize it. Compatible hardware encoding is preferred;
software remains selectable and provides an automatic fallback. CPU decoding/filtering
remain in the current offline graph, so GPU encoding is not a claim of an entirely
GPU-resident pipeline. Quality settings use each encoder's supported quality controls;
identical quality labels do not promise equal bitstreams or file sizes.

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
