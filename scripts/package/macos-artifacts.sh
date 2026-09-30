#!/bin/sh
# Package the already assembled, signed app without rebuilding it.
set -eu
cd "$(dirname "$0")/../.."
OUT="target/release/bundle"
ARCH=$(uname -m)
ZIP="Sidedoor-macos-$ARCH.zip"
DMG="Sidedoor-macos-$ARCH.dmg"
[ -d "$OUT/Sidedoor.app" ] || { echo "Build Sidedoor.app first" >&2; exit 1; }
STAGE=$(mktemp -d "$OUT/dmg.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT HUP INT TERM
ditto "$OUT/Sidedoor.app" "$STAGE/Sidedoor.app"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname Sidedoor -srcfolder "$STAGE" -ov -format UDZO "$OUT/$DMG"
hdiutil verify "$OUT/$DMG"
ditto -c -k --keepParent "$OUT/Sidedoor.app" "$OUT/$ZIP"
(cd "$OUT" && shasum -a 256 "$ZIP" > "$ZIP.sha256" && shasum -a 256 "$DMG" > "$DMG.sha256")
echo "Built $OUT/$ZIP and $OUT/$DMG"
