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
        ("jet-net", &["D-DEP1"]),
        ("jetpack", &["D-DEP-CRYPTO1=A"]),
        // Card #367 / D-PRODUCT-SPLIT1=C: FFI.rs (the rustls test-only
        // loopback peer) moved from `jetpack` to `jet-pkg-model`.
        ("jet-pkg-model", &["D-DEP1", "D-EMAIL-DKIM-CONFIG1"]),
    ];

    let root = root();
    let decisions_doc = fs::read_to_string(root.join("docs/spec/syntax-decisions.md"))
        .expect("docs/spec/syntax-decisions.md missing");
    let tower = fs::read_to_string(root.join(".tower/tower.json"))
        .expect(".tower/tower.json missing");

    for (crate_name, ids) in EXEMPTIONS {
        for id in *ids {
            assert!(
                ratified_decision_exists(&decisions_doc, &tower, id),
                "I6 exemption for `{crate_name}` cites {id}, which is not ratified in \
                 docs/spec/syntax-decisions.md or Tower — revoke the exemption or get \
                 {id} ratified"
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
    line.trim().strip_prefix('[')?.strip_suffix(']')
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
// references to the `Diagnostic` type in jet-codegen. All checking lives in
// sema; codegen must never import, alias, or construct diagnostics.
// ---------------------------------------------------------------------------
#[test]
fn codegen_never_constructs_diagnostics() {
    let root = root();
    let dir = root.join("crates/jet-codegen/src");
    let mut offenders = Vec::new();
    for path in rs_files(&dir) {
        let text = fs::read_to_string(&path).unwrap_or_default();
        for line in diagnostic_identifier_lines(&text) {
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
fn codegen_diagnostic_scanner_rejects_references_without_matching_prose() {
    let forbidden = "use jet_diagnostics::Diagnostic as D;\n\
                     type D = Diagnostic;\n\
                     let d = Diagnostic { code: code };\n\
                     let d = jet::Diagnostic::error(code);\n";
    assert_eq!(diagnostic_identifier_lines(forbidden), vec![1, 2, 3, 4]);

    let allowed = r###"// Diagnostic is forbidden in codegen.
let word = "Diagnostic";
let raw = r#"Diagnostic"#;
let DiagnosticFactory = factory;
"###;
    assert!(diagnostic_identifier_lines(allowed).is_empty());
}

fn diagnostic_identifier_lines(source: &str) -> Vec<usize> {
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
                lines.push(line);
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
                // Durability guards that *mention* include!( in a string are fine.
                if line.contains("contains(\"include!(") || line.contains("contains(\"include!\"") {
                    continue;
                }
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
