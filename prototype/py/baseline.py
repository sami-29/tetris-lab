"""Ticket 06 spike — Python baseline harness for the bitboard benchmark.

Two jobs:

1. Export the exact games the Python app generates (same seeds, same 7-bag
   generator, same pre-filled board) to a portable text file, so the C/Rust
   kernels benchmark *identical* games instead of re-implementing Python's RNG.
2. Measure the current numpy solver twice: `asis` (the shipped code, where the
   `_W_LINES` term is always zero) and `lines_live` (the same search with the
   lines term actually contributing), so the speed claim is not made against a
   crippled search.

Run from the repo root:

    python prototype/py/baseline.py export --count 1000 --out prototype/data/games_goal8_p40_seed0-999.txt
    python prototype/py/baseline.py bench  --variant asis       --count 1000 --out prototype/data/py-asis.jsonl
    python prototype/py/baseline.py bench  --variant lines_live --count 1000 --out prototype/data/py-lines_live.jsonl
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))

import numpy as np  # noqa: E402

import tetris_engine as engine  # noqa: E402
from TetrisGameGenerator import TetrisGameGenerator  # noqa: E402
from TetrisSolver import TetrisSolver, _MoveNode, _State, _unwind  # noqa: E402
from tetromino_data import TETROMINO_SHAPES  # noqa: E402

# App defaults (`app.py`: seed 42, goal 8, tetrominoes 40, initialHeightMax 7).
GOAL = 8
PIECES = 40
HEIGHT_MAX = 7


# ── Game export ───────────────────────────────────────────────────────────────

def export_games(count: int, out: Path) -> None:
    """Write `count` games in a flat text format the kernels can parse.

    Per game: a `seed` line, 20 board rows of 10 chars (0/1, row 0 = top),
    one sequence line of piece letters. Comment lines start with '#'.
    """
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8", newline="\n") as fh:
        fh.write(f"# browser-tetris-lab ticket 06 games: goal={GOAL} pieces={PIECES} height_max={HEIGHT_MAX}\n")
        for seed in range(count):
            game = TetrisGameGenerator(
                seed=seed, goal=GOAL, tetrominoes=PIECES, initial_height_max=HEIGHT_MAX
            )
            fh.write(f"seed {seed}\n")
            for row in game.board:
                fh.write("".join("1" if cell else "0" for cell in row) + "\n")
            fh.write("".join(game.sequence) + "\n")
    print(f"wrote {count} games to {out}")


# ── Solver variants ───────────────────────────────────────────────────────────

def _beam_search_lines_live(solver: TetrisSolver, beam_width: int):
    """`TetrisSolver._beam_search` with the `_W_LINES` term live.

    Byte-for-byte the shipped search loop; the only change is that the score
    includes `_W_LINES * lines_this_move`, which is what the weight was meant
    to do (the shipped code scores the post-clear board, where the term is
    zero). The harness asserts this variant reproduces the shipped solver when
    the weight is set to zero.
    """
    evaluations = 0
    beam = [_State(
        score=engine.evaluate(solver.initial_board),
        lines_cleared=0,
        board=solver.initial_board.copy(),
        move_node=None,
    )]

    for piece_name in solver.sequence:
        rotations = TETROMINO_SHAPES[piece_name]
        candidates = []

        for state in beam:
            for rot_idx, shape in enumerate(rotations):
                for col in engine.get_valid_columns(state.board, shape):
                    evaluations += 1

                    landing_row = engine.drop_row(state.board, shape, col)
                    if landing_row < 0:
                        continue

                    tmp = state.board.copy()
                    engine.place_piece(tmp, shape, landing_row, col)
                    tmp, lines_this_move = engine.clear_lines(tmp)

                    new_lines = state.lines_cleared + lines_this_move
                    new_node = _MoveNode(move=(piece_name, rot_idx, col), parent=state.move_node)

                    if new_lines >= solver.goal:
                        return True, _unwind(new_node), evaluations

                    score = engine.evaluate(tmp) + engine._W_LINES * lines_this_move
                    candidates.append((score, new_lines, tmp, new_node))

        if not candidates:
            return False, [], evaluations

        if len(candidates) > beam_width:
            candidates.sort(key=lambda c: c[0], reverse=True)
            candidates = candidates[:beam_width]

        beam = [
            _State(score=score, lines_cleared=lc, board=board, move_node=node)
            for score, lc, board, node in candidates
        ]

    return False, [], evaluations


def solve_asis(board: np.ndarray, sequence: list[str], goal: int):
    return TetrisSolver(board, sequence, goal).solve()


def solve_lines_live(board: np.ndarray, sequence: list[str], goal: int):
    solver = TetrisSolver(board, sequence, goal)
    total = 0
    for width in solver.beam_schedule:
        ok, moves, evals = _beam_search_lines_live(solver, width)
        total += evals
        if ok:
            return True, moves, total
    return False, [], total


VARIANTS = {"asis": solve_asis, "lines_live": solve_lines_live}


def selfcheck() -> None:
    """The lines_live path must equal the shipped solver when W_LINES == 0."""
    saved = engine._W_LINES
    engine._W_LINES = 0.0
    try:
        for seed in range(5):
            game = TetrisGameGenerator(seed=seed, goal=GOAL, tetrominoes=PIECES,
                                       initial_height_max=HEIGHT_MAX)
            a = solve_asis(game.board, game.sequence, GOAL)
            b = solve_lines_live(game.board, game.sequence, GOAL)
            assert a[0] == b[0] and a[2] == b[2], (seed, a[0], b[0], a[2], b[2])
            assert [m[:3] for m in a[1]] == [m[:3] for m in b[1]], seed
    finally:
        engine._W_LINES = saved
    print("selfcheck ok: lines_live reproduces the shipped solver at W_LINES=0")


# ── Benchmark ─────────────────────────────────────────────────────────────────

def bench(variant: str, count: int, out: Path, goal: int = GOAL) -> None:
    solve = VARIANTS[variant]
    out.parent.mkdir(parents=True, exist_ok=True)

    # Warm up numpy / import paths so they do not land in the first game's time.
    warm = TetrisGameGenerator(seed=0, goal=goal, tetrominoes=PIECES, initial_height_max=HEIGHT_MAX)
    solve(warm.board, warm.sequence, goal)

    rows = []
    t_start = time.perf_counter()
    with out.open("w", encoding="utf-8", newline="\n") as fh:
        for seed in range(count):
            game = TetrisGameGenerator(seed=seed, goal=goal, tetrominoes=PIECES,
                                       initial_height_max=HEIGHT_MAX)
            t0 = time.perf_counter()
            ok, moves, evals = solve(game.board, game.sequence, goal)
            ms = (time.perf_counter() - t0) * 1000.0
            row = {
                "solver": f"py-{variant}", "seed": seed, "goal": goal,
                "pieces": PIECES, "success": bool(ok), "evals": int(evals),
                "ms": ms, "moves": len(moves),
            }
            rows.append(row)
            fh.write(json.dumps(row) + "\n")
    wall = time.perf_counter() - t_start

    evals_total = sum(r["evals"] for r in rows)
    times = sorted(r["ms"] for r in rows)
    summary = {
        "solver": f"py-{variant}",
        "games": len(rows),
        "goal": goal,
        "pieces": PIECES,
        "success_rate": sum(r["success"] for r in rows) / len(rows),
        "evals_total": evals_total,
        "evals_per_s": evals_total / wall,
        "games_per_s": len(rows) / wall,
        "ms_median": statistics.median(times),
        "ms_mean": statistics.fmean(times),
        "ms_p90": times[min(len(times) - 1, int(round(0.9 * (len(times) - 1))))],
        "ms_total": wall * 1000.0,
        "python": sys.version.split()[0],
        "numpy": np.__version__,
        "w_lines": engine._W_LINES,
    }
    print(json.dumps(summary, indent=2))
    out.with_suffix(".summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")


def main() -> None:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    e = sub.add_parser("export")
    e.add_argument("--count", type=int, default=1000)
    e.add_argument("--out", type=Path, default=Path("prototype/data/games_goal8_p40_seed0-999.txt"))
    b = sub.add_parser("bench")
    b.add_argument("--variant", choices=sorted(VARIANTS), required=True)
    b.add_argument("--count", type=int, default=1000)
    b.add_argument("--goal", type=int, default=GOAL)
    b.add_argument("--out", type=Path, default=None)
    b.add_argument("--selfcheck", action="store_true")
    args = ap.parse_args()

    if args.cmd == "export":
        export_games(args.count, args.out)
        return
    if args.selfcheck:
        selfcheck()
    out = args.out or Path(f"prototype/data/py-{args.variant}.jsonl")
    bench(args.variant, args.count, out, args.goal)


if __name__ == "__main__":
    main()
