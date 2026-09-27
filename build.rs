#[allow(dead_code, non_snake_case)]
mod Syntax {
    pub const FILE_EXT: &str = "jet";
}

#[allow(dead_code, non_snake_case)]
#[path = "crates/jet-foundation/src/SHA256.rs"]
mod SHA256;
#[path = "Compiler/Bootstrap/BuildIdentity.rs"]
mod BuildIdentity;

const STDLIB_SOURCES: &[&str] = &[
    "corelib",
    "crates/jet-foundation",
    "crates/jet-codegen/src/Prelude",
];
const RUNNER_SOURCES: &[&str] = &[
    "corelib",
    "crates/jet-foundation",
    "crates/jet-net",
    "crates/jet-codegen",
    "crates/jet-jit",
    "crates/jet-rt",
];

fn main() {
    let target = std::env::var("TARGET").expect("Cargo always provides TARGET to build scripts");
    println!("cargo:rustc-env=JET_BUILD_TARGET={target}");
    let facts = BuildIdentity::build_facts().expect("compiler build facts must be readable");
    let root = std::path::Path::new(".");
    let compiler = BuildIdentity::semantic_id(
        root,
        BuildIdentity::COMPILER_DOMAIN,
        BuildIdentity::COMPILER_SOURCES,
        &facts,
    )
    .expect("compiler source identity must be readable");
    let stdlib = BuildIdentity::semantic_id(root, "jet.stdlib.v2", STDLIB_SOURCES, &facts)
        .expect("stdlib source identity must be readable");
    let runner = BuildIdentity::semantic_id(root, "jet.runner.v2", RUNNER_SOURCES, &facts)
        .expect("runner source identity must be readable");
    println!("cargo:rustc-env=JET_COMPILER_BUILD_ID={compiler}");
    println!("cargo:rustc-env=JET_STDLIB_BUILD_ID={stdlib}");
    println!("cargo:rustc-env=JET_RUNNER_BUILD_ID={runner}");
    let mut watched = BuildIdentity::COMPILER_SOURCES
        .iter()
        .chain(STDLIB_SOURCES)
        .chain(RUNNER_SOURCES)
        .collect::<Vec<_>>();
    watched.sort();
    watched.dedup();
    for path in watched {
        println!("cargo:rerun-if-changed={path}");
    }
    for key in [
        "RUSTC",
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    for key in BuildIdentity::profile_override_keys() {
        println!("cargo:rerun-if-env-changed={key}");
    }
    if let Some(spec) = BuildIdentity::target_spec_path() {
        println!("cargo:rerun-if-changed={}", spec.display());
    }
}

