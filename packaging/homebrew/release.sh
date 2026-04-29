#!/usr/bin/env bash
# Update the Homebrew tap formula to a given version.
#
# Usage:
#   packaging/homebrew/release.sh <version> [<tap-repo-path>]
#
# Example:
#   packaging/homebrew/release.sh 0.1.12 /v/homebrew-runex
#
# What it does:
#   1. Fetches SHA256 of the macOS/Linux (x86_64/aarch64) release tarballs
#   2. Rewrites version and sha256 values in Formula/runex.rb
#   3. Commits the change (push is left to the user)

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <version> [<tap-repo-path>]" >&2
    exit 1
fi

VERSION="$1"
TAP_REPO="${2:-/v/homebrew-runex}"

if [[ ! -d "$TAP_REPO/.git" ]]; then
    echo "Tap clone not found: $TAP_REPO" >&2
    echo "Run: git clone https://github.com/ShortArrow/homebrew-runex.git $TAP_REPO" >&2
    exit 1
fi

FORMULA="$TAP_REPO/Formula/runex.rb"
if [[ ! -f "$FORMULA" ]]; then
    echo "Formula not found: $FORMULA" >&2
    exit 1
fi

sha_of() {
    local url="$1"
    curl -fsSL "$url" | sha256sum | awk '{print $1}'
}

BASE="https://github.com/ShortArrow/runex/releases/download/v${VERSION}"

echo "Fetching SHA256 checksums for v${VERSION}..."
SHA_MAC_ARM=$(sha_of "${BASE}/runex-aarch64-apple-darwin.tar.gz")
SHA_MAC_X64=$(sha_of "${BASE}/runex-x86_64-apple-darwin.tar.gz")
SHA_LINUX_ARM=$(sha_of "${BASE}/runex-aarch64-unknown-linux-gnu.tar.gz")
SHA_LINUX_X64=$(sha_of "${BASE}/runex-x86_64-unknown-linux-gnu.tar.gz")

echo "  macOS arm64:   $SHA_MAC_ARM"
echo "  macOS x86_64:  $SHA_MAC_X64"
echo "  Linux aarch64: $SHA_LINUX_ARM"
echo "  Linux x86_64:  $SHA_LINUX_X64"

python3 - "$FORMULA" "$VERSION" \
    "$SHA_MAC_ARM" "$SHA_MAC_X64" \
    "$SHA_LINUX_ARM" "$SHA_LINUX_X64" <<'PY'
import re, sys
path, version, mac_arm, mac_x64, linux_arm, linux_x64 = sys.argv[1:7]
src = open(path).read()

src = re.sub(r'^(\s*version\s+)"[^"]+"', rf'\g<1>"{version}"', src, count=1, flags=re.M)

# Replace each SHA by URL pattern match. The formula's block order must stay:
#   on_macos -> on_arm, on_intel
#   on_linux -> on_arm, on_intel
def sub_sha(src, url_substr, sha):
    # Find the url line containing url_substr, then replace the sha256 on the next line.
    pattern = re.compile(
        r'(url\s+"[^"]*' + re.escape(url_substr) + r'[^"]*"\s*\n\s*sha256\s+)"[0-9a-f]{64}"',
        re.M,
    )
    return pattern.sub(rf'\1"{sha}"', src, count=1)

src = sub_sha(src, "aarch64-apple-darwin", mac_arm)
src = sub_sha(src, "x86_64-apple-darwin", mac_x64)
src = sub_sha(src, "aarch64-unknown-linux-gnu", linux_arm)
src = sub_sha(src, "x86_64-unknown-linux-gnu", linux_x64)

open(path, "w").write(src)
PY

(cd "$TAP_REPO" && git add Formula/runex.rb && git commit -m "runex: update to ${VERSION}")

cat <<EOF

Done. Review with:
    cd $TAP_REPO && git show

Push when ready:
    cd $TAP_REPO && git push origin main
EOF
