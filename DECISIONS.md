# Decisions

## User-defined upload ceilings and per-event crops (0.1.2)
Use a byte ceiling as a render goal, with quality as the variable. Generic decimal
MB limits do not bake a third-party account's changing upload policy into the app.
Budget AAC as well as video, keep mix gain unchanged, use retained per-job two-pass
statistics, and measure the completed file. CPU two-pass remains available for whole-edit bit allocation. Since 0.1.3,
Auto and explicit GPU choices also support size budgets using bitrate controls
and measured, bounded retries.

Store static source-pixel crops on individual video events, not media assets or
the project. Draw/move/resize in the existing preview, with original context visible
and one undoable commit per released gesture. Direct drawing enters framing without
an Apply step. Match project aspect is the default; Free and result preview
remain explicit controls. Source-time animation extends this model without changing
the meaning of existing static rectangles; the current behavior is documented below.

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
ripple deletion is explicit. Body drops resolve to the nearest available placement across all linked lanes,
with a live outline; touching edges are legal. Neighbours stay in place. Reorder
buttons remain available for explicit swaps.

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

## Public Windows packaging (0.1.3)
Publish the editor, source snapshot, notices and a first-run launcher. The launcher
retrieves the existing checksum-pinned media archives directly from upstream,
uses Windows tar to extract them, verifies every installed binary and retains
upstream notices. It reuses verified interrupted downloads, repairs an incomplete
runtime, and launches offline once complete. The public ZIP does not redistribute
the third-party media binaries; the fully bundled local playtest package stays
separate while its complete native corresponding-source distribution is unresolved.

Clean-PC PE import auditing exposed implicit VC++ and Vulkan-loader dependencies.
Windows release builds now statically link the compiler runtime and ship a small
app-local Vulkan loader built from pinned unmodified sources. Source archives and
notices accompany that loader; it changes no system installation or driver.

## Manual framing animation (0.1.6)

Static framing stays the default. Animate explicitly creates a source-time point;
subsequent seek-and-adjust gestures add or replace points. Linear interpolation
and endpoint holds make timing predictable. Points are per event and persist
through moves/splits/trims; each gesture has one undo. Turning motion off freezes
the current view. A 256-point cap keeps project validation and graph evaluation
bounded. Tracking is deferred until the framing workflow and correction interaction
have been tried on real gameplay; it should eventually produce the same editable
points rather than introduce a second framing system.
