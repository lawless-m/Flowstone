#!/usr/bin/env bash
#
# Build flowstone-wasm and ship the resulting dist/ to a static host.
# Designed to be safe to call from a git hook: no interactive prompts,
# cwd-independent, non-zero exit on any failure.
#
# Defaults match the live deploy at https://steponnopets.net/flowstone/,
# served out of jess:/var/www/html/flowstone/. Override with env vars:
#
#   FLOWSTONE_DEPLOY_TARGET=user@host:/path/   ./deploy.sh
#   FLOWSTONE_DEPLOY_SKIP_BUILD=1              ./deploy.sh  # rsync only
#
# Example pre-push hook (~/.git/hooks/pre-push):
#   #!/usr/bin/env bash
#   exec /home/matt/Git/Flowstone/flowstone-wasm/deploy.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

TARGET="${FLOWSTONE_DEPLOY_TARGET:-jess:/var/www/html/flowstone/}"
SKIP_BUILD="${FLOWSTONE_DEPLOY_SKIP_BUILD:-0}"

if [[ "$SKIP_BUILD" != "1" ]]; then
    echo "==> build.sh"
    ./build.sh
else
    echo "==> skipping build (FLOWSTONE_DEPLOY_SKIP_BUILD=1)"
    if [[ ! -f "$SCRIPT_DIR/dist/index.html" ]]; then
        echo "error: dist/index.html missing — run build.sh first or unset FLOWSTONE_DEPLOY_SKIP_BUILD" >&2
        exit 1
    fi
fi

echo
echo "==> rsync dist/ -> $TARGET"
rsync -av --delete "$SCRIPT_DIR/dist/" "$TARGET"

echo
echo "Deployed."
