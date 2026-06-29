//! Integration tests for `workspace.jet` evaluation (D-WORKSPACE1=B, D-WORKSPACE2=A).
//!
//! Covers I4 snapshot requirements for E0995, E0996, E0997:
//! - E0995: workspace.jet has no `module workspace { … }` declaration
//! - E0996: `members:` evaluated to something other than `[String]`
//! - E0997: `find("…")` names a missing directory
//!
//! These diagnostics fire from `jet::Jetpack::WorkspaceFile::evaluate`, not from
//! `jet check`, so coverage is via programmatic assertion rather than .stderr snapshots.

use jet::Jetpack::WorkspaceFile;
use std::path::Path;

// ──────────────────────────────────────────────
// Happy-path
// ──────────────────────────────────────────────

#[test]
fn empty_member_list_is_valid() {
    let src = "module workspace {\n    members: []\n}\n";
    let plan =
        WorkspaceFile::evaluate(src, Path::new("/tmp")).expect("empty member list should be valid");
    assert!(plan.members.is_empty());
}

#[test]
fn explicit_path_list_yields_members() {
    // Paths don't need to exist: names fall back to the directory basename.
    let src = "module workspace {\n    members: [\"./packages/hello\", \"./packages/ranker\"]\n}\n";
    let plan =
        WorkspaceFile::evaluate(src, Path::new("/tmp")).expect("explicit path list should succeed");
    assert_eq!(plan.members.len(), 2);
    assert_eq!(plan.members[0].name, "hello");
    assert_eq!(plan.members[1].name, "ranker");
}

// ──────────────────────────────────────────────
// E0995 — no `module workspace { … }`
// ──────────────────────────────────────────────

#[test]
fn e0995_no_workspace_module_fires() {
    let src = "module dev { env.dev: Env.{ packages: [] } }\n";
    let d = WorkspaceFile::evaluate(src, Path::new("/tmp"))
        .expect_err("must fail when workspace module is absent");
    assert_eq!(
        d.code, "E0995",
        "expected E0995, got {} — {:?}",
        d.code, d.what
    );
}

#[test]
fn e0995_fires_on_empty_file() {
    let src = "";
    let d = WorkspaceFile::evaluate(src, Path::new("/tmp")).expect_err("empty file must fail");
    assert_eq!(d.code, "E0995");
}

// ──────────────────────────────────────────────
// E0996 — members: not a list of strings
// ──────────────────────────────────────────────

#[test]
fn e0996_members_not_a_list() {
    let src = "module workspace { members: 42 }\n";
    let d =
        WorkspaceFile::evaluate(src, Path::new("/tmp")).expect_err("non-list members must fail");
    assert_eq!(
        d.code, "E0996",
        "expected E0996, got {} — {:?}",
        d.code, d.what
    );
}

#[test]
fn e0996_members_list_with_non_string_element() {
    // A list that contains a non-string element (integer).
    let src = "module workspace { members: [1, 2] }\n";
    let d = WorkspaceFile::evaluate(src, Path::new("/tmp")).expect_err("list of ints must fail");
    assert_eq!(d.code, "E0996");
}

// ──────────────────────────────────────────────
// E0997 — find("…") directory doesn't exist
// ──────────────────────────────────────────────

#[test]
fn e0997_find_missing_dir() {
    let src = "module workspace { members: find(\"./definitely-no-such-packages\") }\n";
    let d =
        WorkspaceFile::evaluate(src, Path::new("/tmp")).expect_err("find of missing dir must fail");
    assert_eq!(
        d.code, "E0997",
        "expected E0997, got {} — {:?}",
        d.code, d.what
    );
}

// ──────────────────────────────────────────────
// D-MONOREF1=A: dot form in RefSpec
// ──────────────────────────────────────────────

