//! D-ALLOC2 / D-REGION1 (c05): real bump-allocated arenas with scope-bound
//! regions. Covers the soundness contract end to end:
//!   * alloc-and-use compiles and runs;
//!   * a view that escapes its region is E0631 (Jet rejects first, I2);
//!   * a view used after the arena is reset is E0632;
//!   * an explicit `#Region(r) { … }` may span two allocators.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

/// Unique temp dir per call. Keying only on PID let parallel tests clobber a
/// shared `fixture.jet`, so a test compiled another's source — flaky races.
static SEQ: AtomicU64 = AtomicU64::new(0);
fn unique_tmp() -> std::path::PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("jet_arena_{}_{}", std::process::id(), n))
}

/// Write the fixture to a real temp file so `use core.mem` resolves like a
/// normal build, then return the diagnostic codes (empty = clean).
fn error_codes(src: &str) -> Vec<String> {
    let dir = unique_tmp();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.jet");
    std::fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    match jet::compile_with_path(src, &shown) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.into_iter().map(|d| d.code.to_string()).collect(),
    }
}

/// Compile to Rust and (if rustc is available) build + run, returning stdout.
/// This is the I2 backstop: the runtime `&mut self`/`self` signatures must also
/// accept what Jet accepted.
fn build_and_run(name: &str, src: &str) -> Option<String> {
    let dir0 = unique_tmp();
    std::fs::create_dir_all(&dir0).unwrap();
    let fpath = dir0.join("fixture.jet");
    std::fs::write(&fpath, src).unwrap();
    let out = jet::compile_with_path(src, &fpath.to_string_lossy()).unwrap_or_else(|d| {
        panic!(
            "front end rejected a should-compile fixture: {:?}",
            d.iter().map(|x| x.code.as_str()).collect::<Vec<_>>()
        )
    });
    // No `unsafe` may leak outside the vetted prelude helpers (I1 / D-LL1).
    // Strip every canonical vetted region (jet_mem, jet_term_unix/windows, and
    // the rest of the list) before checking for `unsafe` in user code.
    let user = common::strip_vetted_prelude_modules(&out.rust);
    assert!(
        user.lines()
            .all(|line| common::unsafe_keyword_columns(line).is_empty()),
        "`unsafe` leaked outside the vetted prelude helpers"
    );

    if !common::have_rustc() {
        eprintln!("note: rustc not found; compiled front end only");
        return None;
    }
    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_arena_{}.rs", name));
    let bin = dir.join(format!("jet_arena_{}", name));
    std::fs::write(&rs, &out.rust).unwrap();
    let c = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        c.status.success(),
        "I2 violated: rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&c.stderr)
    );
    let r = Command::new(&bin).output().unwrap();
    Some(String::from_utf8_lossy(&r.stdout).to_string())
}

#[test]
fn alloc_and_use_compiles_and_runs() {
    let src = r#"
use core.mem

fn run() {
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    x = 43
    y :: arena.alloc(100)
    print(x)
    print(y)
    arena.reset()
    z :: arena.alloc(7)
    print(z)
}
"#;
    assert_eq!(
        error_codes(src),
        Vec::<String>::new(),
        "alloc-and-use should compile clean"
    );
    if let Some(out) = build_and_run("ok", src) {
        assert_eq!(out, "43\n100\n7\n");
    }
    tir_support::assert_tiers_agree("arena_view_write", src, "43\n100\n7\n");
    tir_support::assert_example_cli_tiers_agree(
        "memory/arena",
        include_str!("../examples/features/expected/memory/arena.out"),
    );
    tir_support::assert_example_cli_tiers_agree(
        "memory/try_allocation",
        include_str!("../examples/features/expected/memory/try_allocation.out"),
    );
}

#[test]
fn view_escape_is_e0631() {
    let src = r#"
use core.mem

fn make() Int -> {
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    return x
}
"#;
    assert_eq!(
        error_codes(src),
        vec!["E0631".to_string()],
        "returning an arena view must report exactly E0631"
    );
}

#[test]
fn view_stored_in_binding_is_e0631() {
    let src = r#"
use core.mem

fn run() {
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    stash :: x
    print(stash)
}
"#;
    assert_eq!(
        error_codes(src),
        vec!["E0212".to_string()],
        "moving an owner-backed view while it remains live must report exactly E0212"
    );
}

#[test]
fn use_after_reset_is_e0632() {
    assert_eq!(
        error_codes(include_str!("ui/arena_view_after_reset.jet")),
        vec!["E0632".to_string()],
        "the arena reset fixture must report exactly E0632"
    );
}

#[test]
fn close_while_a_view_is_live_is_e0212() {
    let src = r#"
use core.mem

fn run() {
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    close(^arena)
    print(x)
}
"#;
    assert_eq!(
        error_codes(src),
        vec!["E0220".to_string(), "E0212".to_string()],
        "closing an allocator while its view is live must report exactly E0220, E0212"
    );
}

#[test]
fn reset_on_branch_invalidates_view_at_join() {
    let src = r#"
use core.mem

fn run() {
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    if true {
        arena.reset()
    }
    print(x)
}
"#;
    assert_eq!(
        error_codes(src),
        vec!["E0632".to_string()],
        "a reset on a reachable branch must report exactly E0632"
    );
}

#[test]
fn reset_in_loop_invalidates_view_after_loop() {
    let src = r#"
use core.mem

fn run() {
    arena :: mem.Arena.new()
    x :: arena.alloc(42)
    loop {
        arena.reset()
        break
    }
    print(x)
}
"#;
    assert_eq!(
        error_codes(src),
        vec!["E0632".to_string()],
        "a reset in a loop must report exactly E0632"
    );
}

#[test]
fn explicit_region_spans_two_arenas() {
    let src = r#"
use core.mem

fn run() {
    #Region(work) {
        a :: mem.Arena.new()
        b :: mem.Bump.new()
        first :: a.alloc(1)
        second :: b.alloc(2)
        print(first)
        print(second)
    }
    print(99)
}
"#;
    assert_eq!(
        error_codes(src),
        Vec::<String>::new(),
        "a region spanning two arenas should compile clean"
    );
    if let Some(out) = build_and_run("region", src) {
        assert_eq!(out, "1\n2\n99\n");
    }
}

#[test]
fn region_confines_view_escape() {
    // A view made inside a `region` may not be carried out of it. v1 rejects the
    // move while its owner-backed view remains live (E0212, D-MEM1 S9).
    let src = r#"
use core.mem

fn run() {
    #Region(r) {
        a :: mem.Arena.new()
        v :: a.alloc(5)
        leak :: v
        print(leak)
    }
}
"#;
    assert_eq!(
        error_codes(src),
        vec!["E0212".to_string()],
        "moving a region view while its owner remains live must report exactly E0212"
    );
}
