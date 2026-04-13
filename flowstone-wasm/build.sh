#!/usr/bin/env bash
#
# Build the flowstone-wasm package with tantivy FTS enabled.
#
# This wraps `wasm-pack build --target web --release` in the same
# environment that cozo-lib-wasm needs: tantivy's `tantivy-sstable`
# pulls in `zstd-sys`, which compiles a small C shim when targeting
# `wasm32-unknown-unknown`, which in turn needs a wasm-capable clang
# and llvm-ar. Debian/Ubuntu: `apt install clang-19 llvm-19`.
#
# Override CC_WASM / AR_WASM if you have differently-named binaries.

set -euo pipefail

CC_WASM="${CC_WASM:-clang-19}"
AR_WASM="${AR_WASM:-llvm-ar-19}"

if ! command -v "$CC_WASM" >/dev/null 2>&1; then
    echo "error: $CC_WASM not found in PATH (needed to cross-compile zstd-sys for wasm32)." >&2
    echo "       install it (e.g. 'apt install clang-19') or set CC_WASM to your clang binary." >&2
    exit 1
fi
if ! command -v "$AR_WASM" >/dev/null 2>&1; then
    echo "error: $AR_WASM not found in PATH." >&2
    echo "       install it (e.g. 'apt install llvm-19') or set AR_WASM to your llvm-ar binary." >&2
    exit 1
fi

cd "$(dirname "$0")"

CC_wasm32_unknown_unknown="$CC_WASM" \
AR_wasm32_unknown_unknown="$AR_WASM" \
CARGO_PROFILE_RELEASE_LTO=fat \
    wasm-pack build --target web --release
