#!/usr/bin/env bash
# Reproduce every row of the ticket 06 results table.
#
#   bash prototype/run-bench.sh                  # build, then 5 interleaved sweeps
#   SKIP_PYTHON=1 bash prototype/run-bench.sh    # reuse the existing Python baselines
#   SESSION=b SESSIONS=3 bash prototype/run-bench.sh   # three tagged sessions, pooled
#
# Each sweep runs every variant back to back, so slow drift in CPU clocks or
# background load hits all variants alike instead of landing on one of them.
# summarize.py pools every rep it finds in prototype/data/raw/.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"

GAMES=prototype/data/games_goal8_p40_seed0-999.txt
GOAL=8
REPS=${REPS:-5}
SESSIONS=${SESSIONS:-1}
SESSION=${SESSION:-s1}
RAW=prototype/data/raw
CHECK=prototype/data/py-asis.jsonl
mkdir -p "$RAW" prototype/data

echo "== build"
bash prototype/build.sh >/dev/null

echo "== games"
if [ ! -f "$GAMES" ]; then
  PYTHONDONTWRITEBYTECODE=1 python prototype/py/baseline.py export --count 1000 --out "$GAMES"
fi

if [ "${SKIP_PYTHON:-0}" != "1" ]; then
  echo "== python baselines (1000 games each, ~3 min)"
  PYTHONDONTWRITEBYTECODE=1 python prototype/py/baseline.py bench --variant asis \
    --count 1000 --out prototype/data/py-asis.jsonl
  PYTHONDONTWRITEBYTECODE=1 python prototype/py/baseline.py bench --variant lines_live \
    --count 1000 --out prototype/data/py-lines_live.jsonl
fi

run_native() { # binary, tag
  ./"$1" --games "$GAMES" --goal "$GOAL" \
    | grep -v '"check"' > "$RAW/$2.$SESSION.rep$CURRENT_REP.json"
}

run_wasm() { # artifact name, tag
  node prototype/wasm/bench.mjs --wasm "prototype/artifacts/$1.wasm" --games "$GAMES" \
    --goal "$GOAL" --reps 5 \
    | python -c 'import json,sys; d=json.load(sys.stdin); d.pop("check", None); print(json.dumps(d))' \
    > "$RAW/$2.$SESSION.rep$CURRENT_REP.json"
}

for session in $(seq 1 "$SESSIONS"); do
  SESSION_TAG="$SESSION-s$session"
  for CURRENT_REP in $(seq 1 "$REPS"); do
    echo "== session $SESSION_TAG rep $CURRENT_REP"
    SESSION="$SESSION_TAG"
    run_native prototype/artifacts/bench-native-u32.exe rs-native-u32
    run_native prototype/artifacts/bench-native-u32-native.exe rs-native-u32-native
    run_native prototype/artifacts/bench-native-u64-native.exe rs-native-u64-native
    run_native prototype/cpp/t06_bench_opt3.exe cpp-native-opt3
    run_native prototype/cpp/t06_bench_native.exe cpp-native-march
    run_wasm wasm-u32 wasm-u32
    run_wasm wasm-u64 wasm-u64
    run_wasm wasm-u32-simd wasm-u32-simd
    run_wasm wasm-u32-lines_live wasm-u32-lines_live
  done
done

echo "== gates: exact eval-count parity with the Python run, per-game rows, SIMD parity"
gate_native() { # binary, tag, check-jsonl
  ./"$1" --games "$GAMES" --goal "$GOAL" --out "prototype/data/$2.jsonl" --check "$3" \
    | grep '"check"' > "$RAW/$2.check.json" || true
}
gate_native prototype/artifacts/bench-native-u32.exe rs-native-u32 "$CHECK"
gate_native prototype/artifacts/bench-native-u32-native.exe rs-native-u32-native "$CHECK"
gate_native prototype/artifacts/bench-native-u64-native.exe rs-native-u64-native "$CHECK"
gate_native prototype/cpp/t06_bench_opt3.exe cpp-native-opt3 "$CHECK"
gate_native prototype/cpp/t06_bench_native.exe cpp-native-march "$CHECK"

gate_wasm() { # artifact, tag, check-jsonl
  node prototype/wasm/bench.mjs --wasm "prototype/artifacts/$1.wasm" --games "$GAMES" \
    --goal "$GOAL" --reps 5 --out "prototype/data/$2.jsonl" --check "$3" \
    | python -c 'import json,sys
d = json.load(sys.stdin)
out = {"check": d["check"]}
if d.get("simd_parity_boards"):
    out["simd_parity_boards"] = d["simd_parity_boards"]
    out["simd_parity_mismatches"] = d["simd_parity_mismatches"]
print(json.dumps(out))' > "$RAW/$2.check.json"
}
gate_wasm wasm-u32 wasm-u32 "$CHECK"
gate_wasm wasm-u64 wasm-u64 "$CHECK"
gate_wasm wasm-u32-simd wasm-u32-simd "$CHECK"
gate_wasm wasm-u32-lines_live wasm-u32-lines_live prototype/data/py-lines_live.jsonl

echo "== summarize"
python prototype/summarize.py
