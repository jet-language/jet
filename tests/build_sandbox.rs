//! T1/T2 (card #99): the build-from-source sandbox contract (D-JPK-ADAPTER1) and
//! the pinned build toolchain (D-JPK-BUILDTOOL1). Drives the internal
//! `BuildRecipe` substrate through the public `jetpack` crate surface so the
//! diagnostic codes are covered by a `tests/` snapshot (invariant I4).

mod common;

use jetpack::Recipe::{self, BuildContext, BuildRecipe, BuildStep};
use jetpack::Toolchain;
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn target_sandbox_manifest_runs() {
    let source = r#"
name: "mathkit"
version: "0.1.0"
packages: {
    mathkit: sandbox { export: "mathkit" },
}
"#;
    let facts = jetpack::Package::PackageFacts::parse(source, "package.jet")
        .expect("sandbox target manifest should parse");
    assert!(matches!(
        facts.packages[0].targets.as_slice(),
        [jetpack::Package::Target::Plugin { export: Some(name) }] if name == "mathkit"
    ));
    let (rewritten, count) = jetpack::Package::rewrite_retired_targets(
        &source.replace("sandbox", "plugin"),
    );
    assert_eq!(count, 1);
    assert!(rewritten.contains("mathkit: sandbox"), "{rewritten}");
}

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "build-sandbox-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn sandbox_denies_ambient_network_e1236() {
    let base = scratch("net");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools: HashMap::new(),
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Fetch {
            url: "https://example.invalid/x.tar".to_string(),
            sha256: String::new(),
        }],
    };
    assert_eq!(Recipe::run(&recipe, &ctx, None).unwrap_err().code, "E1236");
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn sandbox_confines_output_e1237() {
    let base = scratch("confine");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("f"), "hi").unwrap();
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools: HashMap::new(),
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Install {
            src: "f".to_string(),
            dest: "../escape".to_string(),
        }],
    };
    assert_eq!(Recipe::run(&recipe, &ctx, None).unwrap_err().code, "E1237");
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn sandbox_tool_must_be_a_dep_e1238() {
    let base = scratch("tool");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools: HashMap::new(),
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "gcc".to_string(),
            args: vec![],
        }],
    };
    // `validate` (the `jet inspect audit` read path) flags it without executing.
    assert_eq!(Recipe::validate(&recipe, &ctx).unwrap_err().code, "E1238");
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn sandbox_tool_path_must_be_an_absolute_realized_artifact() {
    let base = scratch("relative-tool");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    let mut tools = HashMap::new();
    tools.insert("cc".to_string(), PathBuf::from("cc"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "cc".to_string(),
            args: vec![],
        }],
    };
    assert_eq!(Recipe::validate(&recipe, &ctx).unwrap_err().code, "E1238");
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(unix)]
#[test]
fn build_hook_does_not_inherit_host_credentials() {
    let base = scratch("clean-env");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();

    let secret_name = "JET_TEST_SECRET_DO_NOT_LEAK";
    let previous = std::env::var_os(secret_name);
    std::env::set_var(secret_name, "sentinel");
    let mut tools = HashMap::new();
    tools.insert("sh".to_string(), PathBuf::from("/bin/sh"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                format!(
                    "test \"${{{secret_name}:-}}\" = \"\" && printf clean > \"$JET_BUILD_OUTPUT/clean\""
                ),
            ],
        }],
    };
    let result = Recipe::run(&recipe, &ctx, None);
    match previous {
        Some(value) => std::env::set_var(secret_name, value),
        None => std::env::remove_var(secret_name),
    }
    result.unwrap();
    assert_eq!(std::fs::read_to_string(out.join("clean")).unwrap(), "clean");
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(unix)]
#[test]
fn failed_recipe_preserves_previous_output_and_removes_partial_stage() {
    let base = scratch("rollback");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("old"), "previous").unwrap();

    let mut tools = HashMap::new();
    tools.insert("sh".to_string(), PathBuf::from("/bin/sh"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![
            BuildStep::Exec {
                tool: "sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    "printf replacement > \"$JET_BUILD_OUTPUT/new\"".to_string(),
                ],
            },
            BuildStep::Exec {
                tool: "sh".to_string(),
                args: vec!["-c".to_string(), "false".to_string()],
            },
        ],
    };

    let error = Recipe::run(&recipe, &ctx, None).unwrap_err();
    assert_eq!(error.code, "E1238");
    assert_eq!(std::fs::read_to_string(out.join("old")).unwrap(), "previous");
    assert!(!out.join("new").exists());
    assert!(
        std::fs::read_dir(&base)
            .unwrap()
            .flatten()
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".out.jet-stage-")),
        "failed recipe left a partial staged output"
    );
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn toolchain_unavailable_is_e1240() {
    let d = Toolchain::e1240();
    assert_eq!(d.code, "E1240");
    assert!(d.fix.contains("jet update jet"));
}
