//! #741 — tier-boundary warm run cache: hit/miss, invalidation, phases, signpost, budget.

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

fn median_ms(samples: &mut [u128]) -> u128 {
    samples.sort();
    samples[samples.len() / 2]
}

fn p90_ms(samples: &mut [u128]) -> u128 {
    samples.sort();
    let i = ((samples.len() as f64) * 0.9).ceil() as usize;
    samples[i.saturating_sub(1).min(samples.len() - 1)]
}

fn unique() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    std::process::id() as u64 * 1000 + N.fetch_add(1, Ordering::Relaxed)
}

fn peer_ms(argv: &[&str], cwd: Option<&Path>) -> Option<u128> {
    let mut samples = Vec::new();
    for _ in 0..5 {
        let t0 = Instant::now();
        let mut cmd = Command::new(argv[0]);
        cmd.args(&argv[1..]);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            return None;
        }
        samples.push(t0.elapsed().as_millis());
    }
    Some(median_ms(&mut samples))
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
    let _ = run_jet(&cache, &noop, Some(&fix), &[]);
    let mut warm = Vec::new();
    for _ in 0..5 {
        let t0 = Instant::now();
        let (code, _, err) = run_jet(&cache, &noop, Some(&fix), &[]);
        assert_eq!(code, 0, "{err}");
        warm.push(t0.elapsed().as_millis());
    }
    let warm_median = median_ms(&mut warm.clone());
    let warm_p90 = p90_ms(&mut warm);

    let bash = peer_ms(&["bash", "-c", "true"], None);
    let node = peer_ms(&["node", "-e", ""], None);
    let python = peer_ms(&["python3", "-c", "pass"], None)
        .or_else(|| peer_ms(&["python", "-c", "pass"], None));
    let bash_file = peer_ms(&["bash", "-c", "cat data.txt"], Some(&fix));
    let node_file = peer_ms(
        &[
            "node",
            "-e",
            "process.stdout.write(require('fs').readFileSync('data.txt','utf8'))",
        ],
        Some(&fix),
    );
    let bash_sub = peer_ms(&["bash", "-c", "true"], None);
    let node_sub = peer_ms(
        &["node", "-e", "require('child_process').execFileSync('true')"],
        None,
    );

    // D-SCRIPT-BUDGET1=B: warm Jet no-op median ≤ 2× fastest available peer median.
    let fastest_peer = [bash, python, node].into_iter().flatten().min();
    eprintln!(
        "script-start budget evidence: {} warm_jet_noop_median_ms={} warm_jet_noop_p90_ms={} \
         fastest_peer_ms={:?} budget_2x={:?} bash={:?} python={:?} node={:?} \
         bash_file={:?} node_file={:?} bash_sub={:?} node_sub={:?}",
        machine_meta(),
        warm_median,
        warm_p90,
        fastest_peer,
        fastest_peer.map(|p| p.saturating_mul(2)),
        bash,
        python,
        node,
        bash_file,
        node_file,
        bash_sub,
        node_sub
    );
    assert!(
        bash.is_some() || node.is_some() || python.is_some(),
        "need at least one peer runtime"
    );
    let peer = fastest_peer.expect("peer sample present");
    // Floor 1ms: peer spawn that rounds to 0ms must not force a zero budget.
    let budget = peer.max(1).saturating_mul(2);
    assert!(
        warm_median <= budget,
        "warm no-op median {warm_median}ms exceeds D-SCRIPT-BUDGET1=B gate (2× fastest peer {peer}ms = {budget}ms)"
    );
    assert!(
        bash_sub.is_some() || node_sub.is_some(),
        "subprocess peers must be measurable"
    );
}
