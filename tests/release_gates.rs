//! Milestone gate checks, one lane: canon.jet golden run, `--small` binary
//! size gates, the Epoch 2 GA checklist, and release/edition/deprecation
//! policy. Distinct from `tests/golden.rs` (per-example front-end + rustc
//! matrix) — these are one-off milestone exit-criteria checks, not part of
//! the example discovery loop.

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
