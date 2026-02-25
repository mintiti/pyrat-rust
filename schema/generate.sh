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
WORKSPACE_ROOT="$SCRIPT_DIR/.."
if cargo fmt --manifest-path "$WORKSPACE_ROOT/Cargo.toml" -p pyrat-wire 2>/dev/null; then
    echo "Generated and formatted $TARGET"
else
    echo "Generated $TARGET (cargo fmt not available, skipping format)"
fi

# ── Python codegen ──────────────────────────────────────
PY_OUT_DIR="$SCRIPT_DIR/../sdk-python/pyrat_sdk/_wire/generated"

echo "Generating Python code from $SCHEMA..."
flatc --python -o "$PY_OUT_DIR" "$SCHEMA"

PY_TARGET="$PY_OUT_DIR/pyrat/protocol"
if [[ ! -d "$PY_TARGET" ]]; then
    echo "error: expected $PY_TARGET not found" >&2
    ls -la "$PY_OUT_DIR"
    exit 1
fi

echo "Generated Python FlatBuffers code in $PY_TARGET"
