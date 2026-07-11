//! #353: deterministic accepts-invalid and miscompile adversary corpus.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::{build_and_run, fixture_matches, normalize_fixture_selector};
use jet::Interpreter::{dev_iteration, RunOutcome};

const CORPUS: &str = "tests/fuzz/sema";

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CORPUS)
}

fn cases(kind: &str) -> Vec<PathBuf> {
    let dir = corpus_root().join(kind);
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("jet"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "empty sema soundness corpus: {}", dir.display());
    files
}

fn relative(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn selected(path: &Path) -> bool {
    let filter = std::env::var("SEMA_SOUNDNESS_CASE")
        .ok()
        .map(|s| normalize_fixture_selector("SEMA_SOUNDNESS_CASE", &s));
    fixture_matches(filter.as_deref(), &relative(path))
}

fn replay(path: &Path) {
    eprintln!(
        "replay: SEMA_SOUNDNESS_CASE={} cargo test --test sema_soundness -- --nocapture",
        relative(path)
    );
}

fn require_rustc() {
    let out = Command::new("rustc")
        .arg("--version")
        .output()
        .unwrap_or_else(|e| panic!("sema soundness requires rustc; refusing to skip: {e}"));
    assert!(out.status.success(), "rustc unavailable; refusing to skip sema soundness");
}

fn expected_code(path: &Path) -> &str {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit_once('.'))
        .map(|(_, code)| code)
        .unwrap_or_else(|| panic!("invalid fixture must end in .E####.jet: {}", path.display()))
}

#[test]
fn exact_invalid_corpus_rejects_in_jet() {
    let mut ran = 0;
    for path in cases("invalid") {
        if !selected(&path) { continue; }
        ran += 1;
        replay(&path);
        let expected = expected_code(&path);
        let src = fs::read_to_string(&path).unwrap();
        let diags = match jet::compile_with_path(&src, &path.to_string_lossy()) {
            Ok(_) => panic!("{}: sema accepted known-invalid fixture", relative(&path)),
            Err(diags) => diags,
        };
        assert!(
            diags.iter().any(|d| d.code == expected),
            "{}: expected {expected}, got {:?}",
            relative(&path),
            diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
        );
    }
    assert!(ran > 0, "SEMA_SOUNDNESS_CASE selected no invalid fixture");
}

#[test]
fn valid_corpus_reaches_rustc() {
    require_rustc();
    let mut ran = 0;
    for path in cases("valid") {
        if !selected(&path) { continue; }
        ran += 1;
        replay(&path);
        let src = fs::read_to_string(&path).unwrap();
        let name = path.file_stem().unwrap().to_string_lossy();
        let (code, _, stderr) = build_and_run("jet_sema_sound_valid", &name, &src);
        assert_eq!(code, 0, "{} failed:\n{stderr}", relative(&path));
    }
    assert!(ran > 0, "SEMA_SOUNDNESS_CASE selected no valid fixture");
}

#[test]
fn executable_corpus_matches_aot_and_default_dev() {
    require_rustc();
    let mut ran = 0;
    for path in cases("differential") {
        if !selected(&path) { continue; }
        ran += 1;
        replay(&path);
        let src = fs::read_to_string(&path).unwrap();
        let name = path.file_stem().unwrap().to_string_lossy();
        let expected = fs::read_to_string(path.with_extension("out"))
            .unwrap_or_else(|e| panic!("{} needs .out: {e}", relative(&path)));
        let aot = build_and_run("jet_sema_sound_diff", &name, &src);
        let dev = match dev_iteration(&path.to_string_lossy(), false, false) {
            RunOutcome::Ran { stdout, stderr, exit_code } => (exit_code, stdout, stderr),
            RunOutcome::Problems(diags) => panic!("{} default dev refused fixture: {diags:?}", relative(&path)),
        };
        assert_eq!(aot, dev, "{} AOT/default-dev divergence", relative(&path));
        assert_eq!(aot, (0, expected, String::new()), "{} output drift", relative(&path));
    }
    assert!(ran > 0, "SEMA_SOUNDNESS_CASE selected no differential fixture");
}
