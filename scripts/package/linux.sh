#!/bin/sh
# Builds a portable Linux folder and tarball into target/release/bundle.
#
#   scripts/package/linux.sh            build Sidedoor-linux-<arch>.tar.gz
#   scripts/package/linux.sh --install  also install it for this user and start it
#   scripts/package/linux.sh --packages also build DEB and RPM installers
#
# The folder holds the executable, Bun, the SDK and the built-in plugins,
# so it runs without a checkout or a system Bun installation. Installing
# copies it to ~/.local/opt/sidedoor and adds a launcher entry and icon.
set -eu

cd "$(dirname "$0")/../.."

NAME="Sidedoor"
EXECUTABLE="sidedoor"
# SIDEDOOR_VERSION overrides the workspace version, e.g. with a release tag.
VERSION=${SIDEDOOR_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}
ARCH=$(uname -m)

OUT="target/release/bundle"
DIR="$OUT/$NAME-linux-$ARCH"

# Resolve wrappers (e.g. a package-manager shim) to the real Bun executable.
BUN_BIN=$("${SIDEDOOR_BUN:-bun}" -p 'process.execPath')
[ -x "$BUN_BIN" ] || { echo "Bun executable not found" >&2; exit 1; }

echo "Building release binary…"
cargo build --release --locked

echo "Assembling ${DIR}…"
rm -rf "$DIR"
mkdir -p "$DIR/resources"
cp "target/release/$EXECUTABLE" "$DIR/$EXECUTABLE"
cp "$BUN_BIN" "$DIR/bun"
chmod +x "$DIR/$EXECUTABLE" "$DIR/bun"

# Built-ins are ordinary plugins, bundled so they need no SDK links or
# dependencies inside the installation.
for plugin in weather clipboard stats; do
    mkdir -p "$DIR/resources/builtins/$plugin"
    "$BUN_BIN" build "crates/desktop/src/builtins/$plugin/index.tsx" --target=bun \
        --outfile "$DIR/resources/builtins/$plugin/index.js"
done

# The plugin SDK, which plugins import as `@sidedoor/sdk`.
mkdir -p "$DIR/resources/sdk"
cp -R sdk/package.json sdk/tsconfig.json sdk/README.md sdk/src "$DIR/resources/sdk/"

cp crates/desktop/assets/icons/icon.svg "$DIR/sidedoor.svg"
cat > "$DIR/sidedoor.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$NAME
Comment=A dock at the edge of your screen
Exec=$EXECUTABLE
Icon=sidedoor
Categories=Utility;
StartupNotify=false
DESKTOP

tar -C "$OUT" -czf "$OUT/$NAME-linux-$ARCH.tar.gz" "$NAME-linux-$ARCH"
(cd "$OUT" && sha256sum "$NAME-linux-$ARCH.tar.gz" > "$NAME-linux-$ARCH.tar.gz.sha256")
echo "Built $DIR and $OUT/$NAME-linux-$ARCH.tar.gz ($VERSION)"

if [ "${1:-}" = "--packages" ]; then
    SIDEDOOR_VERSION="$VERSION" ./scripts/package/linux-installers.sh
fi

if [ "${1:-}" = "--install" ]; then
    DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
    DEST="$HOME/.local/opt/sidedoor"
    echo "Installing to ${DEST}…"
    pkill -x "$EXECUTABLE" >/dev/null 2>&1 || true
    rm -rf "$DEST"
    mkdir -p "$DEST" "$HOME/.local/bin" "$DATA/applications" "$DATA/icons/hicolor/scalable/apps"
    cp -R "$DIR/." "$DEST/"
    ln -sf "$DEST/$EXECUTABLE" "$HOME/.local/bin/$EXECUTABLE"
    cp "$DIR/sidedoor.svg" "$DATA/icons/hicolor/scalable/apps/sidedoor.svg"
    sed "s|^Exec=.*|Exec=\"$DEST/$EXECUTABLE\"|" "$DIR/sidedoor.desktop" > "$DATA/applications/sidedoor.desktop"
    nohup "$DEST/$EXECUTABLE" >/dev/null 2>&1 &
    echo "Installed and started $DEST"
fi
