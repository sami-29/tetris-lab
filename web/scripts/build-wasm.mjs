// Build the §4 artifact and put it where the loader expects it.
//
//   node scripts/build-wasm.mjs
//
// `cargo build --release --target wasm32-unknown-unknown` from the workspace
// root, then the artifact is copied to `web/pkg/engine.wasm`. No wasm-bindgen, no
// wasm-opt: the exports are already in the module's export section, and `strip`
// removes the names nothing reads (the export names survive).
//
// The copy exists because cargo names the artifact after the crate
// (`engine_wasm.wasm`) and buries it under `target/`, while the page and the node
// test both want one stable path.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB = join(HERE, "..");
const ROOT = join(WEB, "..");

const ARTIFACT = join(ROOT, "target", "wasm32-unknown-unknown", "release", "engine_wasm.wasm");
const OUT_DIR = join(WEB, "pkg");
const OUT = join(OUT_DIR, "engine.wasm");

/** §9.3: warn above 100 KB, fail above 150 KB. Reported, not enforced, here. */
const WARN_BYTES = 100 * 1024;

console.log(`$ cargo build --release --target wasm32-unknown-unknown  (cwd ${ROOT})`);
execFileSync(
  "cargo",
  ["build", "--release", "--target", "wasm32-unknown-unknown", "-p", "engine-wasm"],
  { cwd: ROOT, stdio: "inherit" },
);

mkdirSync(OUT_DIR, { recursive: true });
copyFileSync(ARTIFACT, OUT);
const { size } = statSync(OUT);
console.log(`copied ${ARTIFACT}`);
console.log(`    -> ${OUT} (${size} bytes${size > WARN_BYTES ? ", OVER the §9.3 100 KB warn line" : ""})`);
