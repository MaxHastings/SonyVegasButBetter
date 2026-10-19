# Contributing

Read BUILDING.md for native dependencies, ARCHITECTURE.md for component boundaries,
and DECISIONS.md for editing/export behavior. Keep source timestamps independent
of project frames; edits must validate atomically and remain undoable.

Before a pull request, run formatting, strict Clippy and model tests:

```
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --release --locked
```

Media changes also need the real-runtime integration commands in TESTING.md.
Interaction changes should exercise the native gui-test driver. Build distributable
executables without that feature. Update relevant docs and add a concise validation
note to the pull request. License/provenance records accompany dependency changes.

For bugs, describe the steps, expected/actual behavior, app version, Windows version
and GPU. A small synthetic reproducer is useful; avoid posting private recordings
or projects containing personal file paths. Redact identifying information in logs.

Maintainer release steps are in RELEASE.md.
