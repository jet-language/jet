use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Codegen::MIRRust::{UNIT_BEGIN_MARKER, UNIT_END_MARKER};

/// Stable home of the per-artifact unit crates of a unit-split backend build.
const BOOTSTRAP_UNITS_RELATIVE: &str = ".cache/jet-luna/compiler-bootstrap/units";
/// Dev-profile override for the runtime and unit crates of a unit-split
/// backend build. Each unit crate is one very large rustc (the compiler's
/// Sema and Foundation units are over 100 MB of Rust each); full debuginfo
/// would multiply its codegen memory.
const BACKEND_UNIT_PROFILE: &str = "debug = \"line-tables-only\"\n";
/// Dev-profile rows for the repository crate whose host code dominates a
/// debug jetc0's run time: compiler-image restore and every
/// `execution_identity()` re-encode, verify and re-digest the whole compiler
/// MIR in jet-foundation (MIR image codec, `CanonicalWriter`), which at
/// opt-level 0 with debug assertions (`MIR_VERIFICATION` follows
/// `debug_assertions`) costs tens of minutes per run. Built once per change
/// of the crate; the emitted unit crates keep the fast dev profile.
const BACKEND_DEV_HOST_PROFILE: &str = "\n[profile.dev.package.jet-foundation]\nopt-level = 2\ndebug-assertions = false\n";
/// A unit crate longer than this is sharded (`shard_unit`) into crates of
/// at most about this size on average: rustc's memory and time grow with
/// the crate, and one rustc per shard is the backend build's unit of
/// parallelism. At 12 MB the compiler's unit split into 13 hot shards whose
/// slowest took 590 s at opt-level 1 after the 128 s facade, the whole
/// backend build's critical path; 4 MB shards spread the same code over
/// several times as many rustc runs. `JET_BACKEND_SHARD_BYTES` overrides it
/// (`backend_shard_bytes`).
const BACKEND_SHARD_BYTES: usize = 4 << 20;
/// Impl methods at least this long move out of a sharded unit's facade.
const BACKEND_DELEGATE_METHOD_BYTES: usize = 2048;
/// A moved function at least this long is cold (`shard_unit`) whatever its
/// kind: LLVM's optimization time and memory grow superlinearly within one
/// function (one function is one codegen unit). Measured at opt-level 2:
/// seven Jet functions of 200-455 KB (2.3 MB) build in 55 s at 1.1 GB, while
/// a shard holding 2-3 MB decoders and tables took 773 s at 9.1 GB.
/// `JET_BACKEND_COLD_FUNCTION_BYTES` overrides it
/// (`backend_cold_function_bytes`).
const BACKEND_COLD_FUNCTION_BYTES: usize = 1 << 20;
/// Traits whose impl methods are cold (`shard_unit`): the derived codecs run
/// only on compiler image load and save, derived orderings rarely, and their
/// bodies are among the program's largest functions.
const BACKEND_COLD_TRAITS: [&str; 3] = ["__jet_Decode", "__jet_Encode", "__jet_Comparable"];
/// Release-profile override for cold shard crates (`shard_unit`). Measured on
/// a 12 MB shard of derived decoders and constant tables: opt-level 2 peaks at
/// 9.1 GB in 773 s, opt-level 1 at 7.2 GB in 787 s, opt-level 0 at 2.2 GB in
/// 53 s. Hot shards, facades and the runtime keep the profile's opt-level 2
/// (a 12 MB hot shard: about 3 GB in 110-120 s).
const BACKEND_COLD_PROFILE: &str = "opt-level = 0\ndebug = false\n";

/// A positive integer from environment variable `name`, if set and
/// non-empty; any other value is a harness misconfiguration.
fn backend_env_count(name: &str) -> Option<usize> {
    let value = std::env::var(name).ok().filter(|value| !value.is_empty())?;
    match value.parse::<usize>() {
        Ok(count) if count > 0 => Some(count),
        _ => panic!("`{name}` must be a positive integer, not `{value}`"),
    }
}

/// The shard size of backend unit crates: `JET_BACKEND_SHARD_BYTES`, else
/// `BACKEND_SHARD_BYTES`. Smaller shards trade more crates (each re-reading
/// its facade's metadata) for shorter critical-path rustc runs and more
/// parallelism; shard boundaries never change what the program does.
fn backend_shard_bytes() -> usize {
    backend_env_count("JET_BACKEND_SHARD_BYTES").unwrap_or(BACKEND_SHARD_BYTES)
}

/// The size from which a moved function is cold:
/// `JET_BACKEND_COLD_FUNCTION_BYTES`, else `BACKEND_COLD_FUNCTION_BYTES`.
/// Cold shards build unoptimized under the release profile, so a lower
/// threshold trades the run time of the largest functions for a shorter
/// backend critical path.
fn backend_cold_function_bytes() -> usize {
    backend_env_count("JET_BACKEND_COLD_FUNCTION_BYTES").unwrap_or(BACKEND_COLD_FUNCTION_BYTES)
}

/// `JET_STAGE_ZERO_PROFILE=release` builds this test run's backend artifacts
/// (jetc0 above all) with this Cargo profile instead of dev: stage one is
/// jetc0 compiling the whole compiler, where a debug jetc0 is many times
/// slower. Thin-local LTO is off and debuginfo is off to keep the unit crates'
/// rustc memory and time down; overflow checks stay on (generated code may
/// rely on them). The dev profile stays the default: it links fastest.
const BACKEND_RELEASE_PROFILE_NAME: &str = "jet-stage-release";
const BACKEND_RELEASE_PROFILE: &str = "\n[profile.jet-stage-release]\ninherits = \"release\"\nopt-level = 2\ndebug = false\nlto = \"off\"\ncodegen-units = 16\nincremental = false\noverflow-checks = true\n";

/// The Cargo profile of backend builds: `None` = dev, `Some(name)` = the
/// release profile `JET_STAGE_ZERO_PROFILE=release` selects.
fn backend_release_profile() -> Option<&'static str> {
    match std::env::var("JET_STAGE_ZERO_PROFILE").as_deref() {
        Err(_) | Ok("") | Ok("dev") => None,
        Ok("release") => Some(BACKEND_RELEASE_PROFILE_NAME),
        Ok(other) => panic!("JET_STAGE_ZERO_PROFILE must be `dev` or `release`, not `{other}`"),
    }
}

/// The backend project `build_backend_artifact_with_native_library` builds,
/// written without building it, so later stages built outside `cargo test`
/// (the stage-one scripts) use exactly the harness layout: unit-split
/// workspace, compiler image, manifest and identity build script.
/// `JET_BACKEND_PROJECT_REPO` is the tree the manifest and build script name;
/// `JET_BACKEND_PROJECT_DIR`, `_PACKAGE` and `_SOURCE` name the project, its
/// package and the emitted source. `<source>.image` is the compiler image the
/// source embeds and `<source>.ffi` the FFI bridge it names (a retained
/// `stage-zero.ffi`), each when present: generated compilers emit with no
/// bridge, as the self-compile harness builds stage one and two.
#[test]
#[ignore = "writes the backend project named by JET_BACKEND_PROJECT_*"]
fn bootstrap_backend_project_from_env() {
    let var = |name: &str| {
        std::env::var(name).unwrap_or_else(|error| panic!("`{name}` is unavailable: {error}"))
    };
    let source_path = PathBuf::from(var("JET_BACKEND_PROJECT_SOURCE"));
    let source = fs::read_to_string(&source_path).unwrap_or_else(|error| {
        panic!("cannot read backend source `{}`: {error}", source_path.display())
    });
    let image = fs::read(source_path.with_extension("image")).ok();
    let ffi = fs::read_to_string(source_path.with_extension("ffi"))
        .ok()
        .and_then(|record| BackendFfiCrate::parse(&record));
    write_backend_project(
        Path::new(&var("JET_BACKEND_PROJECT_REPO")),
        Path::new(&var("JET_BACKEND_PROJECT_DIR")),
        &var("JET_BACKEND_PROJECT_PACKAGE"),
        &source,
        image.as_deref(),
        None,
        ffi.as_ref(),
    );
}

fn home_path() -> PathBuf {
    PathBuf::from(
        std::env::var_os("HOME")
            .unwrap_or_else(|| panic!("HOME is required for the private bootstrap cache")),
    )
}

fn bootstrap_session_root() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| panic!("system clock is before the Unix epoch: {error}"))
        .as_nanos();
    home_path()
        .join(".cache/jet-test-scratch")
        .join(format!("bootstrap-self-{}-{timestamp}", std::process::id()))
}

