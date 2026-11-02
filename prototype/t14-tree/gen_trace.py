"""
Ticket 14 prototype: dump a real width-200 beam trace as canned data for the viewer.

Throwaway. It is not the shipped engine (that does not exist yet): it is the same
SHAPE the engine will emit, so the viewer can be judged on real structure rather
than fabricated growth curves like the t10 mockup's.

Shape modelled (from ADR-0002 / ticket 07):
  layered beam, width 200, one layer per piece of a known 7-bag sequence,
  per-layer dedup by board (best score per board survives), Dellacherie-Thiery
  evaluator, board as 10 u32 columns (bit 0 = bottom row).

Deliberately absent, because they do not change what the tree looks like:
  the exact 3-piece tail, the no-suicide height cap, batch worker scheduling.

Output: trace.js (window.TRACE) next to index.html, plus a printed stats table.
Usage:  python prototype/t14-tree/gen_trace.py [--seed 0] [--width 200]
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))

import tetris_engine as engine  # noqa: E402
import tetromino_data as td  # noqa: E402

BOARD_H, BOARD_W = 20, 10
GAMES_FILE = ROOT / "prototype" / "data" / "games_goal8_p40_seed0-999.txt"


# ── Dellacherie-Thiery evaluator (ticket 04 §3) ───────────────────────────────
# score = -landing_height + eroded_cells - row_transitions - column_transitions
#         - 4*holes - cumulative_wells

def board_features(board: np.ndarray) -> tuple[int, int, int, int]:
    """row_transitions, column_transitions, holes, cumulative_wells."""
    rows = np.pad(board, ((0, 0), (1, 1)), constant_values=1)
    row_trans = int((rows[:, 1:] != rows[:, :-1]).sum())

    cols = np.pad(board, ((0, 1), (0, 0)), constant_values=1)
    col_trans = int((cols[1:, :] != cols[:-1, :]).sum())

    filled_above = np.cumsum(board, axis=0) > 0
    holes = int((filled_above & (board == 0)).sum())

    empty = board == 0
    left = np.pad(board, ((0, 0), (1, 0)), constant_values=1)[:, :BOARD_W]
    right = np.pad(board, ((0, 0), (0, 1)), constant_values=1)[:, 1:]
    is_well = empty & (left == 1) & (right == 1)
    depth = np.zeros(BOARD_W, dtype=np.int32)
    wells = 0
    for r in range(BOARD_H):
        depth = np.where(is_well[r], depth + 1, 0)
        wells += int(depth.sum())
    return row_trans, col_trans, holes, wells


def score_board(board: np.ndarray, landing_height: int, eroded: int) -> float:
    rt, ct, holes, wells = board_features(board)
    return float(
        -landing_height + eroded - rt - ct - 4 * holes - wells
    )


def board_key(board: np.ndarray) -> bytes:
    return board.tobytes()


def hex_columns(board: np.ndarray) -> str:
    """10 u32 columns, bit 0 = bottom row, comma-free fixed-width hex."""
    out = []
    for c in range(BOARD_W):
        w = 0
        for r in range(BOARD_H):
            if board[r, c]:
                w |= 1 << (BOARD_H - 1 - r)
        out.append(f"{w:05x}")
    return "".join(out)


# ── game file ────────────────────────────────────────────────────────────────

def load_game(seed: int) -> tuple[np.ndarray, str, dict]:
    text_lines = GAMES_FILE.read_text().splitlines()
    header = text_lines[0]
    lines = [ln for ln in text_lines if ln.strip() and not ln.startswith("#")]
    for i, line in enumerate(lines):
        if line.strip() == f"seed {seed}":
            board = np.array(
                [[1 if ch == "1" else 0 for ch in row] for row in lines[i + 1 : i + 21]],
                dtype=np.uint8,
            )
            seq = lines[i + 21].strip()
            meta = {"seed": seed, "header": header}
            return board, seq, meta
    raise SystemExit(f"seed {seed} not in {GAMES_FILE}")


# ── layered beam ─────────────────────────────────────────────────────────────

def run(seed: int, width: int, goal: int) -> dict:
    board0, seq, meta = load_game(seed)

    # Candidate record: (score, lines, board, parent, piece, rot, col, landing_h, eroded)
    states = [{
        "score": score_board(board0, 0, 0),
        "lines": 0,
        "board": board0,
        "parent": -1,
        "move": None,
        "landing_h": 0,
        "eroded": 0,
    }]
    layers = [{
        "states": states,
        "cand": 1,
        "dup": 0,
        "distinct_boards": 1,
        "label": "root",
    }]

    for pi, piece in enumerate(seq):
        cands = []
        for si, st in enumerate(states):
            for rot_idx, shape in enumerate(td.TETROMINO_SHAPES[piece]):
                for col in engine.get_valid_columns(st["board"], shape):
                    landing_row = engine.drop_row(st["board"], shape, col)
                    if landing_row < 0:
                        continue
                    tmp = st["board"].copy()
                    engine.place_piece(tmp, shape, landing_row, col)
                    full = np.all(tmp, axis=1)
                    cleared = int(full.sum())
                    # eroded piece cells: piece cells that sit in a cleared row
                    eroded = 0
                    if cleared:
                        for rr in range(shape.shape[0]):
                            if full[landing_row + rr]:
                                eroded += int(shape[rr].sum())
                        tmp, _ = engine.clear_lines(tmp)
                    if tmp[0].any():  # stack reaches the spawn row: dead state
                        continue
                    landing_h = BOARD_H - landing_row
                    cands.append((
                        score_board(tmp, landing_h, eroded),
                        st["lines"] + cleared,
                        tmp,
                        si,
                        piece,
                        rot_idx,
                        col,
                        landing_h,
                        eroded,
                    ))
        if not cands:
            break

        raw = len(cands)
        # dedup: highest score per identical board
        best: dict[bytes, tuple] = {}
        for c in cands:
            k = board_key(c[2])
            prev = best.get(k)
            if prev is None or (c[0], -c[3]) > (prev[0], -prev[3]):
                best[k] = c
        deduped = sorted(
            best.values(),
            key=lambda c: (-c[0], board_key(c[2]), c[3]),
        )
        dup = raw - len(deduped)
        kept = deduped[:width]

        new_states = []
        for c in kept:
            new_states.append({
                "score": c[0],
                "lines": c[1],
                "board": c[2],
                "parent": c[3],
                "move": [c[4], c[5], c[6]],
                "landing_h": c[7],
                "eroded": c[8],
            })
        states = new_states
        layers.append({
            "states": states,
            "cand": raw,
            "dup": dup,
            "distinct_boards": len(deduped),
            "label": piece,
        })
        if states[0]["lines"] >= goal:
            print(f"  goal {goal} reached at piece {pi + 1}")
            break

    return {"layers": layers, "meta": {**meta, "width": width, "goal": goal,
                                      "sequence": seq, "pieces_used": len(layers) - 1}}


# ── metrics the legibility answer rests on ───────────────────────────────────

def analyse(trace: dict) -> list[dict]:
    rows = []
    layers = trace["layers"]
    lineages: list[list[tuple[int, ...]]] = []
    for layer in layers:
        st = layer["states"]
        if not lineages:
            lineages.append([() for _ in st])
            continue
        prev_lin = lineages[-1]
        lineages.append([prev_lin[s["parent"]] + (i,) for i, s in enumerate(st)])

    for li, layer in enumerate(layers):
        st = layer["states"]
        scores = np.array([s["score"] for s in st], dtype=float)
        lines = np.array([s["lines"] for s in st], dtype=int)
        profiles = {"".join(f"{h:02d}" for h in column_heights(s["board"])) for s in st}
        parents = {s["parent"] for s in st}

        kids = np.zeros(len(st), dtype=int)
        if li + 1 < len(layers):
            for s in layers[li + 1]["states"]:
                kids[s["parent"]] += 1

        bits = np.array([[1 if s["board"][r, c] else 0 for r in range(BOARD_H)
                         for c in range(BOARD_W)] for s in st], dtype=np.int8)
        if len(bits) > 1:
            diff = bits[:, None, :] != bits[None, :, :]
            iu = np.triu_indices(len(bits), k=1)
            dist = diff.sum(axis=2)[iu].astype(float)
            mean_pair = float(dist.mean())
            p10, p90 = (float(np.percentile(dist, 10)), float(np.percentile(dist, 90)))
            to_best = (bits[1:] != bits[0]).sum(axis=1).astype(float)
            mean_to_best = float(to_best.mean()) if len(to_best) else 0.0
        else:
            mean_pair = p10 = p90 = mean_to_best = 0.0

        half = li // 2
        if li >= 2 and half >= 1:
            anc = [lin[half - 1] for lin in lineages[li]]
            distinct_anc = len(set(anc))
            top_share = max(anc.count(a) for a in set(anc))
            anc_of = half
        else:
            distinct_anc = len(lineages[li])
            top_share = len(lineages[li])
            anc_of = li

        rows.append({
            "layer": li,
            "piece": layer["label"],
            "n": len(st),
            "cand": layer["cand"],
            "dup": layer["dup"],
            "distinct": layer["distinct_boards"],
            "dup_pct": (100.0 * layer["dup"] / layer["cand"]) if layer["cand"] else 0.0,
            "smax": float(scores.max()) if len(scores) else 0.0,
            "smin": float(scores.min()) if len(scores) else 0.0,
            "p50": float(np.median(scores)) if len(scores) else 0.0,
            "spread": float(scores.max() - scores.min()) if len(scores) else 0.0,
            "lines": int(lines.max()) if len(lines) else 0,
            "parents": len(parents),
            "with_kids": int((kids > 0).sum()),
            "max_kids": int(kids.max()) if len(kids) else 0,
            "height_profiles": len(profiles),
            "max_height": int(max(sum(column_heights(s["board"])) for s in st)) if st else 0,
            "dist_mean": mean_pair,
            "dist_p10": p10,
            "dist_p90": p90,
            "dist_best": mean_to_best,
            "anc_of": anc_of,
            "anc_distinct": distinct_anc,
            "anc_top_share": top_share,
        })
    return rows


def column_heights(board: np.ndarray) -> list[int]:
    heights = []
    for c in range(BOARD_W):
        col = board[:, c]
        rows_filled = int(col.sum())
        heights.append(0 if rows_filled == 0 else BOARD_H - int(col.argmax()))
    return heights


def spine(trace: dict) -> list[dict]:
    """Greedy best-child path: at each layer, follow the child of the current node."""
    out = []
    idx = 0
    layers = trace["layers"]
    for li in range(1, len(layers)):
        st = layers[li]["states"]
        kids = [i for i, s in enumerate(st) if s["parent"] == idx]
        if not kids:
            break
        best = max(kids, key=lambda i: st[i]["score"])
        out.append({"layer": li, "rank": best, "of": len(st), "score": st[best]["score"]})
        idx = best
    return out


def best_path(trace: dict) -> list[dict]:
    """The final best state's ancestry: rank of that lineage in every layer."""
    layers = trace["layers"]
    st = layers[-1]["states"]
    idx = max(range(len(st)), key=lambda i: st[i]["score"])
    out = []
    for li in range(len(layers) - 1, 0, -1):
        out.append({"layer": li, "rank": idx, "of": len(layers[li]["states"]),
                    "score": layers[li]["states"][idx]["score"]})
        idx = layers[li]["states"][idx]["parent"]
    out.reverse()
    return out


