#[allow(dead_code, non_snake_case)]
mod Syntax {
    pub const FILE_EXT: &str = "jet";
}

#[allow(dead_code, non_snake_case)]
#[path = "crates/jet-foundation/src/SHA256.rs"]
mod SHA256;

const COMPILER_SOURCES: &[&str] = &[
    "Cargo.toml", "Cargo.lock", "build.rs", "Source",
    "crates/jet-foundation", "crates/jet-lexer", "crates/jet-parser", "crates/jet-net",
    "crates/jet-comptime", "crates/jet-sema", "crates/jet-codegen", "crates/jet-driver",
    "crates/jet-pkg-model", "crates/jet-env-model", "crates/jet-nix-eval", "crates/jetpack",
    "crates/jet-queries", "crates/jet-semindex", "crates/jet-impact", "crates/jet-jit",
    "crates/jet-rt", "crates/jet-repl", "crates/jet-debug", "crates/jet-cli",
    "crates/jet-canvas", "crates/jet-devserver", "docs/reference/core-library.md",
    "docs/spec/diagnostics.md", "examples/features/collections/wordcount.jet",
    "tests/fixtures/nix-compat/oracle.json",
];
const STDLIB_SOURCES: &[&str] = &["corelib", "crates/jet-foundation", "crates/jet-codegen/src/Prelude"];
const RUNNER_SOURCES: &[&str] = &[
    "corelib", "crates/jet-foundation", "crates/jet-net", "crates/jet-codegen",
    "crates/jet-jit", "crates/jet-rt",
];

fn main() {
    let target = std::env::var("TARGET").expect("Cargo always provides TARGET to build scripts");
    println!("cargo:rustc-env=JET_BUILD_TARGET={target}");
    let facts = build_facts().expect("compiler build facts must be readable");
    let compiler = semantic_id("jet.compiler.v2", COMPILER_SOURCES, &facts)
        .expect("compiler source identity must be readable");
    let stdlib = semantic_id("jet.stdlib.v2", STDLIB_SOURCES, &facts)
        .expect("stdlib source identity must be readable");
    let runner = semantic_id("jet.runner.v2", RUNNER_SOURCES, &facts)
        .expect("runner source identity must be readable");
    println!("cargo:rustc-env=JET_COMPILER_BUILD_ID={compiler}");
    println!("cargo:rustc-env=JET_STDLIB_BUILD_ID={stdlib}");
    println!("cargo:rustc-env=JET_RUNNER_BUILD_ID={runner}");
    let mut watched = COMPILER_SOURCES.iter().chain(STDLIB_SOURCES).chain(RUNNER_SOURCES).collect::<Vec<_>>();
    watched.sort();
    watched.dedup();
    for path in watched {
        println!("cargo:rerun-if-changed={path}");
    }
    for key in ["RUSTC", "TARGET", "HOST", "PROFILE", "OPT_LEVEL", "DEBUG", "CARGO_CFG_TARGET_FEATURE", "CARGO_ENCODED_RUSTFLAGS"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    for key in profile_override_keys() {
        println!("cargo:rerun-if-env-changed={key}");
    }
    if let Some(spec) = target_spec_path() {
        println!("cargo:rerun-if-changed={}", spec.display());
    }
}

/// Profile/toolchain env overrides that change compiler semantics without
/// touching any source input; each must feed the semantic IDs and re-trigger
/// this script. Cargo has no wildcard rerun-if-env-changed, so enumerate.
fn profile_override_keys() -> Vec<String> {
    let mut keys = vec!["RUSTC_WRAPPER".to_string(), "RUSTC_WORKSPACE_WRAPPER".to_string()];
    for profile in ["RELEASE", "DEV", "TEST", "BENCH"] {
        for setting in [
            "PANIC", "OVERFLOW_CHECKS", "DEBUG_ASSERTIONS", "LTO", "CODEGEN_UNITS",
            "OPT_LEVEL", "DEBUG", "STRIP", "INCREMENTAL",
        ] {
            keys.push(format!("CARGO_PROFILE_{profile}_{setting}"));
        }
    }
    keys
}

/// A TARGET naming a custom `.json` target spec carries semantics in the file
/// body, not the name.
fn target_spec_path() -> Option<std::path::PathBuf> {
    let target = std::env::var("TARGET").ok()?;
    let path = std::path::PathBuf::from(&target);
    (target.ends_with(".json") && path.is_file()).then_some(path)
}

fn build_facts() -> std::io::Result<Vec<(String, String)>> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = std::process::Command::new(rustc).args(["--version", "--verbose"]).output()?;
    if !output.status.success() {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "rustc --version --verbose failed"));
    }
    let mut facts = vec![("rustc".into(), String::from_utf8_lossy(&output.stdout).into_owned())];
    for key in ["TARGET", "HOST", "PROFILE", "OPT_LEVEL", "DEBUG", "CARGO_CFG_TARGET_FEATURE", "CARGO_ENCODED_RUSTFLAGS"] {
        facts.push((key.into(), std::env::var(key).unwrap_or_default()));
    }
    facts.extend(std::env::vars().filter(|(key, _)| key.starts_with("CARGO_FEATURE_")));
    for key in profile_override_keys() {
        let value = std::env::var(&key).unwrap_or_default();
        facts.push((key, value));
    }
    if let Some(spec) = target_spec_path() {
        facts.push(("TARGET_SPEC_JSON".into(), std::fs::read_to_string(spec)?));
    }
    facts.sort();
    Ok(facts)
}

fn semantic_id(domain: &str, roots: &[&str], facts: &[(String, String)]) -> std::io::Result<String> {
    let root = std::path::Path::new(".");
    let mut files = Vec::new();
    for path in roots {
        collect_files(root.join(path), &mut files)?;
    }
    files.sort();
    files.dedup();

    let mut hash = SHA256::StreamingSha256::new();
    hash.update(domain.as_bytes());
    hash.update(&[0]);
    for (key, value) in facts {
        hash.update(&(key.len() as u64).to_be_bytes());
        hash.update(key.as_bytes());
        hash.update(&(value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    for path in files {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let bytes = std::fs::read(&path)?;
        hash.update(&(relative.len() as u64).to_be_bytes());
        hash.update(relative.as_bytes());
        hash.update(&(bytes.len() as u64).to_be_bytes());
        hash.update(&bytes);
    }
    Ok(hash.finalize().iter().fold(String::with_capacity(64), |mut text, byte| {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
        text
    }))
}

fn collect_files(path: std::path::PathBuf, files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(&path)?;
    if metadata.is_file() {
        files.push(path);
    } else if metadata.is_dir() {
        if path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
            matches!(name, ".git" | "target" | "tests" | "test" | "testdata" | "fixtures" | "examples" | "benches" | "expected")
        }) {
            return Ok(());
        }
        let mut children = std::fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            collect_files(child.path(), files)?;
        }
    }
    Ok(())
}
