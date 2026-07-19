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
use common::{
    fixture_filter, fixture_matches, have_rustc, panic_message, test_worker_count, unified_diff,
    FfiBridgeLock,
};

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
    update_expected: bool,
}

fn gtk_loader_unavailable(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr);
    stderr.contains("symbol lookup error")
        && stderr.contains("libgtk-4.so")
        && stderr.contains("undefined symbol")
}

#[test]
fn statement_attributes_codegen_shape() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("examples/features/tooling/statement_attributes.jet");
    let src = fs::read_to_string(&path).unwrap();
    let out = jet::compile_with_path(&src, "examples/features/tooling/statement_attributes.jet")
        .unwrap_or_else(|diags| {
            panic!(
                "statement attributes example failed front end:\n{}",
                jet::render_diagnostics(
                    "examples/features/tooling/statement_attributes.jet",
                    &src,
                    &diags
                )
            )
        });
    assert!(
        !out.rust.contains("\"off\"") && !out.rust.contains("\"off block\""),
        "`@Off` body must not appear in generated Rust:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("#[cfg(not(jet_release))]") && out.rust.contains("debug"),
        "`@DebugOnly` body must be cfg-gated for release:\n{}",
        out.rust
    );
}

#[test]
fn examples_compile_and_run() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ex_dir = root.join("examples/features");
    let ext = jet::Syntax::FILE_EXT;
    let have_rustc = have_rustc();
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
    let filter = fixture_filter("JET_GOLDEN_FILTER");
    entries.retain(|entry| fixture_matches(filter.as_deref(), &entry.shown));
    assert!(
        !entries.is_empty(),
        "JET_GOLDEN_FILTER matched no examples: {}",
        filter.as_deref().unwrap_or("<unfiltered>")
    );
    let update_expected = match std::env::var("JET_UPDATE_GOLDEN") {
        Err(_) => false,
        Ok(value) if value == "1" => {
            assert!(
                filter.is_some(),
                "JET_UPDATE_GOLDEN=1 requires JET_GOLDEN_FILTER so updates stay scoped"
            );
            true
        }
        Ok(value) => panic!("JET_UPDATE_GOLDEN must be exactly 1, got {value:?}"),
    };
    let env = Arc::new(GoldenEnv {
        ex_dir,
        ext,
        have_rustc,
        have_cargo,
        have_gtk,
        update_expected,
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
        filter.is_some() || checked >= 2,
        "expected at least 2 examples, found {}",
        checked
    );
}

