# Licensing and runtime provenance

SVBB source is **GPL-3.0-or-later**; LICENSE contains GPL version 3. The selected
personal-playtest runtime uses GPL components, including x264. This build is not
offered as a proprietary-license distribution.

## Direct Rust dependencies

| Dependency | Purpose | License |
|---|---|---|
| eframe / egui | Native window and desktop controls | MIT OR Apache-2.0 |
| serde / serde_json | Plain media metadata encoding | MIT OR Apache-2.0 |
| rusqlite / bundled SQLite | Project persistence | MIT / SQLite public domain |
| uuid | Stable entity identifiers | MIT OR Apache-2.0 |
| rfd | Native file/message dialogs | MIT |
| libloading | Dynamic FFmpeg/libmpv ABI adapters | ISC |
| cc (build) | Compile the small native FFmpeg shim | MIT OR Apache-2.0 |
| sha2 | Disposable cache identity | MIT OR Apache-2.0 |
| image (PNG only) | Thumbnails and application mark | MIT OR Apache-2.0 |
| same-file | Source overwrite protection, including hard links | Unlicense OR MIT |
| anyhow | Error propagation | MIT OR Apache-2.0 |

tempfile and proptest are test dependencies. Cargo.lock pins all versions. The
complete target/dev dependency SPDX inventory is `licenses/cargo-dependencies.tsv`.
`licenses/dependencies` contains package and embedded asset notices copied from
the pinned registry sources. It includes packages that are not linked on Windows.
Regenerate using `python scripts/collect-licenses.py` after dependency changes.

## Media binaries in the Windows personal-playtest package

* FFmpeg and ffprobe: Gyan release **9.0.2 essentials**, GPLv3 static builds.
  Source commit **946fcce07b**; exact archive and binary hashes are in
  runtime-manifest.json. Configure flags are in `licenses/FFmpeg-configuration.txt`;
  the package also contains the original upstream build README and GPL text.
* libmpv: shinchiro **20260903**, mpv commit **69e63f425a**, x86_64 shared build.
  This provides audio-device output, buffering and master filtering.
  The pinned build recipe/release and binary hashes are in runtime-manifest.json.
  `licenses/mpv-GPL.txt` preserves the upstream GPL text. mpv/FFmpeg linked
  dependencies retain their own licenses and notices.

* Native preview FFmpeg shared libraries: BtbN autobuild **2026-10-18-13-01**,
  FFmpeg **n9.0.2-28-g669f5ab083**, GPL shared configuration. Five DLLs ship:
  avformat 63, avcodec 63, avutil 61, swresample 7 and swscale 10. The same build's
  headers are vendored under `native/include` with their original notices.
  Archive and DLL hashes are in runtime-manifest.json; the upstream license is
  `licenses/FFmpeg-preview-GPLv3.txt`.
  Source: https://github.com/FFmpeg/FFmpeg/commit/669f5ab083 and build recipe:
  https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-10-18-13-01.

Primary provenance: [Gyan build/source details](https://www.gyan.dev/ffmpeg/builds/),
[pinned FFmpeg source](https://github.com/FFmpeg/FFmpeg/commit/946fcce07b),
[pinned mpv binary/build release](https://github.com/shinchiro/mpv-winbuild-cmake/releases/tag/20260903),
[mpv source](https://github.com/mpv-player/mpv/commit/69e63f425a).

The installed Kdenlive runtime was inspected but is **not packaged**: it enables
`--enable-nonfree`. The selected FFmpeg configure record does not enable nonfree.
Downloads are explicit, versioned and checksum-checked; normal editing is offline.

## Encoder choices and redistribution boundary

Auto export probes actual NVENC, QSV, AMF or VideoToolbox initialization and falls
back to libx264. A failed Auto GPU render can retry on CPU before replacing the
destination. CPU uses x264 fast with CRF 25/21/17; NVENC uses p5/hq VBR with CQ
25/21/17 and zero target bitrate. QSV uses global quality 25/21/17, AMF CQP
25/21/17, and VideoToolbox hardware-only encoding with quality-dependent bitrate.
All paths output H.264 YUV420P and AAC stereo 48 kHz, 192 kbps with MP4 faststart.
GPU driver libraries are provided by the operating system.

The optional size-limit goal uses CPU libx264 medium, two-pass average bitrate
encoding and AAC stereo 48 kHz at 64/96/128/192 kbps according to the byte budget.
It has the same selected runtime licensing boundary as other exports.

Before public redistribution, audit complete corresponding-source/build-material
availability for every linked native runtime dependency and satisfy all notices
and source distribution obligations. The package contains SVBB source, Rust
notices and precise upstream source/build references; it does not contain a full
offline source tree of all third-party codec libraries. Proprietary distribution
and codec patent licensing have **not** been cleared. H.264/AAC patent obligations
depend on distribution/use and jurisdiction and are not resolved by open-source
copyright licenses. This is an outstanding public-distribution review item, not
a claim of legal clearance.
