mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TIER_PARITY_STEMS: [&str; 7] = [
    "cli/subcommands",
    "concurrency/freeze_capture",
    "comptime/embed",
    "comptime/embed_bytes",
    "comptime/find",
    "comptime/find_empty",
    "tooling/declared_text_head",
];

fn copy_comptime_fixture(root: &Path, destination: &Path, stem: &str) -> String {
    let file_name = stem.rsplit('/').next().expect("comptime stem has a file name");
    fs::copy(
        root.join("examples/features").join(format!("{stem}.jet")),
        destination.join(format!("{file_name}.jet")),
    )
    .unwrap_or_else(|error| panic!("copy `{stem}` fixture: {error}"));

    let assets: &[&str] = match stem {
        "comptime/embed" => &["motd.txt"],
        "comptime/embed_bytes" => &["logo.bin"],
        "comptime/find" | "comptime/find_empty" => &[
            "find_inputs/alpha-1.txt",
            "find_inputs/nested/beta-2.txt",
            "find_inputs/nested/gamma-3.txt",
        ],
        _ => &[],
    };
    for relative in assets {
        let target = destination.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("create comptime fixture asset directory");
        }
        fs::copy(root.join("examples/features/comptime").join(relative), &target)
            .unwrap_or_else(|error| panic!("copy `{relative}` for `{stem}`: {error}"));
    }
    format!("{file_name}.jet")
}

fn copy_build_stamp_fixture(root: &Path, destination: &Path) {
    fs::copy(
        root.join("examples/features/comptime/build_stamp.jet"),
        destination.join("build_stamp.jet"),
    )
    .expect("copy build stamp example");
    let lock_dir = destination.join(".jet");
    fs::create_dir_all(&lock_dir).expect("create build stamp lock directory");
    fs::copy(
        root.join("tests/fixtures/build_stamp.lock"),
        lock_dir.join("lock"),
    )
    .expect("copy build stamp lock fixture");
}