#[test]
fn bootstrap_completion_owner_requests_authorized_zero_root_retirement() {
    use crate::compiler_bootstrap_runner::{
        BootstrapRunCompletionFinish, BootstrapRunCompletionOwner,
    };
    let resources = jet_evalhost::SourceResources::SourceResourceSession::new();
    let mut owner = BootstrapRunCompletionOwner::new(
        jet_evalhost::SourceExecutionCompletionScope::new(),
    );
    owner.set_resources(resources.clone());
    assert!(!resources.is_retirement_requested().unwrap());
    assert_eq!(resources.retained_root_count().unwrap(), 0);
    owner.allow_resource_retirement();
    match owner.finish().expect("finish authorized Source retirement") {
        BootstrapRunCompletionFinish::Complete { failures } => assert!(failures.is_empty()),
        finish => panic!("authorized zero-root retirement did not complete: {finish:?}"),
    }
    assert!(resources.is_retirement_requested().unwrap());
    assert!(resources.arena().is_retired().unwrap());
}

#[test]
fn bootstrap_completion_owner_requires_retirement_permission() {
    use crate::compiler_bootstrap_runner::{
        BootstrapRunCompletionFinish, BootstrapRunCompletionOwner,
    };
    let resources = jet_evalhost::SourceResources::SourceResourceSession::new();
    let mut owner = BootstrapRunCompletionOwner::new(
        jet_evalhost::SourceExecutionCompletionScope::new(),
    );
    owner.set_resources(resources.clone());
    assert!(matches!(
        owner.finish().expect("inspect unauthorized Source retirement"),
        BootstrapRunCompletionFinish::Pending {
            retirement_requested: false,
            retained_roots: 0,
            open_callback_sessions: 0,
            pending_jobs: 0,
        }
    ));
    assert!(!resources.is_retirement_requested().unwrap());
    assert!(!resources.arena().is_retired().unwrap());
}

#[test]
fn bootstrap_completion_owner_preserves_retained_root_gate() {
    use crate::compiler_bootstrap_runner::{
        BootstrapRunCompletionFinish, BootstrapRunCompletionOwner,
    };
    let resources = jet_evalhost::SourceResources::SourceResourceSession::new();
    let root = resources.retain_root().expect("retain Source root");
    let mut owner = BootstrapRunCompletionOwner::new(
        jet_evalhost::SourceExecutionCompletionScope::new(),
    );
    owner.set_resources(resources.clone());
    owner.allow_resource_retirement();
    assert!(matches!(
        owner.finish().expect("inspect retained Source root"),
        BootstrapRunCompletionFinish::Pending {
            retirement_requested: false,
            retained_roots: 1,
            open_callback_sessions: 0,
            pending_jobs: 0,
        }
    ));
    assert!(!resources.is_retirement_requested().unwrap());
    assert!(!resources.arena().is_retired().unwrap());
    drop(root);
    match owner.finish().expect("finish after Source root release") {
        BootstrapRunCompletionFinish::Complete { failures } => assert!(failures.is_empty()),
        finish => panic!("released Source root did not allow retirement: {finish:?}"),
    }
    assert!(resources.is_retirement_requested().unwrap());
    assert!(resources.arena().is_retired().unwrap());
}

#[test]
fn native_loose_root_accepts_sources_without_a_manifest() {
    let project = bootstrap_session_root();
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("main.jet"), "use other\nfn run() { other.answer() }\n").unwrap();
    fs::write(project.join("other.jet"), "fn answer() -> Int => 42\n").unwrap();
    let lease = crate::compiler_bootstrap_host::open_authorized_sources(&project, Path::new("main.jet"))
        .unwrap_or_else(|error| panic!("loose source authorization failed: {error:?}"));
    let snapshot = lease.snapshot();
    assert_eq!(snapshot.roots.len(), 1);
    assert_eq!(snapshot.entry_root_identity, snapshot.roots[0].identity);
    assert!(snapshot.entry_path.ends_with("/main.jet"));
    let files = &snapshot.roots[0].files;
    assert!(files.iter().any(|file| file.relative_path == "main.jet"));
    assert!(files.iter().any(|file| file.relative_path == "other.jet"));
    assert!(!files.iter().any(|file| file.relative_path == "package.jet"));
    lease.revalidate().expect("revalidate loose source handles");
    drop(lease);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn native_loose_root_does_not_hide_a_retired_manifest() {
    let project = bootstrap_session_root();
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("main.jet"), "fn run() {}\n").unwrap();
    fs::write(project.join(crate::Syntax::PAYLOAD_FILE), "package { name: \"retired\", version: \"0.0.1\" }\n").unwrap();
    assert!(matches!(
        crate::compiler_bootstrap_host::open_authorized_sources(&project, Path::new("main.jet")),
        Err(crate::Authority::AuthorityError::RetiredManifest(_))
    ));
    fs::remove_dir_all(project).unwrap();
}

#[cfg(unix)]
#[test]
fn native_loose_root_does_not_hide_a_manifest_symlink() {
    let project = bootstrap_session_root();
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("main.jet"), "fn run() {}\n").unwrap();
    std::os::unix::fs::symlink("missing.jet", project.join("package.jet")).unwrap();
    assert!(crate::compiler_bootstrap_host::open_authorized_sources(&project, Path::new("main.jet")).is_err());
    fs::remove_dir_all(project).unwrap();
}

fn backend_identity_build_script(
    repo: &Path,
    native_library: Option<(&Path, &str)>,
) -> String {
    let native_link_directives = native_library.map_or_else(String::new, |(directory, library)| {
        format!(
            "    println!(\"cargo:rustc-link-search=native={{}}\", {:?});\n    println!(\"cargo:rustc-link-lib=static={library}\");\n",
            directory.display().to_string(),
        )
    });
    // The identity code is inlined (the root build.rs `#[path]`-includes the
    // same two files) rather than taken from a `jet` build-dependency: that
    // dependency compiled the `jet` and `jet-evalhost` crates a second time for the
    // host, doubling the largest repository crates' share of the build.
    let inline_module = |relative: &str| {
        let path = repo.join(relative);
        fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("cannot read backend build-script module `{}`: {error}", path.display())
        })
    };
    format!(
        r#"use std::fs;
use std::path::Path;

#[allow(dead_code, non_snake_case)]
mod SHA256 {{
{sha256}
}}

#[allow(dead_code, non_snake_case)]
mod BuildIdentity {{
{build_identity}
}}

fn main() {{
    let root = Path::new({:?});
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read(manifest_dir.join("generated.rs"))
        .expect("generated artifact source must be readable");
    let manifest = fs::read(manifest_dir.join("Cargo.toml"))
        .expect("generated artifact manifest must be readable");
    let build_script = fs::read(manifest_dir.join("build.rs"))
        .expect("generated artifact build script must be readable");
    let facts = BuildIdentity::build_facts()
        .expect("Cargo-supplied backend build facts must be readable");
    let mut extras = vec![
        ("Compiler/Bootstrap/__generated_compiler.rs".to_string(), source),
        ("Compiler/Bootstrap/__generated_cargo_manifest.toml".to_string(), manifest),
        ("Compiler/Bootstrap/__generated_cargo_build.rs".to_string(), build_script),
    ];
    // A compiler artifact embeds its image with `include_bytes!` from here.
    let image_path = manifest_dir.join("src/compiler.image");
    if let Ok(image) = fs::read(&image_path) {{
        extras.push(("Compiler/Bootstrap/__generated_compiler.image".to_string(), image));
        println!("cargo:rerun-if-changed={{}}", image_path.display());
    }}
    let identity = BuildIdentity::semantic_id_with_extra(
        root,
        BuildIdentity::COMPILER_DOMAIN,
        BuildIdentity::COMPILER_SOURCES,
        &facts,
        &extras,
    )
    .expect("generated compiler identity must be computable");
    fs::write(manifest_dir.join(".bootstrap-artifact-id"), &identity)
        .expect("generated compiler identity receipt must be writable");
    println!("cargo:rustc-env=JET_COMPILER_BUILD_ID={{identity}}");
    // Compiler-authored MIR records the source identity, not this binary's
    // backend profile, rustc flags, or generated extras. BUILD_ID above keeps
    // all of those facts for artifact receipts and build/cache invalidation.
    let source_identity = BuildIdentity::compiler_source_id(root)
        .expect("generated compiler source identity must be computable");
    println!("cargo:rustc-env=JET_COMPILER_SOURCE_ID={{source_identity}}");
    println!("cargo::rustc-check-cfg=cfg(jet_bootstrap_compiler_artifact)");
    println!("cargo:rustc-cfg=jet_bootstrap_compiler_artifact");
    println!("cargo:rerun-if-changed={{}}", manifest_dir.join("generated.rs").display());
    println!("cargo:rerun-if-changed={{}}", manifest_dir.join("Cargo.toml").display());
    println!("cargo:rerun-if-changed={{}}", manifest_dir.join("build.rs").display());
    // The root build.rs watch set: SOURCE_ID hashes these roots and facts, so
    // an incremental backend build must rerun this script when any changes.
    for input in BuildIdentity::COMPILER_SOURCES {{
        println!("cargo:rerun-if-changed={{}}", root.join(input).display());
    }}
    for key in [
        "RUSTC",
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {{
        println!("cargo:rerun-if-env-changed={{key}}");
    }}
    for key in BuildIdentity::profile_override_keys() {{
        println!("cargo:rerun-if-env-changed={{key}}");
    }}
    if let Some(spec) = BuildIdentity::target_spec_path() {{
        println!("cargo:rerun-if-changed={{}}", spec.display());
    }}
{native_link_directives}}}
"#,
        repo.display().to_string(),
        native_link_directives = native_link_directives,
        sha256 = inline_module("crates/jet-foundation/src/SHA256.rs"),
        build_identity = inline_module("Compiler/Bootstrap/BuildIdentity.rs"),
    )
}
/// The prepared FFI bridge an emitted program names (`jet_ffi_<key>`). The
/// backend build depends on its generated source package by path, Cargo's
/// equivalent of `jet build`'s `--extern` of the bridge rlib.
struct BackendFfiCrate {
    name: String,
    dir: PathBuf,
}

