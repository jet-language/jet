//! D-ECO-INLINEPACKAGE1=A: one-file Package context must reach every hosted
//! execution tier without a synthetic package.jet.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
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

static INLINE_SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

fn inline_scratch(label: &str) -> PathBuf {
    let base = PathBuf::from(
        std::env::var_os("HOME").expect("inline package tests require HOME"),
    )
    .join(".cache/jet-test-scratch/inline-package");
    let sequence = INLINE_SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let root = base.join(format!("{label}-{}-{sequence}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    // Bound each scratch project so unrelated package metadata above HOME's
    // test root cannot become its authority.
    fs::write(
        root.join("workspace.jet"),
        "module workspace { members: [] }\n",
    )
    .unwrap();
    root
}

fn remove_inline_scratch(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

fn run_inline_cli(root: &Path, args: &[&str]) -> (i32, String, String) {
    let sequence = INLINE_SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(root)
        .env("JET_CACHE_DIR", root.join(format!(".jet-cache-{sequence}")))
        .env("JET_PACKAGE_STORE_DIR", root.join(format!(".jet-store-{sequence}")))
        .env("JETPACK_ROOT", root.join(format!(".jetpack-{sequence}")))
        .env("JETPACK_ENV", "1")
        .env("NO_COLOR", "1")
        .output()
        .expect("run Jet inline-package command");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn inline_package_canonical_surface_has_one_generated_shape() {
    assert_eq!(jet::Syntax::INLINE_PACKAGE_DECL, "package");
    assert_eq!(jet::Syntax::PACKAGE_FILE, "package.jet");

    let syntax = include_str!("../crates/jet-foundation/src/Syntax.rs");
    assert!(syntax.contains("D-ECO-INLINEPACKAGE1=A"));
    let package_files = include_str!("../crates/jet-foundation/src/Syntax/package_files.rs");
    assert!(package_files.contains("pub const INLINE_PACKAGE_DECL: &str = \"package\";"));

    let decision = include_str!("../docs/spec/syntax-decisions.md");
    assert!(decision.contains("D-ECO-INLINEPACKAGE1=A — one optional leading inline"));

    let tree_sitter = include_str!("../editors/tree-sitter/grammar.js");
    assert!(tree_sitter.contains(
        "source_file: ($) => seq(optional($.inline_package), repeat($._item))"
    ));
    assert!(tree_sitter.contains("inline_package: ($) => seq(\"package\", $.record_literal)"));

    let vscode = include_str!("../editors/vscode/syntaxes/jet.tmLanguage.json");
    assert!(vscode.contains("D-ECO-INLINEPACKAGE1=A"));
    let textmate = include_str!("../editors/jet.tmGrammar");
    assert!(textmate.contains("D-ECO-INLINEPACKAGE1=A"));
    let zed = include_str!("../editors/zed/languages/jet/highlights.scm");
    assert!(zed.contains("(inline_package \"package\" @keyword)"));
}

#[test]
fn inline_package_mask_preserves_parser_offsets_and_code() {
    let source = r#"#!/usr/bin/env jet
// leading trivia remains outside the carrier
package {
    name: "mask"
}

fn run() {
    print("ok")
}
"#;
    let block = jet::Package::extract_inline_package(source)
        .unwrap()
        .expect("leading inline Package must be found");
    assert_eq!(block.source(source), "package {\n    name: \"mask\"\n}");

    let (masked, found) = jet::Package::mask_inline_package_source(source).unwrap();
    assert_eq!(found.unwrap().span, block.span);
    assert_eq!(masked.len(), source.len());
    assert_eq!(&masked[block.span.end..], &source[block.span.end..]);
    assert!(
        masked[block.span.start..block.span.end]
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\n' | b'\r')),
        "the parser-facing carrier mask must retain only whitespace and line endings"
    );
}

#[test]
fn inline_package_path_escape_fails_before_state_mutation_with_provenance() {
    let root = inline_scratch("path-escape");
    let path = root.join("run.jet");
    let source = r#"package {
    name: "escape"
    configs: ["../outside-config.jet"]
}
fn run() {}
"#;
    let outside = root
        .parent()
        .unwrap()
        .join(format!("outside-config-{}-{}.jet", std::process::id(), INLINE_SCRATCH_SEQ.load(Ordering::Relaxed)));
    let outside_before = b"host-state\n";
    fs::write(&path, source).unwrap();
    fs::write(&outside, outside_before).unwrap();
    let source_before = fs::read(&path).unwrap();

    let diagnostics = jet::Loader::package_facts_for_entry(&path)
        .expect_err("a config path escaping the source root must be rejected");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E1362"),
        "expected registered inline-package diagnostic: {diagnostics:#?}"
    );
    assert_eq!(fs::read(&path).unwrap(), source_before);
    assert_eq!(fs::read(&outside).unwrap(), outside_before);

    let detailed = jet::Loader::load_entry_with_diagnostics(&path.display().to_string())
        .expect_err("the full loader must reject the same path escape");
    let failure = detailed
        .iter()
        .find(|entry| entry.diagnostic.code == "E1362")
        .expect("path escape must retain its registered diagnostic");
    assert_eq!(failure.file, path.display().to_string());
    assert_eq!(failure.source, source);
    assert!(failure.diagnostic.span.is_some());

    let _ = fs::remove_file(outside);
    remove_inline_scratch(&root);
}

#[cfg(unix)]
#[test]
fn inline_package_symlinked_metadata_is_rejected_without_hidden_host_state() {
    use std::os::unix::fs::symlink;

    let root = inline_scratch("symlink-metadata");
    let path = root.join("run.jet");
    let config = root.join("config.jet");
    let target = root.join("outside-config.jet");
    let source = r#"package {
    name: "symlink"
    configs: ["config.jet"]
}
fn run() {}
"#;
    fs::write(&path, source).unwrap();
    fs::write(&target, "description: \"host-state\"\n").unwrap();
    symlink(&target, &config).unwrap();

    let detailed = jet::Loader::load_entry_with_diagnostics(&path.display().to_string())
        .expect_err("symlinked package metadata must fail closed");
    let failure = detailed
        .iter()
        .find(|entry| matches!(entry.diagnostic.code.as_str(), "E1334" | "E1362"))
        .expect("authority symlink rejection must use a registered diagnostic");
    match failure.diagnostic.code.as_str() {
        "E1334" => {
            assert!(failure.file.ends_with("config.jet"), "wrong provenance: {}", failure.file);
            assert!(failure.source.is_empty());
        }
        "E1362" => {
            assert_eq!(failure.file, path.display().to_string());
            assert_eq!(failure.source, source);
            assert!(failure.diagnostic.span.is_some());
        }
        code => panic!("unexpected symlink diagnostic {code}"),
    }
    assert_eq!(fs::read_to_string(&target).unwrap(), "description: \"host-state\"\n");
    assert!(
        fs::symlink_metadata(&config)
            .unwrap()
            .file_type()
            .is_symlink(),
        "rejection must not replace the metadata link"
    );

    let _ = fs::remove_file(config);
    remove_inline_scratch(&root);
}

#[test]
fn inline_package_import_cycle_fails_with_cycle_diagnostic_and_file_provenance() {
    let root = inline_scratch("cycle");
    let entry = root.join("run.jet");
    let module = root.join("a.jet");
    fs::write(
        &entry,
        "package { name: \"cycle\" version: \"0.1.0\" }\nuse \"a\"\nfn run() {}\n",
    )
    .unwrap();
    fs::write(&module, "use \"run\"\nfn a() {}\n").unwrap();

    let detailed = jet::Loader::load_entry_with_diagnostics(&entry.display().to_string())
        .expect_err("an import cycle must fail before compilation");
    let failure = detailed
        .iter()
        .find(|entry| entry.diagnostic.code == "E0604")
        .expect("cycle must use the registered E0604 diagnostic");
    assert!(failure.file.ends_with("a.jet"), "cycle provenance lost: {}", failure.file);
    assert_eq!(failure.diagnostic.what, "these files import each other in a circle");
    assert!(failure.source.is_empty());
    assert_eq!(fs::read_to_string(&entry).unwrap(), "package { name: \"cycle\" version: \"0.1.0\" }\nuse \"a\"\nfn run() {}\n");
    assert_eq!(fs::read_to_string(&module).unwrap(), "use \"run\"\nfn a() {}\n");

    remove_inline_scratch(&root);
}

#[test]
fn inline_package_rejects_retired_manifest_metadata_without_mutation() {
    let root = inline_scratch("stale-manifest");
    let entry = root.join("run.jet");
    let stale = root.join("pkg.jet");
    let source = "package { name: \"inline\" version: \"0.1.0\" }\nfn run() {}\n";
    let stale_source = "name: \"stale\"\nversion: \"0.1.0\"\n";
    fs::write(&entry, source).unwrap();
    fs::write(&stale, stale_source).unwrap();

    let detailed = jet::Loader::load_entry_with_diagnostics(&entry.display().to_string())
        .expect_err("retired package metadata must not shadow inline Package facts");
    let failure = detailed
        .iter()
        .find(|entry| entry.diagnostic.code == "E1226")
        .expect("retired manifest must use registered E1226");
    assert_eq!(failure.file, entry.display().to_string());
    assert!(failure.diagnostic.what.contains("pkg.jet"));
    assert_eq!(fs::read_to_string(&entry).unwrap(), source);
    assert_eq!(fs::read_to_string(&stale).unwrap(), stale_source);

    remove_inline_scratch(&root);
}

#[test]
fn inline_package_rejects_ambiguous_manifest_metadata_before_loading() {
    let root = inline_scratch("ambiguous-manifest");
    let entry = root.join("run.jet");
    let package = root.join("package.jet");
    let stale = root.join("pkg.jet");
    let entry_source = "package { name: \"inline\" version: \"0.1.0\" }\nfn run() {}\n";
    let package_source = "name: \"manifest\"\nversion: \"0.1.0\"\n";
    let stale_source = "name: \"stale\"\nversion: \"0.1.0\"\n";
    fs::write(&entry, entry_source).unwrap();
    fs::write(&package, package_source).unwrap();
    fs::write(&stale, stale_source).unwrap();

    let detailed = jet::Loader::load_entry_with_diagnostics(&entry.display().to_string())
        .expect_err("two manifest names must fail before inline loading");
    let failure = detailed
        .iter()
        .find(|entry| entry.diagnostic.code == "E1239")
        .expect("ambiguous manifest names must use registered E1239");
    assert_eq!(failure.file, entry.display().to_string());
    assert_eq!(fs::read_to_string(&entry).unwrap(), entry_source);
    assert_eq!(fs::read_to_string(&package).unwrap(), package_source);
    assert_eq!(fs::read_to_string(&stale).unwrap(), stale_source);

    remove_inline_scratch(&root);
}

#[test]
fn inline_package_rejects_stale_lock_metadata_with_lock_provenance() {
    let root = inline_scratch("stale-lock");
    let entry = root.join("run.jet");
    let lock_dir = root.join(".jet");
    let lock = lock_dir.join("lock");
    let source = r#"package {
    name: "locked-inline"
    version: "0.1.0"
    deps: { dep: ./dep }
}
fn run() {}
"#;
    let lock_source = "stale lock bytes\n";
    fs::create_dir_all(&lock_dir).unwrap();
    fs::write(&entry, source).unwrap();
    fs::write(&lock, lock_source).unwrap();

    let detailed = jet::Loader::load_entry_with_diagnostics(&entry.display().to_string())
        .expect_err("an invalid lock must stop inline package loading");
    let failure = detailed
        .iter()
        .find(|entry| entry.diagnostic.code == "E1202")
        .expect("stale lock must use registered E1202");
    assert!(failure.file.ends_with(".jet/lock"), "lock provenance lost: {}", failure.file);
    assert_eq!(failure.source, lock_source);
    assert_eq!(fs::read_to_string(&entry).unwrap(), source);
    assert_eq!(fs::read_to_string(&lock).unwrap(), lock_source);

    remove_inline_scratch(&root);
}

