//! Truthfulness gate (board card c114, P0).
//!
//! Asserts currently-true alignments between docs, examples, and the
//! implementation. Fails if they regress. Gaps that exist today but are
//! not yet fixed are listed in the ACKNOWLEDGED sets below with comments.
//!
//! Run: `cargo test --test truthfulness`

mod common;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Card #1639 (D-ONCE): the one home for "does this test file bypass the
/// `have_rustc()` guard" — walks every file under `tests/` instead of a
/// hardcoded suite list, so a new test file is covered automatically.
/// `tests/common/mod.rs` and `tests/tir_support/mod.rs` are the two
/// legitimate homes that define the probe itself; `tests/truthfulness.rs`
/// is exempt because this function's own source quotes the needles.
const RUSTC_PROBE_ALLOWED_HOMES: &[&str] = &[
    "tests/common/mod.rs",
    "tests/tir_support/mod.rs",
    "tests/truthfulness.rs",
];

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// Files (relative to `root`) under `tests/` that contain `needle`, ignoring
/// whitespace (rustfmt can wrap `.arg(...)` onto its own line) and skipping
/// `RUSTC_PROBE_ALLOWED_HOMES`.
fn stray_probe_sites(root: &Path, needle: &str) -> Vec<String> {
    let mut files = Vec::new();
    collect_rs_files(&root.join("tests"), &mut files);
    files.sort();
    let mut findings = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(root)
            .expect("scanned file stays beneath repository root")
            .to_string_lossy()
            .replace('\\', "/");
        if RUSTC_PROBE_ALLOWED_HOMES.contains(&relative.as_str()) {
            continue;
        }
        let source = fs::read_to_string(&file).expect("test source is readable");
        let collapsed: String = source.chars().filter(|c| !c.is_whitespace()).collect();
        let collapsed_needle: String = needle.chars().filter(|c| !c.is_whitespace()).collect();
        if collapsed.contains(&collapsed_needle) {
            findings.push(relative);
        }
    }
    findings
}