impl BackendFfiCrate {
    /// The manifest dependency row naming the bridge.
    fn dependency(&self) -> String {
        format!("{} = {{ path = {:?} }}\n", self.name, self.dir.display().to_string())
    }

    /// A retained bridge record (`stage-zero.ffi`): crate name and directory
    /// lines; empty when the program names no bridge.
    fn parse(record: &str) -> Option<Self> {
        record.split_once('\n').map(|(name, path)| BackendFfiCrate {
            name: name.to_string(),
            dir: PathBuf::from(path.trim_end_matches('\n')),
        })
    }
}

/// One build unit of an emitted program, built as its own crate.
struct BackendUnit {
    crate_name: String,
    /// Crate names of the units this one names; each is built before it.
    deps: Vec<String>,
    /// The exported crate source (the facade of a sharded unit).
    source: String,
    /// The shard crates of a unit too large for one rustc (`shard_unit`);
    /// empty otherwise.
    shards: Vec<BackendShard>,
}

/// One shard crate of a sharded unit: `<unit>_s<index>` for hot code,
/// `<unit>_cold<index>` for cold code (`shard_unit`), which the release
/// profile builds unoptimized (`backend_unit_profiles`).
struct BackendShard {
    name: String,
    source: String,
    cold: bool,
}

/// An emitted program split into crates: the exported `jet_runtime` crate
/// (its runtime/Core block), the crates of its unit sections (MIRRust
/// `EmissionUnits`) in dependency order, and the main crate — everything
/// outside the sections (Host/Runner glue, the embedded compiler image, the
/// artifact main).
struct BackendWorkspace {
    runtime_lib: String,
    units: Vec<BackendUnit>,
    main: String,
}

