#!/usr/bin/env bash
# Verify the ticket 09 host: wasm MIME, fingerprinted caching, worker, SPA fallback.
#   ./host-probe/verify.sh https://lab.example.com
#   ./host-probe/verify.sh http://127.0.0.1:8787      # against `wrangler dev`
# Falls back to PROBE_URL, then to host-probe/.env's PROBE_URL.
set -uo pipefail
cd "$(dirname "$0")"

BASE="${1:-${PROBE_URL:-}}"
if [[ -z "$BASE" && -f .env ]]; then
  BASE=$(grep -E '^PROBE_URL=' .env | tail -n1 | cut -d= -f2- | tr -d "'\"")
fi
if [[ -z "$BASE" ]]; then
  echo "usage: $0 <base-url>   (or set PROBE_URL, or write it to host-probe/.env)" >&2
  exit 2
fi
BASE="${BASE%/}"

if [[ ! -f dist/manifest.json ]]; then
  echo "host-probe/dist/manifest.json missing; run ./build.sh first" >&2
  exit 2
fi
WASM=$(sed -n 's/.*"wasm": *"\([^"]*\)".*/\1/p' dist/manifest.json)
BYTES=$(sed -n 's/.*"bytes": *\([0-9]*\).*/\1/p' dist/manifest.json)

FAILS=0
CHECKS=0
HEADERS=""
CODE=""

req() {
  local url=$1 out
  if ! out=$(curl -sS -D - -o /dev/null --max-time 25 \
    -H 'Accept-Encoding: gzip, deflate, br, zstd' "$url" 2>&1); then
    printf '  %s✗%s GET %s -> curl failed: %s\n' "$RED" "$RESET" "$url" "$out"
    HEADERS=""
    CODE="000"
    FAILS=$((FAILS + 1))
    CHECKS=$((CHECKS + 1))
    return 1
  fi
  HEADERS=$out
  CODE=$(printf '%s\n' "$out" | tr -d '\r' | awk 'NR==1{print $2}')
  return 0
}

hval() {
  printf '%s\n' "$HEADERS" | tr -d '\r' | grep -i "^$1:" | tail -n1 | cut -d: -f2- | sed 's/^ *//;s/ *$//'
}

expect() { # label actual expected
  local label=$1 actual=$2 want=$3
  CHECKS=$((CHECKS + 1))
  if [[ "$actual" == *"$want"* ]]; then
    printf '  %s✓%s %-24s %s\n' "$GREEN" "$RESET" "$label" "$actual"
  else
    printf '  %s✗%s %-24s got "%s", wanted "%s"\n' "$RED" "$RESET" "$label" "$actual" "$want"
    FAILS=$((FAILS + 1))
  fi
}

if [[ -t 1 ]]; then
  GREEN=$(printf '\033[32m')
  RED=$(printf '\033[31m')
  DIM=$(printf '\033[2m')
  RESET=$(printf '\033[0m')
else
  GREEN=""
  RED=""
  DIM=""
  RESET=""
fi

echo "host probe · $BASE"
echo

echo "1. document"
req "$BASE/" && {
  expect "status" "$CODE" "200"
  expect "content-type" "$(hval content-type)" "text/html"
  expect "cache-control" "$(hval cache-control)" "must-revalidate"
}
echo

echo "2. fingerprinted wasm ($WASM)"
req "$BASE$WASM" && {
  expect "status" "$CODE" "200"
  expect "content-type" "$(hval content-type)" "application/wasm"
  expect "cache-control" "$(hval cache-control)" "max-age=31536000"
  expect "cache immutable" "$(hval cache-control)" "immutable"
  CHECKS=$((CHECKS + 1))
  if [[ -n "$(hval etag)" ]]; then
    printf '  %s✓%s %-24s %s\n' "$GREEN" "$RESET" "etag present" "$(hval etag)"
  else
    printf '  %s✗%s %-24s no ETag\n' "$RED" "$RESET" "etag present"
    FAILS=$((FAILS + 1))
  fi
  printf '  %s·%s %-24s %s\n' "$DIM" "$RESET" "content-encoding" "$(hval content-encoding || true)"
  printf '  %s·%s %-24s %s\n' "$DIM" "$RESET" "content-length" "$(hval content-length || true)"
  printf '  %s·%s %-24s %s\n' "$DIM" "$RESET" "edge cache" "$(hval cf-cache-status || true)"
  printf '  %s·%s %-24s %s B unhashed in manifest\n' "$DIM" "$RESET" "wasm bytes" "${BYTES:-?}"
}
echo

echo "3. module worker"
req "$BASE/worker.js" && {
  expect "status" "$CODE" "200"
  expect "content-type" "$(hval content-type)" "javascript"
}
echo

echo "4. SPA fallback (single-page-application)"
req "$BASE/this-path-does-not-exist" && {
  expect "status" "$CODE" "200"
  expect "content-type" "$(hval content-type)" "text/html"
}
echo

echo "5. cross-origin isolation (optional in v1; EMPTY is expected and fine)"
lines=$(printf '%s\n' "$HEADERS" | tr -d '\r' | grep -i '^cross-origin' || true)
if [[ -n "$lines" ]]; then
  printf '  %s·%s %s\n' "$DIM" "$RESET" "$(printf '%s' "$lines" | tr '\n' ' ')"
else
  printf '  %s·%s no COOP/COEP headers, which v1 does not need\n' "$DIM" "$RESET"
fi
echo

if ((FAILS == 0)); then
  printf '%sPASS%s %d checks\n' "$GREEN" "$RESET" "$CHECKS"
  exit 0
fi
printf '%sFAIL%s %d of %d checks\n' "$RED" "$RESET" "$FAILS" "$CHECKS"
exit 1
