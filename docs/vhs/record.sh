#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

IMAGE_NAME="runex-vhs"

docker build -t "$IMAGE_NAME" -f "$SCRIPT_DIR/Dockerfile" "$REPO_ROOT"

docker run --rm \
    -v "$REPO_ROOT:/src" \
    -v "$SCRIPT_DIR:/out" \
    "$IMAGE_NAME"

echo "Recorded: $SCRIPT_DIR/demo.gif"