/// Split a backend source into crates, as `jet_store::runtime::prepare`
/// splits a real build: the runtime/Core block always builds as its own
/// crate, so programs with the same block share one built runtime. Unit
/// dependencies come from the names each unit's text actually uses (every
/// program item is a unique `__jet_` symbol); units that name each other
/// cyclically merge into one crate. A source without unit sections has no
/// unit crates. `None` when the source has no runtime block.
fn split_backend_workspace(source: &str) -> Option<BackendWorkspace> {
    let Some(split) = jet_store::runtime::split_runtime_crate(source)
        .unwrap_or_else(|error| panic!("backend source: {error}"))
    else {
        assert!(!source.contains(UNIT_BEGIN_MARKER), "unit-split backend source has no runtime block");
        return None;
    };
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut main = String::new();
    let mut open: Option<usize> = None;
    for line in split.program.split_inclusive('\n') {
        if let Some(name) = line.strip_prefix(UNIT_BEGIN_MARKER) {
            let name = name.trim_end();
            assert!(open.is_none(), "backend unit section `{name}` opens inside another section");
            let existing = sections.iter().position(|(existing, _)| existing == name);
            let index = existing.unwrap_or_else(|| {
                sections.push((name.to_string(), String::new()));
                sections.len() - 1
            });
            open = Some(index);
        } else if let Some(name) = line.strip_prefix(UNIT_END_MARKER) {
            let index = open
                .take()
                .unwrap_or_else(|| panic!("backend unit section `{}` closes without opening", name.trim_end()));
            assert_eq!(sections[index].0, name.trim_end(), "backend unit sections interleave");
        } else if let Some(index) = open {
            sections[index].1.push_str(line);
        } else {
            main.push_str(line);
        }
    }
    assert!(open.is_none(), "backend unit section is unterminated");

    let mut owners: HashMap<&str, usize> = HashMap::new();
    for (index, (_, text)) in sections.iter().enumerate() {
        for name in unit_definitions(text) {
            owners.entry(name).or_insert(index);
        }
    }
    let edges = sections
        .iter()
        .enumerate()
        .map(|(index, (_, text))| {
            unit_symbols(text)
                .filter_map(|symbol| owners.get(symbol).copied())
                .filter(|owner| *owner != index)
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();

    // Tarjan emits each strongly connected component after every component
    // it reaches: dependencies first, the build order.
    let components = strongly_connected_units(&edges);
    let mut component_of = vec![0; sections.len()];
    for (component, members) in components.iter().enumerate() {
        for member in members {
            component_of[*member] = component;
        }
    }
    let crate_names = components
        .iter()
        .map(|members| {
            let mut names = members.iter().map(|member| sections[*member].0.as_str()).collect::<Vec<_>>();
            names.sort_unstable();
            let name = names.join("_").to_ascii_lowercase();
            format!(
                "jetc_{}",
                name.chars()
                    .map(|character| if character.is_ascii_alphanumeric() { character } else { '_' })
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>();
    let units = components
        .iter()
        .enumerate()
        .map(|(component, members)| {
            let mut members = members.clone();
            members.sort_unstable();
            let deps = members
                .iter()
                .flat_map(|member| edges[*member].iter().map(|target| component_of[*target]))
                .filter(|target| *target != component)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|target| crate_names[target].clone())
                .collect::<Vec<_>>();
            let crate_name = crate_names[component].clone();
            let mut prelude = String::from("#![allow(warnings)]\nuse jet_runtime::*;\n");
            for dep in &deps {
                prelude.push_str(&format!("use {dep}::*;\n"));
            }
            let text = members.iter().map(|member| sections[*member].1.as_str()).collect::<String>();
            let (source, shards) = match shard_unit(&text, backend_shard_bytes()) {
                Some(sharded) => {
                    let source = format!(
                        "{prelude}{}unsafe extern \"Rust\" {{\n{}}}\n",
                        jet_store::runtime::export_crate_source(&sharded.facade),
                        sharded.declarations,
                    );
                    let shard = |(index, text): &(usize, String), cold: bool| BackendShard {
                        name: format!("{crate_name}_{}{index}", if cold { "cold" } else { "s" }),
                        source: format!(
                            "{prelude}use {crate_name}::*;\n{}",
                            jet_store::runtime::export_crate_source(text)
                        ),
                        cold,
                    };
                    let hot = sharded.shards.iter().map(|indexed| shard(indexed, false));
                    let cold = sharded.cold_shards.iter().map(|indexed| shard(indexed, true));
                    let shards = hot.chain(cold).collect();
                    (source, shards)
                }
                None => (format!("{prelude}{}", jet_store::runtime::export_crate_source(&text)), Vec::new()),
            };
            BackendUnit {
                crate_name,
                deps,
                source,
                shards,
            }
        })
        .collect::<Vec<_>>();

    let anchor = "use jet_runtime::*;\n";
    let at = main
        .find(anchor)
        .unwrap_or_else(|| panic!("split backend main has no `jet_runtime` import"))
        + anchor.len();
    // Shards are named by nothing (their functions are reached through the
    // facade's declarations), so the main crate links each explicitly.
    let uses = units
        .iter()
        .map(|unit| {
            let shards = unit.shards.iter().map(|shard| format!("use {} as _;\n", shard.name));
            std::iter::once(format!("use {}::*;\n", unit.crate_name)).chain(shards).collect::<String>()
        })
        .collect::<String>();
    main.insert_str(at, &uses);
    Some(BackendWorkspace {
        runtime_lib: split.runtime_lib,
        units,
        main,
    })
}

/// The `__jet_` symbols a unit section defines: its top-level items (emitted
/// at column 0) and the functions of its top-level foreign blocks. Indented
/// trait methods and body-local names are not unit-level definitions.
fn unit_definitions(text: &str) -> impl Iterator<Item = &str> {
    let mut in_foreign = false;
    text.lines().filter_map(move |line| {
        if in_foreign {
            in_foreign = !line.starts_with('}');
            return if in_foreign { defined_unit_item(line) } else { None };
        }
        if line.starts_with("extern ") || line.starts_with("unsafe extern ") {
            in_foreign = !line.trim_end().ends_with('}');
            return None;
        }
        if line.starts_with(char::is_whitespace) {
            return None;
        }
        defined_unit_item(line)
    })
}

/// The `__jet_` symbol a unit line defines at item level (`fn`, `struct`,
/// `enum`, `trait`, `type`, `static`, `const`, `union`, after visibility and
/// qualifiers), if any.
fn defined_unit_item(line: &str) -> Option<&str> {
    let mut rest = line.trim_start();
    for visibility in ["pub(crate) ", "pub(super) ", "pub "] {
        if let Some(stripped) = rest.strip_prefix(visibility) {
            rest = stripped;
            break;
        }
    }
    loop {
        if let Some(stripped) = rest.strip_prefix("const fn ") {
            return unit_symbol_at(stripped);
        }
        let current = rest;
        match ["unsafe ", "async ", "extern \"C\" ", "extern \"Rust\" "]
            .iter()
            .find_map(|qualifier| current.strip_prefix(qualifier))
        {
            Some(stripped) => rest = stripped,
            None => break,
        }
    }
    ["fn ", "struct ", "enum ", "trait ", "type ", "static mut ", "static ", "const ", "union "]
        .iter()
        .find_map(|keyword| rest.strip_prefix(keyword))
        .and_then(unit_symbol_at)
}

/// The `__jet_` identifier at the start of `text`.
fn unit_symbol_at(text: &str) -> Option<&str> {
    let end = text
        .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .unwrap_or(text.len());
    let symbol = &text[..end];
    symbol.starts_with("__jet_").then_some(symbol)
}

/// Every whole `__jet_` identifier in `text`.
fn unit_symbols(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices("__jet_").filter_map(move |(at, _)| {
        let starts_identifier = text[..at]
            .chars()
            .next_back()
            .is_none_or(|character| !(character.is_ascii_alphanumeric() || character == '_'));
        if starts_identifier { unit_symbol_at(&text[at..]) } else { None }
    })
}

/// Strongly connected components of the unit graph, each after every
/// component it reaches.
fn strongly_connected_units(edges: &[BTreeSet<usize>]) -> Vec<Vec<usize>> {
    struct Tarjan<'e> {
        edges: &'e [BTreeSet<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        components: Vec<Vec<usize>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, node: usize) {
            self.index[node] = Some(self.next);
            self.low[node] = self.next;
            self.next += 1;
            self.stack.push(node);
            self.on_stack[node] = true;
            for target in self.edges[node].clone() {
                match self.index[target] {
                    None => {
                        self.visit(target);
                        self.low[node] = self.low[node].min(self.low[target]);
                    }
                    Some(index) if self.on_stack[target] => {
                        self.low[node] = self.low[node].min(index);
                    }
                    Some(_) => {}
                }
            }
            if Some(self.low[node]) == self.index[node] {
                let mut component = Vec::new();
                while let Some(member) = self.stack.pop() {
                    self.on_stack[member] = false;
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                self.components.push(component);
            }
        }
    }
    let mut tarjan = Tarjan {
        edges,
        index: vec![None; edges.len()],
        low: vec![0; edges.len()],
        on_stack: vec![false; edges.len()],
        stack: Vec::new(),
        next: 0,
        components: Vec::new(),
    };
    for node in 0..edges.len() {
        if tarjan.index[node].is_none() {
            tarjan.visit(node);
        }
    }
    tarjan.components
}

/// Write the runtime and unit crates under `units_root`; returns the main
/// manifest's dependency rows for them. Every crate may name the FFI bridge,
/// so each depends on it. The runtime crate's directory is named by the
/// digest of its source and bridge row, so programs that alternate between
/// runtime/Core blocks (their runtime parts differ) each keep their built
/// runtime instead of rewriting one directory. Files are rewritten only when
/// their bytes change, so cargo keeps unchanged crates built.
fn write_backend_workspace(
    units_root: &Path,
    workspace: &BackendWorkspace,
    ffi: Option<&BackendFfiCrate>,
) -> String {
    let ffi_dependency = ffi.map_or_else(String::new, BackendFfiCrate::dependency);
    let runtime_digest = jet_foundation::SHA256::sha256_hex(
        format!("{ffi_dependency}\0{}", workspace.runtime_lib).as_bytes(),
    );
    let runtime_dir_name = format!("jet_runtime_{}", &runtime_digest[..16]);
    let runtime_dir = units_root.join(&runtime_dir_name);
    write_backend_crate(&runtime_dir, "jet_runtime", &[], &ffi_dependency, &workspace.runtime_lib);
    let mut dependencies = format!("jet_runtime = {{ path = {:?} }}\n", runtime_dir.display().to_string());
    // Unit and shard crates name the runtime by its digest directory.
    let unit_extra_dependencies = format!("jet_runtime = {{ path = \"../{runtime_dir_name}\" }}\n{ffi_dependency}");
    for unit in &workspace.units {
        let dir = units_root.join(&unit.crate_name);
        let mut deps = unit.deps.clone();
        write_backend_crate(&dir, &unit.crate_name, &deps, &unit_extra_dependencies, &unit.source);
        dependencies.push_str(&format!(
            "{} = {{ path = {:?} }}\n",
            unit.crate_name,
            dir.display().to_string()
        ));
        deps.push(unit.crate_name.clone());
        for shard in &unit.shards {
            let dir = units_root.join(&shard.name);
            write_backend_crate(&dir, &shard.name, &deps, &unit_extra_dependencies, &shard.source);
            dependencies.push_str(&format!("{} = {{ path = {:?} }}\n", shard.name, dir.display().to_string()));
        }
    }
    dependencies
}

/// The main manifest's profile overrides for the runtime and unit crates:
/// under dev `BACKEND_UNIT_PROFILE` for each (repository crates keep the
/// default dev profile); under the release profile `release` (which already
/// builds everything at opt-level 2 without debuginfo or LTO)
/// `BACKEND_COLD_PROFILE` for each cold shard.
fn backend_unit_profiles(workspace: &BackendWorkspace, release: Option<&str>) -> String {
    let shards = || workspace.units.iter().flat_map(|unit| &unit.shards);
    match release {
        None => std::iter::once("jet_runtime")
            .chain(workspace.units.iter().map(|unit| unit.crate_name.as_str()))
            .chain(shards().map(|shard| shard.name.as_str()))
            .map(|name| format!("\n[profile.dev.package.{name}]\n{BACKEND_UNIT_PROFILE}"))
            .collect(),
        Some(profile) => shards()
            .filter(|shard| shard.cold)
            .map(|shard| format!("\n[profile.{profile}.package.{}]\n{BACKEND_COLD_PROFILE}", shard.name))
            .collect(),
    }
}

/// One library crate of the backend workspace; `deps` are sibling crates,
/// `extra_dependencies` further manifest dependency rows.
fn write_backend_crate(dir: &Path, name: &str, deps: &[String], extra_dependencies: &str, lib: &str) {
    let mut manifest = format!("[package]\nname = {name:?}\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n");
    for dep in deps {
        manifest.push_str(&format!("{dep} = {{ path = \"../{dep}\" }}\n"));
    }
    manifest.push_str(extra_dependencies);
    for (path, text) in [(dir.join("Cargo.toml"), manifest.as_str()), (dir.join("src/lib.rs"), lib)] {
        if fs::read(&path).is_ok_and(|existing| existing == text.as_bytes()) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| {
                panic!("cannot create backend crate directory `{}`: {error}", parent.display())
            });
        }
        fs::write(&path, text).unwrap_or_else(|error| {
            panic!("cannot write backend crate file `{}`: {error}", path.display())
        });
    }
}

/// A unit too large for one rustc, split by `shard_unit` (texts before
/// export).
struct ShardedUnit {
    /// Every type, trait, impl and unmovable item: the unit crate itself.
    facade: String,
    /// `unsafe extern "Rust"` block members declaring each moved function.
    declarations: String,
    /// Moved hot function definitions by shard index (`pack_shards`).
    shards: Vec<(usize, String)>,
    /// Moved cold function definitions (`MovedFunction::cold`), packed the
    /// same way into their own crates.
    cold_shards: Vec<(usize, String)>,
}

/// One function moved out of a facade: its declaration and its definition
/// under its own symbol.
struct MovedFunction {
    /// The function's symbol, which alone places it (`pack_shards`).
    name: String,
    declaration: String,
    definition: String,
    /// Code an optimized build should not optimize: a cold-trait impl
    /// method (`BACKEND_COLD_TRAITS`), constant table code (a parameterless
    /// function whose body is a `vec!` literal, or one chunk of a cached
    /// table's row fill, whose body starts `rows.push(`), or any function of
    /// at least `backend_cold_function_bytes()`.
    cold: bool,
}

/// Shard a unit's text when it is longer than `shard_bytes`. Free
/// non-generic `__jet_` functions move to shard crates, which import the
/// facade and define each function `#[no_mangle]`; the facade declares them
/// in an `unsafe extern "Rust"` block, so every caller — facade, sibling
/// shards, dependent units — reaches a moved function through its symbol
/// and no shard depends on another. Impls stay beside their types (orphan
/// rule), but a large method body moves as a free function the method
/// calls. Cold and hot functions pack into separate shards, so the release
/// profile can leave the cold ones unoptimized; each function's shard
/// depends only on its symbol (`pack_shards`). `None` for a small unit or
/// one with macros, which shards could not see.
fn shard_unit(text: &str, shard_bytes: usize) -> Option<ShardedUnit> {
    if text.len() <= shard_bytes {
        return None;
    }
    let mask = jet_foundation::RustSource::rust_code_mask(text);
    let (items, end) = masked_items(&mask);
    let mut facade = String::new();
    let mut uses = String::new();
    let mut declarations = String::new();
    let mut moved = Vec::new();
    for (start, end) in items {
        let (item, code) = (&text[start..end], &mask[start..end]);
        let head = &code[item_head(code)..];
        if head.starts_with("macro_rules!") {
            return None;
        }
        if head.starts_with("use ") || head.starts_with("pub use ") {
            uses.push_str(item);
        }
        if let Some(function) = movable_function(item, code) {
            declarations.push_str(&function.declaration);
            moved.push(function);
        } else if let Some((rewritten, functions)) = delegate_impl_methods(item, code) {
            facade.push_str(&rewritten);
            for function in functions {
                declarations.push_str(&function.declaration);
                moved.push(function);
            }
        } else {
            facade.push_str(item);
        }
    }
    facade.push_str(&text[end..]);
    let (cold, hot): (Vec<MovedFunction>, Vec<MovedFunction>) = moved.into_iter().partition(|function| function.cold);
    Some(ShardedUnit {
        facade,
        declarations,
        shards: pack_shards(&hot, &uses, shard_bytes),
        cold_shards: pack_shards(&cold, &uses, shard_bytes),
    })
}

/// Pack moved functions into shard texts by symbol: a function lands in
/// shard `symbol_hash(name) % count`, where `count` is the functions' total
/// size over `shard_bytes` rounded up to a power of two. A function's shard
/// so depends only on its own symbol and that rarely changing count, never
/// on the size of the functions emitted before it: an edit rewrites only the
/// shards whose functions change (cargo rebuilds nothing else), and the two
/// arms of an A/B build give a function the same shard and profile. Each
/// shard keeps its index; empty ones make no crate.
fn pack_shards(functions: &[MovedFunction], uses: &str, shard_bytes: usize) -> Vec<(usize, String)> {
    let total = functions.iter().map(|function| function.definition.len()).sum::<usize>();
    let count = total.div_ceil(shard_bytes).max(1).next_power_of_two();
    let mut shards = vec![String::new(); count];
    for function in functions {
        shards[(symbol_hash(&function.name) % count as u64) as usize].push_str(&function.definition);
    }
    shards
        .into_iter()
        .enumerate()
        .filter(|(_, text)| !text.is_empty())
        .map(|(index, text)| (index, format!("{uses}{text}")))
        .collect()
}

/// FNV-1a of a symbol: stable across toolchains and runs, unlike std's
/// hashers, so shard placement is reproducible.
fn symbol_hash(symbol: &str) -> u64 {
    symbol.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}

/// Byte ranges of the top-level items of masked Rust `code`, each starting
/// where the previous one ended (leading whitespace and comments travel with
/// their item), and the end of the last one.
fn masked_items(code: &str) -> (Vec<(usize, usize)>, usize) {
    let mut items = Vec::new();
    let (mut start, mut depth) = (0usize, 0usize);
    for (at, byte) in code.bytes().enumerate() {
        match byte {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth = depth
                    .checked_sub(1)
                    .unwrap_or_else(|| panic!("backend unit source closes an unopened bracket at byte {at}"));
                if depth == 0 && byte == b'}' {
                    items.push((start, at + 1));
                    start = at + 1;
                }
            }
            b';' if depth == 0 => {
                items.push((start, at + 1));
                start = at + 1;
            }
            _ => {}
        }
    }
    (items, start)
}

