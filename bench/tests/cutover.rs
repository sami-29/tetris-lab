//! Seam 4: the cutover (§11, ticket 18) -- the repository *is* the Lab.
//!
//! The ticket's acceptance is a set of facts about the tree, not about a program:
//! the Python application is gone, the evidence behind the ADRs survived under
//! `docs/`, every path a committed document cites resolves in a fresh clone, and
//! nothing under the Dataset can ever be ignored. Those are exactly the things a
//! reviewer would otherwise check by eye, once, and get wrong later -- so they are
//! asserted here, against `git ls-files` (the clone's own view) rather than against
//! `Path::exists` (this machine's view, which has `.scratch/` in it).
//!
//! Read-only: every test shells out to `git` and reads files. Nothing is staged,
//! deleted or written.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------- fixtures

/// The workspace root, derived from `bench/` (the same derivation
/// `bench::dataset::root` uses, so the tests and the binary agree on "the repo").
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bench/ always has a parent: the workspace root")
        .to_path_buf()
}

fn git(args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(repo())
        .output()
        .unwrap_or_else(|error| panic!("run `git {}`: {error}", args.join(" ")))
}

/// Every path `git ls-files` reports, which is every path a fresh clone has.
fn tracked() -> Vec<String> {
    let out = git(&["ls-files", "-z"]);
    assert!(
        out.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A path resolves in a fresh clone if and only if git tracks it.
fn is_tracked(path: &str) -> bool {
    git(&["ls-files", "--error-unmatch", "--", path])
        .status
        .success()
}

/// A committed document may cite a path from the repository root or from its own
/// directory (`t14-tree/README.md` links `FINDINGS.md`); both resolve in a clone.
fn resolves(document: &str, target: &str) -> bool {
    if is_tracked(target) {
        return true;
    }
    let mut relative = Path::new(document)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
    relative.push(target);
    let normalized = relative.to_string_lossy().replace('\\', "/");
    is_tracked(normalized.trim_start_matches("./").trim_end_matches('/'))
}

fn is_ignored(path: &str) -> bool {
    // `--no-index` asks git's own matcher even for a path that does not exist yet
    // (the §6.2 archive case), which is the question the guard has to answer.
    git(&["check-ignore", "--no-index", "-q", "--", path])
        .status
        .success()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

// ------------------------------------- 1. the Python application is deleted

#[test]
fn the_python_application_is_not_in_the_tree() {
    let files = tracked();

    // A delete set, not a naming convention: these are the paths ticket 18 removes,
    // and a copy under any directory would still be the old application.
    let gone = [
        "requirements.txt",
        "Procfile",
        "tetris_solver.db",
        "winnable_games.csv",
        "log.txt",
    ];
    for path in &files {
        assert!(
            !path.ends_with(".py"),
            "{path} is a Python file; the cutover deletes every one of them"
        );
        assert!(
            !path.ends_with(".pyc"),
            "{path} is a compiled Python file; the cutover deletes those too"
        );
        for prefix in [
            "templates/",
            "static/",
            "prototype/",
            "host-probe/",
            "__pycache__/",
            ".ruff_cache/",
            "reference/",
        ] {
            assert!(
                !path.starts_with(prefix),
                "{path} is under {prefix}; the cutover deletes that whole directory"
            );
        }
        assert!(
            !gone.contains(&path.as_str()),
            "{path} belongs to the Python application and must be deleted, not kept"
        );
    }

    // The working tree, not just the index: an untracked leftover is the same bug a
    // committed one is, and the ticket names the untracked leftovers explicitly.
    for name in [
        "app.py",
        "main.py",
        "TetrisSolver.py",
        "TetrisGameGenerator.py",
        "tetris_engine.py",
        "tetromino_data.py",
        "db_operations.py",
        "minimization.py",
        "requirements.txt",
        "Procfile",
        "tetris_solver.db",
        "winnable_games.csv",
        "log.txt",
        "templates",
        "static",
        "prototype",
        "host-probe",
        "__pycache__",
        ".ruff_cache",
    ] {
        assert!(
            !repo().join(name).exists(),
            "{name} still exists in the tree; the cutover deletes it from the working tree too"
        );
    }
}

#[test]
fn the_workflows_use_no_python() {
    let workflows = repo().join(".github").join("workflows");
    let mut files = 0;
    for entry in fs::read_dir(&workflows)
        .unwrap_or_else(|error| panic!("read {}: {error}", workflows.display()))
    {
        let path = entry.expect("a workflow entry").path();
        if !path.is_file() {
            continue;
        }
        files += 1;
        let text = read(&path).to_lowercase();
        for needle in [
            "setup-python",
            "pip install",
            "pip3 install",
            "python -m",
            "python3 ",
            "python ",
        ] {
            assert!(
                !text.contains(needle),
                "{} contains `{needle}`; no Python may survive in CI",
                path.display()
            );
        }
    }
    assert!(files > 0, "no workflow files found under .github/workflows");

    // The handoff's workflow debt: fmt was red on a merged tip because the gates did
    // not run it. The layout rewrite is where the gate gains the step.
    let gates = read(&workflows.join("gates.yml"));
    assert!(
        gates.contains("cargo fmt --all --check"),
        "gates.yml does not run `cargo fmt --all --check`"
    );
}

// ---------------------- 2. every committed citation resolves in a fresh clone

/// The extensions a committed document cites as a *repository* path.
const EXTENSIONS: &[&str] = &[
    ".md", ".rs", ".ts", ".mjs", ".sh", ".json", ".jsonl", ".txt", ".toml", ".html", ".css",
    ".wasm", ".yml", ".yaml", ".png",
];

/// The directories this repository has, plus the ones the cutover deletes. A
/// citation whose first segment is one of these is a claim about *this* tree; a
/// citation of another project's file (`bot/src/dag.rs`, `proposals/simd/SIMD.md`)
/// is a source, not a path a clone must have. The deleted names stay in the list
/// on purpose: `prototype/t14-tree/gen_trace.py` is exactly the dangling citation
/// the ticket exists to remove, and dropping the name from the list would make the
/// scanner blind to it.
const ROOTS: &[&str] = &[
    ".github",
    ".scratch",
    "bench",
    "data",
    "docs",
    "engine",
    "engine-wasm",
    "host-probe",
    "prototype",
    "reference",
    "static",
    "templates",
    "web",
];

/// Is this backticked token a repository-relative file path?
///
/// Deliberately narrow. It has to be, because the documents also contain commands
/// (`cargo test -p bench ...`), build output relative to `web/dist`
/// (`assets/index-<hash>.js`), URL paths (`/index.html`) and templates
/// (`<config>/archive/<fingerprint>.txt`). The tokens that matter -- the ADR's
/// `.scratch/...` citations, the design notes' `prototype/...` citations, and the
/// ordinary `docs/...` references -- all have a slash, a known extension, and no
/// syntax marker in them.
fn is_path_citation(token: &str) -> bool {
    if !token.contains('/') || token.starts_with('/') {
        return false;
    }
    let Some(root) = token.split('/').next() else {
        return false;
    };
    if !ROOTS.contains(&root) {
        return false;
    }
    if token.contains([
        '<', '>', '{', '}', '*', '$', ':', '=', '#', '|', '\\', '"', '\'',
    ]) {
        return false;
    }
    if token.starts_with("assets/") || token.starts_with("dist/") {
        return false;
    }
    EXTENSIONS
        .iter()
        .any(|extension| token.ends_with(extension))
}

/// Every inline code span on a line. Fenced blocks are skipped by the caller: they
/// are commands to type, not citations of files the clone has.
fn spans(line: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else {
            break;
        };
        spans.push(&after[..close]);
        rest = &after[close + 1..];
    }
    spans
}

#[test]
fn every_committed_markdown_citation_resolves() {
    let mut misses = Vec::new();
    let mut cites = 0;

    for path in tracked().iter().filter(|path| path.ends_with(".md")) {
        let text = read(&repo().join(path));
        let mut in_fence = false;
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }

            for span in spans(line) {
                for word in span.split_whitespace() {
                    // Punctuation the prose puts around a citation, never part of it.
                    // A leading dot is kept: `.scratch/` is a real prefix, and
                    // `.github/` must not be shortened to a path that does not exist.
                    let token = word.trim_start_matches(['(', '"', '\'']);
                    let token = token.trim_end_matches([',', ';', ')', ':', '"', '\'']);
                    let token = token.strip_suffix('.').unwrap_or(token);
                    if !is_path_citation(token) {
                        continue;
                    }
                    cites += 1;
                    if !resolves(path, token) {
                        misses.push(format!("{path}:{}: `{token}`", number + 1));
                    }
                }
            }

            // Markdown links are the other way a document cites a file.
            let mut rest = line;
            while let Some(open) = rest.find("](") {
                let after = &rest[open + 2..];
                let Some(close) = after.find(')') else {
                    break;
                };
                let target = after[..close].trim();
                rest = &after[close + 1..];
                if target.starts_with("http")
                    || target.starts_with('#')
                    || target.starts_with('/')
                    || target.starts_with('<')
                    || target.contains('{')
                {
                    continue;
                }
                cites += 1;
                if !resolves(path, target) {
                    misses.push(format!("{path}:{}: link `{target}`", number + 1));
                }
            }
        }
    }

    assert!(
        cites >= 10,
        "only {cites} citations found; the scanner stopped seeing the documents"
    );
    assert!(
        misses.is_empty(),
        "{} committed citation(s) do not resolve in a fresh clone:\n  {}",
        misses.len(),
        misses.join("\n  ")
    );
}

