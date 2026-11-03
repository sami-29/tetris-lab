#!/usr/bin/env bash
# Post-deploy smoke test for the Lab (ticket 17): headers, MIME, caching, hashes,
# fallback, and the deployed wasm's own ABI handshake.
#
#   bash web/scripts/verify.sh --url https://lab.samibentaleb.com
#   bash web/scripts/verify.sh http://127.0.0.1:8791        # against `wrangler dev`
#   SMOKE_URL=https://lab.samibentaleb.com bash web/scripts/verify.sh
#
# Born from `host-probe/verify.sh` (ticket 09), which proved 12 of these assertions
# live on this account. The two rows it could not carry over are re-pointed at the
# Lab's own hashed files: the probe served `/worker.js`, the Lab serves
# `/assets/solve.worker-<hash>.js` and names it inside the entry chunk.
#
# It reports loudly and never rolls back. §10: a live-but-wrong deploy is the
# operator's call, and a rollback to a known-bad version is worse than a red badge.
# There is no `wrangler rollback` in this file, and the workflow that calls it has
# no `continue-on-error`: a mismatch exits 1, the step fails, the run goes red.
#
# Exit codes, the convention `host-probe/verify.sh` established and the Gate suite
# follows:
#   0  every check passed
#   1  at least one assertion failed (each failure names itself)
#   2  usage error, or a prerequisite is missing (`curl`, `node`, no built manifest)
#
# The local prerequisites come from the built tree: `dist/manifest.json` is what
# says which hashed files this deploy should be serving, so the smoke test compares
# the wire against the build rather than against a second copy of the truth.
# `npm --prefix web run build` writes it.

set -uo pipefail
cd "$(dirname "$0")/.."

# ─────────────────────────────────────────────────────────────── arguments

BASE="${SMOKE_URL:-}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --url) BASE="${2:-}"; shift 2 ;;
    --url=*) BASE="${1#--url=}"; shift ;;
    -h|--help) sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) BASE="$1"; shift ;;
  esac
done
if [[ -z "$BASE" || "$BASE" == -* ]]; then
  echo "usage: $0 --url <base-url>   (or set SMOKE_URL)" >&2
  exit 2
fi
BASE="${BASE%/}"

for tool in curl node; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "$tool not found; the smoke test needs curl and node" >&2
    exit 2
  fi
done

DIST=dist
if [[ ! -f "$DIST/manifest.json" ]]; then
  echo "$DIST/manifest.json missing; run \`npm --prefix web run build\` first" >&2
  exit 2
fi

# The manifest is JSON, so it is read by node rather than a hand-rolled sed parser —
# the same rule the Gate suite follows for its Node half. One line per asset:
#   <logical> <path> <bytes> <sha256>
manifest_lines() {
  node -e '
    const m = JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8")).assets;
    for (const [logical, asset] of Object.entries(m)) {
      console.log([logical, asset.path, asset.bytes, asset.sha256].join(" "));
    }
  ' "$DIST/manifest.json"
}
asset() { manifest_lines | awk -v k="$1" '$1 == k { print $2 }'; }
bytes() { manifest_lines | awk -v k="$1" '$1 == k { print $3 }'; }

WASM=$(asset engine.wasm)
WORKER=$(asset solve.worker.js)

# ──────────────────────────────────────────────────────────────── the assertions

FAILS=0
CHECKS=0
HEADERS=""
CODE=""
BODY_FILE=$(mktemp "${TMPDIR:-/tmp}/tetris-smoke-body.XXXXXX") || exit 2
trap 'rm -f "$BODY_FILE"' EXIT

if [[ -t 1 ]]; then
  GREEN=$(printf '\033[32m'); RED=$(printf '\033[31m')
  DIM=$(printf '\033[2m'); RESET=$(printf '\033[0m')
else
  GREEN=""; RED=""; DIM=""; RESET=""
fi

