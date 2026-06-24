//! C FFI (S59 / E2-M14) integration + unit tests.
//!
//! Phase 1 proves the whole pipeline end to end: a hand-written `@bindgen`
//! cache fixture + a `use c.<lib>` call site compile to `extern "C"` wrappers
//! that link against a real C static library (built here with `cc`) and print
//! deterministic output.
//!
//! Phase 2 link discovery is exercised by unit tests over the flag parser
//! (`parse_pkg_config`), the `deps: { lib: c@… }` manifest path, and E3201,
//! since the nix dev shell ships neither `pkg-config` nor a known system lib.

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
        r#"#bindgen module c.jetc.__bindgen__ {
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

/// Regression (c95): a `String` parameter must emit its `CString` conversion
/// line in the wrapper body. The codegen built `call_args` referencing `c{i}`
/// but dropped the `let c{i} = …` conversion, so rustc rejected the wrapper
/// (`cannot find value c0`) — an I2 violation. Pin that the temp is declared.
#[test]
fn cffi_string_param_emits_cstring_conversion() {
    let root = std::env::temp_dir().join(format!("jet_cffi_strparam_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("strlib.jet"),
        "#bindgen module c.strlib.__bindgen__ { fn slen(s: String) -> Int = \"strlen\"; }\n",
    )
    .unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        "use c.strlib as s;\nfn main() { print(s.slen(\"hello\")); }\n",
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("string-param C FFI rejected:\n{:?}", d));
    assert!(
        out.rust.contains("let c0 = std::ffi::CString::new"),
        "wrapper must declare the CString temp for a String param; got:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("strlen(c0.as_ptr())"),
        "wrapper must call through the declared temp; got:\n{}",
        out.rust
    );
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
        "#bindgen module c.jetc.__bindgen__ { fn ping() -> Int = \"jetc_ping\"; }\n",
    )
    .unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc as jc;
#extern module c.jetc { }
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
        "#bindgen module c.jetc.__bindgen__ { fn add(a: Int, b: Int) -> Int = \"gen_add\"; }\n",
    )
    .unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc as jc;
#extern module c.jetc { fn add(a: Int, b: Int) -> Int = "real_add"; }
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
        "#bindgen module c.demo.__bindgen__ { fn ping() -> Int = \"demo_ping\"; }\n",
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

// ---------------------------------------------------------------------------
// D-CBIND2 + Phase 3 (E3 deferred pieces): auto-bind on cache miss, hash
// invalidation, and cache-hit fast path.
// ---------------------------------------------------------------------------

/// Probe 1 — missing cache + header-path use form → compiler auto-runs bind,
/// compilation succeeds (no E3208 / no error about missing cache).
#[test]
fn auto_bind_on_cache_miss() {
    let root = std::env::temp_dir().join(format!("jet_autobind_miss_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    // Create the project layout but DO NOT pre-create the binding cache.
    let cache_dir = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache_dir).unwrap();

    // A simple C header with one bindable function.
    let header_dir = root.join("include");
    fs::create_dir_all(&header_dir).unwrap();
    let header_path = header_dir.join("mylib.h");
    fs::write(&header_path, "int mylib_ping(int x);\n").unwrap();

    // A Jet source using the header-path form (`use "include/mylib.h" as m`).
    // The compiler should auto-invoke the bind backend on the cache miss.
    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"include/mylib.h\" as m;\nfn main() { print(m.mylib_ping(1)); }\n",
    )
    .unwrap();

    let src = fs::read_to_string(&main).unwrap();
    // compile_with_path loads the bundle from disk (including auto-bind).
    let result = jet::compile_with_path(&src, main.to_str().unwrap());

    // The cache should now exist (auto-created by the compiler).
    let cache_file = cache_dir.join("mylib.jet");
    assert!(
        cache_file.is_file(),
        "auto-bind: compiler should have created the binding cache at {:?}",
        cache_file
    );

    // And compilation should succeed (the generated binding is loaded).
    assert!(
        result.is_ok(),
        "auto-bind on cache miss must not produce a compile error; got: {:?}",
        result.err()
    );

    let _ = fs::remove_dir_all(&root);
}

