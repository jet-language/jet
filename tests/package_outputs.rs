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
    fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing {}", path.display()))
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
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

    let run = Command::new(&binary).output().expect("built binary should run");
    assert!(
        run.status.success(),
        "built binary failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected_output());

    let _ = fs::remove_dir_all(&build_dir);
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
    fs::write(dir.join("package.jet"), "name: \"outputs_build_demo\"\nversion: \"0.1.0\"\n").unwrap();
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

fn typed_settings_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/packages/typed_settings")
}

fn typed_settings_output(tls: &str) -> String {
    if tls == "on" {
        return include_str!("../examples/features/expected/packages/typed_settings.out").to_string();
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
    let expected = fs::read(
        root.join("examples/features/expected/packages/build_contribution.explain.out"),
    )
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
const ACTION_CYCLE_BUILD: &str = r#"fn build(b: BuildContext) BuildPlan ! {
    alpha :: b.action("alpha", ["gamma.stamp"], ["alpha.stamp"], ["sh", "-c", "true"], [])?
    beta :: b.action("beta", ["alpha.stamp"], ["beta.stamp"], ["sh", "-c", "true"], [])?
    gamma :: b.action("gamma", ["beta.stamp"], ["gamma.stamp"], ["sh", "-c", "true"], [])?
    app :: b.add_executable("app", ["run.jet"], [alpha, beta, gamma])?
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
/// selected runtime program straight through the JIT — `run_compile_cmd`
/// hands it to `Interpreter::run_jit_once_*`, which only seeds build facts —
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
