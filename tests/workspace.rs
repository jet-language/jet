//! Integration tests for `workspace.jet` evaluation (D-WORKSPACE1=B, D-WORKSPACE2=A).
//!
//! Covers I4 snapshot requirements for E0995, E0996, E0997:
//! - E0995: workspace.jet has no `module workspace { … }` declaration
//! - E0996: `members:` evaluated to something other than `[String]`
//! - E0997: `find("…")` names a missing directory
//!
//! These diagnostics fire from `jetpack::WorkspaceFile::evaluate`, not from
//! `jet check`, so coverage is via programmatic assertion rather than .stderr snapshots.

use jetpack::WorkspaceFile;
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
    use jetpack::RefSpec::{classify_in, ProviderKind, Source, SourceTable};

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
    use jetpack::RefSpec::{classify_in, ProviderKind, Source, SourceTable};

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
    use jetpack::RefSpec::{classify_in, RefError, SourceTable};

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
// Slice B: bare/path addressing against the workspace index
// (D-MONOREF1=A; E1230 ambiguous member, E1231 unknown member)
// ──────────────────────────────────────────────

#[test]
fn bare_and_path_form_resolve_workspace_members() {
    use jetpack::RefSpec::{classify_with_workspace, Source, SourceTable, WorkspaceIndex};
    let index = WorkspaceIndex::from_members([
        ("logging".to_string(), "./infra/logging".to_string()),
        ("ranker".to_string(), "packages/ranker".to_string()),
    ]);
    // Bare name → unique member.
    let r = classify_with_workspace("logging", &SourceTable::empty(), &index).unwrap();
    assert_eq!(r.source, Source::Path);
    assert_eq!(r.package, "infra/logging");
    // Path form → member by relative path.
    let r2 = classify_with_workspace("packages/ranker", &SourceTable::empty(), &index).unwrap();
    assert_eq!(r2.package, "packages/ranker");
}

#[test]
fn unknown_workspace_member_is_e1231() {
    use jetpack::RefSpec::{classify_with_workspace, SourceTable, WorkspaceIndex};
    let index =
        WorkspaceIndex::from_members([("logging".to_string(), "infra/logging".to_string())]);
    let err = classify_with_workspace("loggger", &SourceTable::empty(), &index)
        .expect_err("unknown member must fail");
    assert_eq!(err.code(), Some("E1231"), "expected E1231, got {err:?}");
}

#[test]
fn ambiguous_workspace_member_is_e1230() {
    use jetpack::RefSpec::{classify_with_workspace, SourceTable, WorkspaceIndex};
    let index = WorkspaceIndex::from_members([
        ("logging".to_string(), "infra/logging".to_string()),
        ("logging".to_string(), "apps/logging".to_string()),
    ]);
    let err = classify_with_workspace("logging", &SourceTable::empty(), &index)
        .expect_err("ambiguous bare member must fail");
    assert_eq!(err.code(), Some("E1230"), "expected E1230, got {err:?}");
}

// ──────────────────────────────────────────────
// Slice C: monorepo fetch diagnostics (E1232 fetch failure, E1233 dep outside)
// ──────────────────────────────────────────────

#[test]
fn monorepo_fetch_errors_carry_registered_codes() {
    use jetpack::Provider::ProviderError;
    // E1232: sparse subtree checkout + full-clone fallback both failed.
    assert_eq!(
        ProviderError::MonorepoFetch("boom".to_string()).code(),
        Some("E1232")
    );
    // E1233: an in-repo dep names a package outside the workspace index. This is
    // also driven end-to-end by `in_repo_dep_outside_workspace_is_e1233` in the
    // Provider unit tests (a real sparse fetch against a local git repo).
    assert_eq!(
        ProviderError::MemberOutsideWorkspace("ghost".to_string()).code(),
        Some("E1233")
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
// I5: the committed monorepo example evaluates and addresses its members
// (examples/features/packages/monorepo)
// ──────────────────────────────────────────────

#[test]
fn committed_monorepo_example_indexes_and_addresses_members() {
    use jetpack::RefSpec::{classify_with_workspace, Source, SourceTable, WorkspaceIndex};
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/features/packages/monorepo");
    let src = std::fs::read_to_string(dir.join("workspace.jet"))
        .expect("committed monorepo example must have a workspace.jet");

    // The `find("./packages")` index discovers both members.
    let plan = WorkspaceFile::evaluate(&src, &dir).expect("example workspace must evaluate clean");
    let mut names: Vec<&str> = plan.members.iter().map(|m| m.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["hello", "ranker"], "members: {:?}", plan.members);

    // Build the queryable index and address members two ways (Slice B).
    let index = WorkspaceIndex::from_members(
        plan.members
            .iter()
            .map(|m| (m.name.clone(), m.path.clone())),
    );
    // Bare form.
    let hello = classify_with_workspace("hello", &SourceTable::empty(), &index)
        .expect("bare `hello` must resolve");
    assert_eq!(hello.source, Source::Path);
    assert!(hello.package.ends_with("hello"), "got {}", hello.package);
    // Path form — the stored relative path of the ranker member.
    let ranker_path = plan
        .members
        .iter()
        .find(|m| m.name == "ranker")
        .map(|m| m.path.trim_start_matches("./").to_string())
        .unwrap();
    let ranker = classify_with_workspace(&ranker_path, &SourceTable::empty(), &index)
        .expect("path form must resolve");
    assert!(ranker.package.ends_with("ranker"), "got {}", ranker.package);
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

// ──────────────────────────────────────────────
// E1239 — duplicate `module workspace` declarations (D-JPK-FILENAME2)
// ──────────────────────────────────────────────

#[test]
fn e1239_two_discovered_workspace_declarations() {
    use jetpack::WorkspaceFile;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ws-e1239-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.jet"), "module workspace { members: [] }\n").unwrap();
    std::fs::write(dir.join("b.jet"), "module workspace { members: [] }\n").unwrap();
    let d = WorkspaceFile::load(&dir)
        .expect("workspace files present")
        .expect_err("duplicate workspace declarations");
    assert_eq!(d.code, "E1239");
    assert!(d.what.contains("a.jet") && d.what.contains("b.jet"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discovery_probe_skips_deep_non_workspace_jet_without_stack_overflow() {
    use jetpack::WorkspaceFile;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ws-deep-probe-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    // Nested `=>` deep enough that a full parse blows the default ~2MiB
    // test stack — the declaration probe must stay token-cheap.
    let mut deep = String::from("fn run() {\n    x :: ");
    for _ in 0..400 {
        deep.push_str("(() => ");
    }
    deep.push('1');
    for _ in 0..400 {
        deep.push(')');
    }
    deep.push_str("\n}\n");
    std::fs::write(dir.join("noise.jet"), deep).unwrap();
    assert!(
        WorkspaceFile::load(&dir).is_none(),
        "deep non-workspace file must not count as a workspace index"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
