#!/bin/sh
# Builds "Sidekick Clone.app" into target/release/bundle.
#
#   scripts/bundle.sh            build the bundle
#   scripts/bundle.sh --install  also copy it to /Applications and open it
#
# The bundle is signed ad hoc, which is enough to run it on this Mac.
# Distributing it to other Macs needs a Developer ID signature and
# notarization.
set -eu

cd "$(dirname "$0")/.."

NAME="Sidekick Clone"
BUNDLE_ID="com.lassevestergaard.sidekick-clone"
EXECUTABLE="sidekick"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
BUILD=$(git rev-list --count HEAD 2>/dev/null || echo 1)

OUT="target/release/bundle"
APP="$OUT/$NAME.app"

echo "Building release binary…"
cargo build --release --locked

echo "Assembling ${APP}…"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/release/$EXECUTABLE" "$APP/Contents/MacOS/$EXECUTABLE"

# The plugin SDK, which plugins import as `@sidekick/sdk`.
mkdir -p "$APP/Contents/Resources/sdk"
cp -R sdk/package.json sdk/tsconfig.json sdk/README.md sdk/src "$APP/Contents/Resources/sdk/"

# App icon: every size macOS asks for, rendered from the SVG.
ICONSET="$OUT/AppIcon.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    rsvg-convert -w "$size" -h "$size" assets/icon.svg -o "$ICONSET/icon_${size}x${size}.png"
    double=$((size * 2))
    rsvg-convert -w "$double" -h "$double" assets/icon.svg -o "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$ICONSET"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>$NAME</string>
    <key>CFBundleExecutable</key>
    <string>$EXECUTABLE</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$BUILD</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.utilities</string>
    <key>LSMinimumSystemVersion</key>
    <string>15.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>A personal Sidekick-style dock.</string>
</dict>
</plist>
PLIST

echo "Signing (ad hoc)…"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --strict "$APP"

echo "Built $APP ($VERSION, build $BUILD)"

if [ "${1:-}" = "--install" ]; then
    DEST="/Applications/$NAME.app"
    echo "Installing to ${DEST}…"
    osascript -e "tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1 || true
    pkill -x "$EXECUTABLE" >/dev/null 2>&1 || true
    rm -rf "$DEST"
    ditto "$APP" "$DEST"
    open "$DEST"
    echo "Installed and opened $DEST"
fi
