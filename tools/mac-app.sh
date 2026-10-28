#!/bin/sh
# mac-app.sh [--uninstall] -- "Copal TM.app" in APPDIR (default ~/Applications),
# so copal-tm opens from Finder, Launchpad, Spotlight and the Dock. `make
# install` runs it on a Mac after the binary is in place; APP=0 skips it.
#
# copal-tm draws in a terminal, so the app is a launcher: it asks Terminal
# to run the copy of copal-tm inside the bundle, in a new window. `open -a
# Terminal FILE` runs an executable file, which needs no AppleScript and so
# no Automation permission. The bundle is signed ad hoc, the binary first.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
APPDIR=${APPDIR:-$HOME/Applications}
APP="$APPDIR/Copal TM.app"
[ "$(uname -s)" = Darwin ] || { echo "mac-app.sh: this is for macOS" >&2; exit 2; }

if [ "${1:-}" = --uninstall ]; then
    rm -rf "$APP"
    echo "removed $APP"
    exit 0
fi

BIN="$ROOT/target/release/copal-tm"
[ -x "$BIN" ] || { echo "mac-app.sh: build first (make build)" >&2; exit 1; }
VERSION=$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)

# Built beside the old one and swapped in, so a failure leaves the old.
NEW="$APP.new"
rm -rf "$NEW"
mkdir -p "$NEW/Contents/MacOS" "$NEW/Contents/Resources"
install -m 755 "$BIN" "$NEW/Contents/MacOS/copal-tm"

# Not named after the program: a Mac's disk ignores case.
cat > "$NEW/Contents/MacOS/launch" <<'EOF'
#!/bin/sh
# Copal TM.app's launcher, written by copal-tm's tools/mac-app.sh.
exec open -a Terminal "$(dirname "$0")/copal-tm"
EOF
chmod 755 "$NEW/Contents/MacOS/launch"

cat > "$NEW/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Copal TM</string>
    <key>CFBundleDisplayName</key><string>Copal TM</string>
    <key>CFBundleIdentifier</key><string>org.copal.copal-tm</string>
    <key>CFBundleExecutable</key><string>launch</string>
    <key>CFBundleIconFile</key><string>copal-tm</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSUIElement</key><true/>
</dict>
</plist>
EOF

# The icon: one drawing at 1024, scaled by sips into the iconset's sizes.
TMP=$(mktemp -d)
SET="$TMP/copal-tm.iconset"
mkdir -p "$SET"
python3 "$ROOT/tools/mac-icon.py" "$SET/icon_512x512@2x.png"
for s in 16 32 128 256 512; do
    sips -z $s $s "$SET/icon_512x512@2x.png" --out "$SET/icon_${s}x${s}.png" >/dev/null
    d=$((s * 2))
    [ $s = 512 ] || sips -z $d $d "$SET/icon_512x512@2x.png" --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$NEW/Contents/Resources/copal-tm.icns"
rm -rf "$TMP"

codesign --force --sign - "$NEW/Contents/MacOS/copal-tm"
codesign --force --sign - "$NEW"

mkdir -p "$APPDIR"
rm -rf "$APP"
mv "$NEW" "$APP"
touch "$APP"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP" 2>/dev/null || true
echo "installed $APP"