#[test]
fn rustc_availability_probes_use_common_helper() {
    let root = root();
    let forbidden = [
        "Command::new(\"rustc\").arg(\"--version\")",
        "Command::new(\"rustc\").arg(\"-V\")",
    ];
    let mut violations = Vec::new();
    for needle in forbidden {
        for finding in stray_probe_sites(&root, needle) {
            violations.push(format!("{finding}: direct `{needle}` availability probe"));
        }
    }
    for suite in ["tests/archive.rs", "tests/regex.rs"] {
        let source = fs::read_to_string(root.join(suite)).unwrap();
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
    assert!(
        violations.is_empty(),
        "rustc availability must use tests/common::have_rustc so JET_REQUIRE_RUSTC cannot be bypassed:\n{}",
        violations.join("\n")
    );
}

#[test]
fn rustc_probe_scanner_detects_a_seeded_stray_probe() {
    let scratch =
        std::env::temp_dir().join(format!("jet_rustc_probe_guard_test_{}", std::process::id()));
    let seed_dir = scratch.join("tests");
    fs::create_dir_all(&seed_dir).unwrap();
    fs::write(
        seed_dir.join("seeded_probe.rs"),
        "fn f() { Command::new(\"rustc\")\n.arg(\"--version\"); }\n",
    )
    .unwrap();
    let findings = stray_probe_sites(&scratch, "Command::new(\"rustc\").arg(\"--version\")");
    fs::remove_dir_all(&scratch).unwrap();
    assert_eq!(findings, vec!["tests/seeded_probe.rs".to_string()]);
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

#[test]
fn philosophy_agent_optimality_frame_is_complete() {
    let root = root();
    let philosophy = fs::read_to_string(root.join("docs/spec/philosophy.md"))
        .expect("docs/spec/philosophy.md missing");
    let frame_start = philosophy
        .find("## Agent-facing design criteria")
        .expect("philosophy must record the agent-facing design criteria");
    let frame = &philosophy[frame_start..];
    let frame = frame.split_once("\n## ").map_or(frame, |(frame, _)| frame);

    let quantities = [
        "Verdict fidelity",
        "Verdict latency",
        "Verdict actionability",
        "Context economy",
        "Repair determinism",
    ];
    let quantity_rows: Vec<_> = frame
        .lines()
        .filter(|line| line.starts_with("| **"))
        .collect();
    assert_eq!(quantity_rows.len(), quantities.len());
    for quantity in quantities {
        let marker = format!("| **{quantity}** |");
        assert_eq!(
            quantity_rows
                .iter()
                .filter(|row| row.starts_with(&marker))
                .count(),
            1,
            "agent-optimality quantity must have one row: {quantity}"
        );
    }

    for invariant in ["I3", "I4", "I8"] {
        assert_eq!(
            frame.matches(invariant).count(),
            1,
            "agent-optimality frame must link invariant {invariant} once"
        );
    }
    assert_eq!(frame.matches("#1880").count(), 1);
    assert!(frame.contains("architecture.md#incremental-compiler-service"));

    let invariant_ids: Vec<_> = frame
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| {
            let bytes = token.as_bytes();
            bytes.len() > 1
                && bytes[0] == b'I'
                && bytes[1..].iter().all(|byte| byte.is_ascii_digit())
        })
        .collect();
    assert_eq!(invariant_ids, vec!["I3", "I4", "I8"]);
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
    // FEATURE_CLAIM: claim.examples-spec / expected-output-pairs
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
        // Root package: test-only rustls lifecycle HTTPS e2e (D-DEP1).
        ("<root>", &["D-DEP1"]),
        ("jet-jit", &["D-JITDEP1", "D-JIT2", "D-DEP1"]),
        ("jet-net", &["D-DEP1"]),
        ("jetpack", &["D-DEP-CRYPTO1=A"]),
        // D-DX5-HOOK1: compiler-extension Wasmtime host runs only in the
        // sibling binary package; jetpack's compiler-linked library stays clean.
        ("jetpack-bin", &["D-DEP1", "D-DX5-HOOK1"]),
        // Card #367 / D-PRODUCT-SPLIT1=C: FFI.rs (the rustls test-only
        // loopback peer) moved from `jetpack` to `jet-pkg-model`.
        ("jet-pkg-model", &["D-DEP1", "D-EMAIL-DKIM-CONFIG1"]),
    ];

    let root = root();
    let decisions_doc = fs::read_to_string(root.join("docs/spec/syntax-decisions.md"))
        .expect("docs/spec/syntax-decisions.md missing");
    // Live board plus history: ratified exemptions may retire into
    // history.json while remaining law (Tower archive / #461).
    let tower_live = fs::read_to_string(root.join("plugins/tower/.tower/tower.json"))
        .expect("plugins/tower/.tower/tower.json missing");
    let tower_history = fs::read_to_string(root.join("plugins/tower/.tower/history.json"))
        .unwrap_or_default();
    let tower = format!("{tower_live}\n{tower_history}");

    for (crate_name, ids) in EXEMPTIONS {
        for id in *ids {
            assert!(
                ratified_decision_exists(&decisions_doc, &tower, id),
                "I6 exemption for `{crate_name}` cites {id}, which is not ratified in \
                 docs/spec/syntax-decisions.md or Tower (live+history) — revoke the \
                 exemption or get {id} ratified"
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
    for required in ["jet-repl", "jet-debug", "jet-cli", "jet-canvas", "jet-devserver"] {
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
        root_lib.contains("pub use jet_devserver as DevServer;")
            && root_lib.contains("pub use jet_devserver::Canvas;")
            && !root.join("Source/Canvas.rs").exists()
            && !root.join("Source/Canvas").exists(),
        "D-ARCH-SOURCE1 requires jet-devserver ownership of Canvas semantics, not root wrappers"
    );
    let root_main = fs::read_to_string(root.join("Source/main.rs")).expect("Source/main.rs missing");
    let root_compile =
        fs::read_to_string(root.join("Source/CmdCompile.rs")).expect("Source/CmdCompile.rs missing");
    let devserver =
        fs::read_to_string(root.join("crates/jet-devserver/src/lib.rs")).expect("jet-devserver missing");
    assert!(
        !root.join("Source/CmdDevWeb.rs").exists()
            && !root_main.contains("mod CmdDevWeb;")
            && root_compile.contains("pub(crate) fn run_dev_web(")
            && root_compile.contains("jet_devserver::WebHost::WebHost::bind")
            && devserver.contains("pub mod WebHost;"),
        "D-ARCH-SOURCE1 requires inward web-host ownership with only the R5 executor in CmdCompile"
    );
    assert!(
        root_lib.contains("pub use jet_canvas as CanvasUi;")
            && !root.join("Source/Canvas/html.rs").exists()
            && !root.join("Source/Canvas/js.rs").exists()
            && !root.join("Source/Canvas/js").exists(),
        "Canvas browser projection assets must live in jet-canvas"
    );
    assert!(
        root_lib.contains("pub use jet_driver::BudgetView;")
            && root_lib.contains("pub use jet_driver::FixEngine;")
            && !root.join("Source/BudgetView.rs").exists()
            && !root.join("Source/FixEngine.rs").exists(),
        "Canvas shared compiler helpers must live inward in jet-driver"
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
        for (line, context, has_path) in dependency_lines_with_context(&text) {
            if has_path {
                continue;
            }
            match exemption {
                Some((_, ids))
                    if ids
                        .iter()
                        .any(|id| exact_decision_token(&context, id)) => continue,
                Some((_, ids)) => offenders.push(format!(
                    "{name}: {} — external dep must cite one of {ids:?} in a comment \
                     directly above the dependency line",
                    line
                )),
                None => offenders.push(format!(
                    "{name}: {} — not an exempted crate; I6 forbids external \
                     dependencies in compiler seam crates (add to EXEMPTIONS with a \
                     ratified decision ID if this is intentional)", line
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

fn ratified_decision_exists(ratified_doc: &str, tower: &str, authority: &str) -> bool {
    if section_between_pub(ratified_doc)
        .lines()
        .filter_map(|line| {
            line.trim_start()
                .strip_prefix("- ")
                .unwrap_or(line.trim_start())
                .strip_prefix("**")?
                .split_once("**")
                .map(|(heading, _)| heading)
        })
        .any(|heading| exact_decision_token(heading, authority))
    {
        return true;
    }
    let (id, outcome) = authority
        .rsplit_once('=')
        .map_or((authority, None), |(id, outcome)| (id, Some(outcome)));
    let marker = format!("\"id\": \"{id}\"");
    let Some((_, tail)) = tower.split_once(&marker) else {
        return false;
    };
    let record_end = tail.find("\n      \"id\": \"").unwrap_or(tail.len());
    let record = &tail[..record_end];
    record.contains("\"status\": \"ratified\"")
        && outcome.map_or(true, |outcome| {
            record.contains(&format!("\"outcome\": \"{outcome}\""))
        })
}

fn exact_decision_token(text: &str, authority: &str) -> bool {
    text.match_indices(authority).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + authority.len()..].chars().next();
        before.is_none_or(|ch| !decision_id_char(ch))
            && after.is_none_or(|ch| !decision_id_char(ch))
    })
}

fn decision_id_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')
}

#[test]
fn ratified_decision_lookup_requires_exact_id_and_ratified_status() {
    let docs = "## Proposed\n**D-PENDING=A**: not law. See D-LIVE and D-CITED.\n\
                ## Ratified\n**D-LIVE=A**: supersedes D-CITED.\n\
                ## Declined\n**D-NOPE=A**: declined.\n";
    assert!(ratified_decision_exists(docs, "", "D-LIVE"));
    assert!(!ratified_decision_exists(docs, "", "D-LIV"));
    assert!(!ratified_decision_exists(docs, "", "D-CITED"));
    assert!(!ratified_decision_exists(docs, "", "D-PENDING=A"));
    assert!(!ratified_decision_exists(docs, "", "D-NOPE=A"));
}

/// Finds both ordinary Cargo dependency entries and valid dependency subtables.
fn dependency_lines_with_context(text: &str) -> Vec<(String, String, bool)> {
    let lines: Vec<_> = text.lines().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(header) = table_header(lines[index]) else {
            index += 1;
            continue;
        };
        let end = lines[index + 1..]
            .iter()
            .position(|line| table_header(line).is_some())
            .map_or(lines.len(), |offset| index + 1 + offset);
        match dependency_table(header) {
            None => {}
            Some(DependencyTable::Entries) => {
                let mut context = String::new();
                for line in lines[index + 1..end].iter().map(|line| line.trim()) {
                    if line.is_empty() {
                        context.clear();
                        continue;
                    }
                    if line.starts_with('#') {
                        context.push_str(line);
                        context.push('\n');
                        continue;
                    }
                    let Some((_, value)) = line.split_once('=') else {
                        continue;
                    };
                    out.push((
                        line.to_string(),
                        std::mem::take(&mut context),
                        inline_table_has_key(value.trim(), "path"),
                    ));
                }
            }
            Some(DependencyTable::Detail(name)) => {
                let fields: Vec<_> = lines[index + 1..end]
                    .iter()
                    .map(|line| line.trim())
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .collect();
                if !fields.is_empty() {
                    let first = lines[index + 1..end]
                        .iter()
                        .position(|line| {
                            let line = line.trim();
                            !line.is_empty() && !line.starts_with('#')
                        })
                        .map(|offset| index + 1 + offset)
                        .unwrap();
                    let context = preceding_comments(&lines, index);
                    out.push((
                        format!("{name}: {}", fields.join(", ")),
                        if context.is_empty() {
                            preceding_comments(&lines, first)
                        } else {
                            context
                        },
                        fields.iter().any(|field| {
                            field
                                .split_once('=')
                                .is_some_and(|(key, _)| key.trim() == "path")
                        }),
                    ));
                }
            }
        }
        index = end;
    }
    out
}

fn table_header(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('[')?;
    let close = rest.find(']')?;
    let suffix = rest[close + 1..].trim_start();
    if !suffix.is_empty() && !suffix.starts_with('#') {
        return None;
    }
    let header = rest[..close].trim();
    (!header.is_empty()).then_some(header)
}

enum DependencyTable<'a> {
    Entries,
    Detail(&'a str),
}

fn dependency_table(line: &str) -> Option<DependencyTable<'_>> {
    for base in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if line == base || (line.starts_with("target.") && line.ends_with(&format!(".{base}"))) {
            return Some(DependencyTable::Entries);
        }
        if let Some(name) = line.strip_prefix(&format!("{base}.")) {
            return (!name.is_empty()).then_some(DependencyTable::Detail(name));
        }
        let marker = format!(".{base}.");
        if line.starts_with("target.") {
            if let Some((_, name)) = line.rsplit_once(&marker) {
                return (!name.is_empty()).then_some(DependencyTable::Detail(name));
            }
        }
    }
    None
}

fn preceding_comments(lines: &[&str], index: usize) -> String {
    let mut comments: Vec<_> = lines[..index]
        .iter()
        .rev()
        .map(|line| line.trim())
        .take_while(|line| line.starts_with('#'))
        .collect();
    comments.reverse();
    comments.into_iter().map(|line| format!("{line}\n")).collect()
}

fn inline_table_has_key(value: &str, wanted: &str) -> bool {
    let Some(body) = value.strip_prefix('{') else {
        return false;
    };
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0usize;
    for (index, ch) in body.char_indices() {
        if let Some(delimiter) = quote {
            if delimiter == '"' && ch == '\\' && !escaped {
                escaped = true;
                continue;
            }
            if ch == delimiter && !escaped {
                quote = None;
            }
            escaped = false;
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '[' | '{' => depth += 1,
            '}' if depth == 0 => {
                return body[start..index]
                    .split_once('=')
                    .is_some_and(|(key, _)| key.trim() == wanted);
            }
            ']' | '}' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                if body[start..index]
                    .split_once('=')
                    .is_some_and(|(key, _)| key.trim() == wanted)
                {
                    return true;
                }
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    false
}

#[test]
fn dependency_scanner_covers_target_tables_and_one_dependency_per_comment() {
    let manifest = "[dependencies]\n\
                    # D-ONE=A\n\
                    first = \"1\"\n\
                    second = \"2\"\n\
                    [target.'cfg(unix)'.dependencies]\n\
                    # D-TWO=B\n\
                    third = \"3\"\n\
                    [target.x86_64-unknown-linux-gnu.build-dependencies]\n\
                    # D-THREE=C\n\
                    fourth = \"4\"\n\
                    # D-FOUR=D\n\
                    \t[dependencies.local]\n\
                    path = \"../local\"\n\
                    version = \"1\"\n\
                    # D-FIVE=E\n\
                    [dev-dependencies.remote]\n\
                    git = \"https://example.test/repo\"\n\
                    [target.'cfg(unix)'.dependencies.target_remote]\n\
                    version = \"2\"\n\
                    [package.metadata.dependencies]\n\
                    ignored = \"5\"\n";
    assert_eq!(
        dependency_lines_with_context(manifest),
        vec![
            ("first = \"1\"".into(), "# D-ONE=A\n".into(), false),
            ("second = \"2\"".into(), String::new(), false),
            ("third = \"3\"".into(), "# D-TWO=B\n".into(), false),
            ("fourth = \"4\"".into(), "# D-THREE=C\n".into(), false),
            (
                "local: path = \"../local\", version = \"1\"".into(),
                "# D-FOUR=D\n".into(),
                true,
            ),
            (
                "remote: git = \"https://example.test/repo\"".into(),
                "# D-FIVE=E\n".into(),
                false,
            ),
            ("target_remote: version = \"2\"".into(), String::new(), false),
        ]
    );
}

#[test]
fn dependency_scanner_does_not_confuse_values_with_path_keys() {
    let manifest = "[dependencies]\n\
                    remote = { version = \"1\", package = \"not, path = metadata\" } # external\n\
                    local = { version = \"1\", path = \"../local\" } # local\n";
    let dependencies = dependency_lines_with_context(manifest);
    assert!(!dependencies[0].2, "{dependencies:#?}");
    assert!(dependencies[1].2, "{dependencies:#?}");
}

#[test]
fn dependency_scanner_accepts_commented_headers_and_rejects_junk() {
    let manifest = "[dependencies.fake] junk\n\
                    version = \"0\"\n\
                    # D-VALID=A\n\
                      [ dependencies.remote ] # valid Cargo header\n\
                    version = \"1\"\n";
    assert_eq!(
        dependency_lines_with_context(manifest),
        vec![(
            "remote: version = \"1\"".into(),
            "# D-VALID=A\n".into(),
            false,
        )]
    );
}

// ---------------------------------------------------------------------------
// Check 9: E3 feature claims stay bound to executable proof
// ---------------------------------------------------------------------------
#[test]
fn epoch3_feature_manifest_is_current_and_owned() {
    let root = root();
    let output = Command::new("node")
        .arg("scripts/agent/check-feature-ledger.mjs")
        .arg("--check")
        .arg("--tower")
        .arg(root.join("plugins/tower/.tower/tower.json"))
        .current_dir(&root)
        .output()
        .expect("node must run the feature-ledger checker");

    assert!(
        output.status.success(),
        "feature ledger rejected:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn epoch3_feature_manifest_rejects_hostile_real_card_fixtures() {
    let root = root();
    let output = Command::new("node")
        .arg("scripts/agent/check-feature-ledger.mjs")
        .arg("--hostile-fixtures")
        .arg("--tower")
        .arg(root.join("plugins/tower/.tower/tower.json"))
        .current_dir(&root)
        .output()
        .expect("node must run the feature-claim hostile fixtures");

    assert!(
        output.status.success(),
        "feature-claim hostile fixtures failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Check 11 (I3 pin, card #447 / durability W2): codegen is dumb — it must not
// construct a user diagnostic. All checking lives in sema, and evaluator error
// values use the shared registered diagnostic seam.
// ---------------------------------------------------------------------------
#[test]
fn codegen_never_constructs_diagnostics() {
    let root = root();
    let dir = root.join("crates/jet-codegen/src");
    let mut offenders = Vec::new();
    for path in rs_files(&dir) {
        let text = fs::read_to_string(&path).unwrap_or_default();
        for line in diagnostic_constructor_lines(&text) {
            offenders.push(format!(
                "{}:{line}: {}",
                path.strip_prefix(&root).unwrap().display(),
                text.lines().nth(line - 1).unwrap_or_default().trim()
            ));
        }
    }
    assert!(
        offenders.is_empty(),
        "jet-codegen must never reference Diagnostic (I3 — codegen is dumb, \
         all checking lives in sema):\n{}",
        offenders.join("\n")
    );
}

#[test]
fn codegen_diagnostic_scanner_rejects_constructors_without_matching_prose() {
    let forbidden = "use jet_diagnostics::Diagnostic as D;\n\
                     type D = Diagnostic;\n\
                     let d = Diagnostic { code: code };\n\
                     let d = jet::Diagnostic::error(code);\n";
    assert_eq!(diagnostic_constructor_lines(forbidden), vec![3, 4]);

    let allowed = r###"// Diagnostic is forbidden in codegen.
let word = "Diagnostic";
let raw = r#"Diagnostic"#;
let DiagnosticFactory = factory;
fn typed() -> Diagnostic {}
JetParaRuntimeFailure::Diagnostic { rendered }
"###;
    assert!(diagnostic_constructor_lines(allowed).is_empty());
}

fn diagnostic_constructor_lines(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            continue;
        }
        if let Some(end) = rust_raw_string_end(bytes, i) {
            line += bytes[i..end].iter().filter(|byte| **byte == b'\n').count();
            i = end;
            continue;
        }
        let string_prefix = if bytes[i..].starts_with(b"b\"")
            || bytes[i..].starts_with(b"c\"")
        {
            Some(2)
        } else if bytes[i] == b'"' {
            Some(1)
        } else {
            None
        };
        if let Some(prefix) = string_prefix {
            i += prefix;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            continue;
        }
        if bytes[i] == b'_' || bytes[i].is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i] == b'_' || bytes[i].is_ascii_alphanumeric())
            {
                i += 1;
            }
            if &source[start..i] == "Diagnostic" {
                let mut next = i;
                while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                    next += 1;
                }
                let prefix = source[..start].trim_end();
                let line_prefix = source[..start].rsplit('\n').next().unwrap_or("").trim();
                let struct_literal = bytes[next..].starts_with(b"{")
                    && !line_prefix.is_empty()
                    && !prefix.ends_with("->")
                    && !prefix.ends_with("::");
                if bytes[next..].starts_with(b"::") || struct_literal {
                    lines.push(line);
                }
            }
            continue;
        }
        i += 1;
    }
    lines
}

fn rust_raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    if bytes.get(i) == Some(&b'b') || bytes.get(i) == Some(&b'c') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'r') {
        return None;
    }
    i += 1;
    let hashes_start = i;
    while bytes.get(i) == Some(&b'#') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    let hashes = i - hashes_start;
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'"'
            && bytes
                .get(i + 1..i + 1 + hashes)
                .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
        {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    Some(bytes.len())
}