/// Index of the bracket closing the one at `open` in masked `code`.
fn matching_bracket(code: &str, open: usize) -> usize {
    let mut depth = 0usize;
    for (offset, byte) in code[open..].bytes().enumerate() {
        match byte {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return open + offset;
                }
            }
            _ => {}
        }
    }
    panic!("backend unit source leaves a bracket open at byte {open}")
}

/// Offset of an item's keyword in masked `code`, past leading whitespace
/// and attributes.
fn item_head(code: &str) -> usize {
    let mut at = code.len() - code.trim_start().len();
    while code[at..].starts_with("#[") {
        at = matching_bracket(code, at + 1) + 1;
        at += code[at..].len() - code[at..].trim_start().len();
    }
    at
}

/// Whether every attribute in masked `attributes` is one a moved function
/// may keep (none changes its symbol, ABI or presence).
fn movable_attributes(attributes: &str) -> bool {
    attributes
        .split("#[")
        .skip(1)
        .all(|attribute| ["inline", "allow(", "cold]"].iter().any(|name| attribute.starts_with(name)))
}

/// Ranges of masked `code` between its depth-0 `separator`s (angle brackets
/// count as depth; the `>` of `->` does not).
fn split_top_level(code: &str, separator: u8) -> Vec<(usize, usize)> {
    let bytes = code.as_bytes();
    let (mut parts, mut start, mut depth) = (Vec::new(), 0usize, 0isize);
    for (at, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b'{' | b'(' | b'[' | b'<' => depth += 1,
            b'>' if at > 0 && bytes[at - 1] == b'-' => {}
            b'}' | b')' | b']' | b'>' => depth -= 1,
            _ if byte == separator && depth == 0 => {
                parts.push((start, at));
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push((start, code.len()));
    parts
}

/// Offset of the `:` between a parameter's pattern and its type in masked
/// `code` (not a `::` path separator).
fn parameter_colon(code: &str) -> Option<usize> {
    let bytes = code.as_bytes();
    split_top_level(code, b':').into_iter().map(|(_, end)| end).find(|&at| {
        at < bytes.len() && bytes.get(at + 1) != Some(&b':') && (at == 0 || bytes[at - 1] != b':')
    })
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// A free, non-generic, non-const `__jet_` function item: its declaration
/// and its `#[no_mangle]` definition.
fn movable_function(item: &str, code: &str) -> Option<MovedFunction> {
    let lead = code.len() - code.trim_start().len();
    let head = item_head(code);
    if !movable_attributes(&code[lead..head]) || !code.ends_with('}') {
        return None;
    }
    let rest = &code[head..];
    let rest = rest.strip_prefix("pub ").unwrap_or(rest);
    let unsafe_function = rest.starts_with("unsafe ");
    let rest = rest.strip_prefix("unsafe ").unwrap_or(rest).strip_prefix("fn ")?;
    let name_end = rest.find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))?;
    let name = &rest[..name_end];
    if !name.starts_with("__jet_") || !rest[name_end..].starts_with('(') {
        return None;
    }
    let open = code.len() - rest.len() + name_end;
    let close = matching_bracket(code, open);
    let body = close + code[close..].find('{')?;
    if code[open..body].contains("impl ") || code[close..body].contains("where") {
        return None;
    }
    let mut parameters = Vec::new();
    for (start, end) in split_top_level(&code[open + 1..close], b',') {
        let (start, end) = (open + 1 + start, open + 1 + end);
        if code[start..end].trim().is_empty() {
            continue;
        }
        let colon = start + parameter_colon(&code[start..end])?;
        parameters.push(format!("_: {}", item[colon + 1..end].trim()));
    }
    // Emitted Jet code names its locals `__jet_*`: a bare `rows` is the row
    // vector of a cached table's fill chunk.
    let body_code = code[body + 1..].trim_start();
    let table = (parameters.is_empty() && body_code.starts_with("vec![")) || body_code.starts_with("rows.push(");
    Some(MovedFunction {
        name: name.to_string(),
        declaration: format!(
            "    pub {} fn {name}({}) {};\n",
            if unsafe_function { "unsafe" } else { "safe" },
            parameters.join(", "),
            item[close + 1..body].trim()
        ),
        definition: format!("\n#[no_mangle]\n{}", &item[lead..]),
        cold: table || code.len() >= backend_cold_function_bytes(),
    })
}

/// An inherent or trait impl of a plain named type whose large method
/// bodies move: each such method's body becomes a moved free function the
/// method calls. `None` when no method moves.
fn delegate_impl_methods(item: &str, code: &str) -> Option<(String, Vec<MovedFunction>)> {
    let head = item_head(code);
    if !code[head..].starts_with("impl ") || !code.ends_with('}') {
        return None;
    }
    let brace = head + code[head..].find('{')?;
    let header = code[head + "impl ".len()..brace].trim();
    let (trait_path, self_type) = header.split_once(" for ").unwrap_or(("inherent", header));
    let self_type = self_type.trim();
    if !is_identifier(self_type) {
        return None;
    }
    let trait_tag = trait_path
        .trim()
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() { character } else { '_' })
        .collect::<String>();
    let prefix = format!("__jet_impl_{self_type}_{trait_tag}_");
    let cold_trait = BACKEND_COLD_TRAITS.contains(&trait_path.trim());
    let close = code.len() - 1;
    let (methods, tail) = masked_items(&code[brace + 1..close]);
    let mut rewritten = item[..brace + 1].to_string();
    let mut moved = Vec::new();
    for (start, end) in methods {
        let (start, end) = (brace + 1 + start, brace + 1 + end);
        match delegated_method(&item[start..end], &code[start..end], self_type, &prefix) {
            Some((method, mut function)) => {
                rewritten.push_str(&method);
                function.cold |= cold_trait;
                moved.push(function);
            }
            None => rewritten.push_str(&item[start..end]),
        }
    }
    if moved.is_empty() {
        return None;
    }
    rewritten.push_str(&item[brace + 1 + tail..]);
    Some((rewritten, moved))
}