fn run_jet(args: &[&str], project: &Path, cache: &Path) -> Output {
    fs::create_dir_all(cache).expect("create isolated Jet cache");
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(project)
        .env("JET_CACHE_DIR", cache.join("build"))
        .env("JET_RUN_CACHE_DIR", cache.join("run"))
        .env("JET_RUNTIME_CACHE_DIR", cache.join("runtime"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("spawn `{}`: {error}", args.join(" ")))
}

fn assert_tier_parity_case(root: &Path, scratch: &common::Scratch, stem: &str) {
    let case_dir = scratch.join(&stem.replace('/', "_"));
    fs::create_dir_all(&case_dir).expect("create comptime parity case directory");
    let file_name = copy_comptime_fixture(root, &case_dir, stem);
    let expected_path = root.join(format!("examples/features/expected/{stem}.out"));
    let expected = fs::read(&expected_path)
        .unwrap_or_else(|error| panic!("missing golden for `{stem}`: {error}"));
    let cache = case_dir.join("cache");

    let build = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", &file_name])
        .current_dir(&case_dir)
        .env("JET_CACHE_DIR", cache.join("aot"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("failed to spawn AOT build for `{stem}`: {error}"));
    assert!(
        build.status.success(),
        "AOT build failed for `{stem}`:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let binary_name = file_name.strip_suffix(".jet").expect("Jet fixture extension");
    let aot = Command::new(case_dir.join("build").join(binary_name))
        .current_dir(&case_dir)
        .output()
        .unwrap_or_else(|error| panic!("failed to run AOT binary for `{stem}`: {error}"));
    assert!(
        aot.status.success(),
        "AOT run failed for `{stem}`:\n{}",
        String::from_utf8_lossy(&aot.stderr)
    );
    assert_eq!(
        aot.stdout, expected,
        "AOT output differs from the checked-in golden for `{stem}`"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", &file_name, "--trace-tiers"])
        .current_dir(&case_dir)
        .env("JET_RUN_CACHE_DIR", cache.join("run"))
        .env("JET_CACHE_DIR", cache.join("build"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("failed to spawn `jet run` for `{stem}`: {error}"));
    let trace = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "default `jet run` failed for `{stem}`:\nstdout:\n{}\nstderr:\n{trace}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        trace
            .lines()
            .any(|line| line.starts_with("run") && line.contains("tier1 native")),
        "default `jet run` did not report resident native execution for `{stem}`:\n{trace}"
    );
    assert!(
        !trace.contains("tier0 interp"),
        "default `jet run` deopted to the interpreter for `{stem}`:\n{trace}"
    );
    assert_eq!(
        output.stdout, expected,
        "default `jet run` output differs from the checked-in golden for `{stem}`"
    );

    let interpreted_cache = cache.join("interpret");
    let interpreted = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "--interpret", &file_name, "--trace-tiers"])
        .current_dir(&case_dir)
        .env("JET_RUN_CACHE_DIR", interpreted_cache.join("run"))
        .env("JET_CACHE_DIR", interpreted_cache.join("build"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("failed to spawn interpreted `jet run` for `{stem}`: {error}"));
    let interpreted_trace = String::from_utf8_lossy(&interpreted.stderr);

    assert!(
        interpreted.status.success(),
        "interpreted `jet run` failed for `{stem}`:\nstdout:\n{}\nstderr:\n{interpreted_trace}",
        String::from_utf8_lossy(&interpreted.stdout)
    );
    assert!(
        interpreted_trace.contains("tier0 interp"),
        "forced interpreter did not report interpreter execution for `{stem}`:\n{interpreted_trace}"
    );
    assert!(
        !interpreted_trace.contains("E2201"),
        "forced interpreter deopted for `{stem}`:\n{interpreted_trace}"
    );
    assert_eq!(
        interpreted.stdout, expected,
        "interpreter output differs from the checked-in golden for `{stem}`"
    );
}

#[test]
fn tier_parity_examples_run_through_aot_jit_and_interpreter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("comptime_effect_parity");

    for stem in TIER_PARITY_STEMS {
        assert_tier_parity_case(&root, &scratch, stem);
    }
}

#[test]
fn declared_text_head_runs_through_aot_jit_and_interpreter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("declared_text_head_parity");
    assert_tier_parity_case(&root, &scratch, "tooling/declared_text_head");
}

#[test]
fn build_entry_workspace_matches_release_default_and_dev_interpreter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("examples/features/tooling/build_entry_discovery");
    let scratch = common::Scratch::new("build_entry_discovery_parity");
    for relative in [
        "workspace.jet",
        "package.jet",
        "run.jet",
        "tools/build_entry.jet",
        "packages/foundation/package.jet",
        "packages/foundation/run.jet",
        "packages/foundation/tools/build.jet",
        "packages/app/package.jet",
        "packages/app/run.jet",
    ] {
        let destination = scratch.join(relative);
        fs::create_dir_all(destination.parent().unwrap())
            .expect("create build-entry fixture directory");
        fs::copy(source.join(relative), destination)
            .unwrap_or_else(|error| panic!("copy build-entry fixture `{relative}`: {error}"));
    }
    let expected = fs::read(
        root.join("examples/features/expected/tooling/build_entry_discovery.out"),
    )
    .expect("read build-entry golden");

    let named = run_jet(
        &["build", "foundation"],
        &scratch.path,
        &scratch.join("cache/named"),
    );
    assert!(
        named.status.success(),
        "`jet build foundation` failed:\n{}",
        String::from_utf8_lossy(&named.stderr)
    );
    assert!(
        scratch
            .join("packages/foundation/.jet/generated/foundation/foundation_marker.jet")
            .is_file(),
        "the named depth-one member build did not run"
    );

    let workspace = run_jet(
        &["build"],
        &scratch.path,
        &scratch.join("cache/workspace"),
    );
    assert!(
        workspace.status.success(),
        "workspace build failed:\n{}",
        String::from_utf8_lossy(&workspace.stderr)
    );

    let release = run_jet(
        &["run", "--release", "run.jet"],
        &scratch.path,
        &scratch.join("cache/release"),
    );
    let default = run_jet(
        &["run", "run.jet"],
        &scratch.path,
        &scratch.join("cache/default"),
    );
    let interpreted = run_jet(
        &["dev", "run.jet", "--interpret", "--watch=off"],
        &scratch.path,
        &scratch.join("cache/interpreted"),
    );
    for (tier, output) in [
        ("release", &release),
        ("default", &default),
        ("dev interpreter", &interpreted),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{tier} build-entry example failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout, expected,
            "{tier} build-entry output differs from the checked-in golden"
        );
    }
}

