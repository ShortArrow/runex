#!/usr/bin/env bash
# Update runex-bin AUR package to a given version and push.
#
# Usage:
#   packaging/aur-bin/release.sh <version> [<aur-repo-path>]
#
# Example:
#   packaging/aur-bin/release.sh 0.1.12 ~/aur/runex-bin
#
# What it does:
#   1. Fetches SHA256 of the Linux x86_64/aarch64 release tarballs and LICENSE/README
#   2. Rewrites pkgver + sha256sums in PKGBUILD
#   3. Regenerates .SRCINFO via `makepkg --printsrcinfo`
#   4. Copies PKGBUILD and .SRCINFO into the AUR working clone
#   5. Commits and prints the `git push` command (does NOT push automatically)

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <version> [<aur-repo-path>]" >&2
    exit 1
fi

VERSION="$1"
AUR_REPO="${2:-$HOME/aur/runex-bin}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKGBUILD="$SCRIPT_DIR/PKGBUILD"

if [[ ! -f "$PKGBUILD" ]]; then
    echo "PKGBUILD not found: $PKGBUILD" >&2
    exit 1
fi

if [[ ! -d "$AUR_REPO/.git" ]]; then
    echo "AUR clone not found: $AUR_REPO" >&2
    echo "Run: git clone ssh://aur@aur.archlinux.org/runex-bin.git $AUR_REPO" >&2
    exit 1
fi

sha_of() {
    local url="$1"
    curl -fsSL "$url" | sha256sum | awk '{print $1}'
}

BASE="https://github.com/ShortArrow/runex/releases/download/v${VERSION}"
RAW="https://raw.githubusercontent.com/ShortArrow/runex/v${VERSION}"

echo "Fetching SHA256 checksums for v${VERSION}..."
SHA_X64=$(sha_of "${BASE}/runex-x86_64-unknown-linux-gnu.tar.gz")
SHA_ARM=$(sha_of "${BASE}/runex-aarch64-unknown-linux-gnu.tar.gz")
SHA_LIC=$(sha_of "${RAW}/LICENSE")
SHA_README=$(sha_of "${RAW}/README.md")

echo "  x86_64:  $SHA_X64"
echo "  aarch64: $SHA_ARM"
echo "  LICENSE: $SHA_LIC"
echo "  README:  $SHA_README"

sed -i \
    -e "s/^pkgver=.*/pkgver=${VERSION}/" \
    -e "s/^pkgrel=.*/pkgrel=1/" \
    "$PKGBUILD"

# Rewrite sha256sums arrays. We expect the existing format exactly as committed.
python3 - "$PKGBUILD" "$SHA_X64" "$SHA_ARM" "$SHA_LIC" "$SHA_README" <<'PY'
import re, sys
path, sha_x64, sha_arm, sha_lic, sha_readme = sys.argv[1:6]
src = open(path).read()
src = re.sub(r"sha256sums_x86_64=\([^\)]*\)", f"sha256sums_x86_64=('{sha_x64}')", src)
src = re.sub(r"sha256sums_aarch64=\([^\)]*\)", f"sha256sums_aarch64=('{sha_arm}')", src)
src = re.sub(
    r"sha256sums=\([^\)]*\)",
    f"sha256sums=('{sha_lic}'\n            '{sha_readme}')",
    src,
)
open(path, "w").write(src)
PY

echo "Regenerating .SRCINFO..."
(cd "$SCRIPT_DIR" && makepkg --printsrcinfo > .SRCINFO)

echo "Copying PKGBUILD and .SRCINFO into $AUR_REPO..."
cp "$SCRIPT_DIR/PKGBUILD" "$AUR_REPO/PKGBUILD"
cp "$SCRIPT_DIR/.SRCINFO" "$AUR_REPO/.SRCINFO"

(cd "$AUR_REPO" && git add PKGBUILD .SRCINFO && git commit -m "Update to ${VERSION}")

cat <<EOF

Done. Review with:
    cd $AUR_REPO && git show

Push when ready:
    cd $AUR_REPO && GIT_SSH_COMMAND='ssh -i ~/.ssh/aur' git push origin master
EOF