def to_js(trace: dict, metrics: list[dict], spine_rows: list[dict], best_rows: list[dict]) -> str:
    layers = []
    for layer in trace["layers"]:
        packed = [
            [
                s["parent"],
                round(s["score"], 1),
                s["lines"],
                hex_columns(s["board"]),
                s["move"][1] if s["move"] else -1,
                s["move"][2] if s["move"] else -1,
                s["landing_h"],
                s["eroded"],
            ]
            for s in layer["states"]
        ]
        layers.append({
            "n": len(packed),
            "cand": layer["cand"],
            "dup": layer["dup"],
            "distinct": layer["distinct_boards"],
            "piece": layer["label"],
            "st": packed,
        })
    payload = {
        "meta": trace["meta"],
        "layers": layers,
        "metrics": [
            {k: (round(v, 2) if isinstance(v, float) else v) for k, v in m.items()}
            for m in metrics
        ],
        "spine": spine_rows,
        "best": best_rows,
    }
    body = json.dumps(payload, separators=(",", ":"))
    return f"window.TRACE = {body};\n"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--width", type=int, default=200)
    ap.add_argument("--goal", type=int, default=8)
    args = ap.parse_args()

    print(f"beam seed={args.seed} width={args.width} goal={args.goal}")
    trace = run(args.seed, args.width, args.goal)
    metrics = analyse(trace)
    spine_rows = spine(trace)
    best_rows = best_path(trace)

    out = Path(__file__).resolve().parent / "trace.js"
    out.write_text(to_js(trace, metrics, spine_rows, best_rows))
    print(f"wrote {out} ({out.stat().st_size / 1024:.0f} KB)")

    print(
        f"{'L':>3} {'pc':>2} {'n':>4} {'cand':>5} {'dup%':>5} {'dist':>5} "
        f"{'smax':>7} {'smin':>7} {'p50':>7} {'sprd':>5} {'hprof':>5} {'par':>4} "
        f"{'kids':>4} {'pair':>5} {'best':>5} {'anc':>7} {'spine':>6} {'bestp':>6}"
    )
    sp = {r["layer"]: r for r in spine_rows}
    bp = {r["layer"]: r for r in best_rows}
    for m in metrics:
        s = sp.get(m["layer"])
        sr = f"{s['rank'] + 1}/{s['of']}" if s else "—"
        b = bp.get(m["layer"])
        br = f"{b['rank'] + 1}/{b['of']}" if b else "—"
        print(
            f"{m['layer']:>3} {m['piece'][:2]:>2} {m['n']:>4} {m['cand']:>5} "
            f"{m['dup_pct']:>5.1f} {m['distinct']:>5} {m['smax']:>7.1f} {m['smin']:>7.1f} "
            f"{m['p50']:>7.1f} {m['spread']:>5.1f} {m['height_profiles']:>5} "
            f"{m['parents']:>4} {m['with_kids']:>4} {m['dist_mean']:>5.0f} "
            f"{m['dist_best']:>5.0f} {m['anc_distinct']:>3}/{m['anc_top_share']:<3} {sr:>6} {br:>6}"
        )


if __name__ == "__main__":
    main()
