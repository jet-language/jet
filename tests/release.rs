//! E2-M2 — release policy, editions, and the epoch contract.
//!
//! Golden tests for the version banner (E2-D1), the E2001 edition-too-new
//! diagnostic (D-REL3), and the E2002/L2001 deprecation diagnostics (D-REL5),
//! plus a docs-consistency check that every later breaking epoch-2 milestone
//! names the edition/epoch gate it needs (the m2 exit criteria).
//!
//! Fixtures live in tests/release/*.txt. To re-bless after an INTENTIONAL change
//! (read it against docs/spec/diagnostics.md and docs/spec/release-policy.md
//! first):
//!
//!     UPDATE_EXPECT=1 cargo test
//!
//! E2002 and L2001 are not yet user-triggerable (the deprecation registry is
//! empty pre-1.0 — see docs/spec/diagnostics.md). The deprecation fixture is
//! rendered from a synthetic registry entry so the wording is still pinned.

use jet::diag::Diagnostic;
use jet::manifest::{self, Deprecation};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

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
    assert_eq!(stdout, manifest::version_banner());
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
    let mf = manifest::parse(path, raw).expect("manifest should parse");
    let err = manifest::check_edition_support(&mf, "pkg.jet")
        .expect_err("a future edition must be rejected");
    assert_eq!(err.code, "E2001");
    let rendered = jet::render_diagnostics("pkg.jet", raw, std::slice::from_ref(&err));
    check_fixture("edition_too_new.txt", &rendered);
}

#[test]
fn supported_edition_is_accepted() {
    let raw = format!(
        "payload: {{ name: \"x\", version: \"0.1.0\", edition: \"{}\" }}\n",
        manifest::latest_edition()
    );
    let mf = manifest::parse(std::path::Path::new("pkg.jet"), &raw).unwrap();
    assert!(manifest::check_edition_support(&mf, "pkg.jet").is_ok());
}

#[test]
fn no_edition_field_is_accepted() {
    // A manifest with no edition tracks the toolchain's newest stable edition.
    let raw = "payload: { name: \"x\", version: \"0.1.0\" }\n";
    let mf = manifest::parse(std::path::Path::new("pkg.jet"), raw).unwrap();
    assert_eq!(mf.package.edition, None);
    assert!(manifest::check_edition_support(&mf, "pkg.jet").is_ok());
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
    let lint = manifest::l2001(&synth, None);
    let err = manifest::e2002(&synth, None);
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
        manifest::DEPRECATIONS.is_empty(),
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
    assert!(checked >= 1, "expected at least one non-m2 epoch-2 plan to scan");
}
