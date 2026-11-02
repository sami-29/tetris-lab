//! Ticket 07's split guard: **the trainer cannot read the test Split.**
//!
//! §6 puts both ranges in one file — seeds 0–999 test, 1,000–1,999 tuning — so
//! "cannot read the test Split" cannot be a file boundary. It is a type
//! boundary ([`TrainingSplit`] has one inhabitant and private fields) plus a
//! mechanical source lint over `bench/src/tuning/` plus an artifact audit
//! (`tuning_record.rs`). This file is the first two of those three; the
//! compile-fail half lives as doctests on the types themselves, because a
//! runtime test cannot prove that a constructor does not exist.

use std::path::{Path, PathBuf};

use bench::dataset::{self, Config, TrainingDataset, TrainingSplit, GOAL12};
use bench::fingerprint;
use bench::width::Bed;

/// FNV-1a-64 over the tuning Split's seeds as little-endian `u32`, in order.
///
/// The same number the trainer writes to a run's `seed_list_fnv`, so a run that
/// evaluated any seed outside the range cannot produce a matching record.
const TUNING_SEED_LIST_FNV: &str = "0x45ff8c50e8139615";

fn data() -> PathBuf {
    dataset::root()
}

fn tuning() -> TrainingSplit {
    TrainingSplit::tuning()
}

#[test]
fn the_tuning_split_is_exactly_the_second_thousand_seeds() {
    let split = tuning();
    assert_eq!(split.first(), 1_000);
    assert_eq!(split.last(), 1_999);
    assert_eq!(split.len(), 1_000);
    let seeds: Vec<u32> = split.seeds().collect();
    assert_eq!(seeds.len(), 1_000);
    assert_eq!(seeds[0], 1_000);
    assert_eq!(seeds[999], 1_999);
    assert!(seeds.windows(2).all(|pair| pair[1] == pair[0] + 1));
}

#[test]
fn the_split_rejects_every_test_seed() {
    let split = tuning();
    for seed in [0, 1, 500, 998, 999] {
        assert!(!split.contains(seed), "seed {seed} is a test seed");
    }
    for seed in [1_000, 1_500, 1_999] {
        assert!(split.contains(seed), "seed {seed} is a tuning seed");
    }
    assert!(!split.contains(2_000));
}

#[test]
fn a_training_dataset_holds_only_tuning_seeds() {
    let dataset = TrainingDataset::load(&data(), &GOAL12, &tuning()).expect("load the tuning games");
    let games = dataset.games();
    assert_eq!(games.len(), 1_000);
    assert!(games.iter().all(|game| tuning().contains(game.seed)));
    assert!(games.iter().all(|game| game.seed >= 1_000));
    // Ascending, so the row order is the seed order and a row cannot move.
    assert!(games.windows(2).all(|pair| pair[1].seed == pair[0].seed + 1));
}

#[test]
fn the_seed_list_fingerprint_is_the_committed_golden() {
    let dataset = TrainingDataset::load(&data(), &GOAL12, &tuning()).expect("load the tuning games");
    assert_eq!(dataset.seed_list_fnv(), TUNING_SEED_LIST_FNV);
    // And it is the same number computed straight from the split, so the golden
    // is a property of §6's range and not of the file's line order.
    let mut fnv = fingerprint::Fnv::new();
    for seed in tuning().seeds() {
        fnv.u32_le(seed);
    }
    assert_eq!(fingerprint::hex(fnv.finish()), TUNING_SEED_LIST_FNV);
}

