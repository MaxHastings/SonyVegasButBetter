# Windows release checklist

1. Start from a clean main checkout; update the version/changelog as appropriate.
2. Run the checks in TESTING.md, including real media and the native GUI stage(s)
   affected by the change. Build with `cargo build --release --locked`, without
   gui-test. Windows packaging also audits PE imports and builds the app-local
   Vulkan loader from pinned source with static compiler-runtime linkage. Keep Windows/macOS/Linux CI green.
3. Commit source/docs first. Run `powershell -File scripts/package-windows-release.ps1`.
   It embeds `git archive HEAD` and refuses to mix an existing output directory.
4. Verify all SHA256SUMS entries, the executable against the built binary, the
   embedded source against HEAD, and the outer ZIP. Extract into a fresh path with
   spaces, run setup with 64-bit Windows PowerShell, test offline repeat launch and
   checksum-based repair, then open the app and render/decode an output.
5. Tag that commit and create a GitHub draft release. Attach the **first-run ZIP**,
   its SHA-256 checksum, and release notes with setup instructions and known limits.
   Verify the uploaded asset hash before publishing the Windows preview.
6. Update the README's download link for a new version. Keep current packages and
   checked-in verification records; remove superseded generated builds/caches and
   merged branches after publication. Preserve user projects and original media.

The public archive excludes third-party media binaries; setup fetches their pinned
upstream archives and retains upstream notices. `scripts/package-windows.ps1` is
the separate local bundled playtest packager, not the public release command.

The app is unsigned. Do not describe it as signed, certified, thoroughly tested on
every GPU, or immune to Windows download/security prompts. The first release is an
early preview; user real-footage playtesting remains useful.
