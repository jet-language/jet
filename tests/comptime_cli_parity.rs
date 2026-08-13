mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const COMPTIME_STEMS: [&str; 4] = [
    "comptime/embed",
    "comptime/embed_bytes",
    "comptime/find",
    "comptime/find_empty",
];

#[test]
fn comptime_examples_run_through_default_jet_run_and_report_resident_native() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    for stem in COMPTIME_STEMS {
        let file = format!("examples/features/{stem}.jet");
        let expected_path = root.join(format!("examples/features/expected/{stem}.out"));
        let expected = fs::read(&expected_path)
            .unwrap_or_else(|error| panic!("missing golden for `{stem}`: {error}"));
        let cache = std::env::temp_dir().join(format!(
            "jet_1543_cli_{}_{}",
            std::process::id(),
            stem.replace('/', "_")
        ));

        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["run", &file, "--trace-tiers"])
            .current_dir(&root)
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
            .args(["run", "--interpret", &file, "--trace-tiers"])
            .current_dir(&root)
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

        let _ = fs::remove_dir_all(cache);
    }
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
