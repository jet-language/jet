//! Truthfulness gate (board card c114, P0).
//!
//! Asserts currently-true alignments between docs, examples, and the
//! implementation. Fails if they regress. Gaps that exist today but are
//! not yet fixed are listed in the ACKNOWLEDGED sets below with comments.
//!
//! Run: `cargo test --test truthfulness`

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ---------------------------------------------------------------------------
// ACKNOWLEDGED gaps (pre-existing doc debt, not regressions)
// ---------------------------------------------------------------------------
//
// GAP-1: docs/reference/versioning.md references `.github/workflows/release.yml`
//        but no release workflow exists yet (tracked in c113 / release-ci-hygiene).
//
// GAP-2: `examples/features/expected/test.out` exists with no corresponding
//        `examples/features/test.jet` or `examples/features/test/main.jet`.
//        The file appears to be an orphan leftover.
//
// GAP-3: Source/CLISpec.rs is a complete alternative CLI spec but is not
//        declared in lib.rs and is dead code. The authoritative spec is
//        Source/CLI.rs (COMMANDS array).

// ---------------------------------------------------------------------------
// Check 1: Every example *.jet file referenced in docs/ actually exists
// ---------------------------------------------------------------------------
#[test]
fn docs_referenced_examples_exist() {
    let root = root();
    let docs_dir = root.join("docs");
    let readme = root.join("README.md");

    let mut doc_content = String::new();
    if let Ok(s) = fs::read_to_string(&readme) {
        doc_content.push_str(&s);
    }
    walk_md(&docs_dir, &mut |s| doc_content.push_str(s));

    let mut missing: Vec<String> = Vec::new();
    for cap in extract_example_paths(&doc_content) {
        let path = root.join(&cap);
        if !path.exists() {
            missing.push(cap);
        }
    }

    assert!(
        missing.is_empty(),
        "docs reference example files that do not exist:\n{}",
        missing.join("\n")
    );
}