#[test]
fn fact_plane_runs_through_aot_jit_and_interpreter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("fact_plane_parity");

    assert_tier_parity_case(&root, &scratch, "comptime/fact_plane");
}

#[test]
fn build_fact_precedence_example_matches_release_default_and_dev() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("build_fact_precedence_parity");
    fs::copy(
        root.join("examples/features/comptime/build_fact_precedence.jet"),
        scratch.path.join("build_fact_precedence.jet"),
    )
    .expect("copy build fact precedence example");
    let expected = fs::read(
        root.join("examples/features/expected/comptime/build_fact_precedence.out"),
    )
    .expect("read build fact precedence golden");

    let release = run_jet(
        &["run", "--release", "build_fact_precedence.jet"],
        &scratch.path,
        &scratch.join("cache/release"),
    );
    let default = run_jet(
        &["run", "build_fact_precedence.jet"],
        &scratch.path,
        &scratch.join("cache/default"),
    );
    let dev = run_jet(
        &["dev", "build_fact_precedence.jet", "--interpret", "--watch=off"],
        &scratch.path,
        &scratch.join("cache/dev"),
    );

    for (tier, output) in [("release", &release), ("default", &default), ("dev", &dev)] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{tier} build-fact example failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected, "{tier} build-fact output differs");
    }
    assert_eq!(release.status.code(), default.status.code());
    assert_eq!(release.status.code(), dev.status.code());
}

