//! Tower #85 §0: regression test for the content-cache poisoning race.
//!
//! The bug: `jet` compiled straight onto the shared, non-content-addressed
//! display path `build/<stem>`, then `store_cached(key, build/<stem>)` copied
//! *from* it. Two concurrent processes compiling different source that share a
//! file stem (`main.jet` in different dirs, run from a shared cwd) both target
//! `build/main`; process B could overwrite it between A's rustc finishing and
//! A's `store_cached`, so A's key ended up mapped to B's binary in the shared
//! cache — a content-addressing integrity violation.
//!
//! The fix compiles to a private per-process path and `store_cached`s from
//! *that*, so the cache is always correct regardless of the display-path race.
//! This test drives N concurrent compiles that collide on `build/main` and a
//! shared cache, then verifies each source's cache entry still returns *its own*
//! binary (a poisoned entry would print another program's output on the hit).

use std::path::PathBuf;
use std::process::Command;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

const N: usize = 8;

#[test]
fn concurrent_distinct_sources_sharing_a_stem_do_not_poison_the_cache() {
    let root = std::env::temp_dir().join(format!("jet-cache-race-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    // One shared cache and one shared cwd: every process's `build/main` display
    // path collides, maximally reproducing the original race.
    let cache = root.join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let shared_cwd = root.join("cwd");
    std::fs::create_dir_all(&shared_cwd).unwrap();

    // N distinct programs, each in its own dir but all named `main.jet`.
    let mut files = Vec::new();
    for i in 0..N {
        let dir = root.join(format!("src{i}"));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("main.jet");
        std::fs::write(&f, format!("fn run() {{\n    print(\"prog-{i}\");\n}}\n")).unwrap();
        files.push(f);
    }

    // Fire all N `jet run` invocations concurrently from the shared cwd against
    // the shared cache. Under a shared display path the *executed* binary is
    // itself racy, so we only require each process to succeed here; integrity is
    // checked below on the cache entries, which the fix must keep correct.
    let handles: Vec<_> = files
        .iter()
        .cloned()
        .map(|f| {
            let cwd = shared_cwd.clone();
            let cache = cache.clone();
            std::thread::spawn(move || {
                Command::new(jet())
                    .arg("run")
                    .arg(&f)
                    .current_dir(&cwd)
                    .env("JET_CACHE_DIR", &cache)
                    .output()
                    .expect("spawn jet run")
            })
        })
        .collect();
    for h in handles {
        let out = h.join().expect("thread");
        assert!(
            out.status.success(),
            "a concurrent `jet run` failed:\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // Integrity check: re-run each source in its OWN isolated cwd against the
    // same shared cache. Each is now a cache hit; a poisoned entry (some other
    // program's binary stored under this source's key) would print the wrong
    // marker. Every entry must return exactly its own program.
    for (i, f) in files.iter().enumerate() {
        let iso = root.join(format!("verify{i}"));
        std::fs::create_dir_all(&iso).unwrap();
        let out = Command::new(jet())
            .arg("run")
            .arg(f)
            .current_dir(&iso)
            .env("JET_CACHE_DIR", &cache)
            .output()
            .expect("spawn jet run (verify)");
        assert!(
            out.status.success(),
            "verify run {i} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(&format!("prog-{i}")),
            "cache entry for source {i} returned the wrong binary — expected `prog-{i}`, got: {stdout:?}"
        );
    }

    std::fs::remove_dir_all(&root).ok();
}