/// A method of at least `BACKEND_DELEGATE_METHOD_BYTES` whose body can
/// stand alone as a free function (`self` becomes `__jet_self`, `Self` the
/// impl's type): the method calling it, and the moved function.
fn delegated_method(
    method: &str,
    code: &str,
    self_type: &str,
    prefix: &str,
) -> Option<(String, MovedFunction)> {
    // Nested impls and module-relative paths do not survive the move
    // unchanged; such methods stay.
    if code.len() < BACKEND_DELEGATE_METHOD_BYTES || code.contains("impl ") || code.contains("self::") {
        return None;
    }
    let lead = code.len() - code.trim_start().len();
    let head = item_head(code);
    if !movable_attributes(&code[lead..head]) {
        return None;
    }
    let rest = &code[head..];
    let rest = rest.strip_prefix("pub ").unwrap_or(rest);
    let unsafe_method = rest.starts_with("unsafe ");
    let rest = rest.strip_prefix("unsafe ").unwrap_or(rest).strip_prefix("fn ")?;
    let name_end = rest.find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))?;
    let name = &rest[..name_end];
    if !rest[name_end..].starts_with('(') {
        return None;
    }
    let open = code.len() - rest.len() + name_end;
    let close = matching_bracket(code, open);
    let body = close + code[close..].find('{')?;
    // Signature lifetimes (named, or elided into a returned reference) would
    // resolve differently once the receiver is an ordinary parameter.
    if code[..body].contains('\'') || code[close..body].contains('&') || code[close..body].contains("where") {
        return None;
    }
    let mut parameters = Vec::new();
    let mut arguments = Vec::new();
    for (start, end) in split_top_level(&code[open + 1..close], b',') {
        let (start, end) = (open + 1 + start, open + 1 + end);
        let parameter = code[start..end].trim();
        if parameter.is_empty() {
            continue;
        }
        let receiver = match parameter {
            "&self" => Some(format!("__jet_self: &{self_type}")),
            "&mut self" => Some(format!("__jet_self: &mut {self_type}")),
            "self" => Some(format!("__jet_self: {self_type}")),
            "mut self" => Some(format!("mut __jet_self: {self_type}")),
            _ => None,
        };
        if let Some(receiver) = receiver {
            parameters.push(receiver);
            arguments.push("self");
            continue;
        }
        let colon = parameter_colon(parameter)?;
        let pattern = parameter[..colon].trim();
        let binding = pattern.strip_prefix("mut ").unwrap_or(pattern).trim();
        if !is_identifier(binding) || binding == "_" || binding == "self" {
            return None;
        }
        parameters.push(method[start..end].trim().to_string());
        arguments.push(binding);
    }
    let function_name = format!("{prefix}{name}");
    let function = format!(
        "pub {}fn {function_name}({}) {} {}",
        if unsafe_method { "unsafe " } else { "" },
        parameters.join(", "),
        method[close + 1..body].trim(),
        &method[body..]
    );
    let function = replace_self_tokens(&function, self_type);
    let moved = movable_function(&function, &jet_foundation::RustSource::rust_code_mask(&function))?;
    let call = format!("{{ {function_name}({}) }}", arguments.join(", "));
    Some((format!("{}{call}", &method[..body]), moved))
}

/// `text` with its code tokens `self` renamed `__jet_self` and `Self`
/// replaced by `self_type`.
fn replace_self_tokens(text: &str, self_type: &str) -> String {
    let code = jet_foundation::RustSource::rust_code_mask(text);
    let bytes = code.as_bytes();
    let identifier = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut at) = (0, 0);
    while at + 4 <= bytes.len() {
        let token = &bytes[at..at + 4];
        if (token == b"self" || token == b"Self")
            && (at == 0 || !identifier(bytes[at - 1]))
            && bytes.get(at + 4).is_none_or(|byte| !identifier(*byte))
        {
            out.push_str(&text[copied..at]);
            out.push_str(if token == b"self" { "__jet_self" } else { self_type });
            copied = at + 4;
            at += 4;
        } else {
            at += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// The compiler-image envelope reader checks the archive prefix exactly as
/// full restore does, without decoding the MIR payload or hashing the
/// archive: it accepts a well-formed prefix in front of a payload that
/// restore rejects and a checksum that the per-file verification rejects.
/// Restore itself never hashes the archive.
#[test]
fn compiler_image_envelope_reads_only_the_checked_prefix() {
    use crate::compiler_bootstrap_compiler_image::{
        read_compiler_image_envelope, restore_compiler_image, verify_compiler_image_checksum,
        CompilerImageMetadataWriter, FORMAT_VERSION, MAGIC,
    };
    use ::jet_foundation::MIR::{MirArtifactId, MirFunctionId, MIR_SCHEMA_VERSION};
    let digest = [7u8; 32];
    let archive = |schema: u16| {
        let mut writer = CompilerImageMetadataWriter::new();
        writer.write_raw(MAGIC);
        writer.write_u16(FORMAT_VERSION);
        writer.write_u16(schema);
        writer.write_u64(11);
        writer.write_u64(13);
        writer.write_raw(&digest);
        writer.write_raw(&[0u8; 32]);
        writer.write_bytes(b"identity").unwrap();
        writer.write_bytes(b"not a MIR payload").unwrap();
        writer.finish()
    };
    let bytes = archive(MIR_SCHEMA_VERSION);
    let envelope =
        read_compiler_image_envelope(&bytes, digest, MirArtifactId(11), MirFunctionId(13))
            .unwrap_or_else(|error| panic!("envelope rejected a well-formed prefix: {error}"));
    assert_eq!(envelope.source_authority_digest, digest);
    assert_eq!(envelope.artifact, MirArtifactId(11));
    assert_eq!(envelope.entry_function, MirFunctionId(13));
    let restored = restore_compiler_image(&bytes, digest, MirArtifactId(11), MirFunctionId(13));
    assert_eq!(
        restored.err().map(|error| error.0).as_deref(),
        Some("truncated native MIR image"),
    );
    assert_eq!(
        verify_compiler_image_checksum(&bytes).err().map(|error| error.0).as_deref(),
        Some("compiler-image archive checksum mismatch"),
    );
    let envelope_error = |bytes: &[u8], expected_digest: [u8; 32], artifact: u64, entry: u64| {
        read_compiler_image_envelope(
            bytes,
            expected_digest,
            MirArtifactId(artifact),
            MirFunctionId(entry),
        )
        .err()
        .map(|error| error.0)
    };
    for (expected_digest, artifact, entry, message) in [
        ([0u8; 32], 11, 13, "compiler-image source authority does not match the authorized source snapshot"),
        (digest, 12, 13, "compiler-image artifact identity does not match the requested artifact"),
        (digest, 11, 14, "compiler-image entry function does not match the requested entry"),
    ] {
        assert_eq!(
            envelope_error(&bytes, expected_digest, artifact, entry).as_deref(),
            Some(message),
        );
    }
    let stale_schema = archive(MIR_SCHEMA_VERSION.wrapping_add(1));
    assert!(envelope_error(&stale_schema, digest, 11, 13)
        .is_some_and(|error| error.starts_with("compiler-image MIR schema ")));
    let mut bad_magic = bytes.clone();
    bad_magic[0] ^= 1;
    assert_eq!(
        envelope_error(&bad_magic, digest, 11, 13).as_deref(),
        Some("invalid compiler-image magic"),
    );
    assert_eq!(
        envelope_error(&bytes[..40], digest, 11, 13).as_deref(),
        Some("truncated compiler-image archive"),
    );
}

/// Unit sections become crates in dependency order: dependencies come from
/// the top-level `__jet_` items a section names (not from indented trait
/// methods), and units naming each other cyclically merge into one crate.
#[test]
fn backend_workspace_orders_units_and_merges_cycles() {
    let source = format!(
        "#![allow(warnings)]\n\
         // jet:cached-runtime-begin\n\
         pub(crate) fn runtime_item() {{}}\n\
         // jet:cached-runtime-end\n\
         {UNIT_BEGIN_MARKER}base\n\
         fn __jet_base_item() {{}}\n\
         trait __jet_BaseStore {{\n    fn __jet_gamma_field(&self);\n}}\n\
         {UNIT_END_MARKER}base\n\
         {UNIT_BEGIN_MARKER}Alpha\n\
         fn __jet_alpha() {{ __jet_base_item(); __jet_beta(); }}\n\
         {UNIT_END_MARKER}Alpha\n\
         {UNIT_BEGIN_MARKER}Beta\n\
         fn __jet_beta() {{ __jet_alpha(); }}\n\
         {UNIT_END_MARKER}Beta\n\
         {UNIT_BEGIN_MARKER}Gamma\n\
         struct __jet_Gamma {{ __jet_gamma_field: i64 }}\n\
         fn __jet_gamma() {{ __jet_beta(); }}\n\
         {UNIT_END_MARKER}Gamma\n\
         fn main() {{ __jet_gamma(); }}\n"
    );
    let workspace = split_backend_workspace(&source).expect("unit sections split");
    let rows = workspace
        .units
        .iter()
        .map(|unit| (unit.crate_name.as_str(), unit.deps.join(",")))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            ("jetc_base", String::new()),
            ("jetc_alpha_beta", "jetc_base".to_string()),
            ("jetc_gamma", "jetc_alpha_beta".to_string()),
        ]
    );
    let merged = &workspace.units[1].source;
    assert!(merged.starts_with("#![allow(warnings)]\nuse jet_runtime::*;\nuse jetc_base::*;\n"));
    assert!(merged.contains("pub fn __jet_alpha()") && merged.contains("pub fn __jet_beta()"));
    assert!(workspace.runtime_lib.contains("pub fn runtime_item()"));
    assert!(workspace.main.contains(
        "use jet_runtime::*;\nuse jetc_base::*;\nuse jetc_alpha_beta::*;\nuse jetc_gamma::*;\n"
    ));
    assert!(workspace.main.ends_with("fn main() { __jet_gamma(); }\n"));
    assert!(!workspace.main.contains("__jet_alpha"));
}