#[test]
fn inline_package_rejects_duplicate_carriers_before_state_change() {
    let root = inline_scratch("duplicate");
    let entry = root.join("run.jet");
    let source = "package { name: \"first\" }\nfn run() {}\npackage { name: \"second\" }\n";
    fs::write(&entry, source).unwrap();

    let diagnostics = jet::Loader::package_facts_for_entry(&entry)
        .expect_err("duplicate inline Package carriers must fail");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E1361"),
        "expected registered duplicate diagnostic: {diagnostics:#?}"
    );
    assert_eq!(fs::read_to_string(&entry).unwrap(), source);

    remove_inline_scratch(&root);
}

#[test]
fn inline_package_extraction_matches_extracted_entry_across_build_and_run_tiers() {
    let source = include_str!("../examples/features/packages/inline_package.jet");
    let expected = include_str!("../examples/features/expected/packages/inline_package.out");
    let block = jet::Package::extract_inline_package(source)
        .unwrap()
        .expect("the shipped example must carry an inline Package");
    let body = block.body(source).to_owned();
    let extracted_program = source[block.span.end..].to_owned();
    assert!(extracted_program.contains("fn run"));

    let base = inline_scratch("tier-extraction");
    let inline_root = base.join("inline");
    let extracted_root = base.join("extracted");
    fs::create_dir_all(&inline_root).unwrap();
    fs::create_dir_all(&extracted_root).unwrap();
    fs::write(inline_root.join("run.jet"), source).unwrap();
    fs::write(extracted_root.join("package.jet"), &body).unwrap();
    fs::write(extracted_root.join("run.jet"), &extracted_program).unwrap();

    for root in [&inline_root, &extracted_root] {
        let (code, _stdout, stderr) = run_inline_cli(root, &["build", "run.jet"]);
        assert_eq!(code, 0, "AOT build failed in {}:\n{stderr}", root.display());
    }

    let modes: [&[&str]; 3] = [
        &["run", "--release", "run.jet"],
        &["run", "run.jet"],
        &["run", "--interpret", "run.jet"],
    ];
    let mut inline_results = Vec::new();
    let mut extracted_results = Vec::new();
    for args in modes {
        let inline = run_inline_cli(&inline_root, args);
        assert_eq!(inline.0, 0, "inline tier failed {:?}:\n{}", args, inline.2);
        assert_eq!(inline.1, expected, "inline tier output drifted for {:?}", args);
        let extracted = run_inline_cli(&extracted_root, args);
        assert_eq!(extracted.0, 0, "extracted tier failed {:?}:\n{}", args, extracted.2);
        assert_eq!(extracted.1, expected, "extracted tier output drifted for {:?}", args);
        inline_results.push((inline.0, inline.1));
        extracted_results.push((extracted.0, extracted.1));
    }
    assert_eq!(
        inline_results, extracted_results,
        "inline and extracted source must have identical tier receipts"
    );

    remove_inline_scratch(&base);
}
