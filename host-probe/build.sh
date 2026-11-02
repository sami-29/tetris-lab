#!/usr/bin/env bash
# Build the ticket 09 host probe into host-probe/dist.
#   ./host-probe/build.sh
# Fingerprints the wasm filename so the deploy exercises real immutable caching.
set -euo pipefail
cd "$(dirname "$0")"

TARGET=wasm32-unknown-unknown
DIST=dist
WASM="rs/target/$TARGET/release/probe.wasm"

command -v cargo >/dev/null 2>&1 || {
  echo "cargo not found; ticket 01's toolchain requires it" >&2
  exit 1
}
if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
  echo "installing $TARGET target"
  rustup target add "$TARGET"
fi

echo "== cargo build --release --target $TARGET"
cargo build --release --lib --target "$TARGET" --manifest-path rs/Cargo.toml

[[ -f "$WASM" ]] || {
  echo "expected $WASM after the build" >&2
  exit 1
}

if command -v sha256sum >/dev/null 2>&1; then
  hash=$(sha256sum "$WASM" | cut -c1-8)
else
  hash=$(shasum -a 256 "$WASM" | cut -c1-8)
fi
name="probe-$hash.wasm"
path="/assets/$name"
bytes=$(wc -c <"$WASM" | tr -d ' ')

rm -rf "$DIST"
mkdir -p "$DIST/assets"
cp "$WASM" "$DIST/assets/$name"
cp src/worker.js src/_headers "$DIST/"
sed "s|__WASM_PATH__|$path|g" src/index.html.tpl >"$DIST/index.html"
printf '{"wasm":"%s","bytes":%s,"hash":"%s","built":"%s"}\n' \
  "$path" "$bytes" "$hash" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$DIST/manifest.json"

echo
echo "dist/manifest.json: $(cat "$DIST/manifest.json")"
echo "dist/index.html:    $(wc -c <"$DIST/index.html" | tr -d ' ') B"
echo "dist/assets/$name:  $bytes B"
