#!/usr/bin/env bash
# Build every kernel variant the ticket 06 benchmark compares.
#   ./prototype/build.sh
# Artifacts land in prototype/artifacts/ (gitignored below target/).
set -euo pipefail
cd "$(dirname "$0")/.."
RS=prototype/rs
OUT=prototype/artifacts
mkdir -p "$OUT"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"

# Native builds need a working linker. This box has MinGW g++ but no MSVC, so
# the GNU toolchain is the one that links. See README for why.
NATIVE_TOOLCHAIN=+stable-x86_64-pc-windows-gnu

# Two native rows per word width: default flags (generic x86-64, no popcnt/lzcnt)
# and `target-cpu=native` (lzcnt/popcnt enabled, which wasm gets for free).
# Without the second row the native-versus-wasm ratio is unfair to native.
echo "== native u32 (generic)"
cargo "$NATIVE_TOOLCHAIN" build --release --manifest-path "$RS/Cargo.toml" \
  --no-default-features --features word32
cp "$RS/target/release/bench.exe" "$OUT/bench-native-u32.exe"

echo "== native u32 (target-cpu=native)"
RUSTFLAGS="-Ctarget-cpu=native" cargo "$NATIVE_TOOLCHAIN" build --release \
  --manifest-path "$RS/Cargo.toml" --no-default-features --features word32
cp "$RS/target/release/bench.exe" "$OUT/bench-native-u32-native.exe"

echo "== native u64 (target-cpu=native)"
RUSTFLAGS="-Ctarget-cpu=native" cargo "$NATIVE_TOOLCHAIN" build --release \
  --manifest-path "$RS/Cargo.toml" --no-default-features --features word64
cp "$RS/target/release/bench.exe" "$OUT/bench-native-u64-native.exe"

WASM_TARGET=wasm32-unknown-unknown
WASM_DIR="$RS/target/$WASM_TARGET/release"

echo "== wasm u32 scalar"
cargo build --release --lib --manifest-path "$RS/Cargo.toml" --target "$WASM_TARGET" \
  --no-default-features --features word32
cp "$WASM_DIR/t06.wasm" "$OUT/wasm-u32.wasm"

echo "== wasm u64 scalar"
cargo build --release --lib --manifest-path "$RS/Cargo.toml" --target "$WASM_TARGET" \
  --no-default-features --features word64
cp "$WASM_DIR/t06.wasm" "$OUT/wasm-u64.wasm"

echo "== wasm u32 + simd128"
RUSTFLAGS="-Ctarget-feature=+simd128" cargo build --release --lib \
  --manifest-path "$RS/Cargo.toml" --target "$WASM_TARGET" \
  --no-default-features --features word32,simd
cp "$WASM_DIR/t06.wasm" "$OUT/wasm-u32-simd.wasm"

echo "== wasm u32 + lines_live"
cargo build --release --lib --manifest-path "$RS/Cargo.toml" --target "$WASM_TARGET" \
  --no-default-features --features word32,lines_live
cp "$WASM_DIR/t06.wasm" "$OUT/wasm-u32-lines_live.wasm"

ls -l "$OUT"
