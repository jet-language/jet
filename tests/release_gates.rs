//! Milestone gate checks, one lane: canon.jet golden run, `--small` binary
//! size gates, the compiled-workload contract, the Epoch 2 GA checklist, and
//! release/edition/deprecation policy. Distinct from `tests/golden.rs`
//! (per-example front-end + rustc matrix) — these are one-off milestone
//! exit-criteria checks, not part of the example discovery loop.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::have_rustc;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

// ============================================================================
// Section: canon.jet golden run (was tests/canon.rs)
// ============================================================================

#[test]
fn canon_compiles_and_runs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let jet = jet_bin();
    assert!(jet.exists(), "build the jet binary first (cargo build)");

    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping canon golden run");
        return;
    }

    let tool = root.join("examples/canon.jet");
    let out = Command::new(&jet)
        .arg("run")
        .arg(&tool)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "canon.jet failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected = fs::read_to_string(root.join("tests/fixtures/canon/expected.out"))
        .expect("tests/fixtures/canon/expected.out");
    let actual = String::from_utf8_lossy(&out.stdout);
    assert_eq!(actual, expected);
}

// ============================================================================
// Section: `--small` binary size (was tests/small.rs) — M6 phase 4 / S15
// ============================================================================

#[test]
fn small_profile_binary_is_smaller_than_default() {
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping --small size test (need jet + rustc)");
        return;
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/features/collections/wordcount.jet");
    assert!(
        example.is_file(),
        "examples/features/collections/wordcount.jet must exist"
    );

    let dir = std::env::temp_dir().join(format!("jet_small_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();

    let build_default = Command::new(&jet)
        .args(["build", example.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build_default.status.success(),
        "default build failed:\n{}",
        String::from_utf8_lossy(&build_default.stderr)
    );
    fs::rename(
        dir.join("build/wordcount"),
        dir.join("build/wordcount_default"),
    )
    .unwrap();

    let build_small = Command::new(&jet)
        .args(["build", "--small", example.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build_small.status.success(),
        "--small build failed:\n{}",
        String::from_utf8_lossy(&build_small.stderr)
    );
    fs::rename(
        dir.join("build/wordcount"),
        dir.join("build/wordcount_small"),
    )
    .unwrap();

    let default_size = fs::metadata(dir.join("build/wordcount_default"))
        .unwrap()
        .len();
    let small_size = fs::metadata(dir.join("build/wordcount_small"))
        .unwrap()
        .len();

    assert!(
        small_size < default_size,
        "--small binary ({small_size} bytes) should be smaller than default ({default_size} bytes)"
    );

    let _ = fs::remove_dir_all(&dir);
}

// ============================================================================
// Section: Epoch 2 GA checklist (was tests/ga.rs) — E2-M17
//
// Asserts that Epoch 2 exit criteria still hold at the compiler level.
// Showcase programs were retired from `examples/`; milestone coverage now
// lives in `examples/features/` (I5 golden tests).
// ============================================================================

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ── 1. Every diagnostic code has a jet explain entry ──────────────────────

/// Mirrors the check in cli.rs `every_registered_code_has_an_explain_entry`.
#[test]
fn ga_every_diagnostic_has_explain() {
    let md = fs::read_to_string(root().join("docs/spec/diagnostics.md")).expect("diagnostics.md");
    let index = jet::Explain::index();

    let mut missing = Vec::new();
    for line in md.lines() {
        let line = line.trim();
        if !line.starts_with("| E") && !line.starts_with("| L") {
            continue;
        }
        let first = line
            .trim_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .trim();
        if is_code(first) && !index.contains_key(first) {
            missing.push(first.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "M17 GA gate: these diagnostic codes lack a `jet explain` entry:\n  {}",
        missing.join(", ")
    );
}

fn is_code(s: &str) -> bool {
    jet::Explain::is_code(s)
}

// ── 2. Milestone feature examples are front-end clean ─────────────────────

/// D-GA1=B milestone coverage now lives under `examples/features/`.
#[test]
fn ga_milestone_features_front_end_clean() {
    let features: &[(&str, &str)] = &[
        ("modules/library.jet", "library authoring"),
        ("lowlevel/lowlevel.jet", "expert low-level tier"),
        ("net/http_server_tasks.jet", "HTTP service"),
        ("lowlevel/freestanding.jet", "freestanding smoke"),
    ];

    let features_dir = root().join("examples/features");
    for (file, desc) in features {
        let path = features_dir.join(file);
        assert!(
            path.is_file(),
            "M17 GA gate: feature example missing: {}",
            path.display()
        );
        let src =
            fs::read_to_string(&path).unwrap_or_else(|_| panic!("cannot read {}", path.display()));
        let result = jet::compile_with_path(&src, path.to_str().unwrap());
        assert!(
            result.is_ok(),
            "M17 GA gate: '{}' failed front end:\n{:?}",
            desc,
            result.err()
        );
    }
}

// ── 3. Hard size budgets (D-GA2=B) ────────────────────────────────────────

#[test]
fn ga_feature_size_budgets() {
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping GA size budgets (need jet + rustc)");
        return;
    }

    let budgets: &[(&str, u64)] = &[
        ("basics/hello.jet", 512_000),
        ("io/cli.jet", 512 * 1024),
        ("net/http_server_tasks.jet", 3 * 1024 * 1024),
        ("modules/library.jet", 4_194_304),
        ("lowlevel/lowlevel.jet", 4_194_304),
        ("lowlevel/freestanding.jet", 4_194_304),
    ];

    let features_dir = root().join("examples/features");
    let build_dir = std::env::temp_dir().join(format!("jet_ga_budgets_{}", std::process::id()));
    fs::create_dir_all(build_dir.join("build")).unwrap();

    for (file, max_bytes) in budgets {
        let src = features_dir.join(file);
        let stem = std::path::Path::new(file)
            .file_stem()
            .unwrap()
            .to_string_lossy();
        let bin = build_dir.join("build").join(stem.as_ref());

        let out = Command::new(&jet)
            .args(["build", "--small", src.to_str().unwrap()])
            .current_dir(&build_dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "GA size gate: `--small` build of {} failed:\n{}",
            file,
            String::from_utf8_lossy(&out.stderr)
        );

        let size = fs::metadata(&bin).map(|m| m.len()).unwrap_or(0);
        assert!(
            size <= *max_bytes && size > 0,
            "GA size gate: {} --small binary is {} bytes (limit {})",
            file,
            size,
            max_bytes
        );
    }

    let _ = fs::remove_dir_all(&build_dir);
}

// ============================================================================
// Section: release policy, editions, epoch contract (was tests/release.rs) — E2-M2
//
// Golden tests for the version banner (E2-D1), the E2001 edition-too-new
// diagnostic (D-REL3), and the E2002/L2001 deprecation diagnostics (D-REL5),
// plus a docs-consistency check that every later breaking epoch-2 milestone
// names the edition/epoch gate it needs (the m2 exit criteria).
//
// Fixtures live in tests/release/*.txt. To re-bless after an INTENTIONAL change
// (read it against docs/spec/diagnostics.md and docs/spec/release-policy.md
// first):
//
//     UPDATE_EXPECT=1 cargo test --test release_gates
//
// E2002 and L2001 are exercised by the encoding edition/UI tests. This gate
// also pins both Core aliases through the one marker payload.
// ============================================================================

use jet::Manifest::{self};

fn release_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/release")
}

/// Compare `actual` against the fixture, or re-bless it under UPDATE_EXPECT.
fn check_fixture(name: &str, actual: &str) {
    let path = release_dir().join(name);
    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::create_dir_all(release_dir()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        actual, expected,
        "\nrelease fixture mismatch for tests/release/{name}\n(if the new output is intentional and matches the spec, run: UPDATE_EXPECT=1 cargo test)\n",
    );
}

#[test]
fn version_banner() {
    let jet = jet_bin();
    assert!(jet.exists(), "build the jet binary first (cargo build)");
    let out = Command::new(&jet).arg("--version").output().unwrap();
    assert!(out.status.success(), "jet --version exited non-zero");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    // The library banner and the CLI must agree.
    assert_eq!(stdout, Manifest::version_banner());
    check_fixture("version_banner.txt", &stdout);
}

#[test]
fn edition_too_new() {
    // A real package.jet asking for a future edition triggers E2001 through the
    // manifest loader path. We render the diagnostic the way the CLI would.
    let raw = "name: \"wordstats\"\nversion: \"0.1.0\"\nedition: \"2099\"\n";
    let path = std::path::Path::new("package.jet");
    let mf = Manifest::parse(path, raw).expect("manifest should parse");
    let err = Manifest::check_edition_support(&mf, "package.jet")
        .expect_err("a future edition must be rejected");
    assert_eq!(err.code, "E2001");
    let rendered = jet::render_diagnostics("package.jet", raw, std::slice::from_ref(&err));
    check_fixture("edition_too_new.txt", &rendered);
}

#[test]
fn supported_edition_is_accepted() {
    let raw = format!(
        "name: \"x\"\nversion: \"0.1.0\"\nedition: \"{}\"\n",
        Manifest::latest_edition()
    );
    let mf = Manifest::parse(std::path::Path::new("package.jet"), &raw).unwrap();
    assert!(Manifest::check_edition_support(&mf, "package.jet").is_ok());
}

#[test]
fn no_edition_field_is_accepted() {
    // A manifest with no edition tracks the toolchain's newest stable edition.
    let raw = "name: \"x\"\nversion: \"0.1.0\"\n";
    let mf = Manifest::parse(std::path::Path::new("package.jet"), raw).unwrap();
    assert_eq!(mf.package.edition, None);
    assert!(Manifest::check_edition_support(&mf, "package.jet").is_ok());
}

#[test]
fn core_encoding_migrations_use_one_marker_payload() {
    let encode = jet::Syntax::core_marker_application("core.encoding.cbor", "encode")
        .expect("cbor.encode deprecation");
    assert_eq!(encode.since, "2027");
    assert_eq!(encode.replacement, "cbor.to_bytes");
    assert_eq!(encode.removed_in, Some("2028"));
    let decode = jet::Syntax::core_marker_application("core.encoding.cbor", "decode")
        .expect("cbor.decode deprecation");
    assert_eq!(decode.since, "2027");
    assert_eq!(decode.replacement, "cbor.parse");
    assert_eq!(decode.removed_in, Some("2028"));
}

#[test]
fn cbor_deprecation_release_fixture() {
    let root = std::env::temp_dir().join(format!("jet_release_deprecation_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let source = "use core.encoding.cbor as cbor\nuse core.encoding.json as json\n\nfn run() {\n    tree := json.parse(\"{{}}\") ?? panic(\"json\")\n    payload := cbor.encode(tree)\n    print(\"ok\")\n}\n";
    let mut rendered = String::new();
    for edition in ["2027", "2028"] {
        fs::write(
            root.join("package.jet"),
            format!("name: \"cbor_release\"\nversion: \"0.1.0\"\nedition: \"{edition}\"\n"),
        )
        .unwrap();
        let path = root.join("run.jet");
        fs::write(&path, source).unwrap();
        let diagnostics = jet::check_with_path(path.to_str().unwrap());
        rendered.push_str(&format!("edition {edition}\n"));
        rendered.push_str(&jet::render_diagnostics(
            &format!("tests/release/cbor_{edition}.jet"),
            source,
            &diagnostics,
        ));
    }
    let _ = fs::remove_dir_all(&root);
    check_fixture("deprecation.txt", &rendered);
}

#[test]
fn later_breaking_milestones_name_their_gate() {
    // m2 exit criterion: every later breaking epoch-2 milestone names the
    // edition/epoch gate it needs. We scan the epoch-2 plan folder: any plan
    // that calls itself "breaking"/"public-breaking" must also mention an
    // edition or epoch gate. m2 itself defines the gate, so it is exempt.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/plans/epoch-2");
    // Epoch 2 is wrapped (2026-06-19): the per-milestone plan folder was removed,
    // its decisions recorded in syntax-decisions.md and highlights in roadmap.md.
    // The m2 gate criterion was met at GA; with no plans left to scan this check
    // is a no-op. If the folder ever returns, the scan resumes.
    if !dir.exists() {
        return;
    }
    let mut checked = 0;
    for entry in fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("m2-") {
            continue; // m2 defines the gate.
        }
        let text = fs::read_to_string(&path).unwrap().to_lowercase();
        let claims_breaking = text.contains("breaking");
        if claims_breaking {
            assert!(
                text.contains("edition") || text.contains("epoch"),
                "{name} describes breaking changes but names no edition/epoch gate (m2 exit criterion)",
            );
        }
        checked += 1;
    }
    assert!(
        checked >= 1,
        "expected at least one non-m2 epoch-2 plan to scan"
    );
}

// ============================================================================
// Section: #1414 compiled workload contract and report gate
// ============================================================================

#[test]
fn compiled_workload_release_gate_uses_frozen_contract_and_canaries() {
    let root = root();
    let gate = fs::read_to_string(root.join("tools/ci/compiled-workload-gate.sh"))
        .expect("read compiled workload gate");
    for field in [
        "--contract",
        "--check",
        "outcomes.tsv",
        "measurements.tsv",
        "tiers.tsv",
        "jet_tool_version",
        "peer_tool_version",
        "review_status",
        "review_evidence",
        "loss_owner",
        "applicable_targets",
        "peer_applies",
        "not-applicable",
        "tower.mjs",
        "card show \"$owner\" --json",
        "fresh review evidence",
        "candidate=",
        "reviewer=",
        "fairness=",
        "measurements=",
    ] {
        assert!(gate.contains(field), "compiled workload gate lost {field}");
    }

    let self_check = fs::read_to_string(root.join("tools/ci/test-compiled-workload-gate.sh"))
        .expect("read compiled workload gate self-check");
    let canaries = fs::read_to_string(root.join("tests/compiled_workloads/canaries.tsv"))
        .expect("read compiled workload canaries");
    for line in canaries.lines().skip(1).filter(|line| !line.is_empty()) {
        let name = line.split('\t').nth(1).unwrap();
        assert!(
            self_check.contains(&format!("expect_reject {name}")),
            "removal canary is not exercised: {name}"
        );
    }

    let suites = fs::read_to_string(root.join("tests/suites.txt")).expect("read test suites");
    assert!(
        suites.lines().any(|line| line.trim() == "tests/compiled_workloads"),
        "compiled workload self-check must stay in the named test suites"
    );

    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read CI workflow");
    assert!(
        workflow.contains("name: Compiled workload gate (Unix)")
            && workflow.contains("name: Compiled workload gate (Windows)")
            && workflow.contains("tools/ci/compiled-workload-gate.sh --contract")
            && workflow.contains("tools/ci/test-compiled-workload-gate.sh"),
        "CI must invoke the compiled workload contract and seven-canary self-check"
    );

    let output = Command::new("bash")
        .arg(root.join("tools/ci/compiled-workload-gate.sh"))
        .arg("--contract")
        .current_dir(&root)
        .output()
        .expect("run compiled workload contract gate");
    assert!(
        output.status.success(),
        "compiled workload contract gate failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ============================================================================
// Section: #211 CI test-target inventory + sharding (D-CI1=A)
// ============================================================================

/// The exact `cargo metadata` inventory of test-bearing targets, as
/// `-p PKG --lib` / `-p PKG --bin NAME` / `-p PKG --test NAME` lines — the
/// same shape `tools/ci/test-shards.sh` emits. Computed independently here
/// (a separate `jq` invocation, not a call into the script) so a bug in the
/// script's own partition math still shows up as a mismatch below.
fn full_workspace_test_target_inventory(root: &std::path::Path) -> Vec<String> {
    let metadata = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .expect("run cargo metadata");
    assert!(
        metadata.status.success(),
        "cargo metadata failed:\n{}",
        String::from_utf8_lossy(&metadata.stderr)
    );

    let jq_filter = r#".packages[] | .name as $pkg | .targets[]
      | select(.kind[0] == "lib" or .kind[0] == "bin" or .kind[0] == "test")
      | if .kind[0] == "lib" then "-p \($pkg) --lib"
        elif .kind[0] == "bin" then "-p \($pkg) --bin \(.name)"
        else "-p \($pkg) --test \(.name)" end"#;
    let mut child = Command::new("jq")
        .args(["-r", jq_filter])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn jq");
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(&metadata.stdout)
            .expect("pipe cargo metadata into jq");
    }
    let jq_result = child.wait_with_output().expect("jq output");
    assert!(
        jq_result.status.success(),
        "jq filter failed:\n{}",
        String::from_utf8_lossy(&jq_result.stderr)
    );
    String::from_utf8(jq_result.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn ci_shard_matrix_matches_test_shards_script_count() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    assert!(
        workflow.contains("verify-tests:"),
        "CI must name the #211 sharded verify-tests job"
    );
    let shard_list = workflow
        .split("shard: [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap_or_else(|| panic!("verify-tests job must declare a `shard: [...]` matrix"));
    let declared_count = shard_list
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .count();
    assert!(
        declared_count >= 1,
        "verify-tests shard matrix must not be empty"
    );
    assert!(
        workflow.contains(&format!(r#"JET_TEST_SHARD_COUNT: "{declared_count}""#)),
        "JET_TEST_SHARD_COUNT must equal the matrix's shard count ({declared_count})"
    );

    let out = Command::new("bash")
        .arg(root.join("tools/ci/test-shards.sh"))
        .arg("0")
        .arg(declared_count.to_string())
        .current_dir(&root)
        .output()
        .expect("run tools/ci/test-shards.sh");
    assert!(
        out.status.success(),
        "tools/ci/test-shards.sh must accept the workflow's own shard count:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn ci_test_shards_cover_every_workspace_target_exactly_once() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shard_count = 6;

    let want = full_workspace_test_target_inventory(&root);
    assert!(
        want.len() > 200,
        "sanity: expected well over 200 lib/bin/test targets across the workspace, got {}",
        want.len()
    );

    let mut got: Vec<String> = Vec::new();
    for shard in 0..shard_count {
        let out = Command::new("bash")
            .arg(root.join("tools/ci/test-shards.sh"))
            .arg(shard.to_string())
            .arg(shard_count.to_string())
            .current_dir(&root)
            .output()
            .expect("run tools/ci/test-shards.sh");
        assert!(
            out.status.success(),
            "shard {shard} enumeration failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        got.extend(
            String::from_utf8(out.stdout)
                .unwrap()
                .lines()
                .map(str::to_string),
        );
    }

    assert_eq!(
        got.len(),
        got.iter().collect::<std::collections::BTreeSet<_>>().len(),
        "a test target appeared in more than one shard"
    );

    let mut want_sorted = want.clone();
    want_sorted.sort();
    let mut got_sorted = got.clone();
    got_sorted.sort();
    assert_eq!(
        want_sorted, got_sorted,
        "the union of all CI test shards must equal the exact workspace test-target inventory \
         (nothing silently skipped, nothing duplicated)"
    );
    assert_eq!(
        got.iter()
            .filter(|target| target.as_str() == "-p jet --test grammar")
            .count(),
        1,
        "CI grammar drift tests must remain on exactly one production shard"
    );
}

/// #2075: the shard split is weighted by measured cost, and the weight table is
/// machine-checked.
///
/// A table nobody reads is a second place to remember cost (AGENTS.md I8), and a
/// row naming a target that no longer exists is a weight that silently stopped
/// applying — which is exactly how a 45-minute target stayed the critical path
/// while six jobs looked balanced. So every row must parse, name a real target,
/// and appear once; and the script must report the load it computed.
#[test]
fn ci_test_shards_use_the_committed_weight_table() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let table = fs::read_to_string(root.join("tools/ci/test-weights.tsv"))
        .expect("read tools/ci/test-weights.tsv");
    let inventory: std::collections::BTreeSet<String> = full_workspace_test_target_inventory(&root)
        .into_iter()
        .collect();

    let mut weighed = std::collections::BTreeSet::new();
    for (index, line) in table.lines().enumerate() {
        let row = index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            fields.len(),
            4,
            "tools/ci/test-weights.tsv:{row} must be \
             '<package>\\t<kind>\\t<target>\\t<seconds>': {line:?}"
        );
        let (package, kind, name, seconds) = (fields[0], fields[1], fields[2], fields[3]);
        assert!(
            seconds.parse::<u64>().is_ok(),
            "tools/ci/test-weights.tsv:{row} seconds must be a whole number: {seconds:?}"
        );
        let target = match kind {
            "lib" => format!("-p {package} --lib"),
            "bin" => format!("-p {package} --bin {name}"),
            "test" => format!("-p {package} --test {name}"),
            other => panic!("tools/ci/test-weights.tsv:{row} has unknown target kind `{other}`"),
        };
        assert!(
            inventory.contains(&target),
            "tools/ci/test-weights.tsv:{row} weighs `{target}`, which is not a workspace test \
             target — renamed or deleted, so its weight silently stopped applying"
        );
        assert!(
            weighed.insert(target.clone()),
            "tools/ci/test-weights.tsv:{row} repeats `{target}`"
        );
    }
    assert!(
        !weighed.is_empty(),
        "the weight table must carry at least one measured row, or the weighted split is \
         round-robin wearing a hat"
    );

    // The script must actually consult it, and say what it computed: the shard
    // load and the spread are the numbers that show whether the split is honest.
    let out = Command::new("bash")
        .arg(root.join("tools/ci/test-shards.sh"))
        .args(["0", "6"])
        .current_dir(&root)
        .output()
        .expect("run tools/ci/test-shards.sh");
    assert!(
        out.status.success(),
        "weighted shard enumeration failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let summary = String::from_utf8_lossy(&out.stderr);
    assert!(
        summary.contains("predicted ") && summary.contains("spread "),
        "tools/ci/test-shards.sh must report the load and spread it computed: {summary}"
    );
}

#[test]
fn verify_full_default_run_covers_whole_workspace() {
    // The unsharded default path (no JET_TEST_SHARD set) is what local/manual
    // `scripts/agent/verify-full.sh` runs use; it must cover the same
    // complete inventory as the sharded CI path, via plain `cargo test
    // --workspace` rather than the old default-members-only `cargo test`.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let verify = fs::read_to_string(root.join("scripts/agent/verify-full.sh"))
        .expect("read scripts/agent/verify-full.sh");
    assert!(
        verify.contains("cargo test --workspace \"$@\""),
        "the unsharded default path must run `cargo test --workspace`, not the \
         default-members-only `cargo test`"
    );
    assert!(
        verify.contains("tools/ci/test-shards.sh"),
        "the sharded path must delegate enumeration to tools/ci/test-shards.sh"
    );
}

#[test]
fn ci_runs_repository_no_nix_dogfood_gate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let verify = fs::read_to_string(root.join("scripts/agent/verify-full.sh"))
        .expect("read scripts/agent/verify-full.sh");
    assert!(
        verify.contains("node \"$repo/scripts/agent/verify-jet-shell-parity.js\""),
        "verify-full must compare the declared shell manifest with the Nix oracle"
    );
    assert!(
        verify.contains("cargo test --test jetpack_dogfood")
            && verify
                .contains("jet_repository_env_cold_and_offline_without_nix_host_store_or_fixtures")
            && verify.contains("-- --exact --nocapture"),
        "verify-full must run the exact repository no-Nix dogfood test"
    );

    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    assert!(workflow.contains("push:"), "CI must run on pushes");
    assert!(
        workflow.contains("pull_request:"),
        "CI must run on pull requests"
    );
    assert!(
        workflow.contains("scripts/agent/verify-full.sh"),
        "CI must invoke verify-full, which owns the repository no-Nix dogfood gate"
    );
}

fn workflow_job(workflow: &str, name: &str) -> String {
    let marker = format!("  {name}:");
    let mut in_job = false;
    let mut job = String::new();
    for line in workflow.lines() {
        if line == marker {
            in_job = true;
        } else if in_job && line.starts_with("  ") && !line.starts_with("    ") {
            break;
        }
        if in_job {
            job.push_str(line);
            job.push('\n');
        }
    }
    job
}

fn job_has_run(job: &str, command: &str) -> bool {
    let expected = format!("        run: {command}");
    job.lines().any(|line| line == expected.as_str())
}

const GRAMMAR_GATE_COMMAND: &str =
    "nix develop .#full -c cargo test --test grammar --locked -- --nocapture";

fn change_gate_is_accepted(job: &str) -> bool {
    job.contains("ref: ${{ github.sha }}")
        && job_has_run(job, GRAMMAR_GATE_COMMAND)
        && job.contains("RUSTDOCFLAGS: \"-D warnings\"")
        && job.contains("cargo doc --workspace --no-deps --locked")
        && !job.contains("if:")
        && !job.contains("continue-on-error")
        && !job.contains("|| true")
        && !job.contains("|| :")
}

fn nightly_slice_is_accepted(workflow: &str) -> bool {
    let change_gate = workflow_job(workflow, "change-gate");
    !change_gate.is_empty()
        && change_gate_is_accepted(&change_gate)
        && workflow.contains("nightly-fuzz:")
        && workflow.contains("FUZZ_VARIANTS: \"1000\"")
        && workflow.contains("tools/perf/ci-perf-check.sh")
}

/// #806 criterion 6: the nightly slice owns a targeted proof that the real
/// workflow cannot bypass the direct `tests/grammar.rs` gate.
#[test]
fn ci_nightly_slice_rejects_grammar_gate_bypass() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    let job = workflow_job(&workflow, "change-gate");
    assert!(!job.is_empty(), "CI must define the change-gate job");

    assert!(
        job.contains("ref: ${{ github.sha }}"),
        "change-gate must check out the event's exact candidate revision"
    );
    assert!(
        job_has_run(&job, GRAMMAR_GATE_COMMAND),
        "change-gate must invoke tests/grammar.rs directly; the broad test inventory is not enough"
    );
    assert!(nightly_slice_is_accepted(&workflow));
    let bypassed = job.replacen(
        GRAMMAR_GATE_COMMAND,
        "nix develop .#full -c cargo test --workspace --locked -- --nocapture",
        1,
    );
    assert!(
        !nightly_slice_is_accepted(&workflow.replacen(
            &job,
            &bypassed,
            1,
        )),
        "the change-gate proof must fail when its direct tests/grammar.rs invocation is bypassed"
    );
    assert!(
        job.contains("RUSTDOCFLAGS: \"-D warnings\"")
            && job.contains("cargo doc --workspace --no-deps --locked"),
        "change-gate must build workspace documentation with warnings denied"
    );
    assert!(
        !job.contains("if:")
            && !job.contains("continue-on-error")
            && !job.contains("|| true")
            && !job.contains("|| :"),
        "grammar and documentation checks must not have a skip or false-green path"
    );
}

fn verify_tests_evidence_is_accepted(job: &str) -> bool {
    [
        "      - name: Failure propagation canary",
        r#"          bash tools/ci/ci-evidence.sh \
            --report-dir "$JET_CI_EVIDENCE_DIR/canary" \
            -- bash -c 'printf "expected canary failure\\n" >&2; exit 23'"#,
        "          test \"$status\" -eq 23",
        r#"          grep -Fxq "status=fail" "$JET_CI_EVIDENCE_DIR/canary/receipt.txt""#,
        r#"          grep -Fxq "command_exit=23" "$JET_CI_EVIDENCE_DIR/canary/receipt.txt""#,
        "      - name: Finalize durable gate evidence",
        "        if: always()",
        "          JOB_STATUS: ${{ job.status }}",
        "      - name: Upload durable gate evidence",
        "          name: ci-gate-evidence-shard-${{ matrix.shard }}-${{ github.sha }}",
        "          path: ci-evidence/shard-${{ matrix.shard }}/",
        "          if-no-files-found: error",
    ]
    .iter()
    .all(|needle| job.contains(needle))
        && job_has_run(
            job,
            r#"bash tools/ci/ci-evidence.sh --report-dir "$JET_CI_EVIDENCE_DIR" -- nix develop .#full -c scripts/agent/verify-full.sh"#,
        )
        && !job.contains("continue-on-error")
}

fn nightly_gate_is_accepted(job: &str) -> bool {
    job.contains("if: github.event_name == 'schedule'")
        && job.contains("runs-on: ubuntu-latest")
        && job.contains("JET_REQUIRE_RUSTC: \"1\"")
        && job.contains("FUZZ_VARIANTS: \"1000\"")
        && job.contains(
            "FUZZ_SEED=\"$seed\" nix develop -c cargo test --release --test fuzz_sema -- --nocapture",
        )
        && job.contains("CARGO_BUILD_JOBS: \"6\"")
        && job.contains("nix develop .#full -c cargo build")
        && job.contains("nix develop -c sh tools/perf/ci-perf-check.sh")
        && job.contains("nix develop .#full -c bash tools/perf/web-bundle-check.sh")
        && !job.contains("continue-on-error")
        && !job.contains("|| true")
        && !job.contains("|| :")
}

#[test]
fn ci_nightly_gate_runs_production_fuzz_and_perf_checks() {
    let workflow = fs::read_to_string(root().join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    let job = workflow_job(&workflow, "nightly-fuzz");
    assert!(!job.is_empty(), "CI must define the nightly-fuzz job");
    assert!(nightly_gate_is_accepted(&job));

    for (name, command, replacement) in [
        (
            "fuzz",
            "FUZZ_SEED=\"$seed\" nix develop -c cargo test --release --test fuzz_sema -- --nocapture",
            "true",
        ),
        (
            "Jet build",
            "nix develop .#full -c cargo build",
            "true",
        ),
        (
            "compiler-speed perf",
            "nix develop -c sh tools/perf/ci-perf-check.sh",
            "true",
        ),
        (
            "web bundle perf",
            "nix develop .#full -c bash tools/perf/web-bundle-check.sh",
            "true",
        ),
    ] {
        let bypassed = job.replacen(command, replacement, 1);
        assert_ne!(bypassed, job, "{name} command mutation did not apply");
        assert!(
            !nightly_gate_is_accepted(&bypassed),
            "nightly gate proof must fail when the {name} production command is bypassed"
        );
    }
}

#[test]
fn ci_warning_free_gate_is_strict_and_curated() {
    let workflow = fs::read_to_string(root().join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    let verify = fs::read_to_string(root().join("scripts/agent/verify-full.sh"))
        .expect("read scripts/agent/verify-full.sh");
    let gate = workflow
        .split("  rust-lint:\n")
        .nth(1)
        .and_then(|rest| rest.split("  verify-tests:\n").next())
        .expect("CI must define a rust-lint job before verify-tests");

    assert!(
        gate.contains("cargo fmt --all -- --check"),
        "D-CI2 rust-lint job must run rustfmt in check mode"
    );
    assert!(
        gate.contains("ref: ${{ github.sha }}"),
        "D-CI2 rust-lint job must lint the event's exact candidate revision"
    );
    assert!(
        gate.contains("cargo clippy --workspace --all-targets --locked"),
        "D-CI2 rust-lint job must lint every workspace target from the lockfile"
    );
    assert!(
        gate.contains("RUSTFLAGS: \"-D warnings\"") && gate.contains("-D warnings"),
        "D-CI2 rust-lint job must deny Rust warnings"
    );
    assert!(
        gate.contains("-D clippy::correctness") && gate.contains("-D clippy::suspicious"),
        "D-CI2 rust-lint job must deny correctness and suspicious Clippy lints"
    );
    assert!(
        gate.contains("-A clippy::style"),
        "D-CI2 rust-lint job must leave house-conflicting style lints advisory"
    );
    assert!(
        !gate.contains("continue-on-error") && !gate.contains("if: always()"),
        "D-CI2 lint failures must block the change gate"
    );
    assert!(
        verify.contains("D-CI2=A") && verify.contains("-D warnings"),
        "the production test path must inherit the warning wall"
    );

    let jit = fs::read_to_string(root().join("crates/jet-jit/src/lib.rs"))
        .expect("read jet-jit lint policy");
    assert!(
        jit.contains("#![expect(") && jit.contains("reason = \"#804:"),
        "intentional Rust lint exceptions must use card-referenced #[expect]"
    );
}

#[test]
fn ci_gate_evidence_preserves_success_and_failure_receipts() {
    let root = root();
    let scratch = common::test_scratch_root(&format!("ci-evidence-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).unwrap();
    let script = root.join("tools/ci/ci-evidence.sh");

    let success_dir = scratch.join("success");
    let success = Command::new("bash")
        .arg(&script)
        .args(["--report-dir", success_dir.to_str().unwrap(), "--", "bash", "-c"])
        .arg("printf 'gate-ok\\n'")
        .env_remove("GITHUB_ACTIONS")
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .current_dir(&root)
        .output()
        .expect("run CI evidence success path");
    assert!(
        success.status.success(),
        "successful gate must stay successful:\n{}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&success.stdout),
        "gate-ok\n",
        "evidence wrapper must preserve gate stdout"
    );
    let success_receipt = fs::read_to_string(success_dir.join("receipt.txt")).unwrap();
    let candidate = success_receipt
        .lines()
        .find_map(|line| line.strip_prefix("candidate_commit="))
        .expect("receipt must record the candidate")
        .to_owned();
    assert_eq!(candidate.len(), 40, "candidate revision must be a full SHA-1");
    assert!(success_receipt.contains("schema=jet.ci-evidence.v1"));
    assert!(success_receipt.contains("status=pass"));
    assert!(success_receipt.contains(&format!("candidate_commit={candidate}")));
    for field in [
        "source_candidate_commit",
        "toolchain_candidate_commit",
        "artifact_candidate_commit",
        "signature_candidate_commit",
        "test_candidate_commit",
        "support_matrix_candidate_commit",
        "provenance_candidate_commit",
        "release_metadata_candidate_commit",
    ] {
        assert!(
            success_receipt.contains(&format!("{field}={candidate}")),
            "receipt must bind {field} to the candidate"
        );
    }
    let candidate_manifest = fs::read_to_string(success_dir.join("candidate.txt")).unwrap();
    assert!(candidate_manifest.contains("schema=jet.ci-candidate.v1"));
    for field in [
        "candidate_commit",
        "source_candidate_commit",
        "toolchain_candidate_commit",
        "artifact_candidate_commit",
        "signature_candidate_commit",
        "test_candidate_commit",
        "support_matrix_candidate_commit",
        "provenance_candidate_commit",
        "release_metadata_candidate_commit",
    ] {
        assert!(
            candidate_manifest.contains(&format!("{field}={candidate}")),
            "candidate manifest must bind {field} to the candidate"
        );
    }
    assert!(candidate_manifest.contains("signature=not-required-for-ci-test-gate"));
    assert!(success_receipt.contains("command_exit=0"));
    assert!(success_receipt.contains("support_matrix=Linux/X64"));
    assert!(success_receipt.contains("artifact_name=not-published"));
    assert!(success_receipt.contains("provenance=github-actions:local/1"));
    assert!(success_receipt.contains("source_manifest_sha256="));
    assert!(success_receipt.contains("toolchain_sha256="));
    assert_eq!(
        fs::read_to_string(success_dir.join("command.stdout")).unwrap(),
        "gate-ok\n"
    );

    let failure_dir = scratch.join("failure");
    let failure = Command::new("bash")
        .arg(&script)
        .args(["--report-dir", failure_dir.to_str().unwrap(), "--", "bash", "-c"])
        .arg("printf 'gate-failed\\n' >&2; exit 23")
        .env("GITHUB_SHA", &candidate)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .current_dir(&root)
        .output()
        .expect("run CI evidence failure path");
    assert_eq!(failure.status.code(), Some(23));
    let failure_receipt = fs::read_to_string(failure_dir.join("receipt.txt")).unwrap();
    assert!(failure_receipt.contains("status=fail"));
    assert!(failure_receipt.contains("command_exit=23"));
    assert_eq!(
        fs::read_to_string(failure_dir.join("command.stderr")).unwrap(),
        "gate-failed\n"
    );
    assert!(
        failure_dir.join("toolchain.txt").is_file(),
        "failed gates must retain toolchain evidence"
    );

    let missing_runner_dir = scratch.join("missing-runner");
    let missing_runner = Command::new("bash")
        .arg(&script)
        .args([
            "--report-dir",
            missing_runner_dir.to_str().unwrap(),
            "--",
            "bash",
            "-c",
        ])
        .arg("printf 'must-not-run\\n'")
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_SHA", &candidate)
        .env_remove("RUNNER_OS")
        .env_remove("RUNNER_ARCH")
        .env_remove("GITHUB_RUNNER_OS")
        .env_remove("GITHUB_RUNNER_ARCH")
        .current_dir(&root)
        .output()
        .expect("run CI evidence missing-runner path");
    assert_eq!(missing_runner.status.code(), Some(78));
    assert_eq!(
        fs::read_to_string(missing_runner_dir.join("command.stdout")).unwrap(),
        ""
    );
    let missing_runner_receipt =
        fs::read_to_string(missing_runner_dir.join("receipt.txt")).unwrap();
    assert!(missing_runner_receipt.contains("status=fail"));
    assert!(
        fs::read_to_string(missing_runner_dir.join("command.stderr"))
            .unwrap()
            .contains("runner OS identity is missing")
    );

    let stale_dir = scratch.join("stale");
    fs::create_dir_all(&stale_dir).unwrap();
    fs::write(stale_dir.join("receipt.txt"), "status=pass\\n").unwrap();
    let stale = Command::new("bash")
        .arg(&script)
        .args(["--report-dir", stale_dir.to_str().unwrap(), "--", "bash", "-c"])
        .arg("printf 'must-not-run\\n'")
        .env_remove("GITHUB_ACTIONS")
        .env("GITHUB_SHA", &candidate)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .current_dir(&root)
        .output()
        .expect("run CI evidence stale-report path");
    assert_eq!(stale.status.code(), Some(78));
    assert_eq!(
        fs::read_to_string(stale_dir.join("command.stdout")).unwrap(),
        ""
    );
    let stale_receipt = fs::read_to_string(stale_dir.join("receipt.txt")).unwrap();
    assert!(stale_receipt.contains("status=fail"));
    assert!(
        fs::read_to_string(stale_dir.join("command.stderr"))
            .unwrap()
            .contains("stale file")
    );

    let artifact_mismatch_dir = scratch.join("artifact-mismatch");
    let artifact_mismatch = Command::new("bash")
        .arg(&script)
        .args([
            "--report-dir",
            artifact_mismatch_dir.to_str().unwrap(),
            "--",
            "bash",
            "-c",
        ])
        .arg("printf 'must-not-run\\n'")
        .env_remove("GITHUB_ACTIONS")
        .env("GITHUB_SHA", &candidate)
        .env("JET_CI_ARTIFACT_NAME", "ci-gate-evidence-shard-0-stale")
        .current_dir(&root)
        .output()
        .expect("run CI evidence artifact identity failure path");
    assert_eq!(artifact_mismatch.status.code(), Some(78));
    let artifact_mismatch_receipt =
        fs::read_to_string(artifact_mismatch_dir.join("receipt.txt")).unwrap();
    assert!(artifact_mismatch_receipt.contains("status=fail"));
    assert!(
        fs::read_to_string(artifact_mismatch_dir.join("command.stderr"))
            .unwrap()
            .contains("artifact name does not identify candidate")
    );
    assert!(artifact_mismatch_dir.join("candidate.txt").is_file());
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn ci_workflow_uploads_candidate_bound_evidence_without_false_green_controls() {
    let root = root();
    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    let verify = fs::read_to_string(root.join("scripts/agent/verify-full.sh"))
        .expect("read scripts/agent/verify-full.sh");
    let verify_tests = workflow_job(&workflow, "verify-tests");
    assert!(
        verify_tests_evidence_is_accepted(&verify_tests),
        "verify-tests must keep failure canary and durable evidence on the required path"
    );
    let bypassed = verify_tests.replacen(
        r#"          grep -Fxq "command_exit=23" "$JET_CI_EVIDENCE_DIR/canary/receipt.txt""#,
        "          true",
        1,
    );
    assert_ne!(bypassed, verify_tests, "receipt assertion mutation did not apply");
    assert!(
        !verify_tests_evidence_is_accepted(&bypassed),
        "the evidence proof must fail when the failure canary no longer checks its receipt"
    );
    assert!(verify.contains("tools/ci/test-shards.sh"));
    assert!(verify.contains("cargo test $test_target --no-run"));
    assert!(verify.contains("test_targets_repeat"));
    assert!(verify.contains("test-target inventory is nondeterministic"));
    assert!(verify.contains("CARGO_BUILD_JOBS"));
    assert!(!verify.contains("tmp_parent=\"${JET_VERIFY_TMPDIR:-/tmp}\""));
}

// ============================================================================
// Section: #805 read-only Tower hygiene (D-ONCE-LEDGER1=A)

fn run_tower_hygiene_gate(
    repo: &Path,
    tower_dir: &Path,
    docs_root: &Path,
    report: &Path,
) -> std::process::Output {
    Command::new("bash")
        .arg(repo.join("tools/ci/tower-hygiene-gate.sh"))
        .current_dir(repo)
        .env("TOWER_DATA", tower_dir)
        .env("JET_TOWER_LINT_SCOPE", docs_root)
        .env("JET_TOWER_HYGIENE_REPORT", report)
        .env_remove("JET_TEST_SHARD")
        .output()
        .expect("run Tower hygiene gate")
}

#[test]
fn tower_hygiene_gate_is_read_only_and_blocks_missing_records() {
    let repo = root();
    let fixture = common::test_scratch_root(&format!("tower-hygiene-gate-{}", std::process::id()));
    let _ = fs::remove_dir_all(&fixture);
    fs::create_dir_all(fixture.join("docs/spec")).unwrap();
    fs::create_dir_all(fixture.join("tower")).unwrap();

    fs::write(
        fixture.join("tower/tower.json"),
        r#"{
  "meta": {"version": 4, "project": "fixture", "currentEpoch": null, "nextNum": 1, "rev": 0, "ui": {"toggled": []}},
  "epochs": [{"id": "e1", "name": "Epoch 1", "goal": "", "status": "active"}],
  "milestones": [],
  "cards": [],
  "decisions": [{"id": "D-OK1", "status": "ratified", "outcome": "A"}],
  "questions": [],
  "ideas": [],
  "papercuts": [],
  "events": []
}
"#,
    )
    .unwrap();
    fs::write(
        fixture.join("tower/history.json"),
        r#"{"version": 1, "decisions": [], "cards": [], "events": []}
"#,
    )
    .unwrap();
    fs::write(
        fixture.join("tower/read-only-marker.txt"),
        "must not change\n",
    )
    .unwrap();
    fs::write(fixture.join("docs/spec/ok.md"), "D-OK1\n").unwrap();

    let tower_before = fs::read(fixture.join("tower/tower.json")).unwrap();
    let history_before = fs::read(fixture.join("tower/history.json")).unwrap();
    let marker_before = fs::read(fixture.join("tower/read-only-marker.txt")).unwrap();
    let success_report = fixture.join("success.txt");
    let success = run_tower_hygiene_gate(
        &repo,
        &fixture.join("tower"),
        &fixture.join("docs"),
        &success_report,
    );
    assert!(
        success.status.success(),
        "valid Tower hygiene must pass:\n{}",
        String::from_utf8_lossy(&success.stdout)
    );
    let success_text = fs::read_to_string(&success_report).unwrap();
    assert!(success_text.contains("status=pass"), "{success_text}");
    assert!(success_text.contains("read_only=pass"), "{success_text}");
    assert!(success_text.contains("lint_exit=0"), "{success_text}");
    assert!(
        success_text.contains("lint_repeat_exit=0"),
        "{success_text}"
    );
    assert!(success_text.contains("lint_json=pass"), "{success_text}");
    assert!(
        success_text.contains("lint_repeat_json=pass"),
        "{success_text}"
    );
    assert!(success_text.contains("candidate_commit="), "{success_text}");
    assert!(success_text.contains("runner_os="), "{success_text}");
    assert!(success_text.contains("node=v"), "{success_text}");
    assert!(success_text.contains("scope_input="), "{success_text}");
    assert_eq!(
        tower_before,
        fs::read(fixture.join("tower/tower.json")).unwrap()
    );
    assert_eq!(
        history_before,
        fs::read(fixture.join("tower/history.json")).unwrap()
    );
    assert_eq!(
        marker_before,
        fs::read(fixture.join("tower/read-only-marker.txt")).unwrap()
    );

    fs::write(fixture.join("docs/spec/bad.md"), "D-MISSING1\n").unwrap();
    let failure_report = fixture.join("failure.txt");
    let failure = run_tower_hygiene_gate(
        &repo,
        &fixture.join("tower"),
        &fixture.join("docs"),
        &failure_report,
    );
    assert!(
        !failure.status.success(),
        "missing Tower decision record must block:\n{}",
        String::from_utf8_lossy(&failure.stdout)
    );
    let failure_text = fs::read_to_string(&failure_report).unwrap();
    assert!(failure_text.contains("status=fail"), "{failure_text}");
    assert!(failure_text.contains("lint_exit=1"), "{failure_text}");
    assert!(
        failure_text.contains("lint_repeat_exit=1"),
        "{failure_text}"
    );
    assert!(
        failure_text.contains("spec-decision-ref-missing"),
        "{failure_text}"
    );
    assert!(failure_text.contains("D-MISSING1"), "{failure_text}");
    assert_eq!(
        tower_before,
        fs::read(fixture.join("tower/tower.json")).unwrap()
    );
    assert_eq!(
        history_before,
        fs::read(fixture.join("tower/history.json")).unwrap()
    );
    assert_eq!(
        marker_before,
        fs::read(fixture.join("tower/read-only-marker.txt")).unwrap()
    );

    fs::create_dir_all(fixture.join("outside")).unwrap();
    let traversal_report = fixture.join("outside/../tower/traversal.txt");
    let traversal = run_tower_hygiene_gate(
        &repo,
        &fixture.join("tower"),
        &fixture.join("docs"),
        &traversal_report,
    );
    assert!(
        !traversal.status.success(),
        "a report path resolving into Tower must block the gate:\n{}",
        String::from_utf8_lossy(&traversal.stdout)
    );
    assert!(
        !fixture.join("tower/traversal.txt").exists(),
        "read-only gate wrote an audit report into Tower"
    );

    fs::create_dir_all(fixture.join("tower/tower.json.lock")).unwrap();
    fs::write(
        fixture.join("tower/tower.json.lock/info.json"),
        "{\"pid\": 1, \"at\": 0}\n",
    )
    .unwrap();
    let locked_report = fixture.join("locked.txt");
    let locked = run_tower_hygiene_gate(
        &repo,
        &fixture.join("tower"),
        &fixture.join("docs"),
        &locked_report,
    );
    assert!(
        !locked.status.success(),
        "a Tower write lock must block the read-only gate:\n{}",
        String::from_utf8_lossy(&locked.stdout)
    );
    let locked_text = fs::read_to_string(&locked_report).unwrap();
    assert!(locked_text.contains("read_only=blocked"), "{locked_text}");
    assert!(locked_text.contains("lint_exit=not-run"), "{locked_text}");
    assert!(locked_text.contains("write lock"), "{locked_text}");
    let _ = fs::remove_dir_all(fixture.join("tower/tower.json.lock"));

    let verify = fs::read_to_string(repo.join("scripts/agent/verify-full.sh")).unwrap();
    assert!(
        verify.contains("tools/ci/tower-hygiene-gate.sh"),
        "verify-full must run the production Tower hygiene gate"
    );
    let _ = fs::remove_dir_all(&fixture);
}