/// A program without unit sections still links its runtime/Core block as
/// the separate `jet_runtime` crate (shared by every program with the same
/// block); a source without a runtime block stays one crate.
#[test]
fn backend_workspace_splits_runtime_without_unit_sections() {
    let workspace = split_backend_workspace("// jet:cached-runtime-begin\nfn f() {}\n// jet:cached-runtime-end\nfn main() { f(); }\n")
        .expect("runtime block split");
    assert!(workspace.units.is_empty());
    assert!(workspace.runtime_lib.contains("pub fn f()"));
    assert!(workspace.main.contains("use jet_runtime::*;\n"));
    assert!(workspace.main.ends_with("fn main() { f(); }\n"));
    assert!(!workspace.main.contains("fn f()"));
    assert!(split_backend_workspace("fn main() {}\n").is_none());
}

/// `shard_unit` packs cold code (cold-trait impl methods, constant tables,
/// giant functions) into cold shards apart from hot code, so the release
/// profile can leave only the cold code unoptimized.
#[test]
fn backend_shards_separate_cold_code() {
    let steps = "    __jet_n += 1;\n".repeat(200);
    let giant = "    __jet_n += 1;\n".repeat(backend_cold_function_bytes() / 16);
    let text = format!(
        "pub struct __jet_T {{ __jet_n: i64 }}\n\
         pub fn __jet_hot(__jet_x: i64) -> i64 {{ __jet_x + 1 }}\n\
         pub fn __jet_list(__jet_x: i64) -> Vec<i64> {{ vec![__jet_x] }}\n\
         #[inline] pub fn __jet_TABLE() -> Vec<i64> {{ vec![1, 2, 3] }}\n\
         pub fn __jet_ROWS_rows_0(rows: &mut Vec<i64>) {{\n    rows.push(1);\n    rows.push(2);\n}}\n\
         impl __jet_Decode for __jet_T {{\n    fn jet_decode(__jet_tree: &i64) -> i64 {{\n        let mut __jet_n = *__jet_tree;\n{steps}        __jet_n\n    }}\n}}\n\
         impl __jet_Equatable for __jet_T {{\n    fn equals(&self, __jet_other: &__jet_T) -> bool {{\n        let mut __jet_n = self.__jet_n;\n{steps}        __jet_n == __jet_other.__jet_n\n    }}\n}}\n\
         pub fn __jet_giant(mut __jet_n: i64) -> i64 {{\n{giant}    __jet_n\n}}\n"
    );
    let sharded = shard_unit(&text, 64 << 10).expect("a unit over the shard size shards");
    let texts = |shards: &[(usize, String)]| shards.iter().map(|(_, text)| text.as_str()).collect::<String>();
    let (hot, cold) = (texts(&sharded.shards), texts(&sharded.cold_shards));
    for name in ["fn __jet_hot(", "fn __jet_list(", "fn __jet_impl___jet_T___jet_Equatable_equals("] {
        assert!(hot.contains(name) && !cold.contains(name), "`{name}` is hot");
    }
    for name in [
        "fn __jet_TABLE(",
        "fn __jet_ROWS_rows_0(",
        "fn __jet_giant(",
        "fn __jet_impl___jet_T___jet_Decode_jet_decode(",
    ] {
        assert!(cold.contains(name) && !hot.contains(name), "`{name}` is cold");
    }
}

/// `shard_unit` places a function by its symbol alone: growing or adding
/// other functions leaves every function's shard index unchanged while the
/// shard count holds.
#[test]
fn backend_shard_placement_depends_only_on_the_symbol() {
    let body = |lines: usize| "    __jet_n += 1;\n".repeat(lines);
    let unit = |first_lines: usize, extra: bool| {
        let mut text = format!("pub fn __jet_first(mut __jet_n: i64) -> i64 {{\n{}    __jet_n\n}}\n", body(first_lines));
        if extra {
            text.push_str(&format!("pub fn __jet_extra(mut __jet_n: i64) -> i64 {{\n{}    __jet_n\n}}\n", body(8)));
        }
        for index in 0..64 {
            text.push_str(&format!("pub fn __jet_f{index}(mut __jet_n: i64) -> i64 {{\n{}    __jet_n\n}}\n", body(16)));
        }
        shard_unit(&text, 5 << 10).expect("a unit over the shard size shards")
    };
    let placement = |sharded: &ShardedUnit| {
        (0..64)
            .map(|index| {
                let name = format!("fn __jet_f{index}(");
                let shards = sharded.shards.iter().filter(|(_, text)| text.contains(&name)).collect::<Vec<_>>();
                assert_eq!(shards.len(), 1, "`{name}` is in exactly one shard");
                shards[0].0
            })
            .collect::<Vec<_>>()
    };
    let base = unit(16, false);
    assert!(base.shards.len() > 1, "the unit spreads over several shards");
    assert_eq!(placement(&base), placement(&unit(40, false)));
    assert_eq!(placement(&base), placement(&unit(16, true)));
}

/// The release profile leaves only cold shards unoptimized; the dev profile
/// overrides every runtime and unit crate alike.
#[test]
fn backend_unit_profiles_leave_only_cold_shards_unoptimized() {
    let shard = |name: &str, cold| BackendShard {
        name: name.to_string(),
        source: String::new(),
        cold,
    };
    let workspace = BackendWorkspace {
        runtime_lib: String::new(),
        units: vec![BackendUnit {
            crate_name: "jetc_sema".to_string(),
            deps: Vec::new(),
            source: String::new(),
            shards: vec![shard("jetc_sema_s0", false), shard("jetc_sema_cold0", true)],
        }],
        main: String::new(),
    };
    let tables = |profiles: &str| {
        profiles.lines().filter(|line| line.starts_with('[')).map(str::to_string).collect::<Vec<_>>()
    };
    let release = backend_unit_profiles(&workspace, Some(BACKEND_RELEASE_PROFILE_NAME));
    assert_eq!(tables(&release), ["[profile.jet-stage-release.package.jetc_sema_cold0]"]);
    assert!(release.contains("\nopt-level = 0\n"));
    let dev = backend_unit_profiles(&workspace, None);
    assert_eq!(
        tables(&dev),
        ["jet_runtime", "jetc_sema", "jetc_sema_s0", "jetc_sema_cold0"].map(|name| format!("[profile.dev.package.{name}]"))
    );
    assert!(!dev.contains("opt-level"));
}

