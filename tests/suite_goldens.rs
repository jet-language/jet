//! Golden tests for the user-story suite.
//!
//! The feature corpus keeps its existing discovery and acceptance path. This
//! focused test gives the additive suite the same executable-output contract.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn user_story_suite_matches_expected_output() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let suite = root.join("examples/suites");
    let mut programs = fs::read_dir(&suite)
        .expect("user-story suite exists")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("jet")
                && path.file_name().and_then(|name| name.to_str()) != Some("package.jet")
        })
        .collect::<Vec<_>>();
    programs.sort();
    assert_eq!(programs.len(), 8, "the suite must keep eight user stories");

    let cache = std::env::temp_dir().join(format!(
        "jet-user-story-golden-{}",
        std::process::id()
    ));
    let _scratch = Scratch(cache.clone());
    fs::create_dir_all(&cache).expect("create golden cache");

    for program in programs {
        let stem = program
            .file_stem()
            .and_then(|name| name.to_str())
            .expect("program has a utf8 stem");
        let expected = suite.join("expected").join(format!("{stem}.out"));
        let expected_output = fs::read_to_string(&expected)
            .unwrap_or_else(|error| panic!("read {}: {error}", expected.display()));
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(["run", program.to_str().expect("program path is utf8")])
            .current_dir(&root)
            .env("JETPACK_ENV", "1")
            .env_remove("JETPACK_ENV_DIR")
            .env("JET_CACHE_DIR", &cache)
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("run {}: {error}", program.display()));
        assert!(
            output.status.success(),
            "{} failed:\n{}",
            program.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected_output,
            "golden mismatch for {}",
            program.display()
        );
    }
}

#[test]
fn semantic_corpus_policy_runs_with_suite_goldens() {
    common::corpus_policy::CorpusPolicy::load()
        .expect("corpus manifest")
        .check_gate("suite")
        .expect("suite corpus semantic policy");
}
