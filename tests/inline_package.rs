//! D-ECO-INLINEPACKAGE1=A: one-file Package context must reach every hosted
//! execution tier without a synthetic package.jet.

use std::fs;
mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

#[test]
fn inline_package_example_matches_release_default_and_interpreter() {
    tir_support::assert_example_cli_tiers_agree(
        "packages/inline_package",
        include_str!("../examples/features/expected/packages/inline_package.out"),
    );
}

#[test]
fn inline_package_keeps_pub_package_visibility() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("jet-inline-package-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = r#"package {
    name: "inline-visibility"
}

pub(package) fn secret() String -> {
    return "ok"
}

fn run() {
    print(secret())
}
"#;
    let path = root.join("run.jet");
    fs::write(&path, source).unwrap();
    let diagnostics = jet::check_with_path(&path.to_string_lossy());
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "inline Package must preserve pub(package): {diagnostics:?}"
    );
    let _ = fs::remove_dir_all(root);
}
#[test]
fn inline_package_conflicts_with_manifest_for_every_entry() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "jet-inline-package-conflict-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("run.jet"),
        "package { name: \"inline\" }\nfn run() {}\n",
    )
    .unwrap();
    fs::write(root.join("other.jet"), "fn run() {}\n").unwrap();
    fs::write(root.join("package.jet"), "name: \"manifest\"\n").unwrap();

    assert_eq!(
        jet::Loader::find_package_root_checked(&root).unwrap(),
        Some(root.clone())
    );
    let load_diagnostics = jet::Loader::load_entry(root.join("other.jet").to_str().unwrap())
        .expect_err("an inline Package and package.jet must not coexist");
    assert!(
        load_diagnostics.iter().any(|diagnostic| diagnostic.code == "E1363"),
        "{load_diagnostics:#?}"
    );
    let facts_diagnostics = jet::Loader::package_facts_for_entry(&root.join("other.jet"))
        .expect_err("facts lookup must reject the same conflicting carriers");
    assert!(
        facts_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E1363"),
        "{facts_diagnostics:#?}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn nested_manifest_stops_outer_inline_package_discovery() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let outer = std::env::temp_dir().join(format!(
        "jet-inline-package-boundary-{}-{stamp}",
        std::process::id()
    ));
    let inner = outer.join("inner");
    fs::create_dir_all(&inner).unwrap();
    fs::write(
        outer.join("run.jet"),
        "package { name: \"outer\" }\nfn run() {}\n",
    )
    .unwrap();
    fs::write(inner.join("package.jet"), "name: \"inner\"\n").unwrap();

    assert_eq!(
        jet::Loader::find_inline_package_root_checked(&inner).unwrap(),
        None
    );
    let _ = fs::remove_dir_all(outer);
}
#[test]
fn inline_and_extracted_package_keep_facts_graph_and_authority_identity() {
    let body = r#"
name: "inline-parity"
version: "1.2.3"
jet: ">=0.1.0"
edition: "2026"
description: "one source of package facts"
license: "MIT"
repository: "https://example.test/inline-parity"
deps: { dep: ./deps/dep }
outputs: { app: .Executable{ name: "app", entry: run } }
defaults: { run: app }
authority: { holds: { allow: [IO] } }
policy: { unsafe: .Deny }
"#;
    let program = r#"use dep

fn run() {
    print(dep.value())
}
"#;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "jet-inline-package-parity-{}-{stamp}",
        std::process::id()
    ));
    let inline_root = base.join("inline");
    let extracted_root = base.join("extracted");
    for root in [&inline_root, &extracted_root] {
        fs::create_dir_all(root.join("deps/dep")).unwrap();
        fs::write(
            root.join("deps/dep/package.jet"),
            "name: \"dep\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("deps/dep/dep.jet"),
            "pub fn value() String -> \"dependency\"\n",
        )
        .unwrap();
    }
    let inline_entry = inline_root.join("run.jet");
    let inline_source = format!("package {{{body}}}\n{program}");
    let inline_block =
        jet::Package::extract_inline_package(&inline_source)
            .unwrap()
            .expect("inline source must carry one Package block");
    assert_eq!(
        inline_block.body(&inline_source),
        body,
        "extraction must copy the Package body byte-for-byte"
    );
    fs::write(&inline_entry, inline_source).unwrap();
    fs::write(extracted_root.join("package.jet"), body).unwrap();
    let extracted_entry = extracted_root.join("run.jet");
    fs::write(&extracted_entry, program).unwrap();

    let inline_facts = jet::Loader::package_facts_for_entry(&inline_entry)
        .unwrap()
        .expect("inline source must provide Package facts");
    let extracted_facts = jet::Loader::package_facts_for_entry(&extracted_entry)
        .unwrap()
        .expect("package.jet must provide Package facts");
    assert_eq!(
        inline_facts.semantic_digest(),
        extracted_facts.semantic_digest(),
        "inline and extracted forms must share one typed Package identity"
    );
    assert_eq!(
        jet::Package::to_manifest(&inline_facts, body).unwrap(),
        jet::Package::to_manifest(&extracted_facts, body).unwrap(),
        "inline and extracted forms must project to one resolver manifest"
    );

    let inline_bundle =
        jet::Loader::load_entry(&inline_entry.to_string_lossy()).expect("inline graph loads");
    let extracted_bundle =
        jet::Loader::load_entry(&extracted_entry.to_string_lossy()).expect("extracted graph loads");
    assert_eq!(inline_bundle.build_facts, extracted_bundle.build_facts);
    assert_eq!(inline_bundle.comptime_inputs, extracted_bundle.comptime_inputs);
    let mut inline_guarantees = inline_bundle.package_guarantees.clone();
    let mut extracted_guarantees = extracted_bundle.package_guarantees.clone();
    let inline_authority_suffix = format!("{} authority.holds", inline_entry.display());
    let extracted_authority_suffix =
        format!("{} authority.holds", extracted_root.join("package.jet").display());
    assert!(
        inline_guarantees
            .application_authority
            .authority
            .ends_with(&inline_authority_suffix),
        "inline authority provenance changed: {}",
        inline_guarantees.application_authority.authority
    );
    assert!(
        extracted_guarantees
            .application_authority
            .authority
            .ends_with(&extracted_authority_suffix),
        "extracted authority provenance changed: {}",
        extracted_guarantees.application_authority.authority
    );
    inline_guarantees.application_authority.authority = "<entry> authority.holds".to_string();
    extracted_guarantees.application_authority.authority = "<entry> authority.holds".to_string();
    assert_eq!(inline_guarantees, extracted_guarantees);
    assert_eq!(inline_bundle.program_allocator, extracted_bundle.program_allocator);
    assert_eq!(inline_bundle.edition, extracted_bundle.edition);
    assert_eq!(
        inline_bundle.dep_roots.keys().collect::<Vec<_>>(),
        extracted_bundle.dep_roots.keys().collect::<Vec<_>>(),
        "dependency aliases must resolve identically"
    );
    assert_eq!(
        inline_bundle
            .dep_roots
            .get("dep")
            .and_then(|path| path.file_name()),
        extracted_bundle
            .dep_roots
            .get("dep")
            .and_then(|path| path.file_name()),
        "dependency source roots must resolve from each source root"
    );
    let _ = fs::remove_dir_all(base);
}