#[test]
fn computed_constants_match_aot_default_and_interpreter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("computed_constants_parity");
    let source = scratch.join("computed_constants.jet");
    fs::copy(
        root.join("examples/features/comptime/computed_constants.jet"),
        &source,
    )
    .expect("copy computed constants example");
    let source_text = fs::read_to_string(&source).expect("read computed constants source");
    let compiled = jet::compile_with_path(&source_text, &source.to_string_lossy())
        .unwrap_or_else(|diags| panic!("computed constants front end rejected: {diags:#?}"));
    assert!(
        compiled.rust.contains("__jet_First = 41"),
        "computed enum discriminant did not reach generated Rust:\n{}",
        compiled.rust
    );
    let expected = fs::read(root.join("examples/features/expected/comptime/computed_constants.out"))
        .expect("read computed constants golden");
    let cache = scratch.join("cache");

    let build = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", "computed_constants.jet"])
        .current_dir(&scratch.path)
        .env("JET_CACHE_DIR", cache.join("build"))
        .env("NO_COLOR", "1")
        .output()
        .expect("build computed constants example");
    assert!(
        build.status.success(),
        "AOT computed constants build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let aot = Command::new(scratch.join("build/computed_constants"))
        .current_dir(&scratch.path)
        .output()
        .expect("run AOT computed constants example");
    assert!(
        aot.status.success(),
        "AOT computed constants run failed:\n{}",
        String::from_utf8_lossy(&aot.stderr)
    );

    let default = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "computed_constants.jet"])
        .current_dir(&scratch.path)
        .env("JET_RUN_CACHE_DIR", cache.join("run"))
        .env("JET_CACHE_DIR", cache.join("default"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run computed constants through default jet run");
    assert!(
        default.status.success(),
        "default computed constants run failed:\n{}",
        String::from_utf8_lossy(&default.stderr)
    );

    let interpreted = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "--interpret", "computed_constants.jet"])
        .current_dir(&scratch.path)
        .env("JET_RUN_CACHE_DIR", cache.join("interpret-run"))
        .env("JET_CACHE_DIR", cache.join("interpret-build"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run computed constants through the interpreter");
    assert!(
        interpreted.status.success(),
        "interpreter computed constants run failed:\n{}",
        String::from_utf8_lossy(&interpreted.stderr)
    );

    assert_eq!(aot.stdout, expected, "AOT output differs from the golden");
    assert_eq!(default.stdout, expected, "default `jet run` output differs from the golden");
    assert_eq!(
        interpreted.stdout, expected,
        "interpreter output differs from the golden"
    );
}

#[test]
fn job_runner_help_and_named_jobs_match_default_run_aot_and_goldens() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("job_runner_cli_parity");
    let source = scratch.join("job_runner.jet");
    fs::write(
        scratch.join("package.jet"),
        "name: \"job_runner\"\nversion: \"0.1.0\"\n",
    )
    .expect("write job runner package manifest");
    fs::copy(
        root.join("examples/features/devloop/job_runner.jet"),
        &source,
    )
    .expect("copy job runner example");
    let help_expected = fs::read(
        root.join("examples/features/expected/devloop/job_runner.out"),
    )
    .expect("read job runner help golden");
    let cache = scratch.join("cache");

    let default_help = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "job_runner.jet"])
        .current_dir(&scratch.path)
        .env("JET_RUN_CACHE_DIR", cache.join("default-run"))
        .env("JET_CACHE_DIR", cache.join("default-build"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run job runner through default jet run");
    assert!(
        default_help.status.success(),
        "default job runner help failed:\n{}",
        String::from_utf8_lossy(&default_help.stderr)
    );
    assert_eq!(default_help.stdout, help_expected);

    let build = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", "job_runner.jet"])
        .current_dir(&scratch.path)
        .env("JET_CACHE_DIR", cache.join("aot-build"))
        .env("NO_COLOR", "1")
        .output()
        .expect("build job runner example");
    assert!(
        build.status.success(),
        "AOT job runner build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let aot_help = Command::new(scratch.join("build/job_runner"))
        .current_dir(&scratch.path)
        .output()
        .expect("run AOT job runner help");
    assert!(
        aot_help.status.success(),
        "AOT job runner help failed:\n{}",
        String::from_utf8_lossy(&aot_help.stderr)
    );
    assert_eq!(aot_help.stdout, help_expected);
    assert_eq!(default_help.stdout, aot_help.stdout);

    let default_jobs_help = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "job_runner.jet", "--", "--help"])
        .current_dir(&scratch.path)
        .env("JET_RUN_CACHE_DIR", cache.join("job-help-run"))
        .env("JET_CACHE_DIR", cache.join("job-help-build"))
        .env("NO_COLOR", "1")
        .output()
        .expect("show default job subcommand help");
    let aot_jobs_help = Command::new(scratch.join("build/job_runner"))
        .arg("--help")
        .current_dir(&scratch.path)
        .output()
        .expect("show AOT job subcommand help");
    assert!(default_jobs_help.status.success());
    assert!(aot_jobs_help.status.success());
    assert!(String::from_utf8_lossy(&default_jobs_help.stdout).contains("greet"));
    assert!(String::from_utf8_lossy(&aot_jobs_help.stdout).contains("greet"));
    assert!(!String::from_utf8_lossy(&default_jobs_help.stdout).contains("inspect"));
    assert!(!String::from_utf8_lossy(&aot_jobs_help.stdout).contains("inspect"));

    for (job, golden_name) in [
        ("greet", "job_runner.greet.out"),
        ("seed_data", "job_runner.seed_data.out"),
    ] {
        let expected = fs::read(
            root.join("examples/features/expected/devloop").join(golden_name),
        )
        .unwrap_or_else(|error| panic!("read named job golden `{golden_name}`: {error}"));
        let default_job = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["run", "job_runner.jet", "--", job])
            .current_dir(&scratch.path)
            .env("JET_RUN_CACHE_DIR", cache.join(format!("{job}-run")))
            .env("JET_CACHE_DIR", cache.join(format!("{job}-build")))
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("run named job `{job}`: {error}"));
        assert!(
            default_job.status.success(),
            "default named job `{job}` failed:\n{}",
            String::from_utf8_lossy(&default_job.stderr)
        );
        assert_eq!(default_job.stdout, expected, "default job `{job}` differs from golden");

        let aot_job = Command::new(scratch.join("build/job_runner"))
            .arg(job)
            .current_dir(&scratch.path)
            .output()
            .unwrap_or_else(|error| panic!("run AOT named job `{job}`: {error}"));
        assert!(
            aot_job.status.success(),
            "AOT named job `{job}` failed:\n{}",
            String::from_utf8_lossy(&aot_job.stderr)
        );
        assert_eq!(aot_job.stdout, expected, "AOT job `{job}` differs from golden");
        assert_eq!(default_job.stdout, aot_job.stdout);
    }
}

