"""Aggregate the ticket 06 raw runs into one results table.

Reads prototype/data/raw/*.rep*.json (one summary per repetition) plus the two
Python summaries, takes the median of the timing figures across repetitions, and
writes prototype/data/results.csv and results.md.

    python prototype/summarize.py
"""

from __future__ import annotations

import csv
import json
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "prototype" / "data"
RAW = DATA / "raw"

PY_ROWS = [
    ("python-asis", DATA / "py-asis.summary.json", "CPython 3.11 + numpy, shipped heuristic"),
    ("python-lines_live", DATA / "py-lines_live.summary.json", "CPython 3.11 + numpy, _W_LINES live"),
]

ORDER = [
    "python-asis",
    "python-lines_live",
    "rs-native-u32",
    "rs-native-u32-native",
    "rs-native-u64-native",
    "cpp-native-opt3",
    "cpp-native-march",
    "wasm-u32",
    "wasm-u64",
    "wasm-u32-simd",
    "wasm-u32-lines_live",
]


def median_of(values):
    values = [v for v in values if v is not None]
    return statistics.median(values) if values else None


def load_raw(name: str) -> list[dict]:
    rows = []
    for path in sorted(RAW.glob(f"{name}.*rep*.json")):
        rows.append(json.loads(path.read_text(encoding="utf-8").strip()))
    return rows


def main() -> None:
    table = []
    for name, path, note in PY_ROWS:
        d = json.loads(path.read_text(encoding="utf-8"))
        table.append({
            "solver": name,
            "engine": "numpy",
            "note": note,
            "games": d["games"],
            "success_rate": d["success_rate"],
            "evals_total": d["evals_total"],
            "evals_per_s": d["evals_per_s"],
            "ms_median": d["ms_median"],
            "ms_p90": d["ms_p90"],
            "batch_evals_per_s": None,
            "wasm_bytes": None,
            "wasm_simd": None,
            "instantiate_ms": None,
              "parity": "",
            "check": "",
        })

    for name in ORDER[2:]:
        reps = load_raw(name)
        if not reps:
            continue
        print(f"{name}: pooling {len(reps)} runs", flush=True)
        is_wasm = any("wasm_bytes" in r for r in reps)
        check_path = RAW / f"{name}.check.json"
        check = json.loads(check_path.read_text(encoding="utf-8")) if check_path.exists() else None
        if check is None:
            check = next((r.get("check") for r in reps if r.get("check")), None)
        if isinstance(check, dict) and isinstance(check.get("check"), dict):
            check = check["check"]
        parity = ""
        if is_wasm and (reps[0].get("simd_parity_boards") or 0) > 0:
            boards = reps[0]["simd_parity_boards"]
            bad = reps[0].get("simd_parity_mismatches")
            parity = f"{boards} boards, {bad} bad"
        table.append({
            "solver": name,
            "engine": "wasm/Node24" if is_wasm else "native",
            "note": "",
            "games": reps[0]["games"],
            "success_rate": median_of([r["success_rate"] for r in reps]),
            "evals_total": reps[0]["evals_total"],
            "evals_per_s": median_of([r["evals_per_s"] for r in reps]),
            "ms_median": median_of([r["ms_median"] for r in reps]),
            "ms_p90": median_of([r["ms_p90"] for r in reps]),
            "batch_evals_per_s": median_of([r.get("batch_evals_per_s") for r in reps]),
            "wasm_bytes": reps[0].get("wasm_bytes"),
            "wasm_simd": ("yes" if "simd" in name else "no") if is_wasm else None,
            "instantiate_ms": median_of([r.get("instantiate_ms_median") for r in reps]),
            "parity": parity,  # noqa: E501
            "check": "" if check is None else
                     f"success {check.get('success_mismatches')}, evals {check.get('eval_mismatches')}",
        })

    base = next(r for r in table if r["solver"] == "python-asis")
    for row in table:
        row["speedup_vs_python"] = row["evals_per_s"] / base["evals_per_s"]

    fields = ["solver", "engine", "games", "success_rate", "evals_total", "evals_per_s",
              "speedup_vs_python", "ms_median", "ms_p90", "batch_evals_per_s", "wasm_bytes",
              "wasm_simd", "instantiate_ms", "parity", "check", "note"]
    csv_path = DATA / "results.csv"
    with csv_path.open("w", encoding="utf-8", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=fields)
        w.writeheader()
        for row in table:
            w.writerow({k: row.get(k) for k in fields})

    md = [
        "| solver | engine | success | evals/s | vs python | ms/game (median) | p90 ms | batch evals/s | wasm bytes | parity | eval-count check |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for row in table:
        md.append("| {solver} | {engine} | {succ} | {eps} | {sp} | {med} | {p90} | {batch} | {size} | {par} | {chk} |".format(
            solver=row["solver"],
            engine=row["engine"],
            succ=f"{row['success_rate']:.3f}",
            eps=f"{row['evals_per_s']:,.0f}",
            sp=f"{row['speedup_vs_python']:.0f}x",
            med=f"{row['ms_median']:.4f}",
            p90=f"{row['ms_p90']:.4f}",
            batch="-" if row["batch_evals_per_s"] is None else f"{row['batch_evals_per_s']:,.0f}",
            size="-" if row["wasm_bytes"] is None else f"{row['wasm_bytes']:,}",
            par=row["parity"] or "-",
            chk=row["check"] or "-",
        ))
    md_path = DATA / "results.md"
    md_path.write_text("\n".join(md) + "\n", encoding="utf-8")
    print("\n".join(md))
    print(f"\nwrote {csv_path} and {md_path}")


if __name__ == "__main__":
    main()
