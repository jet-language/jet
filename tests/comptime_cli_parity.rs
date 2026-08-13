mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMPTIME_STEMS: [&str; 4] = [
    "comptime/embed",
    "comptime/embed_bytes",
    "comptime/find",
    "comptime/find_empty",
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

#[test]
fn comptime_examples_run_through_default_jet_run_and_report_resident_native() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("comptime_effect_parity");

    for stem in COMPTIME_STEMS {
        let case_dir = scratch.join(&stem.replace('/', "_"));
        fs::create_dir_all(&case_dir).expect("create comptime parity case directory");
        let file_name = copy_comptime_fixture(&root, &case_dir, stem);
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
fn documented_cli_help_matches_aot_and_default_jet_run() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = common::Scratch::new("cli_docs_parity");
    let source = scratch.join("subcommands.jet");
    fs::copy(
        root.join("examples/features/cli/subcommands.jet"),
        &source,
    )
    .expect("copy CLI example");

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
            assert!(help.contains("Manage the service."), "{help}");
            assert!(help.contains("serve                Start the service and listen for requests"), "{help}");
            assert!(help.contains("import               Import one data file"), "{help}");
        } else {
            assert!(help.contains("Start the service and listen for requests"), "{help}");
        }
    }
}