pass() { printf '  %s✓%s %-28s %s\n' "$GREEN" "$RESET" "$1" "${2:-}"; }
fail() { printf '  %s✗%s %-28s %s\n' "$RED" "$RESET" "$1" "$2"; FAILS=$((FAILS + 1)); }
note() { printf '  %s·%s %-28s %s\n' "$DIM" "$RESET" "$1" "${2:-}"; }

# One request: headers into $HEADERS, status into $CODE, body into $BODY_FILE.
# `--compressed` is what makes the byte count the *decompressed* size, which is the
# number the manifest recorded; the response's `content-encoding` is still readable
# in $HEADERS, so the edge's choice (zstd or gzip) is printed, not assumed.
req() {
  local url=$1 out
  CHECKS=$((CHECKS + 1))
  if ! out=$(curl -sS --compressed -D - -o "$BODY_FILE" --max-time 25 "$url" 2>&1); then
    fail "GET $url" "curl failed: $out"
    HEADERS=""
    CODE="000"
    : >"$BODY_FILE"
    return 1
  fi
  HEADERS=$out
  CODE=$(printf '%s\n' "$out" | tr -d '\r' | awk 'NR==1{print $2}')
  return 0
}

# `BODY` as text, and `SIZE` as bytes — a wasm module cannot live in a shell
# variable (bash drops NUL bytes), so the size is always read from the file.
body_text() { cat "$BODY_FILE"; }
body_size() { wc -c <"$BODY_FILE" | tr -d ' '; }

# The last value of a header, CRLF stripped, whitespace trimmed.
hval() {
  printf '%s\n' "$HEADERS" | tr -d '\r' | grep -i "^$1:" | tail -n 1 | cut -d: -f2- | sed 's/^ *//;s/ *$//'
}

# Contains. `text/html; charset=utf-8` must satisfy `text/html`.
expect() {
  local label=$1 actual=$2 want=$3
  CHECKS=$((CHECKS + 1))
  if [[ "$actual" == *"$want"* ]]; then
    pass "$label" "$actual"
  else
    fail "$label" "got \"$actual\", wanted \"$want\""
  fi
}

# Equals. Used where a joined value would pass a `contains` and still be broken.
expect_exact() {
  local label=$1 actual=$2 want=$3
  CHECKS=$((CHECKS + 1))
  if [[ "$actual" == "$want" ]]; then
    pass "$label" "$actual"
  else
    fail "$label" "got \"$actual\", wanted exactly \"$want\""
  fi
}

expect_status() {
  CHECKS=$((CHECKS + 1))
  if [[ "$CODE" == "$2" ]]; then pass "$1" "$CODE"; else fail "$1" "got $CODE, wanted $2"; fi
}

echo "smoke · $BASE"
echo

# 1. The document. `/`, never `/index.html`: the latter answers 307 under the
#    default `html_handling`, which would test a redirect instead of the page.
echo "1. document"
INDEX_HTML=""
if req "$BASE/"; then
  expect_status "status" 200
  expect "content-type" "$(hval content-type)" "text/html"
  expect "cache-control" "$(hval cache-control)" "must-revalidate"
  expect "cache-control max-age" "$(hval cache-control)" "max-age=0"
  expect "x-content-type-options" "$(hval x-content-type-options)" "nosniff"
  note "content-encoding" "$(hval content-encoding)"
  note "etag" "$(hval etag)"
  note "cf-cache-status" "$(hval cf-cache-status)"
  INDEX_HTML=$(body_text)
fi
echo

# 2. The reference chain. The chunk checked is whichever the *deployed* index.html
#    names, so a deploy whose page points at a missing file fails here rather than in
#    a visitor's console.
echo "2. the asset index.html names"
LIVE_ENTRY=$(printf '%s' "$INDEX_HTML" | grep -oE '(src|href)="/assets/[^"]+"' | head -n 1 | sed 's/^[a-z]*="//;s/"$//')
if [[ -z "$LIVE_ENTRY" ]]; then
  CHECKS=$((CHECKS + 1))
  fail "index.html names an asset" "no /assets/ reference in the served page"
