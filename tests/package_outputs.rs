//! D-CONF-PLANE1/D-CONF-NAME1 (card #1517): a package.jet `outputs:` block
//! drives entry resolution for both `jet run` (JIT) and `jet build` (AOT) —
//! I9 tier parity for the manifest-driven build path.
//!
//! `examples/features/packages/outputs_build/` has no `main.jet`/`run.jet`
//! convention file. Its `package.jet` uses a *dotted* entry —
//! `outputs: .{ demo: .Executable.{ entry: service.run } }` — which follows
//! `entry.jet`'s `use "service/module" as service` file import into
//! `service/module.jet`. That's
//! deliberate: a single-segment `entry: run` resolves through the existing
//! root-level source lookup, so it can never prove `outputs:` is doing
//! anything.
//! This fixture has no canonical entry and can only resolve through
//! `outputs:`. `entry_resolution_requires_the_outputs_block` below proves it:
//! the same fixture with `outputs:` deleted fails to resolve at all.
//!
//! `golden.rs`'s directory scan (which requires a `main.<ext>`) never
//! discovers this fixture and always compiles a single file directly,
//! bypassing package/output resolution — so this is a dedicated test, not an
//! addition to that harness.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;

fn example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/packages/outputs_build")
}

fn expected_output() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/expected/packages/outputs_build.out");
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {}", path.display()))
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn nested_example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/packages/outputs_nested")
}

#[test]
fn outputs_build_example_has_no_convention_entry_filename() {
    // The whole point of this fixture: no `main.jet`/`run.jet` file exists,
    // and the entry function lives behind an import the migration-era
    // fallback cannot follow.
    let dir = example_dir();
    assert!(!dir.join("main.jet").is_file());
    assert!(!dir.join("run.jet").is_file());
    assert!(dir.join("entry.jet").is_file());
    assert!(dir.join("service/module.jet").is_file());
    let manifest = fs::read_to_string(dir.join("package.jet")).unwrap();
    assert!(manifest.contains("outputs:"), "{manifest}");
    assert!(manifest.contains("entry: service.run"), "{manifest}");
}

