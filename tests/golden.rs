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
    add_generated_rust, example_stdin, fixture_filter, fixture_matches, have_rustc, panic_message,
    strip_vetted_prelude_modules, test_worker_count, unified_diff, unsafe_keyword_columns,
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

/// Keep golden's per-run program and FFI-bridge caches private to this test
/// process. Those caches are not content-verified end to end, and parallel test
/// binaries can delete or republish their artifacts while a release example is
/// linking them. Golden examples must prove generated code, not depend on
/// another suite's cache timing.
///
/// The runtime rlib cache is deliberately NOT in here: it is content-addressed
/// and digest-verified, so it is safe to share and expensive to rebuild.
struct GoldenScratch {
    path: PathBuf,
}

impl Drop for GoldenScratch {
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

fn assert_compute_vulkan_webgpu_output(stdout: &str) {
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "unexpected output from tooling/compute_vulkan_webgpu: {stdout:?}"
    );
    assert!(
        matches!(lines[0], "vulkan:accepted" | "vulkan:rejected"),
        "Vulkan must report its real availability: {stdout:?}"
    );
    assert_eq!(
        lines[1], "webgpu:rejected",
        "native WebGPU must fail closed: {stdout:?}"
    );
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

/// Does running this example start a service that serves until the process is
/// stopped? One canonical answer, `jet::AST::bundle_serves_until_stopped` — the
/// same fact `tests/dev_parts/support.rs` reads, never a stem list.
fn example_serves_until_stopped(path: &Path) -> bool {
    let Some(path) = path.to_str() else {
        return false;
    };
    let Ok(mut bundle) = jet::Loader::load_entry(path) else {
        return false;
    };
    let _ = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    jet::AST::bundle_serves_until_stopped(&bundle)
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
    let scratch = std::env::temp_dir().join(format!(
        "jet-golden-scratch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_nanos()
    ));
    let build_cache = scratch.join("build-cache");
    let ffi_cache = scratch.join("ffi");
    // #2074: runtime rlibs are content-addressed on (runtime source, exported
    // source, rustc identity, flags, env) and re-verified against
    // `artifact.sha256` on every hit, so sharing them across runs cannot
    // resurrect a stale runtime — a wrong key is a miss, never a reuse. A
    // per-run directory would instead pay one cold runtime compile per key on
    // every golden run, which is the exact cost this substrate exists to
    // remove. Living under the target dir keeps `cargo clean` as the reset.
    let runtime_cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("jet-runtime-rlibs");
    std::env::set_var("JET_CACHE_DIR", &build_cache);
    std::env::set_var("JET_FFI_CACHE_DIR", &ffi_cache);
    std::env::set_var("JET_RUNTIME_CACHE_DIR", &runtime_cache);
    let _scratch = GoldenScratch { path: scratch };
    if !have_rustc {
        eprintln!("note: rustc not found; checking codegen only, skipping build+run");
    }

    // Recursive discovery: examples/features/<topic>/<name>.jet or
    // examples/features/<topic>/<name>/run.jet. Test id (`stem`) is the
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
                let run = path.join(format!("run.{}", ext));
                if run.is_file() {
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    let stem = format!("{}/{}", topic_name, name);
                    entries.push(GoldenEntry {
                        path: run.clone(),
                        stem: stem.clone(),
                        shown: format!("examples/features/{}/run.{}", stem, ext),
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
        handle
            .join()
            .expect("golden worker panicked outside harness");
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

/// I1's one exception: the examples that TEACH the audited `#Unsafe` tier, so
/// their generated Rust must carry `unsafe` — in gated `unsafe { … }` /
/// `unsafe fn` / `unsafe extern` form only. Every other stem must generate none.
///
/// DERIVED, never hand-typed: the repository already keeps exactly one registry
/// of approved user-written unsafe regions — the `unsafe-ratchet` baseline in
/// `docs/spec/safety.md`, scanned and enforced by
/// `scripts/agent/check-unsafe-ratchet.mjs` (`tests/unsafe_ratchet.rs`,
/// `scripts/agent/verify-full.sh`). A new `#Unsafe` gate cannot land without
/// refreshing that baseline in the same change, so reading it here makes the two
/// facts one fact.
///
/// A hand copy was a second registry of the same fact, and it rotted twice: the
/// dev-sentry/gate-ladder rows were red from 2026-08-13 with nothing watching
/// `golden`, and `a5bea5f25` added an approved gate at
/// `examples/features/crypto/random_api_split.jet:28` ("compare the typed and
/// raw HKDF rungs"), refreshed the baseline as the ratchet demands, and left the
/// list behind — so `golden` reported an APPROVED region as an I1 violation.
///
/// Deriving cannot widen I1: a region only enters the baseline through the
/// ratchet, a stem with no approved region that emits `unsafe` still fails the
/// negative assertion below, and a stem with one that stops emitting still fails
/// the positive one.
///
/// Both directions are still asserted below, and the gated form is scanned with
/// `common::unsafe_keyword_columns` — three audited stems carry the word in
/// their own file name, which a plain substring scan could not tell from code.
static GATED_UNSAFE_STEMS: std::sync::LazyLock<std::collections::HashSet<String>> =
    std::sync::LazyLock::new(|| {
        let baseline_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/spec/safety.md");
        let baseline = fs::read_to_string(&baseline_path).unwrap_or_else(|err| {
            panic!(
                "cannot read the approved unsafe-region baseline {}: {err}",
                baseline_path.display()
            )
        });
        let data = baseline
            .split_once("<!-- unsafe-ratchet:data")
            .and_then(|(_, rest)| rest.split_once("\n-->"))
            .map(|(data, _)| data)
            .unwrap_or_else(|| {
                panic!(
                    "{} has no `unsafe-ratchet:data` baseline block",
                    baseline_path.display()
                )
            });
        let example_prefix = "examples/features/";
        let jet_suffix = format!(".{}", jet::Syntax::FILE_EXT);
        let stems: std::collections::HashSet<String> = data
            .lines()
            .filter_map(|line| {
                let quoted = line.trim().strip_prefix("\"file\":")?.trim_start();
                let file = quoted.strip_prefix('"')?.split('"').next()?;
                let stem = file
                    .strip_prefix(example_prefix)?
                    .strip_suffix(&jet_suffix)?;
                Some(stem.to_owned())
            })
            .collect();
        assert!(
            !stems.is_empty(),
            "{} records no approved `examples/features` unsafe region; the audited-tier \
             examples (lowlevel/memory/effects/crypto) must appear there",
            baseline_path.display()
        );
        stems
    });

fn check_golden_entry(entry: &GoldenEntry, env: &GoldenEnv) {
    // D-JPK-TASKRUN1 / I5 (card #476): job_runner proves both `#Job` entry
    // paths — leaf `greet` stays callable while sibling `seed_data` calls it.
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

    // Both compile entry points must name the example with ONE spelling: the
    // repo-relative `entry.shown` every golden was authored against. `entry.path`
    // is absolute (`root.join("examples/features")`), and Loader gives the entry
    // module `display = entry_path` verbatim, so passing it here baked an absolute
    // Jet path into `cx.file` for all 59 examples that `has_package_build_entry`
    // routes this way — every file directly in `examples/features/tooling/` and
    // `examples/features/comptime/`, because those directories have no manifest and
    // a SIBLING (`tooling/compiler_api.jet`, `comptime/build_stamp.jet`) supplies
    // the discovered `fn build`. Examples that print their own source location then
    // disagreed with their golden: `tooling/provenance_track` (`Float.origin()`,
    // D-PROVENANCE1) and `tooling/panic_report` (E3001 `--> file:line`).
    let compiled_result = if has_package_build_entry(&entry.path, &src) {
        jet::compile_programmable_build(&entry.shown, &[])
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
        !compiled
            .lints
            .iter()
            .any(|diagnostic| diagnostic.code == "L0507"),
        "feature example {stem} still teaches the discouraged braced/chained branch form:\n{}",
        jet::render_diagnostics(&entry.shown, &src, &compiled.lints)
    );
    let rust_code = compiled.rust;
    let ffi_link = compiled.ffi;
    let user_code = strip_vetted_prelude_modules(&rust_code);

    if GATED_UNSAFE_STEMS.contains(stem) {
        assert!(
            user_code
                .lines()
                .any(|line| !unsafe_keyword_columns(line).is_empty()),
            "the audited example {} should exercise the gated `unsafe` tier",
            stem
        );
        for (i, line) in user_code.lines().enumerate() {
            for col in unsafe_keyword_columns(line) {
                let after = line[col + "unsafe".len()..].trim_start();
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
        let leftover: Vec<String> = user_code
            .lines()
            .enumerate()
            .filter(|(_, line)| !unsafe_keyword_columns(line).is_empty())
            .map(|(i, line)| format!("  line {}: {}", i + 1, line.trim()))
            .collect();
        assert!(
            leftover.is_empty(),
            "generated Rust for {} contains `unsafe` outside vetted memory helpers:\n{}",
            stem,
            leftover.join("\n")
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
    let rs = dir.join(format!(
        "jet_golden_{}_{}.rs",
        std::process::id(),
        flat_stem
    ));
    let bin = dir.join(format!("jet_golden_{}_{}", std::process::id(), flat_stem));
    let mut rustc_cmd = Command::new("rustc");
    add_generated_rust(&mut rustc_cmd, &rs, &rust_code, ffi_link.is_some(), &[]);
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
    // #2016 / D-WEBAPP-SERVE1=D: an `App`-returning entry is handed to
    // `App::serve`, which binds a listener and then serves until the process is
    // stopped. Such a program has no terminating stdout and no exit code of its
    // own, so it cannot have a `.out` golden — running it here would hang the
    // suite (or, for the three `App` examples, race a fixed port). It stays in
    // the COMPILE universe: front end, I1, and I2 above all still apply.
    // Held back by the canonical derived predicate — the same one
    // `tests/dev_parts/support.rs` uses — so a fourth service example is
    // classified instead of discovered as a timeout.
    if example_serves_until_stopped(&entry.path) {
        eprintln!(
            "note: built examples/features/{stem}.jet; not run (service entry serves until stopped)"
        );
        return;
    }

    let mut run_cmd = Command::new(&bin);
    if needs_gtk {
        run_cmd.env("JET_UI_HEADLESS", "1");
    }
    // An interactive example only reproduces its golden against the answers
    // that golden was recorded with. `common::example_stdin` is the one home
    // for those answers (I8); this run asks for them by stem instead of
    // naming the example and restating its bytes.
    let run = if let Some(answers) = example_stdin(stem) {
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
            .write_all(answers.piped.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    } else {
        run_cmd.output().unwrap()
    };
    if needs_gtk && !run.status.success() && gtk_loader_unavailable(&run.stderr) {
        eprintln!(
            "note: skipping examples/features/{stem}.jet run (gtk4 runtime loader unavailable)"
        );
        return;
    }
    // Suffix rule (examples/README.md "Auxiliary golden stream suffix"):
    // `.err.out` = expected non-zero exit (panic/uncaught Err); `.stderr.out`
    // = expected exit 0 with pinned incidental stderr. Never a third suffix.
    let err_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.err.out", stem));
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
        record_golden_receipt(entry, env, &run, &run.stdout);
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
        // CLI usage intentionally names argv[0]. The golden binary has a
        // process-specific temporary name, so normalize it to the source
        // basename while keeping every other byte exact.
        let actual_normalized;
        let actual = if stem.starts_with("cli/") {
            let source_name = entry
                .path
                .file_stem()
                .and_then(|name| name.to_str())
                .expect("golden source name is utf8");
            let binary_name = bin
                .file_name()
                .and_then(|name| name.to_str())
                .expect("golden binary name is utf8");
            actual_normalized = actual_raw.replacen(
                &format!("Usage: {binary_name}"),
                &format!("Usage: {source_name}"),
                1,
            );
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
            if stem == "tooling/compute_vulkan_webgpu" {
                // Vulkan availability is host-dependent: the shell may expose
                // the loader while the machine still has no usable device.
                // Validate the portable contract instead of pinning one host's
                // acceptance bit in a cross-host golden.
                assert_compute_vulkan_webgpu_output(actual);
            } else if actual != expected {
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
        record_golden_receipt(entry, env, &run, actual.as_bytes());
    }
}

/// Keep the expected channel as the executable-spec input, but record the
/// observed result in the same content-addressed store as CLI test results.
/// A missing expected channel produces no claim; an absent input cannot make a
/// status projection look current.
fn record_golden_receipt(
    entry: &GoldenEntry,
    env: &GoldenEnv,
    output: &std::process::Output,
    stdout: &[u8],
) {
    let err_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.err.out", entry.stem));
    let stdout_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.out", entry.stem));
    let stderr_path = env
        .ex_dir
        .join("expected")
        .join(format!("{}.stderr.out", entry.stem));
    let expected = if err_path.is_file() {
        err_path
    } else if stdout_path.is_file() {
        stdout_path
    } else {
        return;
    };
    let mut expected_paths = vec![expected];
    if stderr_path.is_file() {
        expected_paths.push(stderr_path);
    }
    record_golden_receipt_at_output(
        entry,
        env,
        output,
        stdout,
        &output.stderr,
        "aot",
        None,
        &expected_paths,
        &[],
    );
}

fn record_golden_receipt_at(
    entry: &GoldenEntry,
    env: &GoldenEnv,
    output: &std::process::Output,
    tier: &str,
    variant: Option<&str>,
    expected_paths: &[PathBuf],
    extra_inputs: &[PathBuf],
) {
    record_golden_receipt_at_output(
        entry,
        env,
        output,
        &output.stdout,
        &output.stderr,
        tier,
        variant,
        expected_paths,
        extra_inputs,
    );
}

fn record_golden_receipt_at_output(
    entry: &GoldenEntry,
    env: &GoldenEnv,
    output: &std::process::Output,
    stdout: &[u8],
    stderr: &[u8],
    tier: &str,
    variant: Option<&str>,
    expected_paths: &[PathBuf],
    extra_inputs: &[PathBuf],
) {
    if expected_paths.iter().any(|path| !path.is_file()) {
        return;
    }
    let mut inputs = vec![entry.path.clone()];
    inputs.extend(extra_inputs.iter().cloned().filter(|path| path.is_file()));
    inputs.extend(expected_paths.iter().cloned());
    let root = env
        .ex_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    let store_root = std::env::var_os("JET_RECEIPT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".jet").join("receipts"));
    let store = jet::ReceiptStore::ReceiptStore::new(store_root);
    let mut argv = vec!["golden".into(), entry.stem.clone(), tier.to_string()];
    if let Some(variant) = variant {
        argv.push(variant.to_string());
    }
    let status = output.status.code().unwrap_or(1);
    store
        .record("golden", &argv, &inputs, status, stdout, stderr)
        .unwrap_or_else(|error| {
            panic!(
                "could not record golden receipt for {}: {error}",
                entry.stem
            )
        });
}

/// Use the production package resolver to decide whether an example needs the
/// programmable-build path. The source-text check remains for source files
/// whose package build entry is declared in their ordinary Jet source.
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
        "lowlevel/polyglot_pascal" => ("pascal", "counter", "Counter.pas"),
        "lowlevel/polyglot_ada" => ("ada", "geodesy", "geodesy.ads"),
        "lowlevel/polyglot_fortran" => ("fortran", "matrix", "matrix.f90"),
        "lowlevel/polyglot_tcl" => ("tcl", "eda", "eda.tcl"),
        "lowlevel/polyglot_dart" => ("dart", "callbacks", "callbacks.dart"),
        other => panic!("unknown polyglot golden `{other}`"),
    };
    let src = fs::read_to_string(&entry.path).unwrap();
    if language != "dart" {
        assert_front_end(entry, &src);
    }
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
        "pascal" => "fpc",
        "ada" => "gnatmake",
        "tcl" => "tclsh",
        "dart" => "dart",
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

    let flat_stem = entry.stem.replace('/', "_");
    let dir = std::env::temp_dir().join(format!("jet_golden_{}_{}", std::process::id(), flat_stem));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source_dir = entry.path.parent().expect("polyglot example directory");
    fs::copy(&entry.path, dir.join("run.jet")).unwrap();
    fs::copy(source_dir.join(foreign_source), dir.join(foreign_source)).unwrap();
    if language == "ada" {
        fs::copy(source_dir.join("geodesy.adb"), dir.join("geodesy.adb")).unwrap();
    }
    if language == "dart" {
        fs::copy(source_dir.join("host.dart"), dir.join("host.dart")).unwrap();
    }

    let mut bind = Command::new(env!("CARGO_BIN_EXE_jet"));
    bind.current_dir(&dir).env("NO_COLOR", "1");
    bind.args(["inspect", "bind", language, foreign_source]);
    if language == "dart" {
        bind.args(["--jet", "run.jet"]);
    }
    let bind = bind.args(["--pkg", package]).output().unwrap();
    assert!(
        bind.status.success(),
        "{} binder example failed:\n{}",
        language,
        String::from_utf8_lossy(&bind.stderr)
    );
    if language == "dart" {
        let check = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["check", "run.jet"])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(
            check.status.success(),
            "Dart generated binding failed its front end:\n{}",
            String::from_utf8_lossy(&check.stderr)
        );
    }

    let run = if language == "dart" {
        Command::new("dart")
            .args(["run", "host.dart"])
            .current_dir(&dir)
            .output()
            .unwrap()
    } else {
        Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["run", "run.jet"])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
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
    let mut foreign_inputs = vec![source_dir.join(foreign_source)];
    if language == "ada" {
        foreign_inputs.push(source_dir.join("geodesy.adb"));
    }
    if language == "dart" {
        foreign_inputs.push(source_dir.join("host.dart"));
    }
    record_golden_receipt_at(
        entry,
        env,
        &run,
        "dev",
        None,
        std::slice::from_ref(&out_path),
        &foreign_inputs,
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
                    &format!("examples/features/{}/run.jet stdout (actual)", entry.stem),
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
    let job_runner_user_code = strip_vetted_prelude_modules(&compiled.rust);
    assert!(
        job_runner_user_code
            .lines()
            .all(|line| unsafe_keyword_columns(line).is_empty()),
        "generated Rust for job_runner contains ungated `unsafe`"
    );
    assert!(
        compiled.rust.contains("fn main()"),
        "job_runner has no fn main"
    );
    if !env.have_rustc {
        return;
    }
    let dir = std::env::temp_dir();
    let rs = dir.join(format!(
        "jet_golden_{}_devloop_job_runner.rs",
        std::process::id()
    ));
    let bin = dir.join(format!(
        "jet_golden_{}_devloop_job_runner",
        std::process::id()
    ));
    let mut rustc = Command::new("rustc");
    add_generated_rust(&mut rustc, &rs, &compiled.rust, compiled.ffi.is_some(), &[]);
    rustc.arg("-o").arg(&bin);
    if let Some(link) = &compiled.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let out = rustc.output().unwrap();
    assert!(
        out.status.success(),
        "I2 violated: rustc rejected job_runner:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for (job, expected_name) in [
        ("greet", "job_runner.greet"),
        ("seed_data", "job_runner.seed_data"),
    ] {
        let run = Command::new(&bin).arg(job).output().unwrap();
        assert!(
            run.status.success(),
            "job_runner subcommand `{job}` failed at runtime:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let out_path = env
            .ex_dir
            .join("expected")
            .join(format!("devloop/{expected_name}.out"));
        assert!(out_path.is_file(), "missing expected output for `{job}`");
        record_golden_receipt_at(
            entry,
            env,
            &run,
            "aot",
            Some(job),
            std::slice::from_ref(&out_path),
            &[],
        );
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
    run_cmd.args([
        "run",
        "--release",
        entry.path.to_str().expect("example path is utf8"),
    ]);
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
    record_golden_receipt_at(
        entry,
        env,
        &run,
        "aot-release",
        None,
        std::slice::from_ref(&out_path),
        &[],
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