#[test]
fn documented_cli_program_matches_aot_default_interpreter_and_goldens() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("cli_docs_parity");
    let source = scratch.join("subcommands.jet");
    fs::copy(
        root.join("examples/features/cli/subcommands.jet"),
        &source,
    )
    .expect("copy CLI example");
    let cache = scratch.join("cache");

    let build = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", "subcommands.jet"])
        .current_dir(&scratch.path)
        .output()
        .expect("build CLI example");
    assert!(
        build.status.success(),
        "AOT CLI build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let binary = scratch.join("build/subcommands");

    for args in [["--help"].as_slice(), ["serve", "--help"].as_slice()] {
        let aot = Command::new(&binary)
            .args(args)
            .current_dir(&scratch.path)
            .output()
            .expect("run AOT CLI help");
        let mut run_args = vec!["run", "subcommands.jet", "--"];
        run_args.extend_from_slice(args);
        let jit = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(&run_args)
            .current_dir(&scratch.path)
            .output()
            .expect("run default CLI help");
        assert!(aot.status.success(), "AOT help failed: {}", String::from_utf8_lossy(&aot.stderr));
        assert!(jit.status.success(), "default `jet run` help failed: {}", String::from_utf8_lossy(&jit.stderr));
        assert_eq!(aot.stdout, jit.stdout, "AOT and default `jet run` help differ for {args:?}");
        let help = String::from_utf8_lossy(&aot.stdout);
        if args == ["--help"] {
            for fact in [
                "-v, --verbose",
                "--config CONFIG",
                "plan                     preview changes",
                "serve                    Start the service and listen for requests",
                "import                   Import one data file",
            ] {
                assert!(help.contains(fact), "root help omitted {fact}: {help}");
            }
            assert_eq!(help.matches("--config").count(), 1, "root help repeated shared config: {help}");
        } else {
            assert!(help.contains("Start the service and listen for requests"), "{help}");
            assert!(!help.contains("--config"), "command help repeated the shared config: {help}");
        }
    }

    for (args, golden_name) in [
        (["--config", "prod", "serve", "--port", "8080"].as_slice(), "subcommands.serve.out"),
        (["plan", "--config", "prod"].as_slice(), "subcommands.plan.out"),
    ] {
        let expected = fs::read(root.join("examples/features/expected/cli").join(golden_name))
            .unwrap_or_else(|error| panic!("read CLI command golden `{golden_name}`: {error}"));
        let aot = Command::new(&binary)
            .args(args)
            .current_dir(&scratch.path)
            .output()
            .expect("run AOT CLI command");
        let mut run_args = vec!["run", "subcommands.jet", "--"];
        run_args.extend_from_slice(args);
        let default = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(&run_args)
            .current_dir(&scratch.path)
            .env("JET_RUN_CACHE_DIR", cache.join(format!("{golden_name}-run")))
            .env("JET_CACHE_DIR", cache.join(format!("{golden_name}-build")))
            .env("NO_COLOR", "1")
            .output()
            .expect("run default CLI command");
        let mut interpret_args = vec!["run", "--interpret", "subcommands.jet", "--"];
        interpret_args.extend_from_slice(args);
        let interpreted = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(&interpret_args)
            .current_dir(&scratch.path)
            .env("JET_RUN_CACHE_DIR", cache.join(format!("{golden_name}-interpret-run")))
            .env("JET_CACHE_DIR", cache.join(format!("{golden_name}-interpret-build")))
            .env("NO_COLOR", "1")
            .output()
            .expect("run interpreted CLI command");
        for (label, output) in [
            ("AOT", &aot),
            ("default `jet run`", &default),
            ("interpreter", &interpreted),
        ] {
            assert!(
                output.status.success(),
                "{label} CLI command {args:?} failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, expected, "{label} CLI command {args:?} differs from golden");
        }
        assert_eq!(aot.stdout, default.stdout, "AOT and default CLI command {args:?} differ");
        assert_eq!(aot.stdout, interpreted.stdout, "AOT and interpreter CLI command {args:?} differ");
    }

    let unknown = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "subcommands.jet", "--", "--confg", "prod"])
        .current_dir(&scratch.path)
        .env("JET_RUN_CACHE_DIR", cache.join("unknown-run"))
        .env("JET_CACHE_DIR", cache.join("unknown-build"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run CLI with an unknown flag");
    assert!(!unknown.status.success(), "unknown CLI flag was accepted");
    let unknown_stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(
        unknown_stderr.contains("--config"),
        "unknown CLI flag lost its suggestion:\n{unknown_stderr}"
    );
}

#[test]
fn measured_test_cli_and_selected_claim_keep_aot_golden_contract() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("examples/features/tooling/test_target/main.jet");
    let golden = fs::read(root.join("examples/features/expected/tooling/test_target.out"))
        .expect("read measured test target golden");
    let scratch = common::Scratch::new("test_cli_parity");
    fs::copy(&source, scratch.join("main.jet")).expect("copy measured test target source");
    fs::copy(
        root.join("examples/features/tooling/test_target/package.jet"),
        scratch.join("package.jet"),
    )
    .expect("copy measured test target package");
    fs::copy(
        root.join("examples/features/tooling/test_target/test_perf.jet"),
        scratch.join("test_perf.jet"),
    )
    .expect("copy measured test target module");

    let build = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", "main.jet"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .expect("build measured test target");
    assert!(build.status.success(), "AOT measured test target build failed: {}", String::from_utf8_lossy(&build.stderr));
    let aot = Command::new(scratch.join("build/main"))
        .current_dir(&scratch.path)
        .output()
        .expect("run AOT measured test target");
    assert!(aot.status.success(), "AOT measured test target failed: {}", String::from_utf8_lossy(&aot.stderr));
    assert_eq!(aot.stdout, golden, "AOT measured test target changed its named golden");

    let help = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["test", "--help"])
        .env("NO_COLOR", "1")
        .output()
        .expect("show test help");
    assert!(help.status.success(), "test help failed: {}", String::from_utf8_lossy(&help.stderr));
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("--filter"));
    assert!(help.contains("--measure"));

    let measured = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["test", "main.jet", "--measure", "--filter=sum_to(1000)"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .expect("run selected measured claim");
    assert!(measured.status.success(), "selected measured claim failed: {}", String::from_utf8_lossy(&measured.stderr));
    let selected = String::from_utf8_lossy(&measured.stdout);
    assert!(selected.contains("sum_to(1000)"), "selected measurement lost claim name: {selected}");
    assert!(selected.contains("ns"), "selected measurement lost timing: {selected}");
}