#[test]
fn outputs_block_drives_jet_run_jit() {
    let dir = example_dir();
    let out = Command::new(jet_bin())
        .arg("run")
        .current_dir(&dir)
        .output()
        .expect("jet run should execute");
    assert!(
        out.status.success(),
        "jet run failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected_output());
}

#[test]
fn outputs_block_drives_jet_build_aot() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping outputs_block_drives_jet_build_aot (need rustc)");
        return;
    }
    let dir = example_dir();
    let build_dir = dir.join("build");
    let _ = fs::remove_dir_all(&build_dir);

    let build = Command::new(jet_bin())
        .arg("build")
        .current_dir(&dir)
        .output()
        .expect("jet build should execute");
    assert!(
        build.status.success(),
        "jet build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    // `jet build` names the binary after the entry file it resolved
    // (`module`, from `service/module.jet`) — proof the AOT lens took the
    // same `outputs:` path as the JIT lens above, not a `main`/`run`
    // fallback (which would never find a file two directories away from a
    // bare filename convention).
    let binary = build_dir.join("module");
    assert!(
        binary.is_file(),
        "jet build did not produce build/module (resolved via outputs:); found: {:?}",
        fs::read_dir(&build_dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect::<Vec<_>>())
            .unwrap_or_default()
    );

    let run = Command::new(&binary)
        .output()
        .expect("built binary should run");
    assert!(
        run.status.success(),
        "built binary failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected_output());

    let _ = fs::remove_dir_all(&build_dir);
}

#[test]
fn nested_outputs_entry_invokes_leaf_on_every_run_tier() {
    let dir = nested_example_dir();
    assert!(!dir.join("runner.jet").is_file());
    assert!(dir.join("src/cli/main.jet").is_file());
    assert!(!fs::read_to_string(dir.join("src/cli/main.jet"))
        .unwrap()
        .contains("fn run"));

    let mut outputs = Vec::new();
    for args in [
        vec!["run"],
        vec!["run", "--interpret"],
        vec!["run", "--release"],
    ] {
        let out = Command::new(jet_bin())
            .args(&args)
            .current_dir(&dir)
            .output()
            .expect("nested output should execute");
        assert!(
            out.status.success(),
            "nested output {:?} failed:\n{}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        outputs.push(out.stdout);
    }

    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(
        outputs[0],
        fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("examples/features/expected/packages/outputs_nested.out")
        )
        .unwrap()
    );

    if common::have_rustc() {
        let build = Command::new(jet_bin())
            .arg("build")
            .current_dir(&dir)
            .output()
            .expect("nested output build should execute");
        assert!(
            build.status.success(),
            "nested output build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let binary = dir.join("build/main");
        let run = Command::new(&binary)
            .output()
            .expect("nested output binary should run");
        assert!(
            run.status.success(),
            "nested output binary failed:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, outputs[0]);
    }
}

#[test]
fn package_output_selects_non_run_callable_for_run_dev_and_effects() {
    let scratch = common::Scratch::new("package-output-callable");
    fs::write(
        scratch.join("package.jet"),
        "name: \"selected_callable\"\nversion: \"0.1.0\"\nauthority: .{ holds: { deny: [IO] } }\noutputs: .{ app: .Executable{ entry: launch } }\n",
    )
    .unwrap();
    fs::write(
        scratch.join("entry.jet"),
        "fn run() { print(\"wrong callable\") }\npub fn launch() {}\n",
    )
    .unwrap();

    for args in [
        vec!["run"],
        vec!["run", "--interpret"],
        vec!["dev", "--watch=off"],
    ] {
        let output = Command::new(jet_bin())
            .args(&args)
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .output()
            .expect("selected package callable should execute");
        assert!(
            output.status.success(),
            "selected callable {:?} failed:\n{}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout,
            b"",
            "package execution fell back to run for {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stdout)
        );
    }

    let build = Command::new(jet_bin())
        .arg("build")
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .expect("selected package callable should build");
    assert!(
        build.status.success(),
        "selected callable build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let summary = String::from_utf8_lossy(&build.stderr);
    assert!(
        summary.contains("required effects: none"),
        "effect summary used the parked run callable:\n{summary}"
    );
}

#[test]
fn entry_wrapper_uses_the_full_nested_import_qualification_chain() {
    let scratch = common::Scratch::new("nested-entry-wrapper");
    let entry = scratch.join("entry.jet");
    fs::write(
        scratch.join("package.jet"),
        "name: \"nested_entry_wrapper\"\nversion: \"0.1.0\"\noutputs: .{ app: .Executable{ entry: app.leaf } }\n",
    )
    .unwrap();
    fs::write(&entry, "use \"runner\" as runner\n").unwrap();
    fs::write(scratch.join("runner.jet"), "use \"bridge\" as bridge\n").unwrap();
    fs::write(scratch.join("bridge.jet"), "use \"leaf\" as app\n").unwrap();
    fs::write(scratch.join("leaf.jet"), "pub fn leaf() {}\n").unwrap();

    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).expect("chain should load");
    jet::Driver::swap_entry_point(&mut bundle, "leaf");
    let wrapper = bundle.modules[bundle.entry]
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::Func(function) if function.name == "run" => function.body.first(),
            _ => None,
        })
        .expect("entry swap should inject a run wrapper");
    let call_name = match wrapper {
        jet::AST::Stmt::Expr(jet::AST::Expr::Call(call)) => &call.name,
        other => panic!("expected wrapper call, got {other:?}"),
    };
    assert_eq!(call_name, "runner.bridge.app.leaf");

    jet::Driver::compile_bundle_path_with_entry(entry.to_str().unwrap(), "leaf")
        .expect("qualified nested entry wrapper should compile");
}

#[test]
fn package_default_output_alias_invokes_nested_leaf_from_path_and_cwd() {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dogfood/jetpack");
    let fixture_root = package.join("tests/fixtures/phase1");
    let scratch = common::Scratch::new("package-output-default");
    let home = scratch.join("home");
    let store = home.join(".cache/jet-dogfood/jetpack-store");
    fs::create_dir_all(&home).unwrap();

    for mode in [
        vec!["run"],
        vec!["run", "--interpret"],
        vec!["run", "--release"],
    ] {
        for (label, cwd, target) in [
            (
                "path",
                PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                Some(package.clone()),
            ),
            ("cwd", package.clone(), None),
        ] {
            let mut args = mode
                .iter()
                .map(|arg| (*arg).to_string())
                .collect::<Vec<_>>();
            if let Some(target) = target {
                args.push(target.to_string_lossy().into_owned());
            }
            args.extend([
                "--".to_string(),
                "plan".to_string(),
                "--json".to_string(),
                "--offline".to_string(),
            ]);
            let output = Command::new(jet_bin())
                .args(&args)
                .current_dir(cwd)
                .env("HOME", &home)
                .env("JETPACK_DOGFOOD_ROOT", &store)
                .env("JETPACK_PROJECT_ROOT", &fixture_root)
                .env("JETPACK_OFFLINE", "1")
                .env("NO_COLOR", "1")
                .output()
                .expect("dogfood package output should execute");
            assert!(
                output.status.success(),
                "{label} {mode:?} failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.contains("\"name\":\"dogfood\""),
                "{label} {mode:?} did not execute dogfood output:\n{stdout}"
            );
            assert!(
                stdout.contains("\"selected_profiles\":[\"dogfood\"]"),
                "{label} {mode:?} returned unexpected plan:\n{stdout}"
            );
        }
    }
}

