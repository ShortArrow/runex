#!/bin/sh
# Prints the upstream version of the Debian package a tag builds:
# Cargo.toml's version for a release tag, and that version with the
# prerelease suffix after a "~" for a prerelease tag (v0.2.0-rc.1 gives
# 0.2.0~rc.1), which sorts before the release in Debian's ordering.
#
# Usage: deb-version.sh <Cargo.toml version> <tag>
# Exits 1 when the tag names a version other than Cargo.toml's.
set -eu

cargo="${1:?usage: deb-version.sh <Cargo.toml version> <tag>}"
tag="${2:?tag}"
version="${tag#v}"
base="${version%%-*}"

[ "$base" = "$cargo" ] || {
  echo "deb-version: $tag does not name version $cargo" >&2
  exit 1
}
case "$version" in
  *-*) echo "${base}~${version#*-}" ;;
  *) echo "$base" ;;
esac
