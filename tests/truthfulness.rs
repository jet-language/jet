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
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn rustc_availability_probes_use_common_helper() {
    let root = root();
    let suites = [
        "tests/golden.rs",
        "tests/release_gates.rs",
        "tests/dev.rs",
        "tests/archive.rs",
        "tests/regex.rs",
        "tests/ice_regressions.rs",
        "tests/comptime_diff.rs",
        "tests/jet_test.rs",
    ];
    let forbidden = [
        "Command::new(\"rustc\").arg(\"--version\")",
        "Command::new(\"rustc\").arg(\"-V\")",
    ];
    let mut violations = Vec::new();
    for suite in suites {
        let source = fs::read_to_string(root.join(suite)).unwrap();
        for needle in forbidden {
            if source.contains(needle) {
                violations.push(format!("{suite}: direct `{needle}` availability probe"));
            }
        }
        if matches!(suite, "tests/archive.rs" | "tests/regex.rs") {
            let helper = source
                .split("fn have_toolchain() -> bool {")
                .nth(1)
                .and_then(|tail| tail.split_once('}').map(|(body, _)| body))
                .expect("have_toolchain helper");
            let rustc_pos = helper.find("have_rustc()").expect("rustc gate");
            let cargo_pos = helper.find("Command::new(\"cargo\")").expect("cargo probe");
            if rustc_pos > cargo_pos {
                violations.push(format!(
                    "{suite}: have_rustc must run before the cargo probe so JET_REQUIRE_RUSTC=1 cannot short-circuit"
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "rustc availability must use tests/common::have_rustc so JET_REQUIRE_RUSTC cannot be bypassed:\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// ACKNOWLEDGED gaps (pre-existing doc debt, not regressions)
// ---------------------------------------------------------------------------
//
// GAP-2: `examples/features/expected/test.out` exists with no corresponding
//        `examples/features/test.jet` or `examples/features/test/main.jet`.
//        The file appears to be an orphan leftover.
//
// ---------------------------------------------------------------------------
// Check 1: Every example *.jet file referenced in docs/ actually exists
// ---------------------------------------------------------------------------
// Applies to the durable docs only. The PM history trees (plans/, proposals/,
// sidequests/, ballots/ — moved from tools/Tower/docs on 2026-07-10) are
// point-in-time records and keep example names from older repo layouts.
const PM_HISTORY_DIRS: [&str; 4] = ["plans", "proposals", "sidequests", "ballots"];

#[test]
fn docs_referenced_examples_exist() {
    let root = root();
    let docs_dir = root.join("docs");
    let readme = root.join("README.md");

    let mut doc_content = String::new();
    if let Ok(s) = fs::read_to_string(&readme) {
        doc_content.push_str(&s);
    }
    let Ok(entries) = fs::read_dir(&docs_dir) else {
        panic!("docs/ missing");
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if PM_HISTORY_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk_md(&path, &mut |s| doc_content.push_str(s));
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(s) = fs::read_to_string(&path) {
                doc_content.push_str(&s);
            }
        }
    }

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
// Check 4: Every jet subcommand named in README/docs exists in jet-cli's registry
// ---------------------------------------------------------------------------
#[test]
fn readme_subcommands_exist_in_cli() {
    let root = root();
    let cli_path = root.join("crates/jet-cli/src/CLI.rs");
    let cli_src = fs::read_to_string(&cli_path).expect("jet-cli registry missing");

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

    // `jet self upgrade` is mentioned in docs/reference/versioning.md
    let versioning =
        fs::read_to_string(root.join("docs/reference/versioning.md")).unwrap_or_default();
    if versioning.contains("`jet self upgrade`") && !known.contains("upgrade") {
        missing.push("upgrade (referenced in versioning.md)".to_string());
    }

    assert!(
        missing.is_empty(),
        "README/docs reference jet subcommands not present in jet-cli's registry:\n{}",
        missing.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Check 5: Every examples/features/<topic>/*.jet has a matching expected
// output. `expected/` mirrors the <topic>/ tree (D-REPO-EXAMPLES1=A).
// ---------------------------------------------------------------------------
#[test]
fn every_feature_example_has_expected_output() {
    // CAPABILITY_CLAIM: claim.examples-spec / expected-output-pairs
    let root = root();
    let ex_dir = root.join("examples/features");
    let expected_dir = ex_dir.join("expected");

    let mut missing: Vec<String> = Vec::new();

    for topic_entry in fs::read_dir(&ex_dir).unwrap().flatten() {
        let topic_path = topic_entry.path();
        if !topic_path.is_dir() {
            continue;
        }
        let topic = topic_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
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
// Check 7: jet-cli owns the only in-code CLI command registry
// ---------------------------------------------------------------------------
#[test]
fn cli_rs_is_declared_module() {
    let root = root();
    let lib_src = fs::read_to_string(root.join("Source/lib.rs")).expect("Source/lib.rs missing");
    assert!(lib_src.contains("pub use jet_cli::{CLI, Explain, Help};"),
        "Source/lib.rs must re-export the real jet-cli seam");
    assert!(
        !lib_src.contains("mod CLISpec;"),
        "Source/lib.rs declares mod CLISpec — jet-cli owns the only CLI registry"
    );
}

#[test]
fn cli_has_no_second_command_registry() {
    let root = root();
    assert!(
        !root.join("Source/CLISpec.rs").exists(),
        "Source/CLISpec.rs must not exist — jet-cli owns the single CLI registry"
    );

    let mut registries = Vec::new();
    for path in rs_files(&root.join("Source")).into_iter()
        .chain(rs_files(&root.join("crates/jet-cli/src"))) {
        if path.extension().and_then(|x| x.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        if text.contains("pub const COMMANDS: &[CommandSpec]") {
            registries.push(path.strip_prefix(&root).unwrap().display().to_string());
        }
    }
    assert_eq!(
        registries,
        vec!["crates/jet-cli/src/CLI.rs".to_string()],
        "found a second CLI command registry:\n{}",
        registries.join("\n")
    );

    let main_src = fs::read_to_string(root.join("Source/main.rs")).expect("Source/main.rs missing");
    assert!(
        !main_src.contains("let known ="),
        "Source/main.rs must not rebuild the command registry; use jet::CLI::is_builtin"
    );
    assert!(
        main_src.contains("jet::CLI::is_builtin(cmd)"),
        "Source/main.rs should dispatch unknown-command checks through jet-cli"
    );
}

// ---------------------------------------------------------------------------
// Check 8: Compiler seam crates stay dependency-clean (I6)
// ---------------------------------------------------------------------------
#[test]
fn compiler_seam_crates_have_only_path_dependencies() {
    // I6 pin (card #447 / durability W2): enumerate crates/ instead of a
    // hardcoded allowlist, so a new crate is never invisible to this check.
    // Any crate below not named in EXEMPTIONS is a compiler seam and must
    // stay path-dependency-only. Exemptions require an owner-ratified
    // decision ID cited in a comment directly above the dependency line;
    // each cited ID is cross-checked against docs/spec/syntax-decisions.md's
    // Ratified section so an exemption can never quietly outlive its
    // ratification (or cite an ID that was never ratified).
    const EXEMPTIONS: &[(&str, &[&str])] = &[
        ("jet-jit", &["D-JITDEP1", "D-JIT2"]),
        ("jet-net", &["D-NETDEP1", "D-TLS1"]),
        // Card #367 / D-PRODUCT-SPLIT1=C: FFI.rs (the rustls test-only
        // loopback peer) moved from `jetpack` to `jet-pkg-model`.
        ("jet-pkg-model", &["D-TLS1", "D-EMAIL-DKIM-CONFIG1"]),
    ];

    let root = root();
    let decisions_doc = fs::read_to_string(root.join("docs/spec/syntax-decisions.md"))
        .expect("docs/spec/syntax-decisions.md missing");
    let ratified_doc = section_between_pub(&decisions_doc);

    for (crate_name, ids) in EXEMPTIONS {
        for id in *ids {
            assert!(
                ratified_doc.contains(id),
                "I6 exemption for `{crate_name}` cites {id}, which does not appear in \
                 docs/spec/syntax-decisions.md's Ratified section — revoke the exemption \
                 or get {id} ratified"
            );
        }
    }

    let mut crate_manifests: Vec<(String, PathBuf)> =
        vec![("<root>".to_string(), root.join("Cargo.toml"))];
    let crates_dir = root.join("crates");
    let mut dirs: Vec<_> = fs::read_dir(&crates_dir)
        .expect("crates/ missing")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            crate_manifests.push((name, manifest));
        }
    }
    // D-ARCH-SOURCE1=A: CLI/interactive seams are not optional aliases hidden in
    // the root crate. Their manifests must exist and therefore pass this same
    // path-only dependency audit.
    for required in ["jet-repl", "jet-debug", "jet-cli"] {
        assert!(
            crate_manifests.iter().any(|(name, _)| name == required),
            "D-ARCH-SOURCE1 requires the {required} workspace seam"
        );
    }
    let root_lib = fs::read_to_string(root.join("Source/lib.rs")).expect("Source/lib.rs missing");
    assert!(
        root_lib.contains("pub use jet_debug as Debug;")
            && !root.join("Source/Debug").exists(),
        "D-ARCH-SOURCE1 requires jet-debug ownership, not a root Debug wrapper"
    );
    assert!(
        root_lib.contains("pub use jet_cli::{CLI, Explain, Help};")
            && !root.join("Source/CLI.rs").exists()
            && !root.join("Source/Explain.rs").exists()
            && !root.join("Source/Help").exists(),
        "D-ARCH-SOURCE1 requires jet-cli ownership, not root CLI/help wrappers"
    );
    assert!(
        root_lib.contains("pub use jet_foundation::ExitCodes;")
            && !root.join("Source/ExitCodes.rs").exists()
            && !root.join("Source/LSP/JSON.rs").exists(),
        "shared debugger/LSP policy must live in jet-foundation, not Source wrappers"
    );

    let mut offenders = Vec::new();
    for (name, manifest) in &crate_manifests {
        let text = fs::read_to_string(manifest).unwrap_or_else(|_| panic!("{} missing", manifest.display()));
        let exemption = EXEMPTIONS.iter().find(|(n, _)| n == name);
        for (line, context) in dependency_lines_with_context(&text) {
            if line.contains(" path = ") || line.contains("{ path =") {
                continue;
            }
            match exemption {
                Some((_, ids)) if ids.iter().any(|id| context.contains(id)) => continue,
                Some((_, ids)) => offenders.push(format!(
                    "{name}: {line} — external dep must cite one of {ids:?} in a comment \
                     directly above the dependency line"
                )),
                None => offenders.push(format!(
                    "{name}: {line} — not an exempted crate; I6 forbids external \
                     dependencies in compiler seam crates (add to EXEMPTIONS with a \
                     ratified decision ID if this is intentional)"
                )),
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "compiler seam crates must not add unexempted external dependencies (I6):\n{}",
        offenders.join("\n")
    );
}

/// Returns the `## Ratified` section of syntax-decisions.md (up to the next `## ` heading).
fn section_between_pub(docs: &str) -> &str {
    let from = docs.find("## Ratified").expect("docs/spec/syntax-decisions.md missing ## Ratified");
    let rest = &docs[from + "## Ratified".len()..];
    let to = rest[..].find("\n## ").unwrap_or(rest.len());
    &rest[..to]
}

/// Like `dependency_lines`, but also returns any `#`-comment lines
/// immediately preceding each dependency line (joined), so exemption
/// decision IDs cited above a dep can be matched to it.
fn dependency_lines_with_context(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_deps = false;
    // A comment block covers every dependency line until a blank line or a
    // new comment block resets it — same "no blank line breaks coverage"
    // convention as the I7 keyword/decision-comment check in tests/decisions.rs.
    let mut current_context = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_deps = matches!(
                line,
                "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
            );
            current_context.clear();
            continue;
        }
        if !in_deps {
            continue;
        }
        if line.is_empty() {
            current_context.clear();
            continue;
        }
        if line.starts_with('#') {
            current_context.push_str(line);
            current_context.push('\n');
            continue;
        }
        out.push((line.to_string(), current_context.clone()));
    }
    out
}

// ---------------------------------------------------------------------------
// Check 9: E3 capability claims stay bound to executable proof
// ---------------------------------------------------------------------------
#[test]
fn epoch3_capability_manifest_is_current_and_owned() {
    let root = root();
    let output = Command::new("node")
        .arg("scripts/agent/check-capability-ledger.mjs")
        .arg("--check")
        .current_dir(&root)
        .output()
        .expect("node must run the capability-ledger checker");

    assert!(
        output.status.success(),
        "capability ledger rejected:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn epoch3_capability_manifest_rejects_hostile_real_card_fixtures() {
    let root = root();
    let output = Command::new("node")
        .arg("scripts/agent/check-capability-ledger.mjs")
        .arg("--hostile-fixtures")
        .current_dir(&root)
        .output()
        .expect("node must run the capability-claim hostile fixtures");

    assert!(
        output.status.success(),
        "capability-claim hostile fixtures failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Check 11 (I3 pin, card #447 / durability W2): codegen is dumb — zero
// `Diagnostic::` calls in jet-codegen. All checking lives in sema; codegen
// must never "try rustc and see" or synthesize its own diagnostics.
// ---------------------------------------------------------------------------
#[test]
fn codegen_never_constructs_diagnostics() {
    let root = root();
    let dir = root.join("crates/jet-codegen/src");
    let mut offenders = Vec::new();
    for path in rs_files(&dir) {
        let text = fs::read_to_string(&path).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            if line.contains("Diagnostic::") {
                offenders.push(format!(
                    "{}:{}: {}",
                    path.strip_prefix(&root).unwrap().display(),
                    i + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "jet-codegen must never construct Diagnostic:: (I3 — codegen is dumb, \
         all checking lives in sema):\n{}",
        offenders.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Check 12 (durability pin, card #452): zero `include!` splices of sibling
// .rs fragments in compiler code. All former include!-splice parents were
// converted to real modules; this pins the state so nobody reintroduces the
// pattern. `include_str!` (data embedding, e.g. the prelude) is unaffected —
// only the `include!` code-splice macro is checked.
// ---------------------------------------------------------------------------
#[test]
fn compiler_code_has_no_include_splices() {
    // Exact allowlist: dual-use runtime template also include_str!'d and
    // spliced into generated bridge crates at codegen time. Card #367 /
    // D-PRODUCT-SPLIT1=C moved this file into jet-pkg-model (the shared
    // package/config data model) — still a tool-facing crate, not a
    // compiler seam crate, and this include! is a test-only splice for the
    // template's own unit tests.
    const ALLOWLIST: &[&str] = &["crates/jet-pkg-model/src/FFI.rs"];

    let root = root();
    let mut dirs = vec![root.join("Source")];
    for entry in fs::read_dir(root.join("crates")).expect("crates/ missing").flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }

    let mut offenders = Vec::new();
    for dir in dirs {
        for path in rs_files(&dir) {
            let rel = path.strip_prefix(&root).unwrap().display().to_string();
            if ALLOWLIST.contains(&rel.as_str()) {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap_or_default();
            for (i, line) in text.lines().enumerate() {
                if let Some(pos) = line.find("include!") {
                    let after = line[pos + "include!".len()..].trim_start();
                    if after.starts_with('(') {
                        offenders.push(format!("{}:{}: {}", rel, i + 1, line.trim()));
                    }
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "compiler code must not `include!`-splice sibling .rs fragments — convert to a \
         real module instead (card #452 durability pattern):\n{}",
        offenders.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Check 10: Core spec files referenced by D-STDRUBRIC1 exist (c44)
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

fn rs_files(dir: &PathBuf) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rs_files(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    out.sort();
    out
}
