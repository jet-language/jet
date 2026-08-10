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

        let _ = fs::remove_dir_all(cache);
    }
}