// --------------------------- 3. the evidence survives under docs/

#[test]
fn the_evidence_lives_under_docs() {
    let research = [
        "01-language-and-toolchain.md",
        "02-free-static-host.md",
        "03-browser-compute-ceiling.md",
        "04-algorithm-frontier.md",
        "05-app-architecture.md",
        "06-bitboard-benchmark.md",
    ];
    for name in research {
        let path = repo().join("docs").join("research").join(name);
        let bytes = fs::metadata(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .len();
        assert!(bytes > 1_000, "{} is empty or truncated", path.display());
    }

    for name in [
        "t10-mockup.html",
        "t14-tree/index.html",
        "t14-tree/trace.js",
        "t14-tree/FINDINGS.md",
        "t14-tree/README.md",
    ] {
        let path = repo().join("docs").join("design").join(name);
        let bytes = fs::metadata(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .len();
        assert!(bytes > 1_000, "{} is empty or truncated", path.display());
    }

    let shots = repo()
        .join("docs")
        .join("design")
        .join("t14-tree")
        .join("shots");
    let pngs = fs::read_dir(&shots)
        .unwrap_or_else(|error| panic!("{}: {error}", shots.display()))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
        .count();
    assert!(pngs > 0, "{} holds no screenshot", shots.display());

    // The host probe's two checks, reborn under the web app (ticket 17's pair).
    for name in ["verify.sh", "browser-check.mjs"] {
        let path = repo().join("web").join("scripts").join(name);
        let bytes = fs::metadata(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .len();
        assert!(bytes > 1_000, "{} is missing or empty", path.display());
    }
}

// ------------------------------ 4. nothing under the Dataset may be ignored

#[test]
fn nothing_under_the_dataset_can_be_ignored() {
    let mut dataset = 0;
    for path in tracked() {
        if !path.starts_with("data/dataset/") {
            continue;
        }
        dataset += 1;
        assert!(
            !is_ignored(&path),
            "{path} is ignored; nothing under data/dataset/ may ever be ignored"
        );
    }
    assert!(dataset >= 6, "only {dataset} committed Dataset files found");

    // The archive a §6.2 export will create: the guard has to hold for a path that
    // does not exist yet, which is the one a `*.txt` rule would silently eat.
    let future = "data/dataset/goal8-p40-cap7/archive/4bf1a17b1685a774.txt";
    assert!(
        !is_ignored(future),
        "{future} would be ignored; an archived Dataset must stay committable"
    );
}

// --------------------------------------- 5. the deleted host is not cited

#[test]
fn the_deleted_host_is_not_cited() {
    let text_like = [
        ".md", ".rs", ".ts", ".mjs", ".sh", ".json", ".jsonl", ".txt", ".toml", ".html", ".css",
        ".yml", ".yaml", ".js",
    ];
    for path in tracked() {
        if !text_like.iter().any(|extension| path.ends_with(extension)) {
            continue;
        }
        // The two survivors that may name the host on purpose: the research note that
        // surveyed free hosts (it predates the decision and is evidence, not a
        // citation) and the banned-words list that exists to forbid the word.
        let allowed = path.starts_with("docs/research/") || path == "web/scripts/words.mjs";
        let text = read(&repo().join(&path)).to_lowercase();
        assert!(
            !text.contains("onrender.com"),
            "{path} cites the retired Render URL; the old host is torn down, not linked"
        );
        if !allowed {
            assert!(
                !text.contains("render.com"),
                "{path} names the retired host; only docs/research/ and the banned-words list may"
            );
        }
    }
}

// ------------------------------------- 6. the README is the developer entry

#[test]
fn the_readme_is_the_developer_entry() {
    let readme = read(&repo().join("README.md"));
    // Box 4 of the ticket, as strings: what the Lab is, the live URL, the folder
    // map, the three build/bench/export commands, deploy, and where the ADRs live.
    for needed in [
        "https://lab.samibentaleb.com",
        "engine/",
        "engine-wasm/",
        "bench/",
        "web/",
        "data/dataset/",
        "docs/adr/",
        "docs/research/",
        "docs/design/",
        "cargo run -p bench --release -- verify",
        "npm --prefix web run build",
        "npx wrangler deploy",
        "docs/adr/0001",
        "Explainer",
    ] {
        assert!(readme.contains(needed), "README.md never mentions {needed}");
    }
    assert!(
        !readme.contains("pip install -r requirements.txt"),
        "README.md still carries the Python-era install instructions"
    );
}
