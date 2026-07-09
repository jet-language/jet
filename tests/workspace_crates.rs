use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn manifest_deps(manifest: &str) -> BTreeSet<String> {
    let mut deps = BTreeSet::new();
    let mut in_deps = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, _)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.starts_with("jet-") || name == "jetpack" {
            deps.insert(name.to_string());
        }
    }
    deps
}

fn assert_deps(path: &str, allowed: &[&str]) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join(path)).unwrap();
    let actual = manifest_deps(&manifest);
    let allowed = allowed
        .iter()
        .map(|dep| dep.to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, allowed, "{path} Jet path-dependency drift");
}

fn repo_files_with_suffix(dir: &str, suffix: &str) -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    let mut stack = vec![root.join(dir)];
    while let Some(path) = stack.pop() {
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            for entry in fs::read_dir(&path).unwrap() {
                let entry = entry.unwrap().path();
                let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "target" || name == ".git" {
                    continue;
                }
                stack.push(entry);
            }
        } else if path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .ends_with(suffix)
        {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[test]
fn workspace_crates_keep_declared_dependency_direction() {
    assert_deps("crates/jet-foundation/Cargo.toml", &[]);
    assert_deps("crates/jet-lexer/Cargo.toml", &["jet-foundation"]);
    assert_deps(
        "crates/jet-parser/Cargo.toml",
        &["jet-foundation", "jet-lexer"],
    );
    assert_deps(
        "crates/jet-comptime/Cargo.toml",
        &["jet-foundation", "jet-net"],
    );
    assert_deps(
        "crates/jet-sema/Cargo.toml",
        &[
            "jet-comptime",
            "jet-foundation",
            "jet-lexer",
            "jet-parser",
        ],
    );
    assert_deps(
        "crates/jet-codegen/Cargo.toml",
        &["jet-foundation", "jet-sema"],
    );
    assert_deps(
        "crates/jet-driver/Cargo.toml",
        &[
            "jet-codegen",
            "jet-comptime",
            "jet-foundation",
            "jet-lexer",
            "jet-parser",
            "jet-sema",
            "jetpack",
        ],
    );
    assert_deps(
        "crates/jetpack/Cargo.toml",
        &[
            "jet-codegen",
            "jet-comptime",
            "jet-foundation",
            "jet-lexer",
            "jet-parser",
            "jet-sema",
        ],
    );
    assert_deps("crates/jet-queries/Cargo.toml", &[]);
    assert_deps("crates/jet-rt/Cargo.toml", &[]);
    assert_deps("crates/jet-net/Cargo.toml", &[]);
    assert_deps(
        "crates/jet-semindex/Cargo.toml",
        &["jet-driver", "jet-foundation", "jet-sema"],
    );
    assert_deps("crates/jet-impact/Cargo.toml", &["jet-semindex"]);
    assert_deps(
        "crates/jet-jit/Cargo.toml",
        &["jet-codegen", "jet-foundation", "jet-rt"],
    );
    assert_deps(
        "Cargo.toml",
        &[
            "jet-driver",
            "jet-foundation",
            "jet-impact",
            "jet-jit",
            "jet-queries",
            "jet-semindex",
            "jetpack",
        ],
    );
}

#[test]
fn jetpack_dependency_debt_is_explicit_until_product_split() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let allowed = ["Cargo.toml", "crates/jet-driver/Cargo.toml"];
    let mut actual = Vec::new();
    for manifest in repo_files_with_suffix("crates", "Cargo.toml")
        .into_iter()
        .chain([root.join("Cargo.toml")])
    {
        let rel = manifest
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if manifest_deps(&fs::read_to_string(&manifest).unwrap()).contains("jetpack") {
            actual.push(rel);
        }
    }
    actual.sort();
    assert_eq!(
        actual, allowed,
        "new jetpack crate dependency added outside the current product-split debt list"
    );
}

#[test]
fn direct_jetpack_imports_stay_behind_known_boundaries() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let allowed = [
        "Source/Bin/JetOS.rs",
        "Source/Canvas/project_scan.rs",
        "Source/Canvas/project_transactions.rs",
        "Source/Canvas/query_actions.rs",
        "Source/Canvas/schema_api.rs",
        "Source/LSP/Completion.rs",
        "Source/LSP/Server.rs",
        "Source/lib.rs",
        "crates/jet-driver/src/Loader.rs",
        "crates/jet-driver/src/lib.rs",
    ]
    .into_iter()
    .map(String::from)
    .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    for file in repo_files_with_suffix("Source", ".rs")
        .into_iter()
        .chain(repo_files_with_suffix("crates", ".rs"))
    {
        let text = fs::read_to_string(&file).unwrap();
        let code_text = text
            .lines()
            .map(|line| line.split_once("//").map(|(code, _)| code).unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");
        if code_text.contains("crate::Jetpack")
            || code_text.contains("pub use jetpack")
            || code_text.contains("use jetpack")
            || code_text.contains("jetpack::")
        {
            let rel = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel.contains("/tests/") || code_text.contains("use jetpack as pkg;") {
                continue;
            }
            actual.insert(rel);
        }
    }
    assert_eq!(
        actual, allowed,
        "new direct Jetpack coupling added; route it through a product boundary first"
    );
}