#[test]
fn command_override_examples_match_aot_default_run_and_interpreter() {
    if !common::have_rustc() {
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("command_override_parity");
    let cases = [
        ("test_override", "test_override.jet"),
        ("bench_override", "bench_override.jet"),
    ];

    for (stem, file_name) in cases {
        fs::copy(
            root.join("examples/features/devloop").join(file_name),
            scratch.join(file_name),
        )
        .unwrap_or_else(|error| panic!("copy command override `{file_name}`: {error}"));
        let expected = fs::read(
            root.join("examples/features/expected/devloop").join(format!("{stem}.out")),
        )
        .unwrap_or_else(|error| panic!("read command override golden `{stem}`: {error}"));

        let release = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["run", "--release", file_name])
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("run release command override `{stem}`: {error}"));
        assert!(release.status.success(), "release `{stem}` failed: {}", String::from_utf8_lossy(&release.stderr));
        assert_eq!(release.stdout, expected, "release `{stem}` differs from golden");

        let default_run = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["run", file_name])
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("run default command override `{stem}`: {error}"));
        assert!(default_run.status.success(), "default run `{stem}` failed: {}", String::from_utf8_lossy(&default_run.stderr));
        assert_eq!(default_run.stdout, expected, "default run `{stem}` differs from golden");

        let interpreted = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["dev", file_name, "--interpret", "--watch=off"])
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("run interpreted command override `{stem}`: {error}"));
        assert!(interpreted.status.success(), "interpreted `{stem}` failed: {}", String::from_utf8_lossy(&interpreted.stderr));
        assert_eq!(interpreted.stdout, expected, "interpreted `{stem}` differs from golden");
    }

    let override_run = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["test", "test_override.jet", "--serial"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .expect("run test command override");
    assert!(override_run.status.success(), "test override failed: {}", String::from_utf8_lossy(&override_run.stderr));
    assert!(String::from_utf8_lossy(&override_run.stdout).contains("jet test: using fn test override"));

    for command in ["test"] {
        let help = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args([command, "--help"])
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("show `{command}` help: {error}"));
        assert!(help.status.success(), "{command} help failed: {}", String::from_utf8_lossy(&help.stderr));
        assert!(String::from_utf8_lossy(&help.stdout).contains("--show-default"), "{command} help omitted --show-default");
    }
}

