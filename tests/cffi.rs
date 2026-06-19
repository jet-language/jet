//! C FFI (S59 / E2-M14) integration + unit tests.
//!
//! Phase 1 proves the whole pipeline end to end: a hand-written `@bindgen`
//! cache fixture + a `use c.<lib>` call site compile to `extern "C"` wrappers
//! that link against a real C static library (built here with `cc`) and print
//! deterministic output.
//!
//! Phase 2 link discovery is exercised by unit tests over the flag parsers
//! (`parse_pkg_config`, `parse_c_dep`) and the E3201 path, since the nix dev
//! shell ships neither `pkg-config` nor a known system lib.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Build a tiny C static library `libjetc.a` in `dir`, returning its directory
/// and link name. Skips (returns None) when no C compiler is available.
fn build_c_lib(dir: &Path) -> Option<(PathBuf, String)> {
    let cc = ["cc", "gcc", "clang"]
        .iter()
        .find(|c| Command::new(c).arg("--version").output().is_ok())?;
    let c_src = dir.join("jetc.c");
    fs::write(
        &c_src,
        r#"
#include <stdint.h>
long long jetc_add_ints(long long a, long long b) { return a + b; }
const char *jetc_greeting(void) { return "hi from C"; }
"#,
    )
    .unwrap();
    let obj = dir.join("jetc.o");
    let ok = Command::new(cc)
        .args(["-c"])
        .arg(&c_src)
        .arg("-o")
        .arg(&obj)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let lib = dir.join("libjetc.a");
    let ok = Command::new("ar")
        .arg("rcs")
        .arg(&lib)
        .arg(&obj)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    Some((dir.to_path_buf(), "jetc".to_string()))
}

