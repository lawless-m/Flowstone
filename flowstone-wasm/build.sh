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

echo "==> Building standard wasm (no FTS)…"
CC_wasm32_unknown_unknown="$CC_WASM" \
AR_wasm32_unknown_unknown="$AR_WASM" \
CARGO_PROFILE_RELEASE_LTO=fat \
    wasm-pack build --target web --release --out-dir pkg

echo
echo "==> Building FTS wasm (nightly + atomics + tantivy)…"
# wasm-bindgen's thread transform requires an IMPORTED shared memory, but:
#   --shared-memory (no --import-memory) → defined shared memory + all TLS
#                                          symbols generated — correct, but
#                                          memory is defined, not imported.
#   --import-memory + --shared-memory    → wasm-ld drops __wasm_init_tls
#                                          (regression in LLVM ≥ 18/19).
#
# Solution: build with --shared-memory only (gets TLS right + passive data
# segments), then Python-patch the defined shared memory into an import.
# The four TLS exports are required by wasm-bindgen but not auto-exported
# by newer wasm-ld — request them explicitly.
# max-memory stays just under 2 GB (32767 pages × 65536 B) to keep wasm32
# addressing and prevent wasm-ld from activating memory64.
RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals \
  -C link-arg=--shared-memory \
  -C link-arg=--max-memory=2147418112 \
  -C link-arg=--export=__wasm_init_tls \
  -C link-arg=--export=__tls_size \
  -C link-arg=--export=__tls_align \
  -C link-arg=--export=__tls_base" \
CC_wasm32_unknown_unknown="$CC_WASM" \
AR_wasm32_unknown_unknown="$AR_WASM" \
CARGO_PROFILE_RELEASE_LTO=fat \
    cargo +nightly build \
      --target wasm32-unknown-unknown \
      --release \
      --features fts \
      -Z build-std=std,panic_abort

WASM_IN="../target/wasm32-unknown-unknown/release/flowstone_wasm.wasm"
WASM_PATCHED="../target/wasm32-unknown-unknown/release/flowstone_wasm_shared.wasm"

echo "==> Patching defined shared memory → imported shared memory…"
python3 - "$WASM_IN" "$WASM_PATCHED" <<'PYEOF'
import sys

def read_leb128u(data, pos):
    result = 0; shift = 0
    while True:
        b = data[pos]; pos += 1
        result |= (b & 0x7f) << shift; shift += 7
        if not (b & 0x80): break
    return result, pos

def encode_leb128u(v):
    out = []
    while True:
        b = v & 0x7f; v >>= 7
        if v: out.append(b | 0x80)
        else: out.append(b); break
    return bytes(out)

def parse_sections(raw):
    assert raw[:4] == b'\x00asm'
    pos = 8; sections = []
    while pos < len(raw):
        sid = raw[pos]; pos += 1
        sz, pos = read_leb128u(raw, pos)
        sections.append((sid, bytes(raw[pos:pos+sz])))
        pos += sz
    return sections

def encode_section(sid, payload):
    return bytes([sid]) + encode_leb128u(len(payload)) + payload

src, dst = sys.argv[1], sys.argv[2]
with open(src, 'rb') as f:
    raw = f.read()

# Read the defined memory limits from the memory section.
mem_flags = mem_min = mem_max = None
data = bytearray(raw); pos = 8
while pos < len(data):
    sid = data[pos]; pos += 1
    sz, pos = read_leb128u(data, pos)
    end = pos + sz
    if sid == 5:
        count, p = read_leb128u(data, pos)
        assert count == 1, f"expected 1 defined memory, got {count}"
        mem_flags = data[p]; p += 1
        mem_min, p = read_leb128u(data, p)
        mem_max = None
        if mem_flags & 1:
            mem_max, p = read_leb128u(data, p)
        assert mem_flags & 0x02, f"memory not shared (flags=0x{mem_flags:02x}) — wrong build?"
        print(f"  Defined memory: flags=0x{mem_flags:02x} min={mem_min} max={mem_max}")
        break
    pos = end

assert mem_flags is not None, "memory section not found"

# Build the import entry that mirrors the defined memory exactly.
mem_import_entry = (
    b'\x03env'
    + b'\x06memory'
    + bytes([0x02])             # import kind: memory
    + bytes([mem_flags])        # flags (shared | has_max)
    + encode_leb128u(mem_min)
    + (encode_leb128u(mem_max) if mem_max is not None else b'')
)

sections = parse_sections(raw)
out_sections = []
for sid, payload in sections:
    if sid == 2:  # import section — prepend memory import
        count, _ = read_leb128u(payload, 0)
        new_payload = encode_leb128u(count + 1) + mem_import_entry + payload[len(encode_leb128u(count)):]
        print(f"  Import section: {count} → {count+1}")
        payload = bytes(new_payload)
    elif sid == 5:  # memory section — empty it
        print("  Memory section: removed defined memory")
        payload = b'\x00'
    out_sections.append(encode_section(sid, payload))

out = b'\x00asm' + b'\x01\x00\x00\x00' + b''.join(out_sections)
with open(dst, 'wb') as f:
    f.write(out)
print(f"  Written {len(out):,} B → {dst}")
PYEOF

rm -rf pkg-fts && mkdir -p pkg-fts
wasm-bindgen \
    --target web \
    --out-name flowstone_wasm \
    --out-dir pkg-fts \
    "$WASM_PATCHED"

# workerHelpers.js uses `import('../../..')` which is a directory URL on a
# plain static server (works in bundler contexts via package.json, not here).
# Patch it to the explicit JS filename so raw-file serving works.
find pkg-fts/snippets -name 'workerHelpers.js' | while read f; do
    sed -i "s|import('\\.\\./\\.\\./.\\.')|import('../../../flowstone_wasm.js')|g" "$f"
    echo "==> Patched workerHelpers.js: $f"
done

# Assemble dist/ — a single flat directory for no-FTS hosts (GitHub Pages
# etc.), plus a dist/fts/ subdirectory for cross-origin-isolated hosts
# (steponnopets.net with COOP/COEP).  dist-index.html picks fts/ at
# runtime when crossOriginIsolated is true.
DIST="dist"
rm -rf "$DIST"
mkdir -p "$DIST"
cp pkg/flowstone_wasm.js          "$DIST/"
cp pkg/flowstone_wasm_bg.wasm     "$DIST/"
cp shim.js                         "$DIST/"
cp ../static/graph.js              "$DIST/"
cp ../static/yaml-graph.js         "$DIST/"
cp ../static/style.css             "$DIST/"
cp dist-index.html                 "$DIST/index.html"
cp github-save.js                  "$DIST/"

mkdir -p "$DIST/fts"
cp pkg-fts/flowstone_wasm.js      "$DIST/fts/"
cp pkg-fts/flowstone_wasm_bg.wasm "$DIST/fts/"
[ -d pkg-fts/snippets ] && cp -r pkg-fts/snippets "$DIST/fts/"

echo
echo "dist/ assembled:"
ls -lh "$DIST"
ls -lh "$DIST/fts"
