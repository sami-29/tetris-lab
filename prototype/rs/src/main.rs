//! Native benchmark binary for the ticket 06 spike.
//!
//!   bench --games prototype/data/games_goal8_p40_seed0-999.txt --goal 8 \
//!         --out prototype/data/rs-u32.jsonl [--check data/py-asis.jsonl]

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use t06::W;

struct Args {
    games: PathBuf,
    goal: u32,
    out: Option<PathBuf>,
    limit: Option<usize>,
    check: Option<PathBuf>,
    warmup: usize,
}

fn parse_args() -> Args {
    let mut args = Args {
        games: PathBuf::from("prototype/data/games_goal8_p40_seed0-999.txt"),
        goal: 8,
        out: None,
        limit: None,
        check: None,
        warmup: 1,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || it.next().expect("missing value");
        match flag.as_str() {
            "--games" => args.games = PathBuf::from(val()),
            "--goal" => args.goal = val().parse().expect("goal"),
            "--out" => args.out = Some(PathBuf::from(val())),
            "--limit" => args.limit = Some(val().parse().expect("limit")),
            "--check" => args.check = Some(PathBuf::from(val())),
            "--warmup" => args.warmup = val().parse().expect("warmup"),
            other => panic!("unknown flag {other}"),
        }
    }
    args
}

/// Minimal reader for the Python JSONL rows we validate against.
fn read_expected(path: &PathBuf) -> Vec<(u32, bool, u32)> {
    let text = std::fs::read_to_string(path).expect("python jsonl");
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let field = |key: &str| -> String {
            let pat = format!("\"{key}\":");
            let start = line.find(&pat).unwrap_or_else(|| panic!("{key} missing")) + pat.len();
            line[start..]
                .trim_start()
                .split([',', '}'])
                .next()
                .expect("value")
                .trim()
                .to_string()
        };
        let seed: u32 = field("seed").parse().expect("seed");
        let success = field("success") == "true";
        let evals: u32 = field("evals").parse().expect("evals");
        out.push((seed, success, evals));
    }
    out
}

fn main() {
    let args = parse_args();
    let rots = t06::rots();
    let text = std::fs::read_to_string(&args.games).expect("games file");
    let mut games = t06::games::parse::<W>(&text);
    if let Some(n) = args.limit {
        games.truncate(n);
    }
    assert!(!games.is_empty(), "no games parsed");

    // Warm up caches and the branch predictor, then drop the samples.
    for g in games.iter().take(args.warmup) {
        let _ = g.solve(rots, args.goal);
    }

    let variant = t06::variant();
    let mut rows: Vec<(u32, bool, u32, f64)> = Vec::with_capacity(games.len());
    let t_all = Instant::now();
    for g in games.iter() {
        let t0 = Instant::now();
        let r = g.solve(rots, args.goal);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        rows.push((g.seed, r.success, r.evals, ms));
    }
    let wall = t_all.elapsed().as_secs_f64();

    let evals_total: u64 = rows.iter().map(|r| r.2 as u64).sum();
    let successes = rows.iter().filter(|r| r.1).count();
    let mut times: Vec<f64> = rows.iter().map(|r| r.3).collect();
    times.sort_by(|a, b| a.partial_cmp(b).expect("nan"));

    if let Some(out) = &args.out {
        if let Some(dir) = out.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut fh = std::io::BufWriter::new(std::fs::File::create(out).expect("out file"));
        for (seed, success, evals, ms) in rows.iter() {
            writeln!(
                fh,
                "{{\"solver\":\"rs-{variant}\",\"seed\":{seed},\"goal\":{},\"pieces\":{},\"success\":{success},\"evals\":{evals},\"ms\":{ms:.5}}}",
                args.goal,
                games[0].seq.len()
            )
            .expect("write row");
        }
    }

    let pct = |p: f64| times[((times.len() - 1) as f64 * p).round() as usize];
    let summary = format!(
        "{{\"solver\":\"rs-{variant}\",\"games\":{},\"goal\":{},\"success_rate\":{:.4},\"evals_total\":{},\"evals_per_s\":{:.1},\"games_per_s\":{:.2},\"ms_median\":{:.4},\"ms_mean\":{:.4},\"ms_p90\":{:.4},\"ms_total\":{:.1}}}",
        rows.len(),
        args.goal,
        successes as f64 / rows.len() as f64,
        evals_total,
        evals_total as f64 / wall,
        rows.len() as f64 / wall,
        pct(0.5),
        times.iter().sum::<f64>() / times.len() as f64,
        pct(0.9),
        wall * 1000.0
    );
    println!("{summary}");
    if let Some(out) = &args.out {
        let summary_path = out.with_extension("summary.json");
        std::fs::write(&summary_path, &summary).expect("summary write");
    }

    if let Some(check) = &args.check {
        let expected = read_expected(check);
        let mut bad_success = 0;
        let mut bad_evals = 0;
        for (seed, success, evals, _) in rows.iter() {
            let Some((_, e_success, e_evals)) = expected.iter().find(|e| e.0 == *seed) else {
                continue;
            };
            if *e_success != *success {
                bad_success += 1;
                eprintln!("seed {seed}: success {success} vs python {e_success}");
            }
            if *e_evals != *evals {
                bad_evals += 1;
                eprintln!("seed {seed}: evals {evals} vs python {e_evals}");
            }
        }
        println!(
            "{{\"check\":\"{}\",\"success_mismatches\":{bad_success},\"eval_mismatches\":{bad_evals}}}",
            check.display()
        );
    }
}
