#!/usr/bin/env bash
# Update the AUR `runex` (source build) package to a given version.
#
# Sibling of packaging/aur-bin/release.sh, but the source is the
# `.crate` published to crates.io by release.yml's publish-crates job
# rather than a GitHub Release tarball. Must therefore run AFTER the
# tag push has reached crates.io (same gate as Homebrew).
#
# Usage:
#   packaging/aur/release.sh <version> [<aur-repo-path>]
#
# Example:
#   packaging/aur/release.sh 0.1.17 ~/aur/runex
#
# What it does:
#   1. Fetches SHA256 of the crates.io `.crate` and LICENSE
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
AUR_REPO="${2:-$HOME/aur/runex}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKGBUILD="$SCRIPT_DIR/PKGBUILD"

if [[ ! -f "$PKGBUILD" ]]; then
    echo "PKGBUILD not found: $PKGBUILD" >&2
    exit 1
fi

if [[ ! -d "$AUR_REPO/.git" ]]; then
    echo "AUR clone not found: $AUR_REPO" >&2
    echo "Run: git clone ssh://aur@aur.archlinux.org/runex.git $AUR_REPO" >&2
    exit 1
fi

sha_of() {
    local url="$1"
    curl -fsSL "$url" | sha256sum | awk '{print $1}'
}

CRATE_URL="https://crates.io/api/v1/crates/runex/${VERSION}/download"
RAW="https://raw.githubusercontent.com/ShortArrow/runex/v${VERSION}"

# LICENSE is bundled with the AUR clone (referenced as a bare `LICENSE`
# entry in PKGBUILD's source=), so its sha must match the actual file
# we copy in. We pull it from the in-repo LICENSE rather than from the
# tag's raw URL — that way the runex repo is the single source of
# truth and a missed v-tag push doesn't desync the AUR build.
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
REPO_LICENSE="$REPO_ROOT/LICENSE"
if [[ ! -f "$REPO_LICENSE" ]]; then
    echo "Repo-root LICENSE not found: $REPO_LICENSE" >&2
    exit 1
fi

echo "Fetching SHA256 checksums for v${VERSION}..."
SHA_CRATE=$(sha_of "$CRATE_URL")
SHA_LIC=$(sha256sum "$REPO_LICENSE" | awk '{print $1}')

echo "  crate:   $SHA_CRATE"
echo "  LICENSE: $SHA_LIC (from repo root LICENSE)"

sed -i \
    -e "s/^pkgver=.*/pkgver=${VERSION}/" \
    -e "s/^pkgrel=.*/pkgrel=1/" \
    "$PKGBUILD"

# Rewrite the single multi-line sha256sums=(...) block. The crate is the
# first entry and LICENSE is the second; that order matches the `source=`
# block above it. Whitespace inside the parens is preserved-ish (we
# re-emit with the same 12-space continuation that Dominiquini's original
# template used) so a downstream diff stays small.
python3 - "$PKGBUILD" "$SHA_CRATE" "$SHA_LIC" <<'PY'
import re, sys
path, sha_crate, sha_lic = sys.argv[1:4]
src = open(path).read()
pattern = re.compile(
    r"sha256sums=\(\s*'[0-9a-f]{64}'\s*\n\s*'[0-9a-f]{64}'\s*\)",
    re.M,
)
replacement = f"sha256sums=('{sha_crate}'\n            '{sha_lic}')"
new, n = pattern.subn(replacement, src, count=1)
if n != 1:
    raise SystemExit(f"failed to rewrite sha256sums block in {path} (matched {n} times)")
open(path, "w").write(new)
PY

echo "Regenerating .SRCINFO..."
(cd "$SCRIPT_DIR" && makepkg --printsrcinfo > .SRCINFO)

echo "Copying PKGBUILD, .SRCINFO, .nvchecker.toml, and LICENSE into $AUR_REPO..."
cp "$SCRIPT_DIR/PKGBUILD" "$AUR_REPO/PKGBUILD"
cp "$SCRIPT_DIR/.SRCINFO" "$AUR_REPO/.SRCINFO"
cp "$SCRIPT_DIR/.nvchecker.toml" "$AUR_REPO/.nvchecker.toml"
cp "$REPO_LICENSE" "$AUR_REPO/LICENSE"

(cd "$AUR_REPO" && git add PKGBUILD .SRCINFO .nvchecker.toml LICENSE && git commit -m "Update to ${VERSION}")

cat <<EOF

Done. Review with:
    cd $AUR_REPO && git show

Push when ready:
    cd $AUR_REPO && GIT_SSH_COMMAND='ssh -i ~/.ssh/aur' git push origin master
EOF