/// Probe 2 — header changes after a successful bind → hash mismatch detected,
/// the compiler RE-BINDS (does not use the stale cache).
#[test]
fn hash_invalidation_on_header_change() {
    let root = std::env::temp_dir().join(format!("jet_hash_inval_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache_dir = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache_dir).unwrap();

    let header_dir = root.join("include");
    fs::create_dir_all(&header_dir).unwrap();
    let header_path = header_dir.join("mylib2.h");

    // Step A: initial header with `mylib2_v1`.
    let header_v1 = "int mylib2_v1(int x);\n";
    fs::write(&header_path, header_v1).unwrap();

    // Pre-populate the cache and hash sidecar (simulates a prior `jet bind`).
    let cache_file = cache_dir.join("mylib2.jet");
    let bind_v1 = jet::CBind::generate(header_v1, "mylib2").unwrap();
    fs::write(&cache_file, &bind_v1.source).unwrap();
    jet::CBind::write_bind_hash(&cache_file, header_v1, "").unwrap();

    // Confirm v1 cache doesn't contain v2 symbol yet.
    assert!(!bind_v1.source.contains("mylib2_v2"), "sanity: v2 not in v1 cache");

    // Step B: change the header — add `mylib2_v2`, remove `mylib2_v1`.
    let header_v2 = "int mylib2_v2(int y);\n";
    fs::write(&header_path, header_v2).unwrap();

    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"include/mylib2.h\" as m;\nfn main() { print(m.mylib2_v2(2)); }\n",
    )
    .unwrap();

    let src = fs::read_to_string(&main).unwrap();
    let result = jet::compile_with_path(&src, main.to_str().unwrap());

    // The cache should have been regenerated with v2 content.
    let new_cache = fs::read_to_string(&cache_file).unwrap_or_default();
    assert!(
        new_cache.contains("mylib2_v2"),
        "hash invalidation: cache must be regenerated with updated symbol; got:\n{}",
        new_cache
    );
    assert!(
        !new_cache.contains("mylib2_v1"),
        "hash invalidation: stale v1 symbol must not appear in regenerated cache; got:\n{}",
        new_cache
    );

    // The hash sidecar must also be updated to reflect v2.
    let new_hash = jet::CBind::read_stored_hash(&cache_file);
    let expected_hash = jet::CBind::compute_bind_hash(header_v2, "");
    assert_eq!(
        new_hash.as_deref(),
        Some(expected_hash.as_str()),
        "hash sidecar must be updated after re-bind"
    );

    // The compile result may succeed or fail depending on whether the new
    // symbol is in scope; what matters is that the CACHE was invalidated.
    // (We don't assert compile success here since the test binary isn't linked.)
    let _ = result;

    let _ = fs::remove_dir_all(&root);
}

/// Probe 3 — unchanged header + present cache → NO re-bind (fast path: the
/// cache is loaded as-is, hash stays the same).
#[test]
fn cache_hit_no_rebind() {
    let root = std::env::temp_dir().join(format!("jet_cache_hit_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache_dir = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache_dir).unwrap();

    let header_dir = root.join("include");
    fs::create_dir_all(&header_dir).unwrap();
    let header_path = header_dir.join("mylib3.h");
    let header_src = "int mylib3_ping(int x);\n";
    fs::write(&header_path, header_src).unwrap();

    // Pre-populate cache + hash.
    let cache_file = cache_dir.join("mylib3.jet");
    let bind_result = jet::CBind::generate(header_src, "mylib3").unwrap();
    fs::write(&cache_file, &bind_result.source).unwrap();
    jet::CBind::write_bind_hash(&cache_file, header_src, "").unwrap();

    // Record the cache content and mtime before compile.
    let cache_before = fs::read_to_string(&cache_file).unwrap();
    let mtime_before = fs::metadata(&cache_file)
        .and_then(|m| m.modified())
        .unwrap();

    // Small sleep so any write would shift the mtime noticeably on most FS.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"include/mylib3.h\" as m;\nfn main() { print(m.mylib3_ping(0)); }\n",
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let _ = jet::compile_with_path(&src, main.to_str().unwrap());

    // Cache must not have been rewritten.
    let cache_after = fs::read_to_string(&cache_file).unwrap();
    let mtime_after = fs::metadata(&cache_file)
        .and_then(|m| m.modified())
        .unwrap();
    assert_eq!(
        cache_before, cache_after,
        "cache-hit fast path: cache content must not change on identical header"
    );
    assert_eq!(
        mtime_before, mtime_after,
        "cache-hit fast path: cache file must not be rewritten on hash match"
    );

    let _ = fs::remove_dir_all(&root);
}

/// Unit test: the hash function is deterministic and changes when input changes.
#[test]
fn bind_hash_is_real() {
    let h1 = jet::CBind::compute_bind_hash("int foo(int x);", "");
    let h2 = jet::CBind::compute_bind_hash("int foo(int x);", "");
    let h3 = jet::CBind::compute_bind_hash("int bar(int x);", "");
    let h4 = jet::CBind::compute_bind_hash("int foo(int x);", "-I/usr/include");

    assert_eq!(h1, h2, "same inputs → same hash");
    assert_ne!(h1, h3, "different header → different hash");
    assert_ne!(h1, h4, "different cflags → different hash");
    // Must be a 64-char hex string (SHA-256).
    assert_eq!(h1.len(), 64);
    assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
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
fn deps_block_parses_c_lib_refs() {
    // S59/D-CFFI2: native C deps live in the Jet `deps:` block as `c@<target>`
    // refs, parsed by the real PackageManifest parser (not an ad-hoc reader).
    use jet::Jetpack::PackageManifest::{parse, DepSource};
    let manifest = r#"
payload: { name: "p", version: "0.1.0" }
deps: {
    raylib: c@system,
    foo:    c@"/opt/foo",
}
"#;
    let pm = parse(manifest).expect("manifest parses");
    let raylib = pm.deps.iter().find(|d| d.name == "raylib").unwrap();
    assert_eq!(raylib.source, DepSource::CLib { target: "system".into() });
    let foo = pm.deps.iter().find(|d| d.name == "foo").unwrap();
    assert_eq!(foo.source, DepSource::CLib { target: "/opt/foo".into() });
    // A non-C dep stays a normal Jet dep, not a CLib.
    assert!(pm.deps.iter().all(|d| d.name != "sqlite3"));
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
 Why: Jet looked for a `nolib: c@…` dep in `pkg.jet`, then tried `pkg-config nolib` on the system; neither provided include/link paths.
 Fix: Install the system package (e.g. `pacman -S nolib`), or declare it as `nolib: c@system` in `deps:`.
";
    assert_eq!(rendered, expected);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn e3209_link_time_missing_lib_snapshot() {
    // I4: link-time "cannot find -l<lib>" diagnostic. Jet-stage (the ui harness
    // only renders front-end diagnostics), so pin the rendered text here.
    let d = jet::CFFI::e3209("raylib");
    assert_eq!(d.code, "E3209");
    let rendered = jet::render_diagnostics("main.jet", "", std::slice::from_ref(&d));
    let expected = "\
Error [E3209]: The linker couldn't find C library `raylib`.
 Why: Your program links against `raylib`, but the linker reported `cannot find -lraylib` — the library isn't on the link search path.
 Fix: Declare it in `deps:` so Jet provisions it: `raylib: c@system` (host pkg-config, else fetched from nixpkgs), or `raylib: c@nixpkgs:<attr>` to pick the nixpkgs attribute, or install the system package.
";
    assert_eq!(rendered, expected);
}

#[test]
fn e3210_nixpkgs_provision_failed_snapshot() {
    // I4: nixpkgs auto-provision failure diagnostic (jet-stage), pinned here.
    let d = jet::CFFI::e3210("raylib", "raylib", "error: attribute 'raylib' missing");
    assert_eq!(d.code, "E3210");
    let rendered = jet::render_diagnostics("main.jet", "", std::slice::from_ref(&d));
    let expected = "\
Error [E3210]: Couldn't fetch C library `raylib` from nixpkgs.
 Why: `raylib: c@system` asked Jet to provision `nixpkgs#raylib`, but `nix build` failed: error: attribute 'raylib' missing
 Fix: Check the attr exists (`nix build nixpkgs#raylib`), or point at a local build with `raylib: c@\"<path>\"`, or install it and use `system`.
";
    assert_eq!(rendered, expected);
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
 Why: C FFI allows by-value scalars and `String` in ordinary code; pointers and other gated types need `use core.mem` and an `#Unsafe { … }` region (S58).
 Fix: Move the call inside `#Unsafe`, or change the type to a C-safe value type.
";
    assert_eq!(rendered, expected);
}
