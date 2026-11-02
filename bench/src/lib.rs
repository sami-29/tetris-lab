//! The native bench: the Dataset's exporter and its fingerprint gate (§6, §9.2
//! gate 1).
//!
//! This is the only crate that reads or writes `data/dataset/`, and it does no
//! Board, Sequence or placement arithmetic of its own: it calls
//! [`engine::generate_game`] exactly as `engine-wasm` does and formats the
//! result. That is what makes "the committed games are the games the Playground
//! plays" a checkable claim rather than a convention (§6, ADR 0003).
//!
//! * [`dataset`] — the on-disk formats: header line, one line per game, manifest.
//! * [`fingerprint`] — §6.1's semantic FNV-1a-64, and nothing else.
//! * [`export`] — generate, render, write. Never silently: a change that moves a
//!   game is refused unless it is accepted with a reason (§6.2).
//! * [`verify`] — the gate: manifest agreement, every game regenerated and
//!   compared bit for bit, the fingerprint computed twice, the cap, the pairing.
//! * [`show`] — one game as an ASCII board (the spec's `--pretty`, kept off the
//!   exporter so no flag can produce a non-canonical `games.txt`).
//! * [`width`] — §9.5's width curve: the two numbers the page hangs on, measured
//!   once and written to `data/measurements/width-curve.json`.

pub mod dataset;
pub mod export;
pub mod fingerprint;
pub mod show;
pub mod verify;
pub mod width;
