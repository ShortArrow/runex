#!/usr/bin/env bash
# Behaviour examples for build-deb.sh and deb-version.sh, run against a
# stand-in executable. Needs dpkg-deb.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

cp "$(type -P true)" "$work/runex"
chmod 0644 "$work/runex"

# expect <description> <expected exit code> <command...>
expect() {
  local description="$1" expected="$2"
  shift 2
  local actual=0
  "$@" >"$work/out" 2>"$work/err" || actual=$?
  if [ "$actual" -ne "$expected" ]; then
    echo "FAIL: $description (exit $actual, expected $expected)"
    sed 's/^/  stderr: /' "$work/err"
    failures=$((failures + 1))
  else
    echo "ok: $description"
  fi
}

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

expect "a release version builds a package" 0 \
  bash "$here/build-deb.sh" "$work/runex" 0.2.0 amd64 "$work/out-amd64"
deb="$work/out-amd64/runex_0.2.0-1_amd64.deb"
check "the package is named runex_<version>-1_<arch>.deb" test -f "$deb"
check "build-deb.sh prints the package's path" grep -qxF "$deb" "$work/out"

field() { dpkg-deb --field "$deb" "$1"; }
check "Package is runex" test "$(field Package)" = runex
check "Version carries Debian revision 1" test "$(field Version)" = 0.2.0-1
check "Architecture is the one asked for" test "$(field Architecture)" = amd64
no_depends() { dpkg-deb --field "$deb" Package >/dev/null && test -z "$(field Depends)"; }
check "the statically linked executable depends on nothing" no_depends
check "Installed-Size is set" test -n "$(field Installed-Size)"

contents="$(dpkg-deb --contents "$deb")"
check "the executable is /usr/bin/runex with mode 0755, owned by root" \
  grep -qE '^-rwxr-xr-x root/root .* \./usr/bin/runex$' <<<"$contents"
for doc in copyright LICENSE; do
  check "the package carries /usr/share/doc/runex/$doc" \
    grep -qE " \./usr/share/doc/runex/$doc\$" <<<"$contents"
done

expect "arm64 builds too" 0 \
  bash "$here/build-deb.sh" "$work/runex" 0.2.0 arm64 "$work/out-arm64"
check "the arm64 package says so" \
  test "$(dpkg-deb --field "$work/out-arm64/runex_0.2.0-1_arm64.deb" Architecture)" = arm64

expect "a pre-release spelled with ~ builds a package" 0 \
  bash "$here/build-deb.sh" "$work/runex" 0.2.0~rc.1 amd64 "$work/out-tilde"
check "the ~ pre-release sorts before the release" \
  dpkg --compare-versions "$(dpkg-deb --field "$work/out-tilde/runex_0.2.0~rc.1-1_amd64.deb" Version)" lt 0.2.0-1

check "a release tag gives Cargo.toml's version" \
  test "$(sh "$here/deb-version.sh" 0.2.0 v0.2.0)" = 0.2.0
check "a prerelease tag gives a version that sorts before the release" \
  test "$(sh "$here/deb-version.sh" 0.2.0 v0.2.0-rc.1)" = '0.2.0~rc.1'
expect "a tag for another version is refused" 1 \
  sh "$here/deb-version.sh" 0.2.0 v0.3.0
expect "a prerelease tag for another version is refused" 1 \
  sh "$here/deb-version.sh" 0.2.0 v0.3.0-rc.1

expect "a version with - is refused" 1 \
  bash "$here/build-deb.sh" "$work/runex" 0.2.0-rc.1 amd64 "$work/out-rc"
check "a refused version leaves no package" test ! -e "$work/out-rc"
expect "an unknown architecture is refused" 1 \
  bash "$here/build-deb.sh" "$work/runex" 0.2.0 i386 "$work/out-i386"
expect "a missing executable is refused" 1 \
  bash "$here/build-deb.sh" "$work/missing" 0.2.0 amd64 "$work/out-missing"

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed"
  exit 1
fi
echo "all checks passed"