fn check_golden_entry(entry: &GoldenEntry, env: &GoldenEnv) {
    // D-JPK-TASKRUN1 / I5 (card #476): task_runner proves both `@Task` entry
    // paths — leaf `greet` stays callable while sibling `seed` calls it.
    if entry.stem == "devloop/task_runner" {
        check_task_runner_tasks(entry, env);
        return;
    }

    if entry.stem.starts_with("lowlevel/polyglot_") {
        check_polyglot_binder_example(entry, env);
        return;
    }

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
    let compiled_result = if src.contains("fn build(") {
        jet::compile_programmable_build(
            entry.path.to_str().expect("example path is utf8"),
            &[],
        )
    } else {
        jet::compile_with_path(&src, &entry.shown)
    };
    let compiled = match compiled_result {
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
        assert_eq!(
            run.status.code(),
            Some(70),
            "exit code mismatch for example {}",
            stem
        );
        let actual_err = String::from_utf8_lossy(&run.stderr);
        if env.update_expected {
            fs::write(&err_path, actual_err.as_bytes()).unwrap();
        } else {
            let expected_err = fs::read_to_string(&err_path)
                .unwrap_or_else(|_| panic!("missing examples/features/expected/{}.err.out", stem));
            if actual_err != expected_err {
                panic!(
                    "stderr mismatch for example {stem}:\n{}",
                    unified_diff(
                        &format!("examples/features/expected/{stem}.err.out"),
                        &format!("examples/features/{stem} stderr (actual)"),
                        &expected_err,
                        &actual_err,
                    )
                );
            }
        }
    } else {
        assert!(
            run.status.success(),
            "example {} failed at runtime:\nstdout: {}\nstderr: {}",
            stem,
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let out_path = env.ex_dir.join("expected").join(format!("{}.out", stem));
        assert!(
            out_path.is_file(),
            "missing examples/features/expected/{stem}.out; update mode never creates a new channel"
        );
        let actual = String::from_utf8_lossy(&run.stdout);
        if env.update_expected {
            fs::write(&out_path, actual.as_bytes()).unwrap();
        } else {
            let expected = fs::read_to_string(&out_path).unwrap();
            if actual != expected {
                panic!(
                    "output mismatch for example {stem}:\n{}",
                    unified_diff(
                        &format!("examples/features/expected/{stem}.out"),
                        &format!("examples/features/{stem} stdout (actual)"),
                        &expected,
                        &actual,
                    )
                );
            }
        }
        if success_err_path.exists() {
            let actual_err = String::from_utf8_lossy(&run.stderr);
            if env.update_expected {
                fs::write(&success_err_path, actual_err.as_bytes()).unwrap();
            } else {
                let expected_err = fs::read_to_string(&success_err_path).unwrap_or_else(|_| {
                    panic!("missing examples/features/expected/{}.stderr.out", stem)
                });
                if actual_err != expected_err {
                    panic!(
                        "stderr mismatch for example {stem}:\n{}",
                        unified_diff(
                            &format!("examples/features/expected/{stem}.stderr.out"),
                            &format!("examples/features/{stem} stderr (actual)"),
                            &expected_err,
                            &actual_err,
                        )
                    );
                }
            }
        }
    }
}

/// I5 for card #502: each managed-runtime example binds its checked foreign
/// source through the public CLI, then runs the generated project binding.
/// Generated caches and native archives stay temp-only and host-specific.
fn check_polyglot_binder_example(entry: &GoldenEntry, env: &GoldenEnv) {
    let (language, package, foreign_source) = match entry.stem.as_str() {
        "lowlevel/polyglot_go" => ("go", "handles", "handles.go"),
        "lowlevel/polyglot_java" => ("java", "counter", "Counter.java"),
        "lowlevel/polyglot_dotnet" => ("cs", "counter", "Counter.cs"),
        "lowlevel/polyglot_fortran" => ("fortran", "matrix", "matrix.f90"),
        other => panic!("unknown polyglot golden `{other}`"),
    };
    if !env.have_rustc || !env.have_cargo {
        eprintln!(
            "note: skipping examples/features/{} golden (need provisioned compiler toolchain)",
            entry.stem
        );
        return;
    }

    let _ffi_lock = FfiBridgeLock::acquire();
    let flat_stem = entry.stem.replace('/', "_");
    let dir = std::env::temp_dir().join(format!(
        "jet_golden_{}_{}",
        std::process::id(),
        flat_stem
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source_dir = entry.path.parent().expect("polyglot example directory");
    fs::copy(&entry.path, dir.join("main.jet")).unwrap();
    fs::copy(
        source_dir.join(foreign_source),
        dir.join(foreign_source),
    )
    .unwrap();

    let bind = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "bind", language, foreign_source, "--pkg", package])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "{} binder example failed:\n{}",
        language,
        String::from_utf8_lossy(&bind.stderr)
    );

    let run = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{} binder example failed at runtime:\nstdout: {}\nstderr: {}",
        language,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let out_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.out", entry.stem));
    assert!(
        out_path.is_file(),
        "missing examples/features/expected/{}.out",
        entry.stem
    );
    let actual = String::from_utf8_lossy(&run.stdout);
    if env.update_expected {
        fs::write(&out_path, actual.as_bytes()).unwrap();
    } else {
        let expected = fs::read_to_string(&out_path).unwrap();
        if actual != expected {
            panic!(
                "output mismatch for example {}:\n{}",
                entry.stem,
                unified_diff(
                    &format!("examples/features/expected/{}.out", entry.stem),
                    &format!("examples/features/{}/main.jet stdout (actual)", entry.stem),
                    &expected,
                    &actual,
                )
            );
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

/// I5 for `examples/features/devloop/task_runner.jet`: compile+run both
/// `@Task` entries via `compile_with_entry` (same path as `jet run --task`).
fn check_task_runner_tasks(entry: &GoldenEntry, env: &GoldenEnv) {
    let src = fs::read_to_string(&entry.path).unwrap();
    let path = entry.path.to_str().expect("example path is utf8");
    for (task, expected_name) in [("greet", "task_runner.greet"), ("seed", "task_runner.seed")] {
        let compiled = match jet::compile_with_entry(path, task) {
            Ok(c) => c,
            Err(diags) => panic!(
                "task_runner --task={task} failed the front end:\n{}",
                jet::render_diagnostics(&entry.shown, &src, &diags)
            ),
        };
        assert!(
            !strip_vetted_prelude_modules(&compiled.rust).contains("unsafe"),
            "generated Rust for task_runner --task={task} contains ungated `unsafe`"
        );
        assert!(
            compiled.rust.contains("fn main()"),
            "generated Rust for task_runner --task={task} has no fn main"
        );
        if !env.have_rustc {
            continue;
        }
        let dir = std::env::temp_dir();
        let rs = dir.join(format!(
            "jet_golden_{}_{}_{}.rs",
            std::process::id(),
            "devloop_task_runner",
            task
        ));
        let bin = dir.join(format!(
            "jet_golden_{}_{}_{}",
            std::process::id(),
            "devloop_task_runner",
            task
        ));
        fs::write(&rs, &compiled.rust).unwrap();
        let out = Command::new("rustc")
            .args(["--edition", "2021"])
            .arg(&rs)
            .arg("-o")
            .arg(&bin)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "I2 violated: rustc rejected task_runner --task={task}:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().unwrap();
        assert!(
            run.status.success(),
            "task_runner --task={task} failed at runtime:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let out_path = env
            .ex_dir
            .join("expected")
            .join(format!("devloop/{expected_name}.out"));
        assert!(
            out_path.is_file(),
            "missing examples/features/expected/devloop/{expected_name}.out"
        );
        let actual = String::from_utf8_lossy(&run.stdout);
        if env.update_expected {
            fs::write(&out_path, actual.as_bytes()).unwrap();
        } else {
            let expected = fs::read_to_string(&out_path).unwrap();
            if actual != expected {
                panic!(
                    "output mismatch for task_runner --task={task}:\n{}",
                    unified_diff(
                        &format!("examples/features/expected/devloop/{expected_name}.out"),
                        &format!("examples/features/devloop/task_runner --task={task} stdout"),
                        &expected,
                        &actual,
                    )
                );
            }
        }
        let _ = fs::remove_file(&rs);
        let _ = fs::remove_file(&bin);
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
    let s = strip_mod(&s, "jet_atomic_windows");
    // D-CRYPTO-RNG1=A: direct OS entropy calls and volatile zeroization live in
    // one std-only vetted module shared byte-for-byte by AOT and the FFI bridge.
    let s = strip_mod(&s, "jet_crypto_entropy");
    let s = strip_mod(&s, "jet_gtk");
    // Tower #126 / I1: the emitted scheduler ships raw epoll/kqueue syscalls, the
    // only `unsafe` in it. They live in the `jet:scheduler-native` vetted region
    // (the sole runtime backend that needs `extern "C"`); drop that region before
    // the unsafe scan, exactly as the codegen delimits it in Prelude/Scheduler.rs.
    let mut s = strip_region(
        &s,
        "// jet:scheduler-native-begin",
        "// jet:scheduler-native-end",
    );
    s = strip_region(
        &s,
        "// JET_VETTED_UNSAFE_BEGIN: jet_env_windows",
        "// JET_VETTED_UNSAFE_END: jet_env_windows",
    );
    while s.contains("mod user___c_") {
        let before = s.clone();
        s = strip_mod(&s, "user___c_");
        if s == before {
            break;
        }
    }
    s
}

/// Remove the inclusive text span between `begin` and `end` markers (used to drop
/// a vetted `unsafe` region from the I1 scan without touching the built program).
fn strip_region(src: &str, begin: &str, end: &str) -> String {
    match (src.find(begin), src.find(end)) {
        (Some(b), Some(e)) if e >= b => {
            let mut s = src[..b].to_string();
            s.push_str(&src[e + end.len()..]);
            s
        }
        _ => src.to_string(),
    }
}
