#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
if [ "$(uname -s)" = Darwin ]; then
    dest="artifacts/SonyVegasButBetter.app/Contents"
    mkdir -p "$dest/MacOS" "$dest/Resources"
    cp target/release/svbb "$dest/MacOS/SonyVegasButBetter"
    cp PLAYTEST.md LICENSES.md "$dest/Resources/"
    cat > "$dest/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleExecutable</key><string>SonyVegasButBetter</string><key>CFBundleIdentifier</key><string>local.svbb.editor</string><key>CFBundleName</key><string>SonyVegasButBetter</string><key>CFBundleVersion</key><string>0.1.0</string><key>NSHighResolutionCapable</key><true/></dict></plist>
PLIST
else
    dest=artifacts/SVBB-0.1.0-linux
    mkdir -p "$dest"
    cp target/release/svbb README.md PLAYTEST.md LICENSES.md "$dest/"
fi
printf 'Prepared %s; install system FFmpeg and libmpv as documented.\n' "$dest"
