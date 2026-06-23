//! D-ALLOC2 / D-REGION1 (c05): real bump-allocated arenas with scope-bound
//! regions. Covers the soundness contract end to end:
//!   * alloc-and-use compiles and runs;
//!   * a view that escapes its region is E0631 (Jet rejects first, I2);
//!   * a view used after the arena is `reset`/`free`d is E0632;
//!   * an explicit `region r { … }` may span two allocators.

use std::process::Command;

/// Write the fixture to a real temp file so `use core.mem` resolves like a
/// normal build, then return the diagnostic codes (empty = clean).
fn error_codes(src: &str) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("jet_arena_{}", std::process::id()));
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
    let dir0 = std::env::temp_dir().join(format!("jet_arena_{}", std::process::id()));
    std::fs::create_dir_all(&dir0).unwrap();
    let fpath = dir0.join("fixture.jet");
    std::fs::write(&fpath, src).unwrap();
    let out = jet::compile_with_path(src, &fpath.to_string_lossy())
        .unwrap_or_else(|d| panic!("front end rejected a should-compile fixture: {:?}",
            d.iter().map(|x| x.code).collect::<Vec<_>>()));
    // No `unsafe` may leak outside the vetted prelude helpers (I1 / D-LL1).
    // Strip the vetted `mod jet_mem`, `mod jet_term_unix`, and `mod jet_term_windows`
    // blocks (brace-matched) before checking for `unsafe` in user code.
    fn strip_mod_block(src: &str, marker: &str) -> String {
        let Some(start) = src.find(marker) else { return src.to_string(); };
        let cfg_start = src[..start].rfind('\n').map(|n| n + 1).unwrap_or(start);
        let bytes = src.as_bytes();
        let (mut depth, mut seen, mut i) = (0usize, false, start);
        let mut end = src.len();
        while i < bytes.len() {
            match bytes[i] {
                b'{' => { depth += 1; seen = true; }
                b'}' => { depth -= 1; if seen && depth == 0 { end = i + 1; break; } }
                _ => {}
            }
            i += 1;
        }
        format!("{}{}", &src[..cfg_start], &src[end..].trim_start_matches('\n'))
    }
    let s = strip_mod_block(&out.rust, "mod jet_mem");
    let s = strip_mod_block(&s, "mod jet_term_unix {");
    let user = strip_mod_block(&s, "mod jet_term_windows {");
    assert!(!user.contains("unsafe"), "`unsafe` leaked outside the vetted prelude helpers");

    if Command::new("rustc").arg("--version").output().is_err() {
        eprintln!("note: rustc not found; compiled front end only");
        return None;
    }
    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_arena_{}.rs", name));
    let bin = dir.join(format!("jet_arena_{}", name));
    std::fs::write(&rs, &out.rust).unwrap();
    let c = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs).arg("-o").arg(&bin)
        .output().unwrap();
    assert!(c.status.success(),
        "I2 violated: rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&c.stderr));
    let r = Command::new(&bin).output().unwrap();
    Some(String::from_utf8_lossy(&r.stdout).to_string())
}

#[test]
fn alloc_and_use_compiles_and_runs() {
    let src = r#"
use core.mem

fn main() {
    arena @= mem.Arena.new()
    x @= arena.alloc(42)
    y @= arena.alloc(100)
    print(x)
    print(y)
    arena.reset()
    z @= arena.alloc(7)
    print(z)
}
"#;
    assert!(error_codes(src).is_empty(), "alloc-and-use should compile clean");
    if let Some(out) = build_and_run("ok", src) {
        assert_eq!(out, "42\n100\n7\n");
    }
}

#[test]
fn view_escape_is_e0631() {
    let src = r#"
use core.mem

fn leak() -> Int {
    arena @= mem.Arena.new()
    x @= arena.alloc(42)
    return x
}

fn main() {
    print(leak())
}
"#;
    assert!(
        error_codes(src).contains(&"E0631".to_string()),
        "a returned arena view must be E0631, got {:?}",
        error_codes(src)
    );
}

#[test]
fn view_stored_in_binding_is_e0631() {
    let src = r#"
use core.mem

fn main() {
    arena @= mem.Arena.new()
    x @= arena.alloc(42)
    stash @= x
    print(stash)
}
"#;
    assert!(
        error_codes(src).contains(&"E0631".to_string()),
        "moving a view into another binding must be E0631, got {:?}",
        error_codes(src)
    );
}

#[test]
fn use_after_reset_is_e0632() {
    let src = r#"
use core.mem

fn main() {
    arena @= mem.Arena.new()
    x @= arena.alloc(42)
    arena.reset()
    print(x)
}
"#;
    assert!(
        error_codes(src).contains(&"E0632".to_string()),
        "reading a view after reset must be E0632, got {:?}",
        error_codes(src)
    );
}

#[test]
fn use_after_free_is_e0632() {
    let src = r#"
use core.mem

fn main() {
    arena @= mem.Arena.new()
    x @= arena.alloc(42)
    arena.free()
    print(x)
}
"#;
    assert!(
        error_codes(src).contains(&"E0632".to_string()),
        "reading a view after free must be E0632, got {:?}",
        error_codes(src)
    );
}

#[test]
fn explicit_region_spans_two_arenas() {
    let src = r#"
use core.mem

fn main() {
    region work {
        a @= mem.Arena.new()
        b @= mem.Bump.new()
        first @= a.alloc(1)
        second @= b.alloc(2)
        print(first)
        print(second)
    }
    print(99)
}
"#;
    assert!(
        error_codes(src).is_empty(),
        "a region spanning two arenas should compile clean, got {:?}",
        error_codes(src)
    );
    if let Some(out) = build_and_run("region", src) {
        assert_eq!(out, "1\n2\n99\n");
    }
}

#[test]
fn region_confines_view_escape() {
    // A view made inside a `region` may not be carried out of it. v1 rejects the
    // move into an outer binding (E0631).
    let src = r#"
use core.mem

fn main() {
    region r {
        a @= mem.Arena.new()
        v @= a.alloc(5)
        leak @= v
        print(leak)
    }
}
"#;
    assert!(
        error_codes(src).contains(&"E0631".to_string()),
        "a view escaping a region binding must be E0631, got {:?}",
        error_codes(src)
    );
}