#[test]
fn dot_form_classified_when_source_is_declared() {
    use jet::Jetpack::RefSpec::{classify_in, ProviderKind, Source, SourceTable};

    // Build a table with a named source "mono".
    let table = SourceTable::from_decls([(
        "mono".to_string(),
        "github:acme/monorepo".to_string(),
        ProviderKind::default(),
    )]);

    // `mono.ranker` — dot form resolves via D-MONOREF1=A.
    let r = classify_in("mono.ranker", &table).expect("dot form should classify");
    assert!(
        matches!(r.source, Source::Named(ref s) if s == "mono"),
        "expected Named(\"mono\"), got {:?}",
        r.source
    );
    assert_eq!(r.package, "ranker");
}

#[test]
fn colon_form_still_works() {
    use jet::Jetpack::RefSpec::{classify_in, ProviderKind, Source, SourceTable};

    let table = SourceTable::from_decls([(
        "mono".to_string(),
        "github:acme/monorepo".to_string(),
        ProviderKind::default(),
    )]);

    let r = classify_in("mono:ranker", &table).expect("colon form should still classify");
    assert!(matches!(r.source, Source::Named(_)));
    assert_eq!(r.package, "ranker");
}

#[test]
fn dot_form_not_classified_when_source_is_unknown() {
    use jet::Jetpack::RefSpec::{classify_in, RefError, SourceTable};

    let table = SourceTable::empty(); // no sources declared
    let err =
        classify_in("unknown.pkg", &table).expect_err("unknown source in dot form should fail");
    // Falls through to MissingSeparator since no source matched.
    assert!(
        matches!(err, RefError::MissingSeparator(_)),
        "expected MissingSeparator, got {err:?}"
    );
}

// ──────────────────────────────────────────────
// I5: workspace.jet with find() discovers committed-style members
// ──────────────────────────────────────────────

#[test]
fn workspace_find_example_evaluates() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ws-commit-test-{nanos}"));
    let packages = dir.join("packages");
    let hello = packages.join("hello");
    let ranker = packages.join("ranker");
    std::fs::create_dir_all(&hello).unwrap();
    std::fs::create_dir_all(&ranker).unwrap();
    std::fs::write(hello.join("pkg.jet"), "name: \"hello\"\n").unwrap();
    std::fs::write(ranker.join("pkg.jet"), "name: \"ranker\"\n").unwrap();
    std::fs::write(
        dir.join("workspace.jet"),
        "module workspace {\n    members: find(\"./packages\")\n}\n",
    )
    .unwrap();

    let src = std::fs::read_to_string(dir.join("workspace.jet")).unwrap();
    let plan = WorkspaceFile::evaluate(&src, &dir)
        .expect("workspace.jet with find() must evaluate without errors");
    let names: Vec<&str> = plan.members.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"hello"), "expected hello, got {names:?}");
    assert!(names.contains(&"ranker"), "expected ranker, got {names:?}");
    assert_eq!(names.len(), 2, "expected 2 members, got {names:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

// ──────────────────────────────────────────────
// find() discover packages
// ──────────────────────────────────────────────

#[test]
fn find_discovers_packages_with_pkg_jet() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = std::env::temp_dir().join(format!("ws-find-test-{nanos}"));
    // packages/hello/pkg.jet
    let hello = tmp.join("packages/hello");
    std::fs::create_dir_all(&hello).unwrap();
    std::fs::write(hello.join("pkg.jet"), "name: \"hello\"\n").unwrap();
    // packages/ranker/pkg.jet
    let ranker = tmp.join("packages/ranker");
    std::fs::create_dir_all(&ranker).unwrap();
    std::fs::write(ranker.join("pkg.jet"), "name: \"ranker\"\n").unwrap();
    // packages/bare (no pkg.jet — should be ignored)
    std::fs::create_dir_all(tmp.join("packages/bare")).unwrap();

    let src = "module workspace { members: find(\"./packages\") }\n";
    let plan = WorkspaceFile::evaluate(src, &tmp).expect("find should succeed");
    assert_eq!(plan.members.len(), 2, "expected 2 members: {plan:?}");
    let names: Vec<&str> = plan.members.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"hello"), "names: {names:?}");
    assert!(names.contains(&"ranker"), "names: {names:?}");

    std::fs::remove_dir_all(&tmp).ok();
}
