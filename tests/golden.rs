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
use std::sync::{Arc, Mutex};

mod common;
use common::{panic_message, test_worker_count, FfiBridgeLock};

#[derive(Clone)]
struct GoldenEntry {
    path: PathBuf,
    stem: String,
    shown: String,
}

struct GoldenEnv {
    ex_dir: PathBuf,
    ext: &'static str,
    have_rustc: bool,
    have_cargo: bool,
    have_gtk: bool,
}

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
    let mut entries: Vec<GoldenEntry> = Vec::new();
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
                entries.push(GoldenEntry {
                    path: path.clone(),
                    stem: stem.clone(),
                    shown: format!("examples/features/{}.{}", stem, ext),
                });
            } else if path.is_dir() {
                let main = path.join(format!("main.{}", ext));
                if main.is_file() {
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    let stem = format!("{}/{}", topic_name, name);
                    entries.push(GoldenEntry {
                        path: main.clone(),
                        stem: stem.clone(),
                        shown: format!("examples/features/{}/main.{}", stem, ext),
                    });
                }
            }
        }
    }
    entries.sort_by(|a, b| a.stem.cmp(&b.stem));
    let env = Arc::new(GoldenEnv {
        ex_dir,
        ext,
        have_rustc,
        have_cargo,
        have_gtk,
    });
    let jobs = Arc::new(Mutex::new(std::collections::VecDeque::from(entries)));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let checked = Arc::new(Mutex::new(0usize));
    let mut handles = Vec::new();
    for _ in 0..test_worker_count(16) {
        let env = Arc::clone(&env);
        let jobs = Arc::clone(&jobs);
        let failures = Arc::clone(&failures);
        let checked = Arc::clone(&checked);
        handles.push(std::thread::spawn(move || loop {
            let Some(entry) = jobs.lock().unwrap().pop_front() else {
                break;
            };
            let stem = entry.stem.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                check_golden_entry(&entry, &env)
            }));
            match result {
                Ok(()) => *checked.lock().unwrap() += 1,
                Err(payload) => failures
                    .lock()
                    .unwrap()
                    .push(format!("{stem}: {}", panic_message(payload))),
            }
        }));
    }
    for handle in handles {
        handle.join().expect("golden worker panicked outside harness");
    }
    let failures = failures.lock().unwrap();
    assert!(
        failures.is_empty(),
        "golden example failures:\n{}",
        failures.join("\n\n")
    );
    let checked = *checked.lock().unwrap();
    assert!(
        checked >= 2,
        "expected at least 2 examples, found {}",
        checked
    );
}

fn check_golden_entry(entry: &GoldenEntry, env: &GoldenEnv) {
    let src = fs::read_to_string(&entry.path).unwrap();
    let stem = entry.stem.as_str();
    let uses_ffi_bridge = matches!(
        stem,
        "lowlevel/ffi"
            | "io/archive"
            | "io/db"
            | "crypto/crypto_envelope"
            | "crypto/crypto_sign"
            | "crypto/crypto_migration"
            | "crypto/crypto_suite"
            | "crypto/vault_secret"
            | "io/compress_gzip"
            | "io/compress_zstd"
    );

    if uses_ffi_bridge && !env.have_cargo {
        eprintln!("note: skipping examples/features/{stem}.jet golden (need cargo for FFI bridge)");
        return;
    }

    let _ffi_lock = uses_ffi_bridge.then(FfiBridgeLock::acquire);
    let compiled = match jet::compile_with_path(&src, &entry.shown) {
        Ok(c) => c,
        Err(diags) => panic!(
            "example {} failed the front end:\n{}",
            stem,
            jet::render_diagnostics(
                &format!("examples/features/{}.{}", stem, env.ext),
                &src,
                &diags
            )
        ),
    };
    let rust_code = compiled.rust;
    let ffi_link = compiled.ffi;
    let user_code = strip_vetted_prelude_modules(&rust_code);

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
        for (i, line) in user_code.lines().enumerate() {
            if let Some(col) = line.find("unsafe") {
                let after = line[col..].trim_start_matches("unsafe").trim_start();
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

    let needs_gtk = stem == "ui/ui_native_linux";
    if needs_gtk && (!env.have_gtk || !env.have_rustc) {
        eprintln!("note: skipping examples/features/{stem}.jet build (need gtk4 + rustc)");
        return;
    }
    let needs_raylib_display = stem == "game/raylib_window";
    if needs_raylib_display && std::env::var("JET_RAYLIB_DISPLAY").as_deref() != Ok("1") {
        eprintln!("note: skipping examples/features/{stem}.jet build (set JET_RAYLIB_DISPLAY=1)");
        return;
    }

    if !env.have_rustc {
        return;
    }
    let flat_stem = stem.replace('/', "_");
    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_golden_{}_{}.rs", std::process::id(), flat_stem));
    let bin = dir.join(format!("jet_golden_{}_{}", std::process::id(), flat_stem));
    fs::write(&rs, &rust_code).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin);
    if let Some(link) = &ffi_link {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        if link.deps_dir.is_dir() {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", link.deps_dir.display()));
        }
    }
    let clinks = jet::resolve_c_links(entry.path.to_str().unwrap())
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

    let mut run_cmd = Command::new(&bin);
    if needs_gtk {
        run_cmd.env("JET_UI_HEADLESS", "1");
    }
    let run = run_cmd.output().unwrap();
    if needs_gtk && !run.status.success() && gtk_loader_unavailable(&run.stderr) {
        eprintln!("note: skipping examples/features/{stem}.jet run (gtk4 runtime loader unavailable)");
        return;
    }
    let err_path = env.ex_dir.join("expected").join(format!("{}.err.out", stem));
    let success_err_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.stderr.out", stem));
    if err_path.exists() {
        let expected_err = fs::read_to_string(&err_path)
            .unwrap_or_else(|_| panic!("missing examples/features/expected/{}.err.out", stem));
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
        let expected = fs::read_to_string(env.ex_dir.join("expected").join(format!("{}.out", stem)))
            .unwrap_or_else(|_| panic!("missing examples/features/expected/{}.out", stem));
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "output mismatch for example {}",
            stem
        );
        if success_err_path.exists() {
            let expected_err = fs::read_to_string(&success_err_path).unwrap_or_else(|_| {
                panic!("missing examples/features/expected/{}.stderr.out", stem)
            });
            assert_eq!(
                String::from_utf8_lossy(&run.stderr),
                expected_err,
                "stderr mismatch for example {}",
                stem
            );
        }
    }
}

fn strip_vetted_prelude_modules(rust_code: &str) -> String {
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
    let s = strip_mod(rust_code, "jet_mem");
    let s = strip_mod(&s, "jet_txn");
    let s = strip_mod(&s, "jet_term_unix");
    let s = strip_mod(&s, "jet_term_windows");
    let s = strip_mod(&s, "jet_os_unix");
    let mut s = strip_mod(&s, "jet_gtk");
    while s.contains("mod user___c_") {
        let before = s.clone();
        s = strip_mod(&s, "user___c_");
        if s == before {
            break;
        }
    }
    s
}
