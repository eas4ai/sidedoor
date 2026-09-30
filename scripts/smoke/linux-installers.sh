#!/bin/sh
# Run on an isolated Linux CI runner after linux.sh --packages.
set -eu
cd "$(dirname "$0")/../.."
OUT="$(pwd)/target/release/bundle"
ARCH=$(uname -m)
PAYLOAD="$OUT/Sidedoor-linux-$ARCH"
WORK=$(mktemp -d "$OUT/linux-smoke.XXXXXX")
trap 'rm -rf "$WORK"' EXIT HUP INT TERM
(cd "$OUT" && sha256sum -c "Sidedoor-linux-$ARCH.deb.sha256" "Sidedoor-linux-$ARCH.rpm.sha256")
dpkg-deb --extract "$OUT/Sidedoor-linux-$ARCH.deb" "$WORK/deb"
mkdir -p "$WORK/rpm"
# Keep pipeline failures visible in POSIX sh by extracting in two steps.
rpm2cpio "$OUT/Sidedoor-linux-$ARCH.rpm" > "$WORK/payload.cpio"
(cd "$WORK/rpm" && cpio -id --quiet < "$WORK/payload.cpio")
for FORMAT in deb rpm; do
    diff -r "$PAYLOAD" "$WORK/$FORMAT/usr/lib/sidedoor"
    [ "$(readlink "$WORK/$FORMAT/usr/bin/sidedoor")" = ../lib/sidedoor/sidedoor ]
    desktop-file-validate "$WORK/$FORMAT/usr/share/applications/sidedoor.desktop"
    "$WORK/$FORMAT/usr/lib/sidedoor/bun" --version
done
# Let apt resolve the actual runtime requirements, then check the system paths.
sudo apt-get install -y "$OUT/Sidedoor-linux-$ARCH.deb"
trap 'sudo apt-get remove -y sidedoor; rm -rf "$WORK"' EXIT HUP INT TERM
[ "$(readlink -f /usr/bin/sidedoor)" = /usr/lib/sidedoor/sidedoor ]
diff -r "$PAYLOAD" /usr/lib/sidedoor
/usr/lib/sidedoor/bun --version
sudo apt-get remove -y sidedoor
[ ! -e /usr/bin/sidedoor ]
[ ! -e /usr/lib/sidedoor ]
[ ! -e /usr/share/applications/sidedoor.desktop ]
[ ! -e /usr/share/icons/hicolor/scalable/apps/sidedoor.svg ]
trap 'rm -rf "$WORK"' EXIT HUP INT TERM
echo "DEB installed and uninstalled; DEB/RPM payloads and bundled Bun verified."
