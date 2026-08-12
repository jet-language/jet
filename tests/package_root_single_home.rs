//! Card #1716: every package-root lookup routes through Loader's one walk.
//! The guard keeps a second filename list or parent walk from returning.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

const ROOT_HOME: &str = "crates/jet-driver/src/Loader.rs";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

fn homes_for(needle: &str) -> Vec<String> {
    let root = root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("Source"), &mut files);
    collect_rs_files(&root.join("crates"), &mut files);
    files
        .into_iter()
        .filter_map(|file| {
            let text = fs::read_to_string(&file).ok()?;
            text.contains(needle).then(|| {
                file.strip_prefix(&root)
                    .unwrap_or(&file)
                    .display()
                    .to_string()
            })
        })
        .collect()
}

#[test]
fn manifest_root_fn_lives_only_in_loader() {
    let homes = homes_for("pub fn find_manifest_root");
    assert_eq!(
        homes,
        vec![ROOT_HOME.to_string()],
        "package-root walk must live only in {ROOT_HOME}, found in: {homes:?}"
    );
}

#[test]
fn every_root_resolver_entry_point_calls_loader() {
    let root = root();
    for relative in [
        "Source/LSP/Server.rs",
        "Source/lib.rs",
        "crates/jet-driver/src/Driver/mod.rs",
        "crates/jet-semindex/src/lib.rs",
        "crates/jet-devserver/src/WatchService.rs",
        "crates/jet-devserver/src/Canvas/project_scan.rs",
        "crates/jet-devserver/src/Canvas/query_actions.rs",
    ] {
        let text = fs::read_to_string(root.join(relative)).unwrap();
        assert!(
            text.contains("Loader::find_manifest_root"),
            "{relative} must call Loader::find_manifest_root"
        );
    }
    for forbidden in [
        "project_root_marker",
        "find_canonical_package_root",
        "find_package_root",
        "[\"package.jet\", \"Jet.toml\", \"jet.toml\", \".git\"]",
    ] {
        assert!(
            homes_for(forbidden).is_empty(),
            "private package-root resolver remains: {forbidden}"
        );
    }
}

#[test]
fn nested_entry_points_share_root_and_stale_diagnostic() {
    let fixture = std::env::temp_dir().join(format!("jet-package-root-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&fixture);
    fs::create_dir_all(fixture.join("src/nested")).unwrap();
    fs::write(
        fixture.join("package.jet"),
        "name: \"root\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    let entry = fixture.join("src/nested/main.jet");
    fs::write(&entry, "fn run() {}\n").unwrap();
    let expected = fs::canonicalize(&fixture).unwrap();

    assert_eq!(
        jet::Loader::find_manifest_root(entry.parent().unwrap()),
        Some(expected.clone())
    );
    let facts = jet_semindex::package_facts_for_entry(&entry)
        .unwrap()
        .expect("semantic index must use package root");
    assert_eq!(facts.name, "root");

    let graph = jet_devserver::WatchGraph::from_entry(&entry, &[]).unwrap();
    assert!(graph.watched_paths().contains(&expected.join("package.jet")));

    let project = jet_devserver::Canvas::project_json_for_entry(&entry);
    assert!(project.contains(&format!(
        "\"project_root\":\"{}\"",
        expected.display()
    )));

    let clean_lsp = jet::LSP::check_document(&entry.display().to_string(), "fn run() {}\n");
    assert!(
        clean_lsp.iter().all(|diagnostic| diagnostic.code != "E1226"),
        "canonical package must not emit stale-manifest diagnostic: {clean_lsp:?}"
    );

    fs::remove_file(expected.join("package.jet")).unwrap();
    fs::write(expected.join("jet.toml"), "").unwrap();
    let (_, stale) = jet::Loader::stale_manifest_name_diagnostic(entry.parent().unwrap())
        .expect("stale manifest diagnostic");
    assert_eq!(stale.code, "E1226");

    let stale_lsp = jet::LSP::check_document(&entry.display().to_string(), "fn run() {}\n");
    assert!(
        stale_lsp.iter().any(|diagnostic| diagnostic.code == "E1226"),
        "LSP must receive Loader's stale-manifest diagnostic: {stale_lsp:?}"
    );
    let stale_watch = match jet::Interpreter::dev_iteration(&entry.display().to_string(), false, true)
    {
        jet::Interpreter::RunOutcome::Problems(diagnostics) => diagnostics,
        outcome => panic!("watch entrypoint unexpectedly ran: {outcome:?}"),
    };
    assert!(
        stale_watch.iter().any(|diagnostic| diagnostic.code == "E1226"),
        "watch entrypoint must receive Loader's stale-manifest diagnostic: {stale_watch:?}"
    );
    let stale_canvas = jet_devserver::Canvas::graph_json_for_file(&entry).unwrap_err();
    assert!(
        stale_canvas.iter().any(|diagnostic| diagnostic.code == "E1226"),
        "Canvas must receive Loader's stale-manifest diagnostic: {stale_canvas:?}"
    );
    let stale_build = jet::compile_programmable_build(&entry.display().to_string(), &[])
        .expect_err("policy defaults must reject a retired manifest name");
    assert!(
        stale_build.iter().any(|diagnostic| diagnostic.code == "E1226"),
        "policy defaults must receive Loader's stale-manifest diagnostic: {stale_build:?}"
    );

    let _ = fs::remove_dir_all(&fixture);
}