/// The belt to the type's braces: a `games.txt` that *does* carry a test seed
/// still yields none, because the loader filters by the split before it builds
/// anything the trainer can name.
#[test]
fn an_injected_test_seed_never_reaches_the_trainer() {
    let source = dataset::games_path(&data(), &GOAL12);
    let text = std::fs::read_to_string(&source).expect("read games.txt");
    let mut lines = text.lines();
    let header = lines.next().expect("a header line").to_string();
    // Keep only tuning seeds, then inject three test seeds at the front.
    let tuning_lines: Vec<&str> = lines
        .filter(|line| {
            line.split_whitespace()
                .next()
                .and_then(|seed| seed.parse::<u32>().ok())
                .is_some_and(|seed| tuning().contains(seed))
        })
        .collect();
    let injected: Vec<String> = ["0", "453", "999"]
        .iter()
        .filter_map(|seed| {
            text.lines()
                .find(|line| line.split_whitespace().next() == Some(*seed))
                .map(|line| line.to_string())
        })
        .collect();
    assert_eq!(injected.len(), 3, "the committed file does carry test seeds");

    let mut poisoned = String::from(&header);
    poisoned.push('\n');
    for line in injected.iter().chain(tuning_lines.iter()) {
        poisoned.push_str(line);
        poisoned.push('\n');
    }

    let dir = tempdir("tuning-split");
    let cfg = Config {
        dir: "goal12-p100-cap7",
        goal: 12,
        pieces: 100,
        cap: 7,
    };
    std::fs::create_dir_all(dir.join(cfg.dir)).expect("make the config directory");
    std::fs::write(dir.join(cfg.dir).join("games.txt"), &poisoned).expect("write the poisoned file");

    // The raw reader does see them -- which is exactly why the trainer is not
    // allowed to name it.
    let raw = dataset::parse(&cfg, &poisoned).expect("the poisoned file still parses");
    assert!(raw.games.iter().any(|game| game.seed == 453));

    let dataset = TrainingDataset::load(&dir, &cfg, &tuning()).expect("load through the guard");
    assert_eq!(dataset.games().len(), 1_000);
    assert!(
        dataset.games().iter().all(|game| game.seed >= 1_000),
        "a test seed reached the trainer"
    );
    assert!(!dataset.games().iter().any(|game| game.seed == 453));
    assert_eq!(dataset.seed_list_fnv(), TUNING_SEED_LIST_FNV);

    let _ = std::fs::remove_dir_all(&dir);
}

/// The validation-only bed of §9.1 is generated, not committed, so it has no
/// file to filter. It is still reachable only through the same guard: every seed
/// it generates comes from `split.seeds()`.
#[test]
fn a_generated_training_dataset_is_also_sealed() {
    let dataset = TrainingDataset::generate(&Bed::GOAL16, &tuning()).expect("generate the bed");
    assert_eq!(dataset.games().len(), 1_000);
    assert!(dataset.games().iter().all(|game| tuning().contains(game.seed)));
    assert_eq!(dataset.seed_list_fnv(), TUNING_SEED_LIST_FNV);
}

/// The mechanical half of the seal: `bench/src/tuning/` may not name the raw
/// reader, the test-split constant, or the file that holds both splits.
///
/// This is a lint, not a proof — a determined edit inside `dataset.rs` can move
/// the constant — but it makes the escape hatch something you have to *write*,
/// in a file whose whole job is the tuning Split, rather than something a
/// reviewer has to notice.
#[test]
fn the_tuning_module_names_no_escape_hatch() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("tuning");
    let mut files: Vec<PathBuf> = Vec::new();
    collect(&root, &mut files);
    assert!(!files.is_empty(), "no sources under {}", root.display());

    // Each entry: a token, and why it is forbidden there.
    let forbidden: [(&str, &str); 7] = [
        ("dataset::parse", "the raw reader sees both splits"),
        ("dataset::games_path", "the file holds seeds 0-999"),
        ("games.txt", "the file holds seeds 0-999"),
        ("TEST_SPLIT", "the test Split's range"),
        ("SEED_FIRST", "the Dataset's first seed is a test seed"),
        ("0..=999", "a test-split range written out"),
        ("..999", "a test-split upper bound written out"),
    ];
    for file in &files {
        let text = std::fs::read_to_string(file).expect("read a tuning source");
        for (token, why) in forbidden {
            assert!(
                !text.contains(token),
                "{}: the tuning module must not name {token:?} ({why})",
                file.display()
            );
        }
        // `Dataset` is fine only as `TrainingDataset`.
        for (index, _) in text.match_indices("Dataset") {
            let prefix = &text[..index];
            assert!(
                prefix.ends_with("Training"),
                "{}: bare `Dataset` at byte {index} (the raw type owns both splits)",
                file.display()
            );
        }
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// `std::env::temp_dir` plus the process id: no `tempfile` dependency (`bench`
/// has none by design), and no two test binaries can collide.
fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tetris-lab-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("make a temporary directory");
    dir
}
