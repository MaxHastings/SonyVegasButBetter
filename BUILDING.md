# Build and runtime setup

Rust 1.93.1 is pinned in rust-toolchain.toml; Cargo.lock pins the dependency graph.
Use a C/C++ toolchain for bundled SQLite and the platform's native window libraries.

## Windows x64

Install Visual Studio Build Tools with Desktop C++ and a Windows SDK, Rust/MSVC,
and 7-Zip. Run `powershell -File scripts/bootstrap-runtime.ps1` from this directory.
It downloads checksum-pinned FFmpeg/ffprobe and libmpv packages, then extracts
only the selected runtime and notices. It does not install system-wide software.

```
cargo build --locked
cargo run --locked
cargo test --locked
cargo test --locked --test media_pipeline -- --ignored --nocapture --test-threads=1
cargo test --locked --test media_edges -- --ignored --nocapture --test-threads=1
cargo build --release --locked
powershell -File scripts/package-windows.ps1
```

Do not rebuild an executable while that same build is running: Windows locks it.
Use the release test/build profile while a debug GUI session is active.

The native output is `target/release/svbb.exe`. Packaging copies and renames it to
`artifacts/SVBB-0.1.0-windows-x64/SonyVegasButBetter.exe`, with an adjacent runtime,
documentation, source snapshot, dependency notices and license text.

## Linux

Install Rust 1.93.1, a C compiler, pkg-config, FFmpeg ≥8, libmpv2, GTK3 development
libraries, X11 development libraries, Wayland, xkbcommon and OpenGL. For Debian/
Ubuntu, see the explicit apt list in `.github/workflows/build.yml`; if its FFmpeg
package is older than 8, a newer media runtime is required for editing. Run
`cargo build --release --locked`, then `target/release/svbb`.

Default runtime lookup uses PATH for FFmpeg/ffprobe and `libmpv.so.2` for playback.
Set `SVBB_MPV` if your library has a different absolute path/name. X11 and Wayland
features are compiled; neither desktop environment has been run locally.

## macOS

Install Xcode command line tools and Rust 1.93.1. Install FFmpeg and mpv through
Homebrew. Set `SVBB_MPV=$(brew --prefix mpv)/lib/libmpv.2.dylib` if necessary.
Run `cargo build --release --locked`. `scripts/package-unix.sh` creates a basic
`.app` bundle on macOS or a portable binary folder on Linux. These packages rely
on the documented system media runtime. Signing/notarization is not configured.

## Discovery and reproducibility

`SVBB_RUNTIME` overrides the runtime directory. Otherwise discovery prefers an
adjacent `runtime`, then the source workspace's runtime. Unix library names are
resolved by the dynamic loader. Cache data is independent of project data.

Runtime URLs, binary/archive SHA-256 hashes, source commits and build flags are
recorded in runtime-manifest.json and LICENSES.md. Re-running the download script
fails on a changed archive rather than silently upgrading. Distribution-system
Unix media packages are not bit-for-bit pinned; record their versions in results.
