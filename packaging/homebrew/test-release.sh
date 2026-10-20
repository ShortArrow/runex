#!/usr/bin/env bash
# Behaviour examples for release.sh, run against stand-in release assets
# and a throwaway tap clone. Needs git, curl and python3.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

# check <description> <command...>: passes when the command succeeds
check() {
  local description="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    echo "ok: $description"
  else
    echo "FAIL: $description"
    failures=$((failures + 1))
  fi
}

assets="$work/assets"
mkdir "$assets"
for target in aarch64-apple-darwin x86_64-apple-darwin \
  aarch64-unknown-linux-musl x86_64-unknown-linux-musl; do
  printf '%s\n' "$target" >"$assets/runex-$target.tar.gz"
done
sha() { sha256sum "$assets/runex-$1.tar.gz" | awk '{print $1}'; }

tap="$work/tap"
mkdir -p "$tap/Formula"
old="0000000000000000000000000000000000000000000000000000000000000000"
url="https://github.com/ShortArrow/runex/releases/download/v#{version}"
cat >"$tap/Formula/runex.rb" <<EOF
class Runex < Formula
  version "0.1.20"

  on_macos do
    on_arm do
      url "$url/runex-aarch64-apple-darwin.tar.gz"
      sha256 "$old"
    end
    on_intel do
      url "$url/runex-x86_64-apple-darwin.tar.gz"
      sha256 "$old"
    end
  end

  on_linux do
    on_arm do
      url "$url/runex-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "$old"
    end
    on_intel do
      url "$url/runex-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "$old"
    end
  end
end
EOF
git -C "$tap" init -q
git -C "$tap" -c user.name=test -c user.email=test@example.invalid \
  -c commit.gpgsign=false add Formula/runex.rb
git -C "$tap" -c user.name=test -c user.email=test@example.invalid \
  -c commit.gpgsign=false commit -q -m init

status=0
RUNEX_RELEASE_BASE="file://$assets" \
  GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid \
  GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid \
  GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=commit.gpgsign GIT_CONFIG_VALUE_0=false \
  bash "$here/release.sh" 0.1.21 "$tap" >"$work/out" 2>&1 || status=$?
check "release.sh succeeds when only musl Linux assets exist" test "$status" -eq 0
[ "$status" -eq 0 ] || sed 's/^/  output: /' "$work/out"

formula="$tap/Formula/runex.rb"
# pinned <target> <sha>: the url naming <target> is followed by <sha>
pinned() {
  grep -A1 "runex-$1.tar.gz\"" "$formula" | grep -q "sha256 \"$2\""
}
check "the version is 0.1.21" grep -q '^  version "0.1.21"$' "$formula"
check "no url names a linux-gnu archive" bash -c "! grep -q linux-gnu '$formula'"
for target in aarch64-apple-darwin x86_64-apple-darwin \
  aarch64-unknown-linux-musl x86_64-unknown-linux-musl; do
  check "$target carries its checksum" pinned "$target" "$(sha "$target")"
done
check "the Linux arm block names the aarch64 musl archive" \
  bash -c "grep -A2 'on_linux do' '$formula' | grep -q aarch64-unknown-linux-musl"
check "the change is committed in the tap" \
  bash -c "test -z \"\$(git -C '$tap' status --porcelain)\" && git -C '$tap' log -1 --format=%s | grep -q '0.1.21'"

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed"
  exit 1
fi
echo "all checks passed"