#[test]
fn nested_output_failures_keep_the_package_diagnostic() {
    fn run_case(tag: &str, entry: &str, extra: &[(&str, &str)]) {
        let dir = std::env::temp_dir().join(format!(
            "jet-outputs-nested-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.jet"),
            "name: \"nested_output_negative\"\nversion: \"0.1.0\"\noutputs: .{ app: .Executable{ entry: app.cli_run } }\n",
        )
        .unwrap();
        fs::write(dir.join("entry.jet"), entry).unwrap();
        for &(relative, source) in extra {
            let path = dir.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }

        let out = Command::new(jet_bin())
            .arg("run")
            .current_dir(&dir)
            .output()
            .expect("nested output rejection should execute");
        assert!(
            !out.status.success(),
            "nested output unexpectedly succeeded"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("Error [E2105]"), "missing E2105:\n{stderr}");

        let _ = fs::remove_dir_all(&dir);
    }

    run_case("missing", "use \"src/cli/missing\" as app\n", &[]);
    run_case(
        "ambiguous",
        "use \"src/cli/main\" as app\n",
        &[
            ("src/cli/main.jet", "pub fn cli_run() {}\n"),
            ("src/cli/main/module.jet", "pub fn cli_run() {}\n"),
        ],
    );
    run_case("escaping", "use \"../../outside/main\" as app\n", &[]);
}

