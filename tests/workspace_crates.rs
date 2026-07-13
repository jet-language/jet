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
            "jet-pkg-model",
            "jet-sema",
        ],
    );
    // Card #367 / D-PRODUCT-SPLIT1=C: the shared read-only package/config
    // data model (manifest/lock/store-listing/ref/FFI-binding/script-dep
    // parsing), now also §6 structural `Merge` and the `BuildRecipe` data
    // shape (slice 4). Depends only inward toward the compiler's own checker
    // (jet-sema, transitively jet-comptime/jet-parser/jet-lexer/jet-
    // foundation) — never toward `jetpack`'s provider/network/shell engine.
    assert_deps("crates/jet-pkg-model/Cargo.toml", &["jet-sema"]);
    // Card #367 / D-PRODUCT-SPLIT1=C slice 4: the shared pure plan model
    // (`ModuleEval` + plan `Types`) — L2 between `jet-pkg-model` (L1 data)
    // and the two realizers (jetpack env-runtime + JetOS realization, L3).
    // No provider/store/network/shell dep; both realizers depend down on
    // this crate instead of sharing it by living in one engine crate.
    assert_deps(
        "crates/jet-env-model/Cargo.toml",
        &["jet-codegen", "jet-pkg-model"],
    );
    assert_deps(
        "crates/jetpack/Cargo.toml",
        &[
            "jet-codegen",
            "jet-comptime",
            "jet-env-model",
            "jet-foundation",
            "jet-lexer",
            "jet-parser",
            "jet-pkg-model",
            "jet-sema",
        ],
    );
    assert_deps("crates/jet-queries/Cargo.toml", &[]);
    // Runtime values reuse foundation's compiler-owned, std-only BigInt value;
    // direction remains inward toward the dependency-free foundation layer.
    assert_deps("crates/jet-rt/Cargo.toml", &["jet-foundation"]);
    assert_deps("crates/jet-net/Cargo.toml", &[]);
    assert_deps(
        "crates/jet-semindex/Cargo.toml",
        &["jet-driver", "jet-foundation", "jet-sema"],
    );
    assert_deps("crates/jet-impact/Cargo.toml", &["jet-semindex"]);
    // D-ARCH-SOURCE1=A: interactive shell owns behavior, depending only
    // inward on the compiler driver and shared semantic index.
    assert_deps(
        "crates/jet-repl/Cargo.toml",
        &["jet-driver", "jet-foundation", "jet-semindex"],
    );
    // D-ARCH-SOURCE1=A: source debugger owns its full product behavior and
    // depends only inward on compiler semantics plus dependency-free shared
    // JSON/exit policy.
    assert_deps(
        "crates/jet-debug/Cargo.toml",
        &["jet-driver", "jet-foundation"],
    );
    assert_deps(
        "crates/jet-jit/Cargo.toml",
        &["jet-codegen", "jet-foundation", "jet-rt"],
    );
    // Card #367 / D-PRODUCT-SPLIT1=C slice 2: `jetos` is its own crate/binary
    // boundary now (was a root-package shim). It still dispatches through
    // `jetpack`'s `os` verb until the JetOS realization engine splits out of
    // `jetpack` (slice 4) — that dependency is expected, not debt to shrink.
    assert_deps("crates/jetos/Cargo.toml", &["jetpack"]);
    assert_deps(
        "Cargo.toml",
        &[
            "jet-debug",
            "jet-driver",
            "jet-env-model",
            "jet-foundation",
            "jet-impact",
            "jet-jit",
            "jet-queries",
            "jet-repl",
            "jet-semindex",
            "jetpack",
        ],
    );
}

#[test]
fn jetpack_dependency_debt_is_explicit_until_product_split() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Card #367 / D-PRODUCT-SPLIT1=C slice 1: `jet-driver` moved off
    // `jetpack` onto `jet-pkg-model` (the read-only data model), so it left
    // this debt list. Slice 3 moved the root package's read-only-model
    // touch points (`PackageManifest`/`Manifest`/`ScriptDeps`/`Lock`/`CBind`/
    // `CFFI`/`FFI`/`EffectBudget`/`LintPolicy`/hangar-listing `Store`) off
    // `jetpack` too, onto the same `jet-pkg-model` seam via `jet-driver`'s
    // re-export — but the root package still bundles `jetpack` itself for
    // its remaining genuine engine calls (`Overlay`, `WorkspaceFile`,
    // `JetPin`, `ScriptLock`, `Discovery`) and `jetpack help`/CLI dispatch,
    // so this row isn't debt to shrink to zero — only to keep narrow.
    // `ModuleEval` left this list in slice 4: it now lives in `jet-env-model`
    // (a direct root-package dep), not reached through `jetpack`. `jetos`
    // (slice 2) is expected to depend on `jetpack` until a later card
    // physically relocates the JetOS realization engine out of it (open
    // scope gate, not this slice — see docs/plans/epoch-3/product-split-slice4.md).
    let allowed = ["Cargo.toml", "crates/jetos/Cargo.toml"];
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
        "crates/jetos/src/main.rs",
        "Source/Canvas/project_scan.rs",
        "Source/Canvas/project_transactions.rs",
        "Source/Canvas/schema_api.rs",
        "Source/LSP/Completion.rs",
        "Source/LSP/Server.rs",
        "Source/lib.rs",
        // Card #367 / D-PRODUCT-SPLIT1=C slice 3: was always coupled through
        // `jet::Jetpack::…` (the bin target's path to this crate's `pub use
        // jetpack as Jetpack` re-export), which this scan's `jetpack::`/
        // `crate::Jetpack` patterns never matched — a detection blind spot,
        // not new debt. Now that the alias is gone, main.rs's remaining
        // genuine-engine calls (`WorkspaceFile`, `JetPin`, `ScriptLock`) are
        // direct `jetpack::…` and honestly tracked here. `PackageManifest`/
        // `EffectBudget`/`LintPolicy`/`ScriptDeps`/`Store` usages moved to
        // the shared model (`jet::…`) and left this file entirely.
        "Source/main.rs",
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
        // Match the crate token, not internal modules such as
        // `Syntax::jetpack_config`.
        if code_text.contains("crate::Jetpack")
            || code_text.lines().any(|line| {
                matches!(line.trim(), "use jetpack;" | "pub use jetpack;")
            })
            || code_text.contains("pub use jetpack as ")
            || code_text.contains("use jetpack as ")
            || code_text.contains("jetpack::")
        {
            let rel = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            // The boundary test audits consumers, not Jetpack's own source or
            // embedded test fixtures.
            if rel.contains("/tests/") || rel.starts_with("crates/jetpack/") {
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
