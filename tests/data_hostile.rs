//! #709 / #237 C4: flagship analysis + hostile typed-data corpus (AOT).
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn build_and_run(dir: &Path, name: &str, src: &str) -> (i32, String, String) {
    let path = dir.join(format!("{name}.jet"));
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let out = Command::new(&bin).current_dir(dir).output().unwrap();
    (
        out.status.code().unwrap_or(0),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn assert_example_matches_golden(stem: &str) {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping {stem} AOT corpus (need rustc)");
        return;
    }
    let root = repo_root();
    let src = fs::read_to_string(root.join(format!("examples/features/tooling/{stem}.jet")))
        .unwrap_or_else(|e| panic!("read {stem}.jet: {e}"));
    let expected = fs::read_to_string(root.join(format!(
        "examples/features/expected/tooling/{stem}.out"
    )))
    .unwrap_or_else(|e| panic!("read {stem}.out: {e}"));
    let dir = std::env::temp_dir().join(format!(
        "jet_data_hostile_{stem}_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(&dir, stem, &src);
    assert_eq!(code, 0, "{stem} failed: {stderr}");
    assert_eq!(stdout, expected, "{stem} stdout drifted from golden");
    assert_eq!(stderr, "", "{stem} unexpected stderr: {stderr}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn flagship_analysis_example_matches_golden_aot() {
    assert_example_matches_golden("data_analysis");
}

#[test]
fn hostile_data_corpus_matches_golden_aot() {
    assert_example_matches_golden("data_hostile");
}

#[test]
fn hostile_corpus_covers_required_failure_classes() {
    let root = repo_root();
    let out = fs::read_to_string(root.join("examples/features/expected/tooling/data_hostile.out"))
        .expect("data_hostile.out");
    for needle in [
        "empty_mean: Empty mean:",
        "empty_quantile: Empty quantile:",
        "empty_variance: Empty variance:",
        "missing_count: 3",
        "dup_join_count: 5",
        "pivot: row=a|b col=c sum=12.0",
        "tie_order: 3.0,1.0,2.0",
        "nan_mean: NonFinite",
        "inf_sum: NonFinite",
        "signed_zero_sum: 0.0",
        "signed_zero_mean: 0.0",
        "pop_variance: 0.6666666666666666",
        "one_variance: 0.0",
        "bad_quantile: InvalidArgument quantile:",
        "bad_window: InvalidArgument rolling_mean:",
        "hostile_svg_escaped: true",
        "large_limit: Limit group_mean:",
    ] {
        assert!(
            out.contains(needle),
            "hostile golden missing required class `{needle}`:\n{out}"
        );
    }
}

#[test]
fn flagship_example_covers_analysis_pipeline() {
    let root = repo_root();
    let out = fs::read_to_string(root.join("examples/features/expected/tooling/data_analysis.out"))
        .expect("data_analysis.out");
    for needle in [
        "tickets: 5",
        "focused: 4",
        "owned: Core",
        "mean: Core",
        "describe: count=5",
        "Core | ## 2",
        "svg bytes:",
        "status: core.data.csv native",
    ] {
        assert!(
            out.contains(needle),
            "flagship golden missing `{needle}`:\n{out}"
        );
    }
}
