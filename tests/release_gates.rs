//! Milestone gate checks, one lane: canon.jet golden run, `--small` binary
//! size gates, the Epoch 2 GA checklist, and release/edition/deprecation
//! policy. Distinct from `tests/golden.rs` (per-example front-end + rustc
//! matrix) — these are one-off milestone exit-criteria checks, not part of
//! the example discovery loop.

use std::fs;
use std::path::PathBuf;
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

/// SL9: `01_hello.jet --small` stays under a pinned cross-platform byte budget.
#[test]
fn hello_world_small_binary_stays_under_budget() {
    let jet = jet_bin();
    let have_rustc = have_rustc();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping hello size budget test (need jet + rustc)");
        return;
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/features/basics/hello.jet");
    assert!(
        example.is_file(),
        "examples/features/basics/hello.jet must exist"
    );

    let dir = std::env::temp_dir().join(format!("jet_hello_budget_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();
    fs::copy(&example, dir.join("01_hello.jet")).unwrap();

    let build = Command::new(&jet)
        .args(["build", "--small", "01_hello.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "--small hello build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let size = fs::metadata(dir.join("build/01_hello")).unwrap().len();
    const MAX_HELLO_SMALL_BYTES: u64 = 512_000;
    assert!(
        size <= MAX_HELLO_SMALL_BYTES,
        "01_hello --small binary ({size} bytes) exceeds budget ({MAX_HELLO_SMALL_BYTES} bytes)"
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
    let b = s.as_bytes();
    b.len() == 5 && (b[0] == b'E' || b[0] == b'L') && b[1..].iter().all(|c| c.is_ascii_digit())
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
// E2002 and L2001 are not yet user-triggerable (the deprecation registry is
// empty pre-1.0 — see docs/spec/diagnostics.md). The deprecation fixture is
// rendered from a synthetic registry entry so the wording is still pinned.
// ============================================================================

use jet::Diagnostics::Diagnostic;
use jet::Manifest::{self, Deprecation};

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
    // A real pkg.jet asking for a future edition triggers E2001 through the
    // manifest loader path. We render the diagnostic the way the CLI would.
    let raw = r#"payload: {
    name: "wordstats",
    version: "0.1.0",
    edition: "2099",
}
"#;
    let path = std::path::Path::new("pkg.jet");
    let mf = Manifest::parse(path, raw).expect("manifest should parse");
    let err = Manifest::check_edition_support(&mf, "pkg.jet")
        .expect_err("a future edition must be rejected");
    assert_eq!(err.code, "E2001");
    let rendered = jet::render_diagnostics("pkg.jet", raw, std::slice::from_ref(&err));
    check_fixture("edition_too_new.txt", &rendered);
}

#[test]
fn supported_edition_is_accepted() {
    let raw = format!(
        "payload: {{ name: \"x\", version: \"0.1.0\", edition: \"{}\" }}\n",
        Manifest::latest_edition()
    );
    let mf = Manifest::parse(std::path::Path::new("pkg.jet"), &raw).unwrap();
    assert!(Manifest::check_edition_support(&mf, "pkg.jet").is_ok());
}

#[test]
fn no_edition_field_is_accepted() {
    // A manifest with no edition tracks the toolchain's newest stable edition.
    let raw = "payload: { name: \"x\", version: \"0.1.0\" }\n";
    let mf = Manifest::parse(std::path::Path::new("pkg.jet"), raw).unwrap();
    assert_eq!(mf.package.edition, None);
    assert!(Manifest::check_edition_support(&mf, "pkg.jet").is_ok());
}

#[test]
fn deprecation_e2002_and_l2001() {
    // The real registry is empty pre-1.0. Render from a synthetic deprecation so
    // the E2002/L2001 wording is pinned and ready for the first real one.
    let synth = Deprecation {
        item: "old_keyword",
        since_edition: "2026",
        replacement: "new_keyword",
        removed_in_edition: "2027",
    };
    let lint = Manifest::l2001(&synth, None);
    let err = Manifest::e2002(&synth, None);
    assert_eq!(lint.code, "L2001");
    assert_eq!(err.code, "E2002");

    let mut rendered = String::new();
    rendered.push_str(&render_standalone(&lint));
    rendered.push('\n');
    rendered.push_str(&render_standalone(&err));
    check_fixture("deprecation.txt", &rendered);
}

/// Render a diagnostic with no source span (manifest-level / lint diagnostics).
fn render_standalone(d: &Diagnostic) -> String {
    jet::render_diagnostics("(deprecation registry)", "", std::slice::from_ref(d))
}

#[test]
fn registry_is_honestly_empty_pre_1_0() {
    // Guards the doc claim: the deprecation registry has no entries yet.
    assert!(
        Manifest::DEPRECATIONS.is_empty(),
        "DEPRECATIONS is no longer empty — make E2002/L2001 reachable and update docs/spec/diagnostics.md to drop the not-yet-triggerable note"
    );
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
