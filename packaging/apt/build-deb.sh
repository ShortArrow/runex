#!/bin/sh
# Builds the runex Debian package from a statically linked release
# executable: /usr/bin/runex and the licence under /usr/share/doc/runex.
# The package has no Depends, so the .deb also installs on its own with
# `apt install ./runex_<version>-1_<arch>.deb`.
#
# Usage: build-deb.sh <executable> <version> <amd64|arm64> <out dir>
# Writes <out dir>/runex_<version>-1_<arch>.deb and prints its path.
# A pre-release version is spelled with "~" (see deb-version.sh); one
# with "-" is refused, since it would sort after the release.
# Needs dpkg-deb.
set -eu

exe="${1:?usage: build-deb.sh <executable> <version> <amd64|arm64> <out dir>}"
version="${2:?version}"
arch="${3:?arch}"
out="${4:?out dir}"
repo="$(cd "$(dirname "$0")/../.." && pwd)"

case "$version" in
  *-*)
    echo "build-deb: $version contains '-'; spell a pre-release with '~'" >&2
    exit 1
    ;;
esac
case "$arch" in
  amd64 | arm64) ;;
  *)
    echo "build-deb: unknown architecture $arch" >&2
    exit 1
    ;;
esac
[ -f "$exe" ] || {
  echo "build-deb: $exe is not a file" >&2
  exit 1
}

root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT

chmod 0755 "$root"
mkdir -p "$root/usr/bin" "$root/usr/share/doc/runex" "$root/DEBIAN"
install -m 0755 "$exe" "$root/usr/bin/runex"
install -m 0644 "$repo/LICENSE" "$root/usr/share/doc/runex/"
cat >"$root/usr/share/doc/runex/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: runex
Source: https://github.com/ShortArrow/runex

Files: *
Copyright: ShortArrow
License: MIT or Apache-2.0
 The full texts are in /usr/share/doc/runex/LICENSE.
EOF
chmod 0644 "$root/usr/share/doc/runex/copyright"

installed_size="$(du -sk "$root/usr" | cut -f1)"
cat >"$root/DEBIAN/control" <<EOF
Package: runex
Version: ${version}-1
Architecture: ${arch}
Maintainer: ShortArrow <bamboogeneral@shortarrow.jp>
Installed-Size: ${installed_size}
Section: shells
Priority: optional
Homepage: https://github.com/ShortArrow/runex
Description: cross-shell abbreviation engine
 runex expands a short token into a full command as you type, in bash,
 zsh, PowerShell and Nushell, from one config file.
EOF

mkdir -p "$out"
deb="$out/runex_${version}-1_${arch}.deb"
dpkg-deb --root-owner-group -Zxz --build "$root" "$deb" >/dev/null
echo "$deb"