else
  if req "$BASE$LIVE_ENTRY"; then
    expect_status "status" 200
    case "$LIVE_ENTRY" in
      *.css) expect "content-type" "$(hval content-type)" "text/css" ;;
      *) expect "content-type" "$(hval content-type)" "javascript" ;;
    esac
    expect "cache-control" "$(hval cache-control)" "public, max-age=31536000, immutable"
    note "path" "$LIVE_ENTRY"
  fi
fi
echo

# 3. The wasm: MIME, immutable caching, ETag, and the byte count the build recorded.
echo "3. the wasm ($WASM)"
if req "$BASE$WASM"; then
  expect_status "status" 200
  expect "content-type" "$(hval content-type)" "application/wasm"
  expect "cache-control max-age" "$(hval cache-control)" "max-age=31536000"
  expect "cache-control immutable" "$(hval cache-control)" "immutable"
  # Exact, not contains: two matching `_headers` rules are comma-joined by the
  # platform, and a joined value still contains `immutable`.
  expect_exact "cache-control exact" "$(hval cache-control)" "public, max-age=31536000, immutable"
  CHECKS=$((CHECKS + 1))
  if [[ -n "$(hval etag)" ]]; then pass "etag present" "$(hval etag)"; else fail "etag present" "no ETag"; fi
  expect_exact "bytes served" "$(body_size)" "$(bytes engine.wasm)"
  note "content-encoding" "$(hval content-encoding)"
  note "content-length" "$(hval content-length || true)"
  note "cf-cache-status" "$(hval cf-cache-status)"
fi
echo

# 4. The worker chunk is a hashed asset too, so it caches forever as well.
echo "4. the worker chunk ($WORKER)"
if req "$BASE$WORKER"; then
  expect_status "status" 200
  expect "content-type" "$(hval content-type)" "javascript"
  expect "cache-control immutable" "$(hval cache-control)" "immutable"
fi
echo

