//! Golden tests: every example program must pass the front end, and (when
//! rustc is available) build and print exactly its expected output.
//! Examples are the executable spec (invariant I5).
//!
//! Also enforces:
//!   I1 — generated code never contains `unsafe`
//!   I2 — rustc accepting the generated code; a rejection here is a
//!        front-end soundness bug, reported loudly

use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;
use common::FfiBridgeLock;

fn gtk_loader_unavailable(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr);
    stderr.contains("symbol lookup error")
        && stderr.contains("libgtk-4.so")
        && stderr.contains("undefined symbol")
}

#[test]
fn examples_compile_and_run() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ex_dir = root.join("examples/features");
    let ext = jet::Syntax::FILE_EXT;
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    let have_cargo = Command::new("cargo").arg("--version").output().is_ok();
    // D-UIDEVSHELL1=A (c134 Phase 8): the native GTK4 example links `-lgtk-4`
    // via `pkg-config gtk4`. Only build+run it where gtk4 dev headers exist
    // (the nix dev shell); elsewhere the front-end check still runs.
    let have_gtk = Command::new("pkg-config")
        .args(["--exists", "gtk4"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !have_rustc {
        eprintln!("note: rustc not found; checking codegen only, skipping build+run");
    }

    // Recursive discovery: examples/features/<topic>/<name>.jet or
    // examples/features/<topic>/<name>/main.jet. Test id (`stem`) is the
    // relative path without extension, e.g. "net/http_server". `expected/`
    // mirrors the same <topic>/<name> tree.
    let mut entries: Vec<(PathBuf, String, String)> = Vec::new();
    for topic_entry in fs::read_dir(&ex_dir).unwrap().flatten() {
        let topic_path = topic_entry.path();
        if !topic_path.is_dir() {
            continue;
        }
        let topic_name = topic_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if topic_name == "expected" {
            continue;
        }
        for e in fs::read_dir(&topic_path).unwrap().flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) == Some(ext) {
                let name = path.file_stem().unwrap().to_string_lossy().into_owned();
                let stem = format!("{}/{}", topic_name, name);
                entries.push((
                    path.clone(),
                    stem.clone(),
                    format!("examples/features/{}.{}", stem, ext),
                ));
            } else if path.is_dir() {
                let main = path.join(format!("main.{}", ext));
                if main.is_file() {
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    let stem = format!("{}/{}", topic_name, name);
                    entries.push((
                        main.clone(),
                        stem.clone(),
                        format!("examples/features/{}/main.{}", stem, ext),
                    ));
                }
            }
        }
    }
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    let mut checked = 0;
    for (path, stem, shown) in entries {
        let src = fs::read_to_string(&path).unwrap();

        let uses_ffi_bridge = stem == "lowlevel/ffi"
            || stem == "io/archive"
            || stem == "io/db"
            || stem == "crypto/crypto_envelope"
            || stem == "crypto/crypto_sign"
            || stem == "crypto/crypto_migration"
            || stem == "crypto/vault_secret"
            || stem == "io/compress_gzip"
            || stem == "io/compress_zstd";

        if uses_ffi_bridge && !have_cargo {
            eprintln!(
                "note: skipping examples/features/{stem}.jet golden (need cargo for FFI bridge)"
            );
            checked += 1;
            continue;
        }

        // These stems build Jet's hidden global FFI/cargo bridge cache
        // (`~/.cache/jet/ffi/<key>/`), which has no synchronization of its
        // own. tests/cffi.rs compiles some of the same fixtures (e.g.
        // `lowlevel/ffi`'s base64@0.22/b64encode signature) — hold the shared
        // lock so the two test binaries never race the same cache key.
        // See FfiBridgeLock for the full root cause.
        let _ffi_lock = uses_ffi_bridge.then(FfiBridgeLock::acquire);

        let compiled = match jet::compile_with_path(&src, &shown) {
            Ok(c) => c,
            Err(diags) => panic!(
                "example {} failed the front end:\n{}",
                stem,
                jet::render_diagnostics(
                    &format!("examples/features/{}.{}", stem, ext),
                    &src,
                    &diags
                )
            ),
        };
        let rust_code = compiled.rust;
        let ffi_link = compiled.ffi;

        // I1 (amended by D-LL1, E2-M13): memory safety is never traded away in
        // ordinary Jet. Generated `unsafe` appears ONLY inside the gated
        // low-level tier (`use core.mem` + `#Unsafe`) and vetted prelude helpers:
        //   - `mod jet_mem`        (D-ALLOC2 — arena lifetime-extension)
        //   - `mod jet_term_unix`  (D-TERM1 — POSIX termios via extern "C")
        //   - `mod jet_term_windows` (D-TERM1 — Windows console API via extern "system")
        //   - `mod user___c_<lib>`  (S58 — C-FFI wrappers: the only place
        //                            compiler-vetted `unsafe` calls extern "C")
        // These are audited platform-FFI blocks, not user code. All other `unsafe`
        // in the file must come from the gated `#Unsafe` tier only.
        let user_code: String = {
            // Helper: strip one `mod <name> { … }` block (brace-matched), where
            // `name` is matched as a prefix so families like `user___c_*` can be
            // removed regardless of the library segment.
            fn strip_mod(src: &str, name: &str) -> String {
                if let Some(start) = src.find(&format!("mod {}", name)) {
                    let bytes = src.as_bytes();
                    let mut depth = 0usize;
                    let mut i = start;
                    let mut end = src.len();
                    let mut seen_brace = false;
                    while i < bytes.len() {
                        match bytes[i] {
                            b'{' => {
                                depth += 1;
                                seen_brace = true;
                            }
                            b'}' => {
                                depth -= 1;
                                if seen_brace && depth == 0 {
                                    end = i + 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    let mut s = src[..start].to_string();
                    s.push_str(&src[end..]);
                    s
                } else {
                    src.to_string()
                }
            }
            // Strip all vetted prelude modules (order doesn't matter).
            let s = strip_mod(&rust_code, "jet_mem");
            let s = strip_mod(&s, "jet_txn");
            let s = strip_mod(&s, "jet_term_unix");
            let s = strip_mod(&s, "jet_term_windows");
            // D-UIDEVSHELL1=A (c134 Phase 8): the native GTK4 backend's vetted
            // `extern "C"` boundary. Same treatment as the term/C-FFI shims —
            // its `unsafe` is audited platform-FFI, not user code.
            let mut s = strip_mod(&s, "jet_gtk");
            // Strip every C-FFI wrapper module (`user___c_<lib>` and the cache
            // module `user___c_cache_<lib>`) — their `unsafe` is the vetted S58
            // boundary shim, not user code. Loop until none remain.
            while s.contains("mod user___c_") {
                let before = s.clone();
                s = strip_mod(&s, "user___c_");
                if s == before {
                    break;
                }
            }
            s
        };
        // Examples that exercise the gated `unsafe` tier (`#Unsafe` blocks /
        // `#Unsafe fn`, or `#Uninit` which lowers to `MaybeUninit::uninit().assume_init()`
        // inside an inline `unsafe { }` block). Their generated `unsafe` is allowed, but
        // ONLY in the gated block/fn form — never ungated (I1).
        if stem == "lowlevel/lowlevel"
            || stem == "lowlevel/pointer_cast_deref"
            || stem == "memory/rawptr"
            || stem == "effects/single_use_discard"
            || stem == "memory/uninit"
            || stem == "memory/uninit_buffer"
            || stem == "crypto/crypto_migration"
        {
            assert!(
                user_code.contains("unsafe"),
                "the low-level example {} should exercise the gated `unsafe` tier",
                stem
            );
            // Even in the audited example, every `unsafe` is a gated form.
            for (i, line) in user_code.lines().enumerate() {
                if let Some(col) = line.find("unsafe") {
                    let after = line[col..].trim_start_matches("unsafe");
                    let after = after.trim_start();
                    assert!(
                        after.starts_with('{') || after.starts_with("fn "),
                        "{} emits an ungated `unsafe` at line {}: {}",
                        stem,
                        i + 1,
                        line.trim()
                    );
                }
            }
        } else if stem == "game/raylib_window" {
            assert!(
                !user_code.contains("unsafe fn user_"),
                "raylib user functions must stay safe; bridge unsafe stays in vetted prelude"
            );
        } else {
            // Every other example's user code is fully safe — the only `unsafe`
            // in the file is the vetted `jet_mem` arena helper, already excluded.
            assert!(
                !user_code.contains("unsafe"),
                "generated Rust for {} contains `unsafe` outside the vetted `jet_mem` helper",
                stem
            );
        }
        assert!(
            rust_code.contains("fn main()"),
            "generated Rust for {} has no fn main",
            stem
        );

        // D-UIDEVSHELL1=A (c134 Phase 8): the native GTK4 example needs gtk4 to
        // link. Front-end check already ran above; skip build+run without it.
        let needs_gtk = stem == "ui/ui_native_linux";
        if needs_gtk && (!have_gtk || !have_rustc) {
            eprintln!("note: skipping examples/features/{stem}.jet build (need gtk4 + rustc)");
            checked += 1;
            continue;
        }
        // D-FLAGSHIP-RAYLIB1=A: display remains explicit. The headless path is
        // checked above; native display build/run stays opt-in so CI doesn't need
        // a display server or raylib installation.
        let needs_raylib_display = stem == "game/raylib_window";
        if needs_raylib_display && std::env::var("JET_RAYLIB_DISPLAY").as_deref() != Ok("1") {
            eprintln!(
                "note: skipping examples/features/{stem}.jet build (set JET_RAYLIB_DISPLAY=1)"
            );
            checked += 1;
            continue;
        }

        if have_rustc {
            // stem contains '/' (topic/name) — flatten for the temp filename.
            let flat_stem = stem.replace('/', "_");
            let dir = std::env::temp_dir();
            let rs = dir.join(format!("jet_golden_{}.rs", flat_stem));
            let bin = dir.join(format!("jet_golden_{}", flat_stem));
            fs::write(&rs, &rust_code).unwrap();
            let mut rustc_cmd = Command::new("rustc");
            rustc_cmd
                .args(["--edition", "2021"])
                .arg(&rs)
                .arg("-o")
                .arg(&bin);
            if let Some(link) = &ffi_link {
                rustc_cmd.arg("--extern").arg(format!(
                    "{}={}",
                    link.crate_name,
                    link.rlib_path.display()
                ));
                if link.deps_dir.is_dir() {
                    rustc_cmd
                        .arg("-L")
                        .arg(format!("dependency={}", link.deps_dir.display()));
                }
            }
            // S59/E2-M14: native C-library link flags (`-L native=…`, `-l <lib>`),
            // resolved the same way `jet build` does. Examples that bind a C
            // header (e.g. 102_cbind) need these threaded into the link line.
            let clinks = jet::resolve_c_links(path.to_str().unwrap())
                .expect("resolve_c_links should succeed for example C bindings");
            for arg in &clinks {
                rustc_cmd.arg(arg);
            }
            let out = rustc_cmd.output().unwrap();
            assert!(
                out.status.success(),
                "I2 violated: rustc rejected generated code for {} — this is a jet bug:\n{}",
                stem,
                String::from_utf8_lossy(&out.stderr)
            );
            // D-UIDEVSHELL1=A (c134 Phase 8): the native GTK4 example calls
            // `backend.present(...)`, which opens a real window and blocks on the
            // GLib main loop. `JET_UI_HEADLESS=1` makes it skip the window so the
            // golden run stays deterministic and terminating; a real `jet run`
            // (no env var) opens the window.
            let mut run_cmd = Command::new(&bin);
            if needs_gtk {
                run_cmd.env("JET_UI_HEADLESS", "1");
            }
            let run = run_cmd.output().unwrap();
            if needs_gtk && !run.status.success() && gtk_loader_unavailable(&run.stderr) {
                eprintln!(
                    "note: skipping examples/features/{stem}.jet run (gtk4 runtime loader unavailable)"
                );
                checked += 1;
                continue;
            }
            let err_path = ex_dir.join("expected").join(format!("{}.err.out", stem));
            if err_path.exists() {
                let expected_err = fs::read_to_string(&err_path).unwrap_or_else(|_| {
                    panic!("missing examples/features/expected/{}.err.out", stem)
                });
                assert_eq!(
                    run.status.code(),
                    Some(70),
                    "exit code mismatch for example {}",
                    stem
                );
                assert_eq!(
                    String::from_utf8_lossy(&run.stderr),
                    expected_err,
                    "stderr mismatch for example {}",
                    stem
                );
            } else {
                assert!(
                    run.status.success(),
                    "example {} failed at runtime:\nstdout: {}\nstderr: {}",
                    stem,
                    String::from_utf8_lossy(&run.stdout),
                    String::from_utf8_lossy(&run.stderr)
                );
                let expected =
                    fs::read_to_string(ex_dir.join("expected").join(format!("{}.out", stem)))
                        .unwrap_or_else(|_| {
                            panic!("missing examples/features/expected/{}.out", stem)
                        });
                assert_eq!(
                    String::from_utf8_lossy(&run.stdout),
                    expected,
                    "output mismatch for example {}",
                    stem
                );
            }
        }
        checked += 1;
    }
    assert!(
        checked >= 2,
        "expected at least 2 examples, found {}",
        checked
    );
}
