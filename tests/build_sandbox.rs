//! T1/T2 (card #99): the build-from-source sandbox contract (D-JPK-ADAPTER1) and
//! the pinned build toolchain (D-JPK-BUILDTOOL1). Drives the internal
//! `BuildRecipe` substrate through the public `jetpack` crate surface so the
//! diagnostic codes are covered by a `tests/` snapshot (invariant I4).

use jetpack::Recipe::{self, BuildContext, BuildRecipe, BuildStep};
use jetpack::Toolchain;
use std::collections::HashMap;
use std::path::PathBuf;

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
fn toolchain_unavailable_is_e1240() {
    let d = Toolchain::e1240();
    assert_eq!(d.code, "E1240");
    assert!(d.fix.contains("jet update jet"));
}
