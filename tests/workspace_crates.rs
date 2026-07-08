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