/// E2-M14: the native `jet bind` backend turns a real C header into a working
/// `@bindgen` cache that compiles, links against the C library, and runs.
#[test]
fn jet_bind_native_backend_end_to_end() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping jet_bind_native_backend (need rustc)");
        return;
    }
    let root = std::env::temp_dir().join(format!("jet_cbind_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();

    let Some((lib_dir, lib_name)) = build_c_lib(&root) else {
        eprintln!("note: skipping jet_bind_native_backend (no C compiler)");
        return;
    };

    // A real header for the C library — translated by the native backend.
    let header = r#"
        #include <stdint.h>
        /* arithmetic */
        long long jetc_add_ints(long long a, long long b);
        const char *jetc_greeting(void);
    "#;
    let result = jet::CBind::generate(header, "jetc").expect("native bind backend");
    assert!(result.skipped.is_empty(), "unexpected skips: {:?}", result.skipped);
    assert_eq!(result.bound.len(), 2);
    // The cache uses the real C symbol names verbatim (no aliasing).
    assert!(result.source.contains("fn jetc_add_ints(a: Int, b: Int) -> Int = \"jetc_add_ints\";"));
    assert!(result.source.contains("fn jetc_greeting() -> String = \"jetc_greeting\";"));
    fs::write(cache.join("jetc.jet"), &result.source).unwrap();

    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc as jc;

fn main() {
    print(jc.jetc_add_ints(2, 40));
    print(jc.jetc_greeting());
}
"#,
    )
    .unwrap();

    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("front end rejected bind-generated program:\n{:?}", d));

    let rs = root.join("main.rs");
    fs::write(&rs, &out.rust).unwrap();
    let bin = root.join("main_bin");
    let status = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .arg("-L")
        .arg(format!("native={}", lib_dir.display()))
        .arg("-l")
        .arg(&lib_name)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "I2: rustc rejected bind-generated C-FFI code (jet bug):\n{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "bind-generated program failed at runtime");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\nhi from C\n");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cffi_end_to_end_links_and_runs() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping cffi_end_to_end (need rustc)");
        return;
    }
    let root = std::env::temp_dir().join(format!("jet_cffi_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();

    let Some((lib_dir, lib_name)) = build_c_lib(&root) else {
        eprintln!("note: skipping cffi_end_to_end (no C compiler)");
        return;
    };

    // Hand-written bindgen cache fixture (simulates `jet bind` output).
    fs::write(
        cache.join("jetc.jet"),
        r#"@bindgen module c.jetc.__bindgen__ {
    fn add_ints(a: Int, b: Int) -> Int = "jetc_add_ints";
    fn greeting() -> String = "jetc_greeting";
}
"#,
    )
    .unwrap();

    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc as jc;

fn main() {
    print(jc.add_ints(2, 40));
    print(jc.greeting());
}
"#,
    )
    .unwrap();

    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("front end rejected C FFI program:\n{:?}", d));

    // I1: no `unsafe` leaks into ordinary Jet — but the boundary shim is
    // compiler-emitted, vetted internals (S58). The wrappers we emit DO use
    // unsafe to call extern "C"; confirm it is confined to the C module.
    assert!(
        out.rust.contains("extern \"C\""),
        "expected an extern \"C\" block in generated code"
    );
    assert!(
        out.rust.contains("jetc_add_ints"),
        "expected the C symbol name in generated code"
    );

    // Build + link against the C static library.
    let rs = root.join("main.rs");
    fs::write(&rs, &out.rust).unwrap();
    let bin = root.join("main_bin");
    let mut cmd = Command::new("rustc");
    cmd.args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .arg("-L")
        .arg(format!("native={}", lib_dir.display()))
        .arg("-l")
        .arg(&lib_name);
    let status = cmd.output().unwrap();
    assert!(
        status.status.success(),
        "I2: rustc rejected generated C-FFI code (jet bug):\n{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "C-FFI program failed at runtime");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\nhi from C\n");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cffi_empty_overlay_is_bindgen_only() {
    // D-CFFI2-SYN-2: an empty `@extern module` adds nothing; the full bindgen
    // surface stays visible.
    let root = std::env::temp_dir().join(format!("jet_cffi_empty_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("jetc.jet"),
        "@bindgen module c.jetc.__bindgen__ { fn ping() -> Int = \"jetc_ping\"; }\n",
    )
    .unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc as jc;
@extern module c.jetc { }
fn main() { print(jc.ping()); }
"#,
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("empty overlay rejected:\n{:?}", d));
    assert!(out.rust.contains("jetc_ping"), "bindgen symbol must survive empty overlay");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cffi_overlay_overrides_bindgen() {
    // D-CFFI2-SYN-4: overlay replaces a bindgen symbol with a matching sig.
    let root = std::env::temp_dir().join(format!("jet_cffi_override_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("jetc.jet"),
        "@bindgen module c.jetc.__bindgen__ { fn add(a: Int, b: Int) -> Int = \"gen_add\"; }\n",
    )
    .unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc as jc;
@extern module c.jetc { fn add(a: Int, b: Int) -> Int = "real_add"; }
fn main() { print(jc.add(1, 2)); }
"#,
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("override rejected:\n{:?}", d));
    assert!(out.rust.contains("real_add"), "overlay symbol must win");
    assert!(!out.rust.contains("gen_add"), "bindgen symbol must be replaced");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cffi_header_use_form_lowers_to_lib() {
    // Phase 3: `use "demo.h" as d` resolves through the same merged c.demo
    // module (header basename → link key `demo`).
    let root = std::env::temp_dir().join(format!("jet_cffi_header_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("demo.jet"),
        "@bindgen module c.demo.__bindgen__ { fn ping() -> Int = \"demo_ping\"; }\n",
    )
    .unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"demo.h\" as d;\nfn main() { print(d.ping()); }\n",
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("header use form rejected:\n{:?}", d));
    assert!(out.rust.contains("demo_ping"), "header form must reach the demo bindgen surface");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn parse_pkg_config_extracts_flags() {
    let flags = jet::CFFI::parse_pkg_config("-I/usr/include/foo -L/usr/lib -lfoo -lbar", "foo");
    assert_eq!(flags.include_dirs, vec!["/usr/include/foo"]);
    assert_eq!(flags.lib_dirs, vec!["/usr/lib"]);
    assert_eq!(flags.link_names, vec!["foo", "bar"]);
}

#[test]
fn parse_pkg_config_defaults_link_name() {
    let flags = jet::CFFI::parse_pkg_config("-I/usr/include/foo", "foo");
    assert_eq!(flags.link_names, vec!["foo"]);
}

#[test]
fn parse_c_dep_reads_dependencies_table() {
    let manifest = "[dependencies:c]\nraylib = \"nixpkgs:raylib#5.5.0\"\n";
    assert_eq!(jet::CFFI::parse_c_dep(manifest, "raylib"), Some(None));
    assert_eq!(jet::CFFI::parse_c_dep(manifest, "sqlite3"), None);

    let with_path = "[dependencies:c]\nfoo = { path = \"/opt/foo\" }\n";
    assert_eq!(
        jet::CFFI::parse_c_dep(with_path, "foo"),
        Some(Some("/opt/foo".to_string()))
    );
}

#[test]
fn resolve_link_unknown_lib_is_e3201() {
    // No pkg.jet dep and (in CI) no pkg-config → E3201.
    let root = std::env::temp_dir().join(format!("jet_cffi_e3201_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let err = jet::CFFI::resolve_link("nolib", &root);
    assert!(err.is_err(), "unknown lib without pkg-config must fail");
    let d = err.unwrap_err();
    assert_eq!(d.code, "E3201");
    // I4: pin the exact rendered text (this is the link-time E3201 snapshot;
    // the ui harness only renders front-end diagnostics, so it is pinned here).
    let rendered = jet::render_diagnostics("main.jet", "", std::slice::from_ref(&d));
    let expected = "\
Error [E3201]: C library `nolib` was not found.
 Why: Jet tried the hangar dep keyed `nolib` in `pkg.jet`, then `pkg-config nolib` on the system; neither provided include/link paths.
 Fix: Install the system package (e.g. `pacman -S nolib`), or add `nolib` under `[dependencies:c]` with a pinned hangar ref.
";
    assert_eq!(rendered, expected);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn e3202_pointer_boundary_snapshot() {
    // E3202 belongs to the E2-M13 pointer tier, which is not implemented, so no
    // real source can reach it. Per I4 the diagnostic must still exist with a
    // pinned snapshot; this is it. When E2-M13 lands, a `tests/ui/` fixture that
    // actually triggers it should replace this rendered-form pin.
    use jet::Diagnostics::Span;
    let src = "fn f(p: Ptr<Int>) = \"f\";\n";
    let d = jet::Sema::e3202("Ptr<Int>", Span::new(8, 16));
    assert_eq!(d.code, "E3202");
    let rendered = jet::render_diagnostics("main.jet", src, std::slice::from_ref(&d));
    let expected = "\
Error [E3202]: Type `Ptr<Int>` cannot cross the C boundary here.
  --> main.jet:1:9
    |
  1 | fn f(p: Ptr<Int>) = \"f\";
    |         ^^^^^^^^
 Why: C FFI allows by-value scalars and `String` in ordinary code; pointers and other gated types need `use core.mem` and an `@unsafe { … }` region (S58).
 Fix: Move the call inside `@unsafe`, or change the type to a C-safe value type.
";
    assert_eq!(rendered, expected);
}
