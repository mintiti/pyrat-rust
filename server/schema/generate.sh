#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCHEMA="$SCRIPT_DIR/pyrat.fbs"
OUT_DIR="$SCRIPT_DIR/../wire/src"

if ! command -v flatc &>/dev/null; then
    echo "error: flatc not found. Install with: brew install flatbuffers" >&2
    exit 1
fi

echo "Generating Rust code from $SCHEMA..."
flatc --rust -o "$OUT_DIR" "$SCHEMA"

TARGET="$OUT_DIR/pyrat_generated.rs"
if [[ ! -f "$TARGET" ]]; then
    echo "error: expected $TARGET not found" >&2
    ls -la "$OUT_DIR"
    exit 1
fi

# Format the generated code to match project style
WORKSPACE_ROOT="$SCRIPT_DIR/../.."
if cargo fmt --manifest-path "$WORKSPACE_ROOT/Cargo.toml" -p pyrat-wire 2>/dev/null; then
    echo "Generated and formatted $TARGET"
else
    echo "Generated $TARGET (cargo fmt not available, skipping format)"
fi

# ── Python codegen ──────────────────────────────────────
PY_FINAL_DIR="$SCRIPT_DIR/../../sdk/python/pyrat_sdk/_wire/protocol"
PY_TMP_DIR=$(mktemp -d)

echo "Generating Python code from $SCHEMA..."
flatc --python -o "$PY_TMP_DIR" "$SCHEMA"

PY_TMP_TARGET="$PY_TMP_DIR/pyrat/protocol"
if [[ ! -d "$PY_TMP_TARGET" ]]; then
    echo "error: expected $PY_TMP_TARGET not found" >&2
    ls -la "$PY_TMP_DIR"
    rm -rf "$PY_TMP_DIR"
    exit 1
fi

rm -rf "$PY_FINAL_DIR"
mkdir -p "$PY_FINAL_DIR"
cp "$PY_TMP_TARGET/"*.py "$PY_FINAL_DIR/"

# Rewrite internal imports so standard Python resolution works.
# macOS sed needs '' after -i; use a temp suffix and remove backup files.
sed -i.bak 's/from pyrat\.protocol\./from pyrat_sdk._wire.protocol./g' "$PY_FINAL_DIR/"*.py
rm -f "$PY_FINAL_DIR/"*.bak
rm -rf "$PY_TMP_DIR"

echo "Generated Python FlatBuffers code in $PY_FINAL_DIR"
