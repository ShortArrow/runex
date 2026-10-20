#!/bin/sh
# Installs the runex package in fresh Debian and Ubuntu containers and
# runs `runex version`, which must print the version and nothing on
# stderr.
#
# The package is installed twice per image: with apt from a throwaway
# repository signed with a key made for this run, beside a stand-in
# shortarrow-archive-keyring package holding that key, so apt checks the
# signature as it does against pkg.shortarrow.jp; and as a lone file with
# `apt-get install ./runex_*.deb`, as a user who downloads it from the
# GitHub Release would.
#
# Usage: install-test.sh <deb dir> <version> <amd64|arm64>
# Needs docker, gpg, dpkg-deb and apt-ftparchive (apt-utils).
set -eu

debs="${1:?usage: install-test.sh <deb dir> <version> <amd64|arm64>}"
version="${2:?version}"
arch="${3:?arch}"
keyring_name=shortarrow-archive-keyring

work="$(mktemp -d)"
containers=''
cleanup() {
  for c in $containers; do docker rm -f "$c" >/dev/null 2>&1 || true; done
  rm -rf "$work"
}
trap cleanup EXIT

export GNUPGHOME="$work/gnupg"
mkdir -m 0700 "$GNUPGHOME"
gpg --batch --quiet --passphrase '' --quick-gen-key 'runex install test <test@invalid>' ed25519 sign 1d

repo="$work/repo/apt"
mkdir -p "$repo/pool/test" "$repo/dists/stable/main/binary-$arch"
gpg --batch --export >"$repo/$keyring_name.gpg"

kr="$work/keyring"
mkdir -p "$kr/DEBIAN" "$kr/usr/share/keyrings"
cp "$repo/$keyring_name.gpg" "$kr/usr/share/keyrings/"
cat >"$kr/DEBIAN/control" <<EOF
Package: $keyring_name
Version: 0.0.0~test
Architecture: all
Maintainer: runex install test <test@invalid>
Description: throwaway keyring for the runex install test
EOF
dpkg-deb --root-owner-group --build "$kr" "$repo/pool/test/${keyring_name}_0.0.0~test_all.deb" >/dev/null
cp "$debs"/runex_*_"$arch".deb "$repo/pool/test/"

(
  cd "$repo"
  apt-ftparchive --arch "$arch" packages pool >"dists/stable/main/binary-$arch/Packages"
  gzip -k9 "dists/stable/main/binary-$arch/Packages"
  apt-ftparchive \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Components=main \
    -o "APT::FTPArchive::Release::Architectures=$arch" \
    release dists/stable >"$work/Release"
  mv "$work/Release" dists/stable/Release
  gpg --batch --yes --clearsign -o dists/stable/InRelease dists/stable/Release
)

check_runex='
    out=$(runex version 2>/tmp/stderr)
    echo "runex version -> $out"
    case "$out" in
      "runex '"$version"' "*) ;;
      *) echo "expected runex '"$version"'"; exit 1 ;;
    esac
    if [ -s /tmp/stderr ]; then cat /tmp/stderr; exit 1; fi'

from_repo="
    set -eu
    cp /repo/apt/$keyring_name.gpg /usr/share/keyrings/
    echo 'deb [signed-by=/usr/share/keyrings/$keyring_name.gpg] file:/repo/apt stable main' \
      >/etc/apt/sources.list.d/shortarrow.list
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends $keyring_name runex >/dev/null
    $check_runex"

from_file="
    set -eu
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends /repo/apt/pool/test/runex_*_$arch.deb >/dev/null
    $check_runex"

failed=0
for image in debian:bookworm ubuntu:22.04 ubuntu:24.04; do
  for how in repo file; do
    name="runex-install-test-$$-$how-$(echo "$image" | tr ':.' '--')"
    containers="$containers $name"
    if [ "$how" = repo ]; then script="$from_repo"; else script="$from_file"; fi
    docker create --name "$name" "$image" sh -c "$script" >/dev/null
    docker cp "$work/repo" "$name:/repo"
    echo "== $image ($arch), from $how"
    if docker start -a "$name"; then
      echo "install test passed on $image from $how"
    else
      echo "install test FAILED on $image from $how"
      failed=1
    fi
  done
done
exit "$failed"