/// The negative half of the proof: copy the fixture but drop `outputs:` from
/// `package.jet`, keeping the same `entry.jet` + `service/module.jet` layout.
/// There is no canonical `run.jet`, so entry resolution must fail. If this
/// ever starts resolving without `outputs:`, the fixture above would stop
/// proving anything.
#[test]
fn entry_resolution_requires_the_outputs_block() {
    let dir = std::env::temp_dir().join(format!(
        "jet-outputs-build-negative-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("service")).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"outputs_build_demo\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::copy(example_dir().join("entry.jet"), dir.join("entry.jet")).unwrap();
    fs::copy(
        example_dir().join("service/module.jet"),
        dir.join("service/module.jet"),
    )
    .unwrap();

    let out = Command::new(jet_bin())
        .arg("run")
        .current_dir(&dir)
        .output()
        .expect("jet run should execute");
    assert!(
        !out.status.success(),
        "jet run unexpectedly succeeded without outputs: driving the entry"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("run.jet"),
        "expected a missing-entry error naming run.jet, got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Card #2352: a nested file named `main.jet` is an ordinary import target.
/// Only a root-level `main.jet` is the retired convention entry.
#[test]
fn manifest_output_resolves_nested_main_file_across_run_and_build() {
    let scratch = common::Scratch::new("package-output-nested-main");
    fs::create_dir_all(scratch.join("src/cli")).expect("create nested source directory");
    fs::write(
        scratch.join("package.jet"),
        concat!(
            "name: \"nested_main_output\"\n",
            "version: \"0.1.0\"\n",
            "outputs: .{ app: .Executable{ entry: cli.cli_run } }\n",
            "defaults: .{ run: app }\n",
            "authority: .{ holds: { allow: [IO] } }\n",
        ),
    )
    .expect("write package manifest");
    fs::write(scratch.join("entry.jet"), "use \"src/cli/main\" as cli\n")
        .expect("write package import root");
    fs::write(
        scratch.join("src/cli/main.jet"),
        "pub fn cli_run() {\n    print(\"nested-main\")\n}\n",
    )
    .expect("write nested package entry");

    let run = Command::new(jet_bin())
        .arg("run")
        .current_dir(&scratch.path)
        .output()
        .expect("manifest-selected nested output should run");
    assert!(
        run.status.success(),
        "jet run failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"nested-main\n");

    if !common::have_rustc() {
        return;
    }
    let build = Command::new(jet_bin())
        .arg("build")
        .current_dir(&scratch.path)
        .output()
        .expect("manifest-selected nested output should build");
    assert!(
        build.status.success(),
        "jet build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let binary = scratch.join("build/main");
    assert!(binary.is_file(), "jet build did not produce build/main");
    let built = Command::new(binary)
        .output()
        .expect("manifest-selected nested binary should run");
    assert!(
        built.status.success(),
        "built binary failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert_eq!(built.stdout, b"nested-main\n");
}

/// Card #2352: unsafe or non-unique nested selectors fail at the Package
/// boundary instead of reaching sema or rustc with a guessed entry.
#[test]
fn manifest_output_rejects_missing_ambiguous_and_escaping_nested_entries() {
    for (tag, imports, sources) in [
        (
            "missing",
            "use \"src/missing\" as cli\n",
            Vec::<(&str, &str)>::new(),
        ),
        (
            "ambiguous",
            "use \"src/one\" as cli\nuse \"src/two\" as cli\n",
            vec![
                ("src/one.jet", "pub fn cli_run() {}\n"),
                ("src/two.jet", "pub fn cli_run() {}\n"),
            ],
        ),
        (
            "escaping",
            "use \"../outside\" as cli\n",
            Vec::<(&str, &str)>::new(),
        ),
    ] {
        let scratch = common::Scratch::new(&format!("package-output-nested-{tag}"));
        fs::create_dir_all(scratch.join("src")).expect("create nested source directory");
        fs::write(
            scratch.join("package.jet"),
            concat!(
                "name: \"invalid_nested_output\"\n",
                "version: \"0.1.0\"\n",
                "outputs: .{ app: .Executable{ entry: cli.cli_run } }\n",
                "defaults: .{ run: app }\n",
            ),
        )
        .expect("write package manifest");
        fs::write(scratch.join("entry.jet"), imports).expect("write package import root");
        for (path, source) in sources {
            fs::write(scratch.join(path), source).expect("write candidate entry source");
        }

        let out = Command::new(jet_bin())
            .arg("run")
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .output()
            .expect("invalid nested manifest output should be rejected");
        assert!(
            !out.status.success(),
            "{tag} nested output unexpectedly ran"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("Error [E2105]:") && stderr.contains("no unique source entry"),
            "{tag} nested output lost the registered package diagnostic:\n{stderr}"
        );
    }
}

fn typed_settings_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/packages/typed_settings")
}

fn typed_settings_output(tls: &str) -> String {
    if tls == "on" {
        return include_str!("../examples/features/expected/packages/typed_settings.out")
            .to_string();
    }
    format!("tls-{tls}\nhttps://api.example.com\n")
}

#[test]
fn typed_settings_preserves_tier_parity_and_cli_override() {
    let dir = typed_settings_dir();
    let expected = typed_settings_output("on");

    for args in [vec!["run"], vec!["run", "--interpret"]] {
        let output = Command::new(jet_bin())
            .args(&args)
            .current_dir(&dir)
            .output()
            .expect("typed settings run should execute");
        assert!(
            output.status.success(),
            "typed settings {:?} failed:\n{}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }

    let build_dir = dir.join("build");
    let _ = fs::remove_dir_all(&build_dir);
    let build = Command::new(jet_bin())
        .arg("build")
        .current_dir(&dir)
        .output()
        .expect("typed settings AOT build should execute");
    assert!(
        build.status.success(),
        "typed settings AOT build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let binary = build_dir.join("run");
    let aot = Command::new(&binary)
        .output()
        .expect("typed settings AOT binary should execute");
    assert!(aot.status.success());
    assert_eq!(String::from_utf8_lossy(&aot.stdout), expected);

    let explain = Command::new(jet_bin())
        .args(["explain", "build.settings.tls"])
        .current_dir(&dir)
        .output()
        .expect("typed settings explain should execute");
    assert!(explain.status.success());
    let explanation = String::from_utf8_lossy(&explain.stdout);
    assert!(explanation.contains("Build.Settings.tls = true"));
    assert!(explanation.contains("[effective] declaration / package"));
    assert!(!explanation.contains("CLI: --set tls=<value>"));
    assert!(!explanation.contains("default: Bool = true"));

    let explained_override = Command::new(jet_bin())
        .args([
            "explain",
            "build.settings.tls",
            "--profile=release",
            "--set",
            "tls=true",
        ])
        .current_dir(&dir)
        .output()
        .expect("typed settings contribution chain should execute");
    assert!(explained_override.status.success());
    let override_explanation = String::from_utf8_lossy(&explained_override.stdout);
    assert!(override_explanation.contains("[shadowed] optimization bundle / package false"));
    assert!(override_explanation.contains("[effective] command line / package true"));

    let _ = fs::remove_dir_all(&build_dir);
    let override_build = Command::new(jet_bin())
        .args(["build", "--set", "tls=false"])
        .current_dir(&dir)
        .output()
        .expect("typed settings CLI override should execute");
    assert!(
        override_build.status.success(),
        "typed settings CLI override failed:\n{}",
        String::from_utf8_lossy(&override_build.stderr)
    );
    let overridden = Command::new(&binary)
        .output()
        .expect("typed settings override binary should execute");
    assert!(overridden.status.success());
    assert_eq!(
        String::from_utf8_lossy(&overridden.stdout),
        typed_settings_output("off")
    );

    let _ = fs::remove_dir_all(&build_dir);
}

#[test]
fn computed_build_contribution_records_lock_and_matches_explain_golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("examples/features/packages/build_contribution");
    let scratch = common::Scratch::new("build-contribution");
    for file in ["package.jet", "run.jet"] {
        fs::copy(fixture.join(file), scratch.join(file))
            .unwrap_or_else(|error| panic!("copy build contribution {file}: {error}"));
    }

    let run = Command::new(jet_bin())
        .args(["run", "run.jet"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .expect("computed build contribution should execute");
    assert!(
        run.status.success(),
        "computed build contribution failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"4\n");

    let lock = fs::read_to_string(scratch.join(".jet/lock"))
        .expect("computed contribution must write the unified lock");
    let lock = jet::Lock::parse(&lock).expect("computed contribution lock must parse");
    let record = lock
        .build_contributions
        .iter()
        .find(|record| record.key == "Build.Settings.cache_slots")
        .expect("computed contribution writer must be recorded");
    assert_eq!(record.package, "build_contribution_demo");
    assert_eq!(record.value, "Int(4)");
    assert_eq!(record.scope, "function");
    assert_eq!(record.layer, "environment");
    assert_eq!(record.source, "build_contribution_demo::build");
    assert_eq!(record.reason, "computed by fn build");

    let explain = Command::new(jet_bin())
        .args(["explain", "build.settings.cache_slots", "run.jet"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .expect("computed build contribution explain should execute");
    assert!(
        explain.status.success(),
        "computed build contribution explain failed:\n{}",
        String::from_utf8_lossy(&explain.stderr)
    );
    let expected =
        fs::read(root.join("examples/features/expected/packages/build_contribution.explain.out"))
            .expect("read computed contribution explain golden");
    assert_eq!(explain.stdout, expected);

    let changed = fs::read_to_string(scratch.join("run.jet"))
        .expect("read computed contribution source")
        .replace("\"cache_slots\", 4", "\"cache_slots\", 5");
    fs::write(scratch.join("run.jet"), changed).expect("change computed contribution");
    let locked = Command::new(jet_bin())
        .args(["run", "--locked", "run.jet"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .expect("locked computed contribution should execute");
    assert!(!locked.status.success());
    assert!(
        String::from_utf8_lossy(&locked.stderr).contains("E3512"),
        "locked writer drift must name E3512:\n{}",
        String::from_utf8_lossy(&locked.stderr)
    );
}

/// A three-node action cycle declared by `fn build`: every action consumes
/// the next action's declared output, so the graph has no build order.
/// `alpha` waits on `gamma`, `gamma` waits on `beta`, `beta` waits on
/// `alpha`.
///
/// The actions declare no capabilities, so this package reaches graph
/// validation without an `#Impure` gate or an execution grant, and the cycle
/// is rejected before any action is spawned.
const ACTION_CYCLE_BUILD: &str = r#"fn build(b: BuildContext) BuildPlan {
    alpha :: b.action("alpha", ["gamma.stamp"], ["alpha.stamp"], ["sh", "-c", "true"], [])
    beta :: b.action("beta", ["alpha.stamp"], ["beta.stamp"], ["sh", "-c", "true"], [])
    gamma :: b.action("gamma", ["beta.stamp"], ["gamma.stamp"], ["sh", "-c", "true"], [])
    app :: b.add_executable("app", ["run.jet"], [alpha, beta, gamma])
    return b.plan(app)
}

fn run() {
    print("unreachable")
}
"#;

/// The one ordered chain the three-node cycle above must always render.
const ACTION_CYCLE_CHAIN: &str = "`alpha` -> `gamma` -> `beta` -> `alpha`";

fn action_cycle_package(tag: &str) -> common::Scratch {
    let scratch = common::Scratch::new(tag);
    fs::write(
        scratch.join("package.jet"),
        "name: \"build_cycle_demo\"\nversion: \"0.1.0\"\n",
    )
    .expect("write cycle package manifest");
    fs::write(scratch.join("run.jet"), ACTION_CYCLE_BUILD).expect("write cycle build entry");
    scratch
}

/// Run one command that must reject the cyclic graph, and return the single
/// `E3502` line that names the chain.
///
/// Only the programmable build path evaluates `fn build` and validates the
/// graph it returns (`jet build`, and `jet inspect graph` for the
/// no-execution variant). The default `jet run` lens compiles and runs the
/// selected runtime program straight through the JIT — the native execution
/// workflow hands it to `Interpreter::run_jit_once_*`, which only seeds build
/// facts —
/// so `jet run` never walks a build graph and can never observe this fault.
fn cycle_diagnostic(scratch: &common::Scratch, args: &[&str]) -> String {
    let out = Command::new(jet_bin())
        .args(args)
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("jet {args:?} should execute: {error}"));
    assert!(
        !out.status.success(),
        "a cyclic build graph must be rejected by jet {args:?}:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    stderr
        .lines()
        .find(|line| line.contains("build plan is invalid"))
        .unwrap_or_else(|| {
            panic!("jet {args:?} must reject the cyclic graph as an invalid plan:\n{stderr}")
        })
        .to_string()
}

/// Card #1522 criterion 4: a build dependency cycle is reported with the full
/// chain, not as a bare "there is a cycle" or as one arbitrary member.
#[test]
fn build_action_dependency_cycle_reports_the_full_chain() {
    let scratch = action_cycle_package("build-action-cycle");
    let reported = cycle_diagnostic(&scratch, &["build", "run.jet"]);
    assert!(
        reported.contains(ACTION_CYCLE_CHAIN),
        "the action cycle must be named in traversal order:\n{reported}"
    );
}

/// The chain has to be snapshot-stable: one graph renders one text, run after
/// run, under the release profile, and on the inspection path that orders the
/// graph without executing it. A traversal that inherited hash order would
/// drift here, and a second renderer on one of those paths would show up as a
/// different line.
#[test]
fn build_dependency_cycle_chain_is_deterministic_across_runs_and_tiers() {
    let scratch = action_cycle_package("build-action-cycle-stable");
    let first = cycle_diagnostic(&scratch, &["build", "run.jet"]);
    let second = cycle_diagnostic(&scratch, &["build", "run.jet"]);
    let release = cycle_diagnostic(&scratch, &["build", "--release", "run.jet"]);
    let inspected = cycle_diagnostic(&scratch, &["inspect", "graph", "run.jet"]);
    assert!(
        first.contains(ACTION_CYCLE_CHAIN),
        "jet build must name the cycle chain:\n{first}"
    );
    assert_eq!(first, second, "the cycle chain must not vary between runs");
    assert_eq!(
        first, release,
        "the release profile must name the same cycle chain as the dev profile"
    );
    assert_eq!(
        first, inspected,
        "graph inspection must name the same cycle chain as an executing build"
    );
}

#[test]
fn semantic_corpus_policy_runs_with_package_templates() {
    common::corpus_policy::CorpusPolicy::load()
        .expect("corpus manifest")
        .check_gate("package")
        .expect("package corpus semantic policy");
}