#[test]
fn package_build_entry_discovery_matches_committed_golden_across_tiers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/features/tooling/build_entry_discovery");
    let entry = example.join("run.jet");
    let expected = fs::read(
        root.join("examples/features/expected/tooling/build_entry_discovery.out"),
    )
    .expect("read build-entry discovery golden");
    let scratch = common::Scratch::new("package_build_entry_discovery_parity");

    let release = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "--release", entry.to_str().expect("entry path is utf8")])
        .current_dir(&example)
        .env("JET_RUN_CACHE_DIR", scratch.join("cache/release-run"))
        .env("JET_CACHE_DIR", scratch.join("cache/release-build"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run committed package entry through release jet run");
    assert!(
        release.status.success(),
        "release jet run failed:\n{}",
        String::from_utf8_lossy(&release.stderr)
    );
    assert_eq!(release.stdout, expected);

    let default = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", entry.to_str().expect("entry path is utf8")])
        .current_dir(&example)
        .env("JET_RUN_CACHE_DIR", scratch.join("cache/run"))
        .env("JET_CACHE_DIR", scratch.join("cache/default"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run committed package entry through default jet run");
    assert!(
        default.status.success(),
        "default jet run failed:\n{}",
        String::from_utf8_lossy(&default.stderr)
    );
    assert_eq!(default.stdout, expected);

    let dev = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "dev",
            entry.to_str().expect("entry path is utf8"),
            "--interpret",
            "--watch=off",
        ])
        .current_dir(&example)
        .env("JET_RUN_CACHE_DIR", scratch.join("cache/dev-run"))
        .env("JET_CACHE_DIR", scratch.join("cache/dev-build"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run committed package entry through interpreted jet dev");
    assert!(
        dev.status.success(),
        "interpreted jet dev failed:\n{}",
        String::from_utf8_lossy(&dev.stderr)
    );
    assert_eq!(dev.stdout, expected);
    assert_eq!(release.status.code(), default.status.code());
    assert_eq!(release.status.code(), dev.status.code());
}

#[test]
fn build_stamp_release_rebuilds_have_identical_binary_bytes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("build_stamp_reproducibility");
    let project = scratch.join("project");
    fs::create_dir_all(&project).expect("create build stamp project");
    copy_build_stamp_fixture(&root, &project);
    let lock_path = project.join(".jet/lock");
    let lock_bytes = fs::read(&lock_path).expect("read build stamp lock fixture");

    let build_clean = |cache: &Path| {
        let output = run_jet(
            &["build", "--release", "--locked", "build_stamp.jet"],
            &project,
            cache,
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "clean release build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let binary = fs::read(project.join("build/build_stamp"))
            .expect("clean release build must publish build/build_stamp");
        assert_eq!(
            fs::read(&lock_path).expect("read lock after clean release build"),
            lock_bytes,
            "locked release build changed the checked-in lock bytes"
        );
        fs::remove_dir_all(project.join("build")).expect("remove first clean build output");
        binary
    };

    let first = build_clean(&scratch.join("cache/first"));
    let second = build_clean(&scratch.join("cache/second"));
    assert_eq!(first, second, "clean release rebuild binary bytes differ");
}

#[test]
fn build_stamp_example_matches_release_jit_and_interpreter_tiers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("build_stamp_tier_parity");
    copy_build_stamp_fixture(&root, &scratch.path);
    let expected = fs::read(root.join("examples/features/expected/comptime/build_stamp.out"))
        .expect("read build stamp golden");

    let release = run_jet(
        &["run", "--release", "build_stamp.jet"],
        &scratch.path,
        &scratch.join("cache/release"),
    );
    let default = run_jet(
        &["run", "build_stamp.jet"],
        &scratch.path,
        &scratch.join("cache/default"),
    );
    let interpreter = run_jet(
        &["dev", "build_stamp.jet", "--interpret", "--watch=off"],
        &scratch.path,
        &scratch.join("cache/interpreter"),
    );

    for (tier, output) in [
        ("AOT release", &release),
        ("default JIT", &default),
        ("interpreter", &interpreter),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{tier} build stamp run failed:\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout, expected,
            "{tier} build stamp stdout differs from the committed golden"
        );
    }
    assert_eq!(release.status.code(), default.status.code());
    assert_eq!(release.status.code(), interpreter.status.code());
}
