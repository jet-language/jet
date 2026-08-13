//! Golden tests: every example program must pass the front end, and (when
//! rustc is available) build and print exactly its expected output.
//! Examples are the executable spec (invariant I5).
//!
//! `serde/encoding*` examples run via `jet run --release` (D-LENS-RUN1) because
//! strict default JIT cannot lower the full encoding prelude yet (#728).
//!
//! Also enforces:
//!   I1 — generated code never contains `unsafe`
//!   I2 — rustc accepting the generated code; a rejection here is a
//!        front-end soundness bug, reported loudly

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

mod common;
use common::{
    add_generated_rust, fixture_filter, fixture_matches, have_rustc, panic_message,
    strip_vetted_prelude_modules, test_worker_count, unified_diff, FfiBridgeLock,
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

/// Keep golden's compiler and embedded-runtime caches private to this test
/// process. The normal caches are intentionally shared by `jet` invocations,
/// but parallel test binaries can delete or republish artifacts while a
/// release example is linking them. Golden examples must prove generated code,
/// not depend on another suite's cache timing.
struct GoldenRuntimeCache {
    path: PathBuf,
}

impl Drop for GoldenRuntimeCache {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn gtk_loader_unavailable(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr);
    stderr.contains("symbol lookup error")
        && stderr.contains("libgtk-4.so")
        && stderr.contains("undefined symbol")
}

fn assert_front_end(entry: &GoldenEntry, src: &str) {
    let diagnostics = jet::check_with_path(entry.path.to_str().expect("example path is utf8"));
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error)),
        "example {} failed the front end:\n{}",
        entry.stem,
        jet::render_diagnostics(&entry.shown, src, &diagnostics)
    );
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
        "`#Off` body must not appear in generated Rust:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("#[cfg(not(jet_release))]") && out.rust.contains("debug"),
        "`#DebugOnly` body must be cfg-gated for release:\n{}",
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
    // (the nix dev shell).
    let have_gtk = Command::new("pkg-config")
        .args(["--exists", "gtk4"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let runtime_cache = std::env::temp_dir().join(format!(
        "jet-golden-runtime-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_nanos()
    ));
    let build_cache = runtime_cache.join("build-cache");
    let ffi_cache = runtime_cache.join("ffi");
    std::env::set_var("JET_CACHE_DIR", &build_cache);
    std::env::set_var("JET_FFI_CACHE_DIR", &ffi_cache);
    std::env::set_var("JET_RUNTIME_CACHE_DIR", &runtime_cache);
    let _runtime_cache = GoldenRuntimeCache {
        path: runtime_cache,
    };
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
            // A package manifest is Jet-shaped data, not an executable
            // example. Keep the manifest beside module examples without
            // turning it into a golden entry.
            if path.file_name().and_then(|name| name.to_str()) == Some("package.jet") {
                continue;
            }
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
        // Match `scripts/agent/jet-env` / nix RUST_MIN_STACK (~60MiB). Default
        // worker stacks (~2–8MiB) overflow on large `if subject OP { … }` tables
        // (D-IFDIST1 value/statement dispatch) during sema.
        handles.push(
            std::thread::Builder::new()
                .stack_size(64 * 1024 * 1024)
                .spawn(move || loop {
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
                })
                .expect("golden worker thread"),
        );
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

/// D-LENS-RUN1 / #728: strict JIT cannot lower the full encoding prelude yet;
/// prove serde encoding examples via native AOT (`jet run --release`).
fn golden_uses_release_run(stem: &str) -> bool {
    stem.starts_with("serde/encoding")
}

fn check_golden_entry(entry: &GoldenEntry, env: &GoldenEnv) {
    // D-JPK-TASKRUN1 / I5 (card #476): job_runner proves both `#Job` entry
    // paths — leaf `greet` stays callable while sibling `seed` calls it.
    if entry.stem == "devloop/job_runner" {
        check_job_runner_jobs(entry, env);
        return;
    }

    if entry.stem.starts_with("lowlevel/polyglot_") {
        check_polyglot_binder_example(entry, env);
        return;
    }

    if golden_uses_release_run(&entry.stem) {
        check_golden_entry_release_run(entry, env);
        return;
    }

    let src = fs::read_to_string(&entry.path).unwrap();
    let stem = entry.stem.as_str();
    let needs_gtk = stem == "ui/ui_native_linux";
    if needs_gtk && !env.have_gtk {
        assert_front_end(entry, &src);
        eprintln!(
            "note: front end checked; skipping examples/features/{stem}.jet build (need gtk4)"
        );
        return;
    }
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
            | "crypto/auth_tokens"
            | "io/compress_gzip"
            | "io/compress_zstd"
    );

    if uses_ffi_bridge && !env.have_cargo {
        assert_front_end(entry, &src);
        eprintln!(
            "note: front end checked; skipping examples/features/{stem}.jet golden (need cargo for FFI bridge)"
        );
        return;
    }

    let _ffi_lock = uses_ffi_bridge.then(FfiBridgeLock::acquire);
    let compiled_result = if has_package_build_entry(&entry.path, &src) {
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
    assert!(
        !compiled.lints.iter().any(|diagnostic| diagnostic.code == "L0507"),
        "feature example {stem} still teaches the discouraged braced/chained branch form:\n{}",
        jet::render_diagnostics(&entry.shown, &src, &compiled.lints)
    );
    let rust_code = compiled.rust;
    let ffi_link = compiled.ffi;
    let user_code = strip_vetted_prelude_modules(&rust_code);

    if stem == "lowlevel/lowlevel"
        || stem == "lowlevel/pointer_cast_deref"
        || stem == "lowlevel/inline_c"
        || stem == "lowlevel/inline_asm"
        || stem == "lowlevel/unsafe_obligations"
        || stem == "lowlevel/mmio_board_write"
        || stem == "memory/rawptr"
        || stem == "memory/pin"
        || stem == "io/os_process_control"
        || stem == "io/process_exit_cleanup"
        || stem == "effects/single_use_discard"
        || stem == "memory/uninit"
        || stem == "crypto/crypto_migration"
        || stem == "crypto/vault_keys"
    {
        assert!(
            user_code.contains("unsafe"),
            "the low-level example {} should exercise the gated `unsafe` tier",
            stem
        );
        for (i, line) in user_code.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
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
            !user_code.contains("unsafe fn __jet_"),
            "raylib user functions must stay safe; bridge unsafe stays in vetted prelude"
        );
    } else {
        assert!(
            !user_code.contains("unsafe"),
            "generated Rust for {} contains `unsafe` outside vetted memory helpers",
            stem
        );
    }
    assert!(
        rust_code.contains("fn main()"),
        "generated Rust for {} has no fn main",
        stem
    );

    if needs_gtk && !env.have_rustc {
        eprintln!(
            "note: front end checked; skipping examples/features/{stem}.jet build (need gtk4 + rustc)"
        );
        return;
    }
    let needs_raylib_display = stem == "game/raylib_window";
    if needs_raylib_display && std::env::var("JET_RAYLIB_DISPLAY").as_deref() != Ok("1") {
        eprintln!(
            "note: front end checked; skipping examples/features/{stem}.jet build (set JET_RAYLIB_DISPLAY=1)"
        );
        return;
    }

    if !env.have_rustc {
        return;
    }
    let flat_stem = stem.replace('/', "_");
    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_golden_{}_{}.rs", std::process::id(), flat_stem));
    let bin = dir.join(format!("jet_golden_{}_{}", std::process::id(), flat_stem));
    let mut rustc_cmd = Command::new("rustc");
    add_generated_rust(
        &mut rustc_cmd,
        &rs,
        &rust_code,
        ffi_link.is_some(),
        &[],
    );
    rustc_cmd.arg("-o").arg(&bin);
    if let Some(link) = &ffi_link {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
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
    let run = if stem == "io/terminal_parity" {
        use std::io::Write;
        use std::process::Stdio;
        let mut child = run_cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"\nnot-a-number\n3\n2\n")
            .unwrap();
        child.wait_with_output().unwrap()
    } else {
        run_cmd.output().unwrap()
    };
    if needs_gtk && !run.status.success() && gtk_loader_unavailable(&run.stderr) {
        eprintln!("note: skipping examples/features/{stem}.jet run (gtk4 runtime loader unavailable)");
        return;
    }
    // Suffix rule (examples/README.md "Auxiliary golden stream suffix"):
    // `.err.out` = expected non-zero exit (panic/uncaught Err); `.stderr.out`
    // = expected exit 0 with pinned incidental stderr. Never a third suffix.
    let err_path = env.ex_dir.join("expected").join(format!("{}.err.out", stem));
    let success_err_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.stderr.out", stem));
    if err_path.exists() {
        let code = run.status.code();
        assert!(
            code == Some(70) || code == Some(1),
            "exit code mismatch for example {stem}: expected 70 (panic) or 1 (uncaught Err), got {code:?}"
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
        let actual_raw = String::from_utf8_lossy(&run.stdout);
        // CLI usage intentionally names argv[0]. Golden binaries have a
        // process-specific temporary name, so normalize only that product
        // field while keeping every other byte exact.
        let actual_normalized;
        let actual = if stem.starts_with("cli/") {
            let binary_name = bin
                .file_name()
                .and_then(|name| name.to_str())
                .expect("golden binary name is utf8");
            actual_normalized =
                actual_raw.replacen(&format!("Usage: {binary_name}"), "Usage: <program>", 1);
            actual_normalized.as_str()
        } else {
            actual_raw.as_ref()
        };
        if env.update_expected {
            fs::create_dir_all(out_path.parent().unwrap()).unwrap();
            fs::write(&out_path, actual.as_bytes()).unwrap();
        } else {
            assert!(
                out_path.is_file(),
                "missing examples/features/expected/{stem}.out; run with JET_UPDATE_GOLDEN=1 and a scoped JET_GOLDEN_FILTER to create it"
            );
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

/// Use the production package resolver to decide whether an example needs the
/// programmable-build path. The source-text check remains for the retired
/// `main.jet` convention, which is intentionally excluded from package-wide
/// source discovery as the legacy entry filename.
fn has_package_build_entry(path: &Path, source: &str) -> bool {
    if source.contains("fn build(") {
        return true;
    }
    let Some(root) = path.parent() else {
        return false;
    };
    let Ok(resolver) = jet::Authority::AuthorityResolver::open(root) else {
        return false;
    };
    let facts = match resolver.checked_manifest(Path::new(".")) {
        Ok(manifest) => manifest.facts,
        Err(error) if error.is_missing() => jet::Package::PackageFacts::default(),
        Err(_) => return false,
    };
    facts
        .resolve_build_entry_checked(&resolver)
        .ok()
        .flatten()
        .is_some()
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
    let src = fs::read_to_string(&entry.path).unwrap();
    assert_front_end(entry, &src);
    if !env.have_rustc || !env.have_cargo {
        eprintln!(
            "note: front end checked; skipping examples/features/{} golden (need provisioned compiler toolchain)",
            entry.stem
        );
        return;
    }
    // These examples exercise the real foreign-language inspection path. The
    // source fixture is still valid when a host does not provision that
    // language tool, so use the same capability-gated skip shape as the GTK
    // and raylib examples instead of turning an environmental absence into a
    // golden failure.
    let tool = match language {
        "go" => "go",
        "fortran" => "gfortran",
        "java" => "javac",
        "cs" => "dotnet",
        _ => unreachable!("binder language has no provisioned tool"),
    };
    let have_tool = Command::new(tool)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !have_tool {
        eprintln!(
            "note: front end checked; skipping examples/features/{} golden (need provisioned {})",
            entry.stem, tool
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

/// I5 for `examples/features/devloop/job_runner.jet`: one generated binary
/// dispatches every named `#Job` entry through argv.
fn check_job_runner_jobs(entry: &GoldenEntry, env: &GoldenEnv) {
    let src = fs::read_to_string(&entry.path).unwrap();
    let path = entry.path.to_str().expect("example path is utf8");
    let compiled = jet::compile_with_path(&src, path).unwrap_or_else(|diags| {
        panic!(
            "job_runner subcommand table failed the front end:\n{}",
            jet::render_diagnostics(&entry.shown, &src, &diags)
        )
    });
    assert!(
        !strip_vetted_prelude_modules(&compiled.rust).contains("unsafe"),
        "generated Rust for job_runner contains ungated `unsafe`"
    );
    assert!(compiled.rust.contains("fn main()"), "job_runner has no fn main");
    if !env.have_rustc {
        return;
    }
    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_golden_{}_devloop_job_runner.rs", std::process::id()));
    let bin = dir.join(format!("jet_golden_{}_devloop_job_runner", std::process::id()));
    let mut rustc = Command::new("rustc");
    add_generated_rust(&mut rustc, &rs, &compiled.rust, compiled.ffi.is_some(), &[]);
    rustc.arg("-o").arg(&bin);
    if let Some(link) = &compiled.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc.arg("-L").arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let out = rustc.output().unwrap();
    assert!(
        out.status.success(),
        "I2 violated: rustc rejected job_runner:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for (job, expected_name) in [("greet", "job_runner.greet"), ("seed", "job_runner.seed")] {
        let run = Command::new(&bin).arg(job).output().unwrap();
        assert!(
            run.status.success(),
            "job_runner subcommand `{job}` failed at runtime:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let out_path = env.ex_dir.join("expected").join(format!("devloop/{expected_name}.out"));
        assert!(out_path.is_file(), "missing expected output for `{job}`");
        let actual = String::from_utf8_lossy(&run.stdout);
        if env.update_expected {
            fs::write(&out_path, actual.as_bytes()).unwrap();
        } else {
            let expected = fs::read_to_string(&out_path).unwrap();
            if actual != expected {
                panic!(
                    "output mismatch for job_runner subcommand `{job}`:\n{}",
                    unified_diff(
                        &format!("examples/features/expected/devloop/{expected_name}.out"),
                        &format!("examples/features/devloop/job_runner subcommand `{job}` stdout"),
                        &expected,
                        &actual,
                    )
                );
            }
        }
    }
    let _ = fs::remove_file(&rs);
    let _ = fs::remove_file(&bin);
}

fn check_golden_entry_release_run(entry: &GoldenEntry, env: &GoldenEnv) {
    let stem = entry.stem.as_str();
    let src = fs::read_to_string(&entry.path).unwrap();
    let compiled = jet::compile_with_path(&src, &entry.shown).unwrap_or_else(|diags| {
        panic!(
            "example {} failed the front end:\n{}",
            stem,
            jet::render_diagnostics(&entry.shown, &src, &diags)
        )
    });
    assert!(
        compiled.rust.contains("fn main()"),
        "generated Rust for {} has no fn main",
        stem
    );

    let jet_bin = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    let mut run_cmd = Command::new(&jet_bin);
    run_cmd.args(["run", "--release", entry.path.to_str().expect("example path is utf8")]);
    let run = run_cmd.output().expect("jet run --release should spawn");
    assert!(
        run.status.success(),
        "example {} failed at runtime via `jet run --release`:\nstdout: {}\nstderr: {}",
        stem,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let out_path = env.ex_dir.join("expected").join(format!("{stem}.out"));
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
                    &format!("examples/features/{stem} stdout (`jet run --release`)"),
                    &expected,
                    &actual,
                )
            );
        }
    }
}

#[test]
fn watcher_process_probe_is_vetted_without_hiding_user_unsafe() {
    let generated = "// JET_VETTED_UNSAFE_BEGIN: jet_watch_process_probe\nunsafe { ffi() }\n// JET_VETTED_UNSAFE_END: jet_watch_process_probe\nunsafe { user_pointer() }";
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("ffi()"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}

#[test]
fn shared_guard_runtime_is_vetted_without_hiding_user_unsafe() {
    let generated = "// jet:shared-guard-internal-begin\nunsafe { first() }\n// jet:shared-guard-internal-end\n// jet:shared-guard-internal-begin\nunsafe { second() }\n// jet:shared-guard-internal-end\nunsafe { user_pointer() }";
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("first()"));
    assert!(!stripped.contains("second()"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}