# 5. The manifest, and through it every path the build emitted. This is what proves
#    the deploy is the build: the manifest travels with the assets, so every path in
#    it must be a 200 on the live origin.
echo "5. manifest.json"
if req "$BASE/manifest.json"; then
  expect_status "status" 200
  expect "content-type" "$(hval content-type)" "application/json"
  expect "cache-control" "$(hval cache-control)" "must-revalidate"
  while read -r logical path _bytes _sha; do
    [[ -z "$logical" ]] && continue
    CHECKS=$((CHECKS + 1))
    live_status=$(curl -sS -o /dev/null -w '%{http_code}' --max-time 25 "$BASE$path" 2>/dev/null)
    case "$path" in
      # `/index.html` is the one asset the platform answers with a redirect: under
      # the default `html_handling` it 307s to `/`. That still resolves the
      # reference, so either answer passes and the status is printed.
      /index.html)
        if [[ "$live_status" == 200 || "$live_status" == 30[12378] ]]; then
          pass "manifest asset $logical" "$path ($live_status to /)"
        else
          fail "manifest asset $logical" "$path answered $live_status on the live origin"
        fi
        ;;
      *)
        if [[ "$live_status" == 200 ]]; then
          pass "manifest asset $logical" "$path"
        else
          fail "manifest asset $logical" "$path answered $live_status, wanted 200"
        fi
        ;;
    esac
  done < <(manifest_lines)
  # The live manifest must be the one this build wrote, or the smoke test is reading
  # a different deploy's truth. Compare the asset table, not the file's formatting.
  CHECKS=$((CHECKS + 1))
  LIVE_MANIFEST=$(body_text | node -e '
    let text = "";
    process.stdin.on("data", (chunk) => (text += chunk));
    process.stdin.on("end", () => {
      const assets = JSON.parse(text).assets;
      for (const [logical, asset] of Object.entries(assets)) {
        console.log([logical, asset.path, asset.bytes, asset.sha256].join(" "));
      }
    });
  ')
  if [[ "$LIVE_MANIFEST" == "$(manifest_lines)" ]]; then
    pass "the deploy is this build" "$(printf '%s\n' "$LIVE_MANIFEST" | wc -l | tr -d ' ') assets, same hashes"
  else
    fail "the deploy is this build" "the live manifest differs from $DIST/manifest.json"
  fi
fi
echo

# 6. The SPA fallback: an unknown path is the page, not a stub and not a 404, and it
#    must not be cacheable forever.
echo "6. SPA fallback"
if req "$BASE/this-path-does-not-exist"; then
  expect_status "status" 200
  expect "content-type" "$(hval content-type)" "text/html"
  expect "cache-control" "$(hval cache-control)" "must-revalidate"
  CHECKS=$((CHECKS + 1))
  if [[ -n "$INDEX_HTML" && "$(body_text)" == "$INDEX_HTML" ]]; then
    pass "the fallback is the page" "identical to GET /"
  else
    fail "the fallback is the page" "the body differs from GET /"
  fi
fi
echo

# 7. The ABI handshake — the assertion that names a stale artifact precisely. The
#    page's loader checks `abi_version() == 2` before it trusts a buffer layout; this
#    checks the *deployed bytes* the same way, over the wire, with no browser.
echo "7. the deployed module's ABI"
CHECKS=$((CHECKS + 1))
HANDSHAKE=$(node --input-type=module -e '
  const url = process.argv[1];
  try {
    const response = await fetch(url);
    if (!response.ok) throw new Error("GET " + url + " -> " + response.status);
    const module = await WebAssembly.compile(await response.arrayBuffer());
    const instance = await WebAssembly.instantiate(module, {});
    const ex = instance.exports;
    const wanted = ["abi_version", "alloc", "gen_game", "solve"];
    const have = wanted.filter((name) => typeof ex[name] === "function");
    console.log("abi_version=" + ex.abi_version());
    console.log("exports=" + have.join(","));
  } catch (error) {
    console.log("error=" + error.message);
    process.exit(1);
  }
' "$BASE$WASM" 2>&1)
if [[ $? -ne 0 ]]; then
  fail "abi_version" "$HANDSHAKE"
else
  expect_exact "abi_version" "$(printf '%s\n' "$HANDSHAKE" | sed -n 's/^abi_version=//p')" "2"
  EXPORTS=$(printf '%s\n' "$HANDSHAKE" | sed -n 's/^exports=//p')
  for export in abi_version alloc gen_game solve; do
    CHECKS=$((CHECKS + 1))
    if [[ ",$EXPORTS," == *",$export,"* ]]; then pass "export $export" "present"; else fail "export $export" "missing, got [$EXPORTS]"; fi
  done
fi
echo

# 8. Cross-origin isolation is absent by design in v1 (§7: one instance per worker,
#    no SharedArrayBuffer). Printed, not counted: empty is the expected answer.
echo "8. cross-origin isolation (absent by design; empty is expected)"
COI=$(printf '%s\n' "$HEADERS" | tr -d '\r' | grep -i '^cross-origin' || true)
if [[ -n "$COI" ]]; then
  note "cross-origin" "$(printf '%s' "$COI" | tr '\n' ' ')"
else
  note "cross-origin" "none, which v1 does not need"
fi
echo

if ((FAILS == 0)); then
  printf '%sPASS%s %d checks · %s\n' "$GREEN" "$RESET" "$CHECKS" "$BASE"
  exit 0
fi
printf '%sFAIL%s %d of %d checks · %s\n' "$RED" "$RESET" "$FAILS" "$CHECKS" "$BASE"
echo "the deploy is live and wrong; that is the operator's call. Nothing was rolled back." >&2
exit 1
