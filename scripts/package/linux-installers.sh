#!/bin/sh
# Build native packages from linux.sh's complete portable payload.
# Requires dpkg-dev, rpm, and desktop-file-utils on the Linux build host.
set -eu
cd "$(dirname "$0")/../.."
OUT="$(pwd)/target/release/bundle"
ARCH=$(uname -m)
case "$ARCH" in
    x86_64) DEB_ARCH=amd64 ;;
    aarch64) DEB_ARCH=arm64 ;;
    *) echo "Unsupported Linux architecture: $ARCH" >&2; exit 1 ;;
esac
VERSION=${SIDEDOOR_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}
printf '%s\n' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$' || {
    echo "Expected a semantic release version, got: $VERSION" >&2; exit 1;
}
# Both package managers sort prereleases before the corresponding stable version.
PACKAGE_VERSION=$(printf '%s' "${VERSION%%+*}" | sed 's/-/~/')
PAYLOAD="$OUT/Sidedoor-linux-$ARCH"
for file in sidedoor bun resources/sdk/package.json resources/builtins/weather/index.js resources/builtins/clipboard/index.js resources/builtins/stats/index.js; do
    [ -f "$PAYLOAD/$file" ] || { echo "Missing Linux payload: $file" >&2; exit 1; }
done
WORK=$(mktemp -d "$OUT/linux-packages.XXXXXX")
trap 'rm -rf "$WORK"' EXIT HUP INT TERM
ROOT="$WORK/root"
mkdir -p "$ROOT/usr/lib/sidedoor" "$ROOT/usr/bin" "$ROOT/usr/share/applications" \
    "$ROOT/usr/share/icons/hicolor/scalable/apps" "$WORK/debian"
cp -R "$PAYLOAD/." "$ROOT/usr/lib/sidedoor/"
install -m 0644 "$PAYLOAD/sidedoor.svg" "$ROOT/usr/share/icons/hicolor/scalable/apps/sidedoor.svg"
sed 's|^Exec=.*|Exec=/usr/lib/sidedoor/sidedoor|' "$PAYLOAD/sidedoor.desktop" > "$ROOT/usr/share/applications/sidedoor.desktop"
desktop-file-validate "$ROOT/usr/share/applications/sidedoor.desktop"
ln -s ../lib/sidedoor/sidedoor "$ROOT/usr/bin/sidedoor"
# Normalize permissions; never ship files owned by the CI runner.
find "$ROOT" -type d -exec chmod 0755 {} +
find "$ROOT" -type f -exec chmod 0644 {} +
chmod 0755 "$ROOT/usr/lib/sidedoor/sidedoor" "$ROOT/usr/lib/sidedoor/bun"
cat > "$WORK/debian/control" <<CONTROL
Source: sidedoor
Section: utils
Priority: optional
Maintainer: Lasse Vestergaard <77295879+lassejlv@users.noreply.github.com>

Package: sidedoor
Architecture: any
Description: A second dock at the edge of your screen
CONTROL
# Derive dependencies from BOTH executables, using the build distribution's
# library metadata rather than copying a development-package dependency list.
DEPENDS=$(cd "$WORK" && dpkg-shlibdeps -O -e"$ROOT/usr/lib/sidedoor/sidedoor" -e"$ROOT/usr/lib/sidedoor/bun")
DEPENDS=${DEPENDS#shlibs:Depends=}
mkdir -p "$ROOT/DEBIAN"
cat > "$ROOT/DEBIAN/control" <<CONTROL
Package: sidedoor
Version: $PACKAGE_VERSION
Section: utils
Priority: optional
Architecture: $DEB_ARCH
Maintainer: Lasse Vestergaard <77295879+lassejlv@users.noreply.github.com>
Depends: $DEPENDS, libvulkan1, xdg-utils
Description: A second dock at the edge of your screen
 Includes Bun, the plugin SDK, and built-in widgets.
CONTROL
DEB="$OUT/Sidedoor-linux-$ARCH.deb"
dpkg-deb --root-owner-group --build "$ROOT" "$DEB"
dpkg-deb --info "$DEB"
dpkg-deb --contents "$DEB"
# DEBIAN is control metadata, not part of the RPM payload.
rm -rf "$ROOT/DEBIAN"
mkdir -p "$WORK/rpm/BUILD" "$WORK/rpm/BUILDROOT" "$WORK/rpm/RPMS" "$WORK/rpm/SOURCES" "$WORK/rpm/SPECS" "$WORK/rpm/SRPMS"
cat > "$WORK/rpm/SPECS/sidedoor.spec" <<SPEC
Name: sidedoor
Version: $PACKAGE_VERSION
Release: 1
Summary: A second dock at the edge of your screen
# The repository has no declared project license; do not invent one.
License: LicenseRef-Unknown
URL: https://github.com/lassejlv/sidedoor
AutoReqProv: yes
# Vulkan is loaded dynamically, and opening files uses xdg-open.
Requires: libvulkan.so.1()(64bit), xdg-utils
%global debug_package %{nil}

%description
A second dock at the edge of your screen, including Bun, the plugin SDK,
and built-in widgets. Requires an X11 desktop and a Vulkan driver.

%install
mkdir -p "%{buildroot}"
cp -a "$ROOT/usr" "%{buildroot}/"

%files
%defattr(-,root,root,-)
/usr/lib/sidedoor
/usr/bin/sidedoor
/usr/share/applications/sidedoor.desktop
/usr/share/icons/hicolor/scalable/apps/sidedoor.svg
SPEC
# RPM discovers ELF/soname and symbol-version requirements for its own
# distributions. Do not strip the already built binaries again.
rpmbuild -bb --target "$ARCH" --define "_topdir $WORK/rpm" --define '__os_install_post %{nil}' "$WORK/rpm/SPECS/sidedoor.spec"
RPM="$OUT/Sidedoor-linux-$ARCH.rpm"
cp "$WORK/rpm/RPMS/$ARCH/sidedoor-$PACKAGE_VERSION-1.$ARCH.rpm" "$RPM"
rpm -qip "$RPM"
rpm -qlp "$RPM"
rpm -qp --requires "$RPM"
(cd "$OUT" && sha256sum "$(basename "$DEB")" > "$(basename "$DEB").sha256" && sha256sum "$(basename "$RPM")" > "$(basename "$RPM").sha256")
echo "Built $DEB and $RPM"