// ---------------------------------------------------------------------------
// Check 12 (durability pin, card #452): compiler seams may splice only the
// canonical shared Prelude or a generated/host adapter source. Those are
// executable copies of one semantic source across tiers, not second
// implementations. The comptime Sync seam itself must be a module boundary;
// it must not hide a direct include in SyncLite.rs.
// ---------------------------------------------------------------------------
#[test]
fn compiler_code_has_no_include_splices() {
    let root = root();
    let sync_lite = fs::read_to_string(root.join("crates/jet-comptime/src/Comptime/SyncLite.rs"))
        .expect("SyncLite.rs must be readable");
    assert!(
        !sync_lite.lines().any(is_include_macro_line),
        "SyncLite.rs must load its shared Prelude through a module seam, not include!"
    );

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
            let text = fs::read_to_string(&path).unwrap_or_default();
            for (i, line) in text.lines().enumerate() {
                if !is_include_macro_line(line) || canonical_shared_include(&rel, line) {
                    continue;
                }
                offenders.push(format!("{}:{}: {}", rel, i + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "compiler code must not splice an unowned sibling .rs fragment — use the canonical \
         Prelude or a real module instead (card #452 durability pattern):\n{}",
        offenders.join("\n")
    );
}

fn is_include_macro_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("include!(") || trimmed.starts_with("include! (")
}

fn canonical_shared_include(relative: &str, line: &str) -> bool {
    let prelude_path = [
        "include!(\"Prelude/",
        "include! (\"Prelude/",
        "include!(\"../Prelude/",
        "include! (\"../Prelude/",
        "include!(\"../../../Prelude/",
        "include! (\"../../../Prelude/",
        "include!(\"../../jet-codegen/src/Prelude/",
        "include! (\"../../jet-codegen/src/Prelude/",
        "include!(\"../../../jet-codegen/src/Prelude/",
        "include! (\"../../../jet-codegen/src/Prelude/",
        "include!(\"../../../../jet-codegen/src/Prelude/",
        "include! (\"../../../../jet-codegen/src/Prelude/",
        "include!(\"../../jet-pkg-model/src/Prelude/",
        "include! (\"../../jet-pkg-model/src/Prelude/",
        "include!(\"../../jet-foundation/src/",
        "include! (\"../../jet-foundation/src/",
    ];
    prelude_path.iter().any(|prefix| line.contains(prefix))
        || line.contains("include!(concat!(env!(\"OUT_DIR\")")
        || line.contains("include! (concat!(env!(\"OUT_DIR\")")
        || (relative.starts_with("crates/jet-codegen/src/Prelude/")
            && (line.trim_start().starts_with("include!(")
                || line.trim_start().starts_with("include! (")))
        || (relative == "crates/jet-codegen/src/lib.rs"
            && line.contains("include!(\"SchedulerHost.rs\")"))
        || (relative == "crates/jet-jit/src/net_http_rt.rs"
            && line.contains("include!(\"net_http_hosts.rs\")"))
        || (relative == "crates/jet-foundation/src/CoreArchive.rs"
            && line.contains("include!(\"../../../corelib/core.archive/pkgs/archive/src/lib.rs\")"))
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
// Check 11: the vocabulary page is one definition home and its doc lint is live
// ---------------------------------------------------------------------------
#[test]
fn vocabulary_page_has_one_definition_and_no_retired_senses() {
    let root = root();
    let vocabulary_path = root.join("docs/spec/vocabulary.md");
    let vocabulary = fs::read_to_string(&vocabulary_path).expect("vocabulary page is readable");
    for heading in ["Stream", "Reader", "Event", "Collecting loop"] {
        assert_eq!(
            vocabulary.matches(&format!("## {heading}\n\nDefinition:")).count(),
            1,
            "vocabulary page must define `{heading}` exactly once"
        );
    }
    assert_eq!(
        vocabulary.matches("Definition:").count(),
        4,
        "vocabulary page must have one definition for each vocabulary word"
    );

    let row = jet_foundation::Registry::row("JetVocabulary")
        .expect("vocabulary truth must be registered in the corpus table");
    assert_eq!(row.home, Some("docs/spec/vocabulary.md"));
    assert_eq!(
        row.guard.map(|guard| guard.test),
        Some("vocabulary_page_has_one_definition_and_no_retired_senses")
    );

    let hostile = [
        (
            "codec mode called a stream",
            "The codec mode is called a stream.",
        ),
        ("event called a stream", "The event is called a stream."),
        (
            "collecting loop called yielding",
            "The collecting loop is called yielding.",
        ),
    ];
    for (label, fixture) in hostile {
        assert!(
            retired_vocabulary_senses(fixture).contains(&label),
            "hostile fixture must be rejected: {label}"
        );
    }

    let docs_root = root.join("docs");
    let mut markdown = Vec::new();
    collect_markdown_paths(&docs_root, &mut markdown);
    let vocabulary_path = fs::canonicalize(vocabulary_path).expect("vocabulary path canonical");
    let docs_root = fs::canonicalize(docs_root).expect("docs path canonical");
    let mut targets = HashSet::new();
    for source in markdown {
        let Ok(text) = fs::read_to_string(&source) else { continue };
        for target in markdown_link_targets(&text) {
            let target = target.trim_matches('<').trim_matches('>');
            let target = target.split(['#', '?']).next().unwrap_or_default();
            if target.is_empty() || target.starts_with("/") || target.contains("://") {
                continue;
            }
            let candidate = source.parent().unwrap_or(Path::new(".")).join(target);
            let Ok(candidate) = fs::canonicalize(candidate) else { continue };
            if !candidate.starts_with(&docs_root)
                || candidate.extension().and_then(|ext| ext.to_str()) != Some("md")
            {
                continue;
            }
            targets.insert(candidate);
        }
    }

    let mut violations = Vec::new();
    for target in targets {
        let Ok(text) = fs::read_to_string(&target) else { continue };
        if target != vocabulary_path {
            let retired = retired_vocabulary_senses(&text);
            if !retired.is_empty() {
                violations.push(format!(
                    "{}: retired senses: {}",
                    target.strip_prefix(&root).unwrap_or(&target).display(),
                    retired.join(", ")
                ));
            }
        }
        if target != vocabulary_path
            && ["stream", "reader", "event", "collecting loop"]
                .into_iter()
                .any(|word| contains_word(&text, word))
            && !text.contains("vocabulary.md")
        {
            violations.push(format!(
                "{}: uses Jet vocabulary without linking docs/spec/vocabulary.md",
                target.strip_prefix(&root).unwrap_or(&target).display()
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "linked Markdown targets must use the vocabulary page and current senses:\n{}",
        violations.join("\n")
    );
}

#[test]
fn concurrency_spec_states_deadlock_stance() {
    let root = root();
    let spec = fs::read_to_string(root.join("docs/spec/spec.md"))
        .expect("language spec is readable");
    let architecture = fs::read_to_string(root.join("docs/spec/architecture.md"))
        .expect("architecture spec is readable");
    let heading = "### Deadlock stance";

    assert_eq!(
        spec.matches(heading).count(),
        1,
        "spec must have one Deadlock section"
    );
    let stance = spec
        .split_once(heading)
        .and_then(|(_, rest)| rest.split_once("\n## ").map(|(section, _)| section))
        .expect("Deadlock section must have a body before the next top-level section");

    assert_eq!(
        stance.matches("**Guarantee.**").count(),
        1,
        "Deadlock section must have one guarantee"
    );
    assert_eq!(
        stance.matches("**Non-guarantee.**").count(),
        1,
        "Deadlock section must have one non-guarantee"
    );
    assert!(stance.contains("Jet guarantees deadlock-free lock acquisition"));
    assert!(stance.contains("Jet does not guarantee deadlock freedom"));
    assert!(stance.contains("does not detect arbitrary deadlocks at runtime"));

    for link in [
        "[task and scheduler rules](#e2-m1--concurrency-tasks-and-channels-verified-2026-08-06)",
        "[channel buffering](#bounded-buffering-law)",
        "[concurrency boundary safety](architecture.md#concurrency-boundary-safety-status)",
    ] {
        assert!(stance.contains(link), "Deadlock section must link `{link}`");
    }

    assert_eq!(
        architecture.matches("[Deadlock stance](spec.md#deadlock-stance)").count(),
        1,
        "architecture must link the canonical Deadlock section"
    );
    for mechanism in ["M:N scheduler", "`task`", "`task.group`", "`tasks.channel`"] {
        assert!(
            architecture.contains(mechanism),
            "architecture must name concurrency mechanism `{mechanism}`"
        );
    }
}

fn collect_markdown_paths(dir: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_paths(&path, paths);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            paths.push(path);
        }
    }
}

fn markdown_link_targets(text: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else { break };
        targets.push(after[..end].split_whitespace().next().unwrap_or_default());
        rest = &after[end + 1..];
    }
    targets
}

fn contains_word(text: &str, needle: &str) -> bool {
    let text = text.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(found) = text[offset..].find(&needle) {
        let start = offset + found;
        let end = start + needle.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        if !before.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            && !after.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return true;
        }
        offset = end;
    }
    false
}

fn retired_vocabulary_senses(text: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.to_ascii_lowercase();
        let called = line.contains("called") || line.contains("as a");
        if called && line.contains("codec") && contains_word(&line, "stream") {
            found.push("codec mode called a stream");
        }
        if called && line.contains("event") && contains_word(&line, "stream") {
            found.push("event called a stream");
        }
        if called && line.contains("collecting loop") && line.contains("yielding") {
            found.push("collecting loop called yielding");
        }
    }
    found.sort_unstable();
    found.dedup();
    found
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
