#!/usr/bin/env bash
#
# runex CI image sanity check.
#
# Asserts that every shell tool the test suite reaches for is present
# and meets the version requirement that test bisects up to. Runs
# at image build time (last RUN in `containers/ci/ubuntu.Dockerfile`)
# so a missing or broken tool is caught here, not at CI runtime.
#
# Also re-runnable inside a built image:
#
#   docker run --rm ghcr.io/shortarrow/runex-ci:latest \
#     /usr/local/bin/runex-ci-sanity
#
# Exit non-zero on the first failure; print a short diagnostic to
# stderr so `docker build` red lines tell you which tool is missing.

set -euo pipefail

fail() {
    printf 'sanity: %s\n' "$1" >&2
    exit 1
}

# bash >= 4 (test suite requires this; macOS bash 3.2 is rejected by
# the same bash_available()/bash4_available() probe in tests/support).
bash_major="${BASH_VERSION%%.*}"
[ "${bash_major:-0}" -ge 4 ] || fail "bash 4+ required, got $BASH_VERSION"

require() {
    local cmd="$1"
    command -v "$cmd" >/dev/null 2>&1 || fail "$cmd not found on PATH"
}

require zsh
require pwsh
require nu
require xclip
require wl-paste
require xsel
require git
require curl
require node
require cargo
require rustc

# Print the version line for each tool. Useful as build evidence
# (`docker buildx build --progress=plain` prints these in the log)
# and for `runex doctor` parity checks.
echo "=== runex-ci sanity (image tool versions) ==="
echo "bash:    $(bash --version | head -1)"
echo "zsh:     $(zsh --version)"
echo "pwsh:    $(pwsh --version)"
echo "nu:      $(nu --version)"
echo "xclip:   $(xclip -version 2>&1 | head -1)"
echo "wl-paste: $(wl-paste --version)"
echo "xsel:    $(xsel --version | head -1)"
echo "git:     $(git --version)"
echo "node:    $(node --version)"
echo "cargo:   $(cargo --version)"
echo "rustc:   $(rustc --version)"
echo "============================================"