/// Write the backend Cargo project `build_backend_artifact_with_native_library`
/// builds (main crate, unit-split workspace, compiler image, manifest, identity
/// build script) and return its manifest path. `compiler_image` is the image a
/// compiler artifact's source embeds with `include_bytes!("compiler.image")`:
/// it is written beside `src/main.rs`.
fn write_backend_project(
    repo: &Path,
    project: &Path,
    package_name: &str,
    source: &str,
    compiler_image: Option<&[u8]>,
    native_library: Option<(&Path, &str)>,
    ffi: Option<&BackendFfiCrate>,
) -> PathBuf {
    let source_dir = project.join("src");
    fs::create_dir_all(&source_dir).unwrap_or_else(|error| {
        panic!("cannot create backend project `{}`: {error}", project.display())
    });
    let source_path = source_dir.join("main.rs");
    let manifest_path = project.join("Cargo.toml");
    let build_script_path = project.join("build.rs");
    let profile = backend_release_profile();
    // The build identity hashes the complete emitted text (`generated.rs`):
    // every unit crate below is a pure function of it.
    let generated_path = project.join("generated.rs");
    fs::write(&generated_path, source).unwrap_or_else(|error| {
        panic!("cannot write backend source `{}`: {error}", generated_path.display())
    });
    // A program with a runtime block builds as a workspace: the runtime
    // crate and one crate per unit live in a stable directory per artifact
    // (outside the repository's cargo workspace), rewritten only when their
    // bytes change, so unchanged crates stay built across runs and programs
    // that share a runtime/Core block share its built crate.
    let (main_source, unit_dependencies, unit_profiles) = match split_backend_workspace(source) {
        Some(workspace) => {
            let units_root = home_path()
                .join(BOOTSTRAP_UNITS_RELATIVE)
                .join(package_name);
            let dependencies = write_backend_workspace(&units_root, &workspace, ffi);
            let profiles = backend_unit_profiles(&workspace, profile);
            (workspace.main, dependencies, profiles)
        }
        None => (source.to_string(), String::new(), String::new()),
    };
    let ffi_dependency = ffi.map_or_else(String::new, BackendFfiCrate::dependency);
    fs::write(&source_path, main_source).unwrap_or_else(|error| {
        panic!("cannot write backend source `{}`: {error}", source_path.display())
    });
    let image_path = source_dir.join("compiler.image");
    match compiler_image {
        // The compiler restores this image from its own executable without
        // hashing it, so the archive checksum is verified here, once per file.
        Some(image) => {
            crate::compiler_bootstrap_compiler_image::verify_compiler_image_checksum(image)
                .unwrap_or_else(|error| {
                    panic!("backend compiler image for `{}` is corrupt: {error}", image_path.display())
                });
            fs::write(&image_path, image).unwrap_or_else(|error| {
                panic!("cannot write backend compiler image `{}`: {error}", image_path.display())
            })
        }
        None => match fs::remove_file(&image_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("cannot remove stale backend compiler image `{}`: {error}", image_path.display()),
        },
    }
    // The backend project is its own workspace root, so it starts from the
    // repository lockfile, keeping every dependency at the version the
    // repository builds with.
    let manifest = format!(
        "[package]\nname = {:?}\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n\n[dependencies]\njet = {{ path = {:?} }}\njet-codegen = {{ path = {:?} }}\njet-driver = {{ path = {:?} }}\njet-evalhost = {{ path = {:?} }}\njet-foundation = {{ path = {:?} }}\njet-net = {{ path = {:?} }}\njet-pkg-model = {{ path = {:?} }}\njet-rt = {{ path = {:?} }}\njet-store = {{ path = {:?} }}\n{ffi_dependency}{unit_dependencies}\n{unit_profiles}{build_profile}",
        package_name,
        repo.display().to_string(),
        // The linked Host modules name these crates directly (codegen's native
        // loop cursor key, the package model's devtools policy, the record store).
        repo.join("crates/jet-codegen").display().to_string(),
        repo.join("crates/jet-driver").display().to_string(),
        repo.join("crates/jet-evalhost").display().to_string(),
        repo.join("crates/jet-foundation").display().to_string(),
        repo.join("crates/jet-net").display().to_string(),
        repo.join("crates/jet-pkg-model").display().to_string(),
        repo.join("crates/jet-rt").display().to_string(),
        repo.join("crates/jet-store").display().to_string(),
        ffi_dependency = ffi_dependency,
        unit_dependencies = unit_dependencies,
        unit_profiles = unit_profiles,
        build_profile = if profile.is_some() { BACKEND_RELEASE_PROFILE } else { BACKEND_DEV_HOST_PROFILE },
    );
    fs::copy(repo.join("Cargo.lock"), project.join("Cargo.lock")).unwrap_or_else(|error| {
        panic!("cannot seed backend lockfile in `{}`: {error}", project.display())
    });
    fs::write(&manifest_path, manifest).unwrap_or_else(|error| {
        panic!("cannot write backend manifest `{}`: {error}", manifest_path.display())
    });
    fs::write(
        &build_script_path,
        backend_identity_build_script(repo, native_library),
    )
    .unwrap_or_else(|error| {
        panic!(
            "cannot write backend identity build script `{}`: {error}",
            build_script_path.display()
        )
    });
    manifest_path
}

/// I9 parity of the Jet host's runtime prefix (Compiler/JetCodegen
/// Emit/RuntimePrefix.jet) with MIRRust `emit_mir_runtime_text`. Driven by
/// the Jet-hosted build proof: `JET_RUNTIME_PREFIX_CASES` holds
/// `;`-separated `<release|test>=<source.jet>=<hosted .rs>` cases, where the
/// hosted `.rs` is what the Jet-hosted `jet build --release` / `jet test`
/// wrote. The Rust compiler emits the same program through the profile that
/// command uses; its text must begin with the hosted file's prefix (every
/// byte before the Jet emitter's header), and that prefix must end where the
/// runtime block does.
#[test]
#[ignore = "driven by the Jet-hosted runtime-prefix parity proof"]
fn jet_hosted_runtime_prefix_matches_rust() {
    const JET_ITEMS_HEADER: &str =
        "// Generated by Jet MIR Rust source emission; checked MIR is authoritative.";
    let cases = std::env::var("JET_RUNTIME_PREFIX_CASES")
        .expect("JET_RUNTIME_PREFIX_CASES names the parity cases");
    let mut checked = 0usize;
    for case in cases.split(';').filter(|case| !case.is_empty()) {
        let fields = case.split('=').collect::<Vec<_>>();
        let [kind, source_path, hosted_path] = fields.as_slice() else {
            panic!("parity case `{case}` is not `<kind>=<source>=<hosted .rs>`");
        };
        let source = std::fs::read_to_string(source_path)
            .unwrap_or_else(|error| panic!("cannot read {source_path}: {error}"));
        let rust = match *kind {
            "release" => crate::compile_with_target_and_gates_and_profile(
                &source,
                source_path,
                crate::Policy::GateSet::default(),
                None,
                "release",
            )
            .unwrap_or_else(|diagnostics| panic!("{source_path}: {diagnostics:?}"))
            .rust,
            "test" => crate::compile_tests_with_path(&source, source_path)
                .unwrap_or_else(|diagnostics| panic!("{source_path}: {diagnostics:?}"))
                .0,
            other => panic!("parity case kind `{other}` is neither `release` nor `test`"),
        };
        let hosted = std::fs::read_to_string(hosted_path)
            .unwrap_or_else(|error| panic!("cannot read {hosted_path}: {error}"));
        let prefix_len = hosted
            .find(JET_ITEMS_HEADER)
            .unwrap_or_else(|| panic!("{hosted_path} has no Jet emitter header"));
        let prefix = &hosted[..prefix_len];
        assert!(
            prefix.ends_with("// jet:cached-core-end\n\n"),
            "{hosted_path}: the hosted prefix does not end at the runtime block"
        );
        if !rust.starts_with(prefix) {
            let at = rust
                .bytes()
                .zip(prefix.bytes())
                .position(|(left, right)| left != right)
                .unwrap_or(rust.len().min(prefix.len()));
            let window = |text: &str| {
                let mut start = at.saturating_sub(120).min(text.len());
                let mut end = (at + 120).min(text.len());
                while !text.is_char_boundary(start) {
                    start -= 1;
                }
                while !text.is_char_boundary(end) {
                    end += 1;
                }
                text[start..end].to_string()
            };
            panic!(
                "{kind} {source_path}: the hosted prefix ({} bytes) and the Rust program ({} bytes) differ at byte {at}\nrust:   {:?}\nhosted: {:?}",
                prefix.len(),
                rust.len(),
                window(&rust),
                window(prefix),
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "JET_RUNTIME_PREFIX_CASES selected no case");
}