fn extract_example_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let prefixes = ["examples/features/", "examples/canon.jet"];
    for prefix in prefixes {
        if prefix.ends_with(".jet") {
            if text.contains(prefix) {
                out.push(prefix.to_string());
            }
            continue;
        }
        let mut rest = text;
        while let Some(pos) = rest.find(prefix) {
            rest = &rest[pos + prefix.len()..];
            let end = rest
                .find(|c: char| {
                    !c.is_alphanumeric() && c != '_' && c != '.' && c != '-' && c != '/'
                })
                .unwrap_or(rest.len());
            let tail = &rest[..end];
            if tail.ends_with(".jet") {
                out.push(format!("{}{}", prefix, tail));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Check 2: canon.jet exists and compiles
// ---------------------------------------------------------------------------
#[test]
fn canon_jet_exists() {
    let canon = root().join("examples/canon.jet");
    assert!(canon.is_file(), "examples/canon.jet must exist");
    let src = fs::read_to_string(&canon).unwrap();
    assert!(
        jet::compile_with_path(&src, "examples/canon.jet").is_ok(),
        "examples/canon.jet must pass the front end"
    );
}

// ---------------------------------------------------------------------------
// Check 3: Error pages referenced in README exist
// ---------------------------------------------------------------------------
#[test]
fn readme_error_pages_exist() {
    let root = root();
    let readme = fs::read_to_string(root.join("README.md")).expect("README.md missing");
    let errors_dir = root.join("docs/reference/errors");

    let mut missing: Vec<String> = Vec::new();
    let mut i = 0;
    let bytes = readme.as_bytes();
    while i + 8 <= bytes.len() {
        // Match `ENNNN.md` (e.g. in `[E0102](docs/reference/errors/E0102.md)`)
        if bytes[i] == b'E'
            && bytes[i + 1..i + 5].iter().all(|b| b.is_ascii_digit())
            && bytes[i + 5..i + 8] == *b".md"
        {
            let code = format!("E{}", std::str::from_utf8(&bytes[i + 1..i + 5]).unwrap());
            let page = errors_dir.join(format!("{}.md", code));
            if !page.is_file() {
                missing.push(format!("{}.md", code));
            }
        }
        i += 1;
    }

    assert!(
        missing.is_empty(),
        "README references error doc pages that do not exist in docs/reference/errors/:\n{}",
        missing.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Check 4: Every jet subcommand named in README/docs exists in Source/CLI.rs
// ---------------------------------------------------------------------------
#[test]
fn readme_subcommands_exist_in_cli() {
    let root = root();
    let cli_path = root.join("Source/CLI.rs");
    let cli_src = fs::read_to_string(&cli_path).expect("Source/CLI.rs missing");

    // Extract all known command names from the COMMANDS array in CLI.rs
    let mut known: HashSet<String> = HashSet::new();
    {
        let mut rest: &str = &cli_src;
        while let Some(pos) = rest.find("name: \"") {
            rest = &rest[pos + 7..];
            if let Some(end) = rest.find('"') {
                known.insert(rest[..end].to_string());
            }
        }
    }

    // Subcommands the README/docs explicitly name that must exist in the CLI.
    let required_commands = [
        "run", "check", "build", "test", "fmt", "fix", "doctor", "publish", "gc",
    ];

    let mut missing: Vec<String> = Vec::new();
    for cmd in required_commands {
        if !known.contains(cmd) {
            missing.push(cmd.to_string());
        }
    }

    // `jet upgrade` is mentioned in docs/reference/versioning.md
    let versioning =
        fs::read_to_string(root.join("docs/reference/versioning.md")).unwrap_or_default();
    if versioning.contains("`jet upgrade`") && !known.contains("upgrade") {
        missing.push("upgrade (referenced in versioning.md)".to_string());
    }

    assert!(
        missing.is_empty(),
        "README/docs reference jet subcommands not present in Source/CLI.rs:\n{}",
        missing.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Check 5: Every examples/features/<topic>/*.jet has a matching expected
// output. `expected/` mirrors the <topic>/ tree (D-REPO-EXAMPLES1=A).
// ---------------------------------------------------------------------------
#[test]
fn every_feature_example_has_expected_output() {
    let root = root();
    let ex_dir = root.join("examples/features");
    let expected_dir = ex_dir.join("expected");

    let mut missing: Vec<String> = Vec::new();

    for topic_entry in fs::read_dir(&ex_dir).unwrap().flatten() {
        let topic_path = topic_entry.path();
        if !topic_path.is_dir() {
            continue;
        }
        let topic = topic_path.file_name().unwrap().to_string_lossy().into_owned();
        if topic == "expected" {
            continue;
        }
        let expected_topic_dir = expected_dir.join(&topic);

        for entry in fs::read_dir(&topic_path).unwrap().flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());

            if ext == Some("jet") {
                let name = path.file_stem().unwrap().to_string_lossy().into_owned();
                let stem = format!("{}/{}", topic, name);
                let out = expected_topic_dir.join(format!("{}.out", name));
                let errout = expected_topic_dir.join(format!("{}.err.out", name));
                if !out.is_file() && !errout.is_file() {
                    missing.push(format!(
                        "examples/features/{}.jet → missing expected/{}.out or .err.out",
                        stem, stem
                    ));
                }
            } else if path.is_dir() {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                let stem = format!("{}/{}", topic, name);
                let main = path.join("main.jet");
                if main.is_file() {
                    let out = expected_topic_dir.join(format!("{}.out", name));
                    let errout = expected_topic_dir.join(format!("{}.err.out", name));
                    if !out.is_file() && !errout.is_file() {
                        missing.push(format!(
                            "examples/features/{}/main.jet → missing expected/{}.out or .err.out",
                            stem, stem
                        ));
                    }
                }
            }
        }
    }

    assert!(
        missing.is_empty(),
        "feature examples without a matching expected output:\n{}",
        missing.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Check 6: Cargo.toml and flake.nix agree on the version
// ---------------------------------------------------------------------------
#[test]
fn cargo_and_flake_versions_match() {
    let root = root();

    let cargo = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml missing");
    let flake = fs::read_to_string(root.join("flake.nix")).expect("flake.nix missing");

    let cargo_version = cargo
        .lines()
        .find(|l| l.starts_with("version"))
        .and_then(|l| l.split('"').nth(1))
        .expect("version not found in Cargo.toml");

    // flake.nix has `version = "x.y.z";` inside the buildRustPackage block
    let flake_version = flake
        .lines()
        .find(|l| l.contains("version = \"") && !l.trim_start().starts_with('#'))
        .and_then(|l| l.split('"').nth(1))
        .expect("version not found in flake.nix");

    assert_eq!(
        cargo_version, flake_version,
        "Cargo.toml version ({}) and flake.nix version ({}) do not match",
        cargo_version, flake_version
    );
}

// ---------------------------------------------------------------------------
// Check 7: Source/CLI.rs is the declared CLI module (CLISpec.rs is dead)
// ---------------------------------------------------------------------------
#[test]
fn cli_rs_is_declared_module() {
    let root = root();
    let lib_src = fs::read_to_string(root.join("Source/lib.rs")).expect("Source/lib.rs missing");
    assert!(
        lib_src.contains("pub mod CLI;") || lib_src.contains("mod CLI;"),
        "Source/lib.rs does not declare `mod CLI` — the CLI module is not wired up"
    );
    // CLISpec.rs exists but is intentionally not declared (GAP-3 above).
    assert!(
        !lib_src.contains("mod CLISpec;"),
        "Source/lib.rs declares mod CLISpec — reconcile with CLI.rs before wiring it in"
    );
}

// ---------------------------------------------------------------------------
// Check 8: Compiler seam crates stay dependency-clean (I6)
// ---------------------------------------------------------------------------
#[test]
fn compiler_seam_crates_have_only_path_dependencies() {
    let root = root();
    let compiler_crates = [
        "Cargo.toml",
        "crates/jet-foundation/Cargo.toml",
        "crates/jet-lexer/Cargo.toml",
        "crates/jet-parser/Cargo.toml",
        "crates/jet-comptime/Cargo.toml",
        "crates/jet-sema/Cargo.toml",
        "crates/jet-codegen/Cargo.toml",
        "crates/jet-driver/Cargo.toml",
        "crates/jet-semindex/Cargo.toml",
        // `crates/jet-jit` carries owner-approved Cranelift deps (D-JITDEP1 / D-JIT2=A);
        // I6 applies to compiler `Source/` and seam crates above, not the JIT sibling.
        // `crates/jet-net` is a bootstrap HTTP helper (D-NETDEP1=A, owner-approved
        // `ureq`) — not a compiler seam; I6 applies to `Source/` and seam crates only.
        // D-REGEX1 explicitly owner-approved `regex` for `core.regex`; comptime
        // reuses that engine for REPL/dev parity until the native engine replaces it.
    ];

    let mut offenders = Vec::new();
    for rel in compiler_crates {
        let text = fs::read_to_string(root.join(rel)).unwrap_or_else(|_| panic!("{rel} missing"));
        for line in dependency_lines(&text) {
            if !line.contains(" path = ")
                && !line.contains("{ path =")
                && !(rel == "crates/jet-comptime/Cargo.toml" && line == "regex = \"1\"")
            {
                offenders.push(format!("{rel}: {line}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "compiler seam crates must not add external dependencies (I6):\n{}",
        offenders.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Check 9: Core spec files referenced by D-STDRUBRIC1 exist (c44)
// ---------------------------------------------------------------------------
#[test]
fn stdlib_api_laws_doc_exists() {
    let root = root();
    let path = root.join("docs/spec/stdlib-api-laws.md");
    assert!(
        path.is_file(),
        "docs/spec/stdlib-api-laws.md is missing — required by D-STDRUBRIC1 (c44)"
    );
}

fn dependency_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_deps = matches!(
                line,
                "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
            );
            continue;
        }
        if in_deps && !line.is_empty() && !line.starts_with('#') {
            out.push(line.to_string());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn walk_md(dir: &PathBuf, cb: &mut impl FnMut(&str)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            walk_md(&path, cb);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(s) = fs::read_to_string(&path) {
                cb(&s);
            }
        }
    }
}
