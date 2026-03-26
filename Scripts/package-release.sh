#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_TRIPLE="${1:-aarch64-apple-darwin}"
VERSION="${2:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n1)}"
OUTPUT_DIR="${3:-$ROOT_DIR/dist}"
ASSET_BASENAME="qc-$TARGET_TRIPLE"
ASSET_DIR="$OUTPUT_DIR/tmp-$ASSET_BASENAME"
ASSET_PATH="$OUTPUT_DIR/$ASSET_BASENAME.tar.gz"

mkdir -p "$ASSET_DIR"
mkdir -p "$OUTPUT_DIR"
rm -rf "$ASSET_DIR"
mkdir -p "$ASSET_DIR"

cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml"
cp "$ROOT_DIR/target/release/qc" "$ASSET_DIR/qc"

tar -C "$ASSET_DIR" -czf "$ASSET_PATH" qc
shasum -a 256 "$ASSET_PATH" | awk '{print $1}' > "$ASSET_PATH.sha256"

echo "Packaged version $VERSION"
echo "Asset: $ASSET_PATH"
echo "SHA256: $(cat "$ASSET_PATH.sha256")"
