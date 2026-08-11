//! #741 — tier-boundary warm run cache: hit/miss, invalidation, phases, signpost, budget.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Process-global env / phase counters — serialize in-process tests.
fn lock_run_cache_tests() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn jet_bin() -> String {
    std::env::var("CARGO_BIN_EXE_jet").unwrap_or_else(|_| {
        let mut p = std::env::current_exe().expect("exe");
        p.pop();
        p.push("jet");
        p.display().to_string()
    })
}

/// Product binary for D-SCRIPT-BUDGET1 peer-ratio gate.
/// Debug `CARGO_BIN_EXE_jet` is ~9× larger and fails B on load cost alone;
/// prefer `JET_BUDGET_BIN` or `target/release/jet` when present.
fn budget_jet_bin() -> Option<String> {
    if let Ok(p) = std::env::var("JET_BUDGET_BIN") {
        if Path::new(&p).is_file() {
            return Some(p);
        }
    }
    let release = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/release/jet");
    if release.is_file() {
        return Some(release.display().to_string());
    }
    None
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/script_speed")
}

fn run_jet(
    cache: &Path,
    file: &Path,
    cwd: Option<&Path>,
    extra_env: &[(&str, &str)],
) -> (i32, String, String) {
    let mut cmd = Command::new(jet_bin());
    cmd.arg("run").arg(file).env("JET_RUN_CACHE_DIR", cache);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("jet run");
    (
        out.status.code().unwrap_or(1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Process-spawn only (no pipe capture) — same shape as peer_us for budget compares.
fn run_jet_status_bin(bin: &str, cache: &Path, file: &Path, cwd: Option<&Path>) -> bool {
    let mut cmd = Command::new(bin);
    cmd.arg("run")
        .arg(file)
        .env("JET_RUN_CACHE_DIR", cache)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

fn median_u(samples: &mut [u128]) -> u128 {
    samples.sort();
    samples[samples.len() / 2]
}

fn p90_u(samples: &mut [u128]) -> u128 {
    samples.sort();
    let i = ((samples.len() as f64) * 0.9).ceil() as usize;
    samples[i.saturating_sub(1).min(samples.len() - 1)]
}

fn us_as_ms(us: u128) -> f64 {
    us as f64 / 1000.0
}

fn unique() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    std::process::id() as u64 * 1000 + N.fetch_add(1, Ordering::Relaxed)
}

/// Peer process-spawn median in microseconds (avoids as_millis truncation noise).
fn peer_us(argv: &[&str], cwd: Option<&Path>) -> Option<u128> {
    let mut samples = Vec::new();
    // One discard + 7 timed samples — same shape as warm Jet budget loop.
    for i in 0..8 {
        let t0 = Instant::now();
        let mut cmd = Command::new(argv[0]);
        cmd.args(&argv[1..])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            return None;
        }
        if i > 0 {
            samples.push(t0.elapsed().as_micros());
        }
    }
    Some(median_u(&mut samples))
}

fn machine_meta() -> String {
    format!(
        "os={} arch={} cpus={:?} host={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().ok().map(|n| n.get()),
        std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("HOST"))
            .unwrap_or_else(|_| "unknown".into())
    )
}

#[test]
fn warm_hit_skips_front_end_and_matches_stdout() {
    let _guard = lock_run_cache_tests();
    let root = std::env::temp_dir().join(format!("jet_run_cache_{}", unique()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("hi.jet");
    std::fs::write(&file, "fn run() {\n    print(\"warm-ok\")\n}\n").unwrap();
    let cache = root.join("cache");

    let (c1, out1, err1) = run_jet(&cache, &file, None, &[("JET_RUN_TRACE", "1")]);
    assert_eq!(c1, 0, "cold failed: {err1}");
    assert_eq!(out1, "warm-ok\n");
    assert!(err1.contains("[run-cache] store"), "expected store: {err1}");

    let (c2, out2, err2) = run_jet(&cache, &file, None, &[("JET_RUN_TRACE", "1")]);
    assert_eq!(c2, 0, "warm failed: {err2}");
    assert_eq!(out2, "warm-ok\n");
    assert!(err2.contains("[run-cache] hit"), "expected hit: {err2}");

    std::fs::write(&file, "fn run() {\n    print(\"changed\")\n}\n").unwrap();
    let (c3, out3, err3) = run_jet(&cache, &file, None, &[("JET_RUN_TRACE", "1")]);
    assert_eq!(c3, 0, "re-cold failed: {err3}");
    assert_eq!(out3, "changed\n");
    assert!(
        err3.contains("[run-cache] store"),
        "expected re-store after edit: {err3}"
    );
}

#[test]
fn dependency_and_argv_invalidate_cache_key() {
    let _guard = lock_run_cache_tests();
    // WatchService discover folds import deps into the key even when ModuleCall
    // still gaps at run time — content / argv change must flip the key.
    let root = std::env::temp_dir().join(format!("jet_run_dep_{}", unique()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let dep = root.join("dep.jet");
    let main = root.join("main.jet");
    std::fs::write(&dep, "pub fn greet() {\n    print(\"v1\")\n}\n").unwrap();
    std::fs::write(&main, "use dep\nfn run() {\n    dep.greet()\n}\n").unwrap();

    let k1 = jet::RunCache::run_cache_key(&main, &[]);
    std::fs::write(&dep, "pub fn greet() {\n    print(\"v2\")\n}\n").unwrap();
    let k2 = jet::RunCache::run_cache_key(&main, &[]);
    assert_ne!(k1, k2, "dependency content must invalidate run-cache key");

    let k3 = jet::RunCache::run_cache_key(&main, &["--x"]);
    assert_ne!(k2, k3, "argv must participate in run-cache key");
}

#[test]
fn argv_change_misses_cache() {
    let _guard = lock_run_cache_tests();
    let root = std::env::temp_dir().join(format!("jet_run_argv_{}", unique()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("a.jet");
    std::fs::write(&file, "fn run() {\n    print(\"ok\")\n}\n").unwrap();
    let cache = root.join("cache");

    let out = Command::new(jet_bin())
        .arg("run")
        .arg(&file)
        .arg("--")
        .arg("one")
        .env("JET_RUN_CACHE_DIR", &cache)
        .env("JET_RUN_TRACE", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("[run-cache] store"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out2 = Command::new(jet_bin())
        .arg("run")
        .arg(&file)
        .arg("--")
        .arg("two")
        .env("JET_RUN_CACHE_DIR", &cache)
        .env("JET_RUN_TRACE", "1")
        .output()
        .unwrap();
    let err2 = String::from_utf8_lossy(&out2.stderr);
    assert!(out2.status.success(), "{err2}");
    assert!(
        err2.contains("[run-cache] store"),
        "argv change must miss: {err2}"
    );
}

#[test]
fn phases_zero_on_inprocess_warm_hit() {
    let _guard = lock_run_cache_tests();
    let root = std::env::temp_dir().join(format!("jet_run_phases_{}", unique()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("p.jet");
    std::fs::write(&file, "fn run() {\n    print(1)\n}\n").unwrap();
    let cache = root.join("cache");
    std::env::set_var("JET_RUN_CACHE_DIR", &cache);

    jet::RunCache::reset_phases();
    let cold = jet::Interpreter::run_jit_once(file.to_str().unwrap());
    assert!(matches!(cold, jet::Interpreter::RunOutcome::Ran { .. }));
    let after_cold = jet::RunCache::phases();
    assert!(
        after_cold.cache_misses >= 1 || after_cold.parse >= 1,
        "cold must miss or parse: {after_cold:?}"
    );

    jet::RunCache::reset_phases();
    let warm = jet::Interpreter::run_jit_once(file.to_str().unwrap());
    match warm {
        jet::Interpreter::RunOutcome::Ran { stdout, .. } => {
            assert_eq!(stdout, "1\n");
        }
        other => panic!("warm problems: {other:?}"),
    }
    let after_warm = jet::RunCache::phases();
    assert_eq!(after_warm.parse, 0, "warm must skip parse: {after_warm:?}");
    assert_eq!(after_warm.check, 0, "warm must skip check: {after_warm:?}");
    assert_eq!(after_warm.lower, 0, "warm must skip lower: {after_warm:?}");
    assert_eq!(after_warm.codegen, 0, "warm must skip codegen: {after_warm:?}");
    assert_eq!(after_warm.link, 0, "warm must skip link: {after_warm:?}");
    assert!(
        after_warm.cache_hits >= 1,
        "expected cache hit: {after_warm:?}"
    );

    std::env::remove_var("JET_RUN_CACHE_DIR");
}

#[test]
fn signpost_respects_tty_nocolor_json_and_once() {
    let _guard = lock_run_cache_tests();
    jet::RunCache::reset_signpost_for_test();
    let started = Instant::now() - Duration::from_millis(500);
    assert!(
        !jet::RunCache::signpost_eligible(started, false),
        "non-tty must stay silent"
    );
    std::env::set_var("NO_COLOR", "1");
    assert!(!jet::RunCache::signpost_eligible(started, true));
    std::env::remove_var("NO_COLOR");
    std::env::set_var("JET_JSON", "1");
    assert!(!jet::RunCache::signpost_eligible(started, true));
    std::env::remove_var("JET_JSON");
    assert!(jet::RunCache::signpost_eligible(started, true));
    jet::RunCache::maybe_signpost(started, true);
    assert!(
        !jet::RunCache::signpost_eligible(started, true),
        "once-guard must block repeat"
    );
    assert!(jet::RunCache::signpost_line().contains("`jet dev`"));
    jet::RunCache::reset_signpost_for_test();
}

#[test]
fn default_run_stays_jit_module_cache_not_aot() {
    let _guard = lock_run_cache_tests();
    let root = std::env::temp_dir().join(format!("jet_jit_not_aot_{}", unique()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("x.jet");
    std::fs::write(&file, "fn run() {\n    print(\"jit\")\n}\n").unwrap();
    let cache = root.join("cache");
    let (c1, _, err1) = run_jet(&cache, &file, None, &[("JET_RUN_TRACE", "1")]);
    assert_eq!(c1, 0, "{err1}");
    let (c2, out2, err2) = run_jet(&cache, &file, None, &[("JET_RUN_TRACE", "1")]);
    assert_eq!(c2, 0, "{err2}");
    assert_eq!(out2, "jit\n");
    assert!(err2.contains("[run-cache] hit"), "{err2}");
    let has_module = std::fs::read_dir(&cache)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .any(|e| e.path().join("module.bin").is_file());
    assert!(has_module, "expected module.bin under run cache");
}

#[test]
fn script_start_budget_fixtures_and_peers() {
    let _guard = lock_run_cache_tests();
    let fix = fixture_dir();
    let cache = std::env::temp_dir().join(format!("jet_run_fix_{}", unique()));
    let _ = std::fs::remove_dir_all(&cache);

    let hello = fix.join("hello.jet");
    let (c1, out1, err1) = run_jet(&cache, &hello, Some(&fix), &[("JET_RUN_TRACE", "1")]);
    assert_eq!(c1, 0, "{err1}");
    assert_eq!(out1, "script-speed-hello\n");
    assert!(err1.contains("[run-cache] store"), "{err1}");
    let (c2, out2, err2) = run_jet(&cache, &hello, Some(&fix), &[("JET_RUN_TRACE", "1")]);
    assert_eq!(c2, 0, "{err2}");
    assert_eq!(out2, "script-speed-hello\n");
    assert!(err2.contains("[run-cache] hit"), "{err2}");

    let file = fix.join("file_read.jet");
    let (fc, fout, ferr) = run_jet(&cache, &file, Some(&fix), &[]);
    assert_eq!(fc, 0, "{ferr}");
    let (fw, fout2, ferr2) = run_jet(&cache, &file, Some(&fix), &[]);
    assert_eq!(fw, 0, "{ferr2}");
    assert_eq!(fout, fout2);

    let json = fix.join("json_text.jet");
    let (jc, jout, jerr) = run_jet(&cache, &json, Some(&fix), &[]);
    assert_eq!(jc, 0, "{jerr}");
    let (jw, jout2, jerr2) = run_jet(&cache, &json, Some(&fix), &[]);
    assert_eq!(jw, 0, "{jerr2}");
    assert_eq!(jout, jout2);

    let sub = fix.join("subprocess.jet");
    let (sc, sout, serr) = run_jet(&cache, &sub, Some(&fix), &[]);
    assert_eq!(sc, 0, "{serr}");
    assert_eq!(sout.trim(), "0");

    let noop = fix.join("noop.jet");
    // D-SCRIPT-BUDGET1=B measures the product binary (release), not debug test exe.
    let budget_bin = budget_jet_bin();
    let using_product = budget_bin.is_some();
    let timed_bin = budget_bin.unwrap_or_else(jet_bin);
    assert!(
        run_jet_status_bin(&timed_bin, &cache, &noop, Some(&fix)),
        "noop populate failed ({timed_bin})"
    );
    let mut warm = Vec::new();
    for _ in 0..7 {
        let t0 = Instant::now();
        assert!(
            run_jet_status_bin(&timed_bin, &cache, &noop, Some(&fix)),
            "warm noop status failed"
        );
        warm.push(t0.elapsed().as_micros());
    }
    let warm_median_us = median_u(&mut warm.clone());
    let warm_p90_us = p90_u(&mut warm);

    let bash = peer_us(&["bash", "-c", "true"], None);
    let node = peer_us(&["node", "-e", ""], None);
    let python = peer_us(&["python3", "-c", "pass"], None)
        .or_else(|| peer_us(&["python", "-c", "pass"], None));
    let bash_file = peer_us(&["bash", "-c", "cat data.txt"], Some(&fix));
    let node_file = peer_us(
        &[
            "node",
            "-e",
            "process.stdout.write(require('fs').readFileSync('data.txt','utf8'))",
        ],
        Some(&fix),
    );
    let bash_sub = peer_us(&["bash", "-c", "true"], None);
    let node_sub = peer_us(
        &["node", "-e", "require('child_process').execFileSync('true')"],
        None,
    );

    // D-SCRIPT-BUDGET1=B: warm Jet no-op median ≤ 2× fastest available peer median.
    // µs clocks avoid as_millis truncation (bash ~2ms was recorded as 1ms).
    let fastest_us = [bash, python, node].into_iter().flatten().min();
    let budget_us = fastest_us.map(|p| p.saturating_mul(2));
    eprintln!(
        "script-start budget evidence: {} bin={} product={} warm_jet_noop_median_ms={} \
         warm_jet_noop_p90_ms={} fastest_peer_ms={:?} budget_2x_ms={:?} bash_ms={:?} \
         python_ms={:?} node_ms={:?} bash_file_ms={:?} node_file_ms={:?} bash_sub_ms={:?} \
         node_sub_ms={:?}",
        machine_meta(),
        timed_bin,
        using_product,
        us_as_ms(warm_median_us),
        us_as_ms(warm_p90_us),
        fastest_us.map(us_as_ms),
        budget_us.map(us_as_ms),
        bash.map(us_as_ms),
        python.map(us_as_ms),
        node.map(us_as_ms),
        bash_file.map(us_as_ms),
        node_file.map(us_as_ms),
        bash_sub.map(us_as_ms),
        node_sub.map(us_as_ms)
    );
    assert!(
        bash.is_some() || node.is_some() || python.is_some(),
        "need at least one peer runtime"
    );
    assert!(
        bash_sub.is_some() || node_sub.is_some(),
        "subprocess peers must be measurable"
    );
    if using_product {
        let peer = fastest_us.expect("peer sample present");
        let budget = budget_us.expect("peer budget");
        assert!(
            warm_median_us <= budget,
            "warm no-op median {}ms exceeds D-SCRIPT-BUDGET1=B (≤2× fastest peer {}ms = {}ms)",
            us_as_ms(warm_median_us),
            us_as_ms(peer),
            us_as_ms(budget)
        );
    } else {
        // Debug test exe is not the product binary; keep absolute sanity until
        // `cargo build --release` (or JET_BUDGET_BIN) is available for B.
        assert!(
            warm_median_us < 100_000,
            "warm no-op median {}ms exceeds 100ms debug sanity (build release jet to enforce D-SCRIPT-BUDGET1=B)",
            us_as_ms(warm_median_us)
        );
        eprintln!(
            "note: D-SCRIPT-BUDGET1=B hard gate skipped — no target/release/jet or JET_BUDGET_BIN"
        );
    }
}
