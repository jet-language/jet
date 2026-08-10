mod common;

use jet::Comptime::Build::{
    ActionCache, ActionCacheProvenance, ActionCacheStatus, ActionOutcome, ActionSpec,
    ActionInputSnapshot, ActionKey, ActionOutputRecord, ActionResultRecord, BuildCapability,
    BuildContext, BuildError, BuildExecutionEvent, BuildGraphSubject, BuildPath, BuildPolicy,
    BuildProvenance, BuildResourcePool, CacheHitReason, CacheMissReason, ContentDigest,
    FrontEndCompletion, GeneratedModuleSpec, LegacyWrapperKind, LegacyWrapperSpec, LinkerIdentity,
    LocalCas, LockRecord,
    PluginContribution, ProbeKind, ProbeSpec, ProvenanceSource, RemoteActionRequest,
    RemoteBuildBinding, RemoteCacheError, RemoteCachePolicy, RemoteCacheTransport,
    RemoteDeniedReason, RemoteExecutionRequest, RemoteExecutionResult, RemoteSandboxProof,
    ReproducibilityClass, SdkIdentity,
    SigningIdentitySpec, TargetKind, TargetSpec, ToolchainRole, ToolchainSpec,
    WasmComponentPluginSpec, BUILD_PLUGIN_API_VERSION, execute_build_plan_with_front_end_and_remote,
    read_packaged_file_bounded,
};
use std::fs;
use std::sync::{Arc, Barrier, Mutex};

static REMOTE_HOST_ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn registers_typed_targets_and_default_plan() {
    let mut b = BuildContext::new();
    let gen = b
        .action(
            "generate-assets",
            ActionSpec::cached(["jet-tool", "pack"])
                .with_inputs(["assets/sprite.png"])
                .with_outputs([".jet/generated/assets.jet"])
                .with_cap(BuildCapability::FS),
        )
        .unwrap();
    let lib = b
        .add_library(
            "engine",
            TargetSpec::new()
                .with_source("src/engine.jet")
                .with_action(gen),
        )
        .unwrap();
    let app = b
        .add_executable(
            "game",
            TargetSpec::new().with_source("src/main.jet").with_dep(lib),
        )
        .unwrap();
    b.add_test("unit", TargetSpec::new().with_dep(app)).unwrap();
    b.add_bench("frame-time", TargetSpec::new().with_dep(app))
        .unwrap();
    b.add_asset_bundle("assets", TargetSpec::new().with_action(gen))
        .unwrap();
    b.add_doc("manual", TargetSpec::new().with_source("docs/manual.md"))
        .unwrap();
    b.add_install("install", TargetSpec::new().with_dep(app))
        .unwrap();
    b.add_package("package", TargetSpec::new().with_dep(app))
        .unwrap();
    b.add_publish("publish", TargetSpec::new().with_dep(app))
        .unwrap();

    let plan = b.plan_with_default(app).unwrap();

    assert_eq!(plan.default_target(), Some(app.into()));
    assert_eq!(plan.target(app).unwrap().kind, TargetKind::Executable);
    assert_eq!(
        plan.target(app).unwrap().toolchain,
        b.default_host_toolchain()
    );
    assert_eq!(plan.default_host_toolchain().role, ToolchainRole::Host);
    assert_eq!(
        plan.default_host_toolchain().provenance.source,
        ProvenanceSource::InferredHost
    );
    assert_eq!(plan.target(lib).unwrap().kind, TargetKind::Library);
    assert_eq!(plan.targets_by_kind(TargetKind::AssetBundle).len(), 1);
    assert_eq!(plan.actions().len(), 1);
    assert_eq!(
        plan.action(gen).unwrap().outputs[0].as_str(),
        ".jet/generated/assets.jet"
    );
}

#[test]
fn selected_default_executes_only_its_target_dependency_closure() {
    let mut b = BuildContext::new();
    let selected_action = b.action(
        "selected",
        ActionSpec::cached(["tool"]).with_outputs(["selected.out"]),
    ).unwrap();
    let unrelated_action = b.action(
        "unrelated",
        ActionSpec::cached(["tool"]).with_outputs(["unrelated.out"]),
    ).unwrap();
    let selected = b.add_executable(
        "selected",
        TargetSpec::new().with_source("main.jet").with_action(selected_action),
    ).unwrap();
    b.add_test(
        "unrelated",
        TargetSpec::new().with_source("other.jet").with_action(unrelated_action),
    ).unwrap();
    let plan = b.plan_with_default(selected).unwrap();
    let model = plan.execution_model().unwrap();
    assert_eq!(model.nodes.len(), 1);
    assert_eq!(model.nodes[0].name, "selected");
    assert_eq!(plan.selected_sources().unwrap()[0].as_str(), "main.jet");
}

#[test]
fn records_explicit_toolchains_sdks_linkers_and_signers() {
    let mut b = BuildContext::new();
    let sdk_lock = LockRecord::new("sdk:core.embedded-sdk#1.4", "sha256:sdk");
    let linker_lock = LockRecord::new("linker:lld#18", "sha256:linker");
    let toolchain_lock = LockRecord::new("toolchain:aarch64-linux-musl", "sha256:toolchain");
    let toolchain = b
        .toolchain(
            "arm-musl",
            ToolchainSpec::target(
                "aarch64-linux-musl",
                BuildProvenance::jetpack_dependency(
                    "toolchain.aarch64-linux-musl#2026.07",
                    toolchain_lock,
                ),
            )
            .with_host_triple("x86_64-linux")
            .with_sdk(SdkIdentity::new(
                "core.embedded-sdk",
                "1.4",
                BuildProvenance::jetpack_dependency("core.embedded-sdk#1.4", sdk_lock),
            ))
            .with_linker(LinkerIdentity::new(
                "lld",
                BuildProvenance::jetpack_dependency("lld#18", linker_lock),
            )),
        )
        .unwrap();
    let signer = b
        .signing_identity(
            "mac-release",
            SigningIdentitySpec::new(
                "developer-id:ACME",
                BuildProvenance::user_declared(
                    "keychain:mac-release",
                    Some(LockRecord::new("signing:mac-release", "sha256:signer")),
                ),
            ),
        )
        .unwrap();
    let link = b
        .action(
            "link-firmware",
            ActionSpec::cached(["ld.lld", "-o", "build/sensor"])
                .with_outputs(["build/sensor"])
                .with_cap(BuildCapability::Exec)
                .with_toolchain(toolchain),
        )
        .unwrap();
    let app = b
        .add_executable(
            "sensor",
            TargetSpec::new()
                .with_source("src/sensor.jet")
                .with_action(link)
                .with_toolchain(toolchain)
                .with_signing_identity(signer),
        )
        .unwrap();

    let plan = b.plan_with_default(app).unwrap();
    let recorded_toolchain = plan.toolchain(toolchain).unwrap();
    assert_eq!(recorded_toolchain.role, ToolchainRole::Target);
    assert_eq!(recorded_toolchain.host_triple, "x86_64-linux");
    assert_eq!(recorded_toolchain.target_triple, "aarch64-linux-musl");
    assert_eq!(
        recorded_toolchain
            .sdk
            .as_ref()
            .unwrap()
            .provenance
            .lock
            .as_ref()
            .unwrap()
            .digest,
        "sha256:sdk"
    );
    assert_eq!(
        recorded_toolchain
            .linker
            .as_ref()
            .unwrap()
            .provenance
            .lock
            .as_ref()
            .unwrap()
            .digest,
        "sha256:linker"
    );
    assert_eq!(plan.target(app).unwrap().toolchain, toolchain);
    assert_eq!(plan.action(link).unwrap().toolchain, toolchain);
    assert_eq!(plan.target(app).unwrap().signing_identity, Some(signer));
    assert_eq!(
        plan.signing_identity(signer).unwrap().label,
        "developer-id:ACME"
    );
    assert_eq!(
        plan.signing_identity(signer)
            .unwrap()
            .provenance
            .lock
            .as_ref()
            .unwrap()
            .digest,
        "sha256:signer"
    );
}

#[test]
fn typed_probes_record_reproducibility_and_provenance() {
    let mut b = BuildContext::new();
    let cc = b.find_program("cc", "cc").unwrap();
    let sqlite = b
        .probe(
            "sqlite",
            ProbeSpec::pkg_config("sqlite3")
                .with_min_version("3.42")
                .with_reproducibility(ReproducibilityClass::Reproducible)
                .with_provenance(BuildProvenance::jetpack_dependency(
                    "pkg-config:sqlite3#3.42",
                    LockRecord::new("probe:sqlite3", "sha256:sqlite"),
                )),
        )
        .unwrap();
    let header = b.header_check("sqlite-header", "sqlite3.h").unwrap();
    let compile = b
        .compile_check(
            "sqlite-wal",
            "sqlite-wal",
            ["sqlite3.h"],
            "sqlite3_wal_checkpoint_v2",
        )
        .unwrap();
    let lib = b
        .add_library(
            "sqlite-bridge",
            TargetSpec::new()
                .with_probe(cc)
                .with_probe(sqlite)
                .with_probe(header)
                .with_probe(compile),
        )
        .unwrap();

    let plan = b.plan_with_default(lib).unwrap();
    assert_eq!(plan.probes().len(), 4);
    assert_eq!(
        plan.probe(cc).unwrap().reproducibility,
        ReproducibilityClass::Ambient
    );
    assert_eq!(
        plan.probe(sqlite).unwrap().reproducibility,
        ReproducibilityClass::Reproducible
    );
    assert_eq!(
        plan.probe(sqlite)
            .unwrap()
            .provenance
            .lock
            .as_ref()
            .unwrap()
            .digest,
        "sha256:sqlite"
    );
    assert!(matches!(
        plan.probe(sqlite).unwrap().kind,
        ProbeKind::PkgConfig { ref package, .. } if package == "sqlite3"
    ));
    assert!(matches!(
        plan.probe(header).unwrap().kind,
        ProbeKind::HeaderCheck { ref header } if header == "sqlite3.h"
    ));
    assert!(matches!(
        plan.probe(compile).unwrap().kind,
        ProbeKind::CompileCheck { ref name, .. } if name == "sqlite-wal"
    ));
    assert_eq!(
        plan.target(lib).unwrap().probes,
        vec![cc, sqlite, header, compile]
    );
}

#[test]
fn declared_target_toolchain_wins_over_ambient_host_tool() {
    let false_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join("false"))
        .find(|path| path.is_file())
        .expect("the test host needs a `false` executable");
    let declared_false = fs::canonicalize(&false_path).unwrap();
    let mut b = BuildContext::new();
    let toolchain = b
        .toolchain(
            "declared-native",
            ToolchainSpec::target(
                "x86_64-linux-gnu",
                BuildProvenance::jetpack_dependency(
                    "toolchain.native#1",
                    LockRecord::new("toolchain:native", "sha256:native"),
                ),
            )
            .with_host_triple("x86_64-linux-gnu")
            .with_tool("cc", false_path.to_string_lossy()),
        )
        .unwrap();
    let probe = b
        .probe(
            "declared-cc",
            ProbeSpec::header_check("stddef.h").with_toolchain(toolchain),
        )
        .unwrap();
    let target = b
        .add_library(
            "declared-native-probe",
            TargetSpec::new()
                .with_toolchain(toolchain)
                .with_probe(probe),
        )
        .unwrap();
    let plan = b.plan_with_default(target).unwrap();
    let root = std::env::temp_dir().join(format!(
        "jet-declared-toolchain-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let grants = [BuildCapability::Exec]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let error = execute_build_plan_with_front_end_and_remote(
        &plan,
        &root,
        &grants,
        FrontEndCompletion::all_complete(),
        None,
    )
    .unwrap_err();
    assert!(
        matches!(
            &error,
            jet::Comptime::Build::BuildExecutionError::ProbeFailed { detail, .. }
                if detail.contains(declared_false.to_string_lossy().as_ref())
        ),
        "declared target tool must be used instead of ambient PATH: {error:?}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn foreign_toolchain_probe_and_signer_handles_are_rejected() {
    let mut a = BuildContext::new();
    let mut b = BuildContext::new();
    let toolchain = b
        .toolchain(
            "other",
            ToolchainSpec::target(
                "wasm32-wasi",
                BuildProvenance::jetpack_dependency(
                    "toolchain.wasm32-wasi#2026.07",
                    LockRecord::new("toolchain:wasm32-wasi", "sha256:wasm"),
                ),
            ),
        )
        .unwrap();
    let signer = b
        .signing_identity(
            "other-signer",
            SigningIdentitySpec::new(
                "developer-id:OTHER",
                BuildProvenance::user_declared("keychain:other", None),
            ),
        )
        .unwrap();
    let probe = b.find_program("other-cc", "cc").unwrap();

    let err = a
        .add_executable("bad-toolchain", TargetSpec::new().with_toolchain(toolchain))
        .unwrap_err();
    assert_eq!(err, BuildError::UnknownToolchain(toolchain.id()));
    let err = a
        .add_executable(
            "bad-signer",
            TargetSpec::new().with_signing_identity(signer),
        )
        .unwrap_err();
    assert_eq!(err, BuildError::UnknownSigningIdentity(signer.id()));
    let err = a
        .add_library("bad-probe", TargetSpec::new().with_probe(probe))
        .unwrap_err();
    assert_eq!(err, BuildError::UnknownProbe(probe.id()));
}

#[test]
fn invalid_toolchain_probe_and_signer_specs_do_not_reserve_names() {
    let mut b = BuildContext::new();
    let bad_toolchain = b
        .toolchain(
            "cross",
            ToolchainSpec::target("", BuildProvenance::inferred_host()),
        )
        .unwrap_err();
    assert_eq!(
        bad_toolchain,
        BuildError::EmptyToolchainTriple("cross".to_string())
    );
    let toolchain = b
        .toolchain(
            "cross",
            ToolchainSpec::target(
                "wasm32-wasi",
                BuildProvenance::jetpack_dependency(
                    "toolchain.wasm32-wasi#2026.07",
                    LockRecord::new("toolchain:wasm32-wasi", "sha256:wasm"),
                ),
            ),
        )
        .unwrap();

    let bad_signer = b
        .signing_identity(
            "release",
            SigningIdentitySpec::new("", BuildProvenance::user_declared("keychain:release", None)),
        )
        .unwrap_err();
    assert_eq!(
        bad_signer,
        BuildError::EmptyIdentityField("release".to_string())
    );
    b.signing_identity(
        "release",
        SigningIdentitySpec::new(
            "developer-id:ACME",
            BuildProvenance::user_declared("keychain:release", None),
        ),
    )
    .unwrap();

    let bad_probe = b.probe("sqlite", ProbeSpec::pkg_config("")).unwrap_err();
    assert_eq!(bad_probe, BuildError::EmptyProbeField("sqlite".to_string()));
    b.probe(
        "sqlite",
        ProbeSpec::pkg_config("sqlite3").with_toolchain(toolchain),
    )
    .unwrap();
}

#[test]
fn duplicate_target_or_action_names_are_rejected() {
    let mut b = BuildContext::new();
    b.add_executable("app", TargetSpec::new()).unwrap();
    let err = b.add_library("app", TargetSpec::new()).unwrap_err();
    assert_eq!(err, BuildError::DuplicateTargetName("app".to_string()));

    b.action(
        "codegen",
        ActionSpec::cached(["tool"]).with_outputs(["out.jet"]),
    )
    .unwrap();
    let err = b
        .action(
            "codegen",
            ActionSpec::cached(["tool"]).with_outputs(["other.jet"]),
        )
        .unwrap_err();
    assert_eq!(err, BuildError::DuplicateActionName("codegen".to_string()));
}

#[test]
fn cached_actions_need_outputs() {
    let mut b = BuildContext::new();
    let err = b
        .action("probe", ActionSpec::cached(["cc", "--version"]))
        .unwrap_err();
    assert_eq!(
        err,
        BuildError::CachedActionWithoutOutputs("probe".to_string())
    );
}

#[test]
fn outputless_commands_are_explicit_uncached_phony_actions() {
    let mut b = BuildContext::new();
    let clean = b
        .action(
            "clean-generated",
            ActionSpec::uncached_phony(["rm", "-rf", ".jet/generated"])
                .with_cap(BuildCapability::FS),
        )
        .unwrap();

    let plan = b.plan().unwrap();
    let action = plan.action(clean).unwrap();
    assert_eq!(action.cache, ActionCache::UncachedPhony);
    assert!(action.outputs.is_empty());
    assert_eq!(plan.phony_actions(), vec![action]);
}

#[test]
fn phony_actions_need_a_declared_capability() {
    let mut b = BuildContext::new();
    let err = b
        .action(
            "clean-generated",
            ActionSpec::uncached_phony(["rm", "-rf", ".jet/generated"]),
        )
        .unwrap_err();
    assert_eq!(
        err,
        BuildError::PhonyActionWithoutCaps("clean-generated".to_string())
    );
}

#[test]
fn action_outputs_have_one_owner_in_the_plan() {
    let mut b = BuildContext::new();
    b.action(
        "generate-a",
        ActionSpec::cached(["tool-a"]).with_outputs([".jet/generated/out.jet"]),
    )
    .unwrap();
    b.action(
        "generate-b",
        ActionSpec::cached(["tool-b"]).with_outputs([".jet/generated/out.jet"]),
    )
    .unwrap();

    let err = b.plan().unwrap_err();
    assert_eq!(
        err,
        BuildError::DuplicateBuildOutput {
            output: ".jet/generated/out.jet".to_string(),
            first_action: "generate-a".to_string(),
            second_action: "generate-b".to_string(),
        }
    );
}

#[test]
fn action_keys_are_deterministic_and_cover_cache_contract() {
    fn key(
        argv: &[&str],
        env_value: &str,
        input: &str,
        output: &str,
        cap: BuildCapability,
        target_triple: &str,
        probe_package: &str,
        signer_label: &str,
        label_value: &str,
    ) -> String {
        let mut b = BuildContext::new();
        let toolchain = b
            .toolchain(
                "tc",
                ToolchainSpec::target(
                    target_triple,
                    BuildProvenance::jetpack_dependency(
                        format!("toolchain.{target_triple}#2026.07"),
                        LockRecord::new(format!("toolchain:{target_triple}"), "sha256:tc"),
                    ),
                ),
            )
            .unwrap();
        let probe = b
            .probe(
                "probe",
                ProbeSpec::pkg_config(probe_package)
                    .with_reproducibility(ReproducibilityClass::Reproducible)
                    .with_provenance(BuildProvenance::jetpack_dependency(
                        format!("pkg-config:{probe_package}#1"),
                        LockRecord::new(format!("probe:{probe_package}"), "sha256:probe"),
                    )),
            )
            .unwrap();
        let signer = b
            .signing_identity(
                "signer",
                SigningIdentitySpec::new(
                    signer_label,
                    BuildProvenance::user_declared(
                        format!("keychain:{signer_label}"),
                        Some(LockRecord::new("signing:release", "sha256:signer")),
                    ),
                ),
            )
            .unwrap();
        let action = b
            .action(
                "compile",
                ActionSpec::cached(argv.iter().copied())
                    .with_inputs([input])
                    .with_outputs([output])
                    .with_env("MODE", env_value)
                    .with_cap(cap)
                    .with_toolchain(toolchain)
                    .with_probe(probe)
                    .with_signing_identity(signer)
                    .with_label("profile", label_value),
            )
            .unwrap();
        let plan = b.plan().unwrap();
        plan.action_key(action).unwrap().as_str().to_string()
    }

    let base = key(
        &["jetc", "--emit", "obj"],
        "release",
        "src/main.jet",
        "build/main.o",
        BuildCapability::Exec,
        "x86_64-linux",
        "sqlite3",
        "developer-id:ACME",
        "release",
    );
    assert_eq!(
        base,
        key(
            &["jetc", "--emit", "obj"],
            "release",
            "src/main.jet",
            "build/main.o",
            BuildCapability::Exec,
            "x86_64-linux",
            "sqlite3",
            "developer-id:ACME",
            "release",
        )
    );
    assert_ne!(
        base,
        key(
            &["jetc", "--emit", "asm"],
            "release",
            "src/main.jet",
            "build/main.o",
            BuildCapability::Exec,
            "x86_64-linux",
            "sqlite3",
            "developer-id:ACME",
            "release",
        )
    );
    assert_ne!(
        base,
        key(
            &["jetc", "--emit", "obj"],
            "debug",
            "src/main.jet",
            "build/main.o",
            BuildCapability::Exec,
            "x86_64-linux",
            "sqlite3",
            "developer-id:ACME",
            "release",
        )
    );
    assert_ne!(
        base,
        key(
            &["jetc", "--emit", "obj"],
            "release",
            "src/lib.jet",
            "build/main.o",
            BuildCapability::Exec,
            "x86_64-linux",
            "sqlite3",
            "developer-id:ACME",
            "release",
        )
    );
    assert_ne!(
        base,
        key(
            &["jetc", "--emit", "obj"],
            "release",
            "src/main.jet",
            "build/lib.o",
            BuildCapability::Exec,
            "x86_64-linux",
            "sqlite3",
            "developer-id:ACME",
            "release",
        )
    );
    assert_ne!(
        base,
        key(
            &["jetc", "--emit", "obj"],
            "release",
            "src/main.jet",
            "build/main.o",
            BuildCapability::FS,
            "x86_64-linux",
            "sqlite3",
            "developer-id:ACME",
            "release",
        )
    );
    assert_ne!(
        base,
        key(
            &["jetc", "--emit", "obj"],
            "release",
            "src/main.jet",
            "build/main.o",
            BuildCapability::Exec,
            "aarch64-linux-musl",
            "sqlite3",
            "developer-id:ACME",
            "release",
        )
    );
    assert_ne!(
        base,
        key(
            &["jetc", "--emit", "obj"],
            "release",
            "src/main.jet",
            "build/main.o",
            BuildCapability::Exec,
            "x86_64-linux",
            "openssl",
            "developer-id:ACME",
            "release",
        )
    );
    assert_ne!(
        base,
        key(
            &["jetc", "--emit", "obj"],
            "release",
            "src/main.jet",
            "build/main.o",
            BuildCapability::Exec,
            "x86_64-linux",
            "sqlite3",
            "developer-id:OTHER",
            "release",
        )
    );
    assert_ne!(
        base,
        key(
            &["jetc", "--emit", "obj"],
            "release",
            "src/main.jet",
            "build/main.o",
            BuildCapability::Exec,
            "x86_64-linux",
            "sqlite3",
            "developer-id:ACME",
            "coverage",
        )
    );
}

#[test]
fn local_cas_round_trips_blobs_and_restores_declared_outputs() {
    let root = std::env::temp_dir().join(format!(
        "jet_build_cache_{}_{}",
        std::process::id(),
        "restore"
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("work/build")).unwrap();

    let cas = LocalCas::new(root.join("cache"));
    let digest = cas.put_blob(b"hello cache").unwrap();
    assert_eq!(cas.read_blob(&digest).unwrap(), b"hello cache");

    let mut b = BuildContext::new();
    let action = b
        .action(
            "emit",
            ActionSpec::cached(["jetc", "src/main.jet"]).with_outputs(["build/out.txt"]),
        )
        .unwrap();
    let plan = b.plan().unwrap();
    let key = plan.action_key(action).unwrap();
    fs::write(root.join("work/build/out.txt"), "compiled bytes").unwrap();

    let record = cas
        .capture_declared_outputs(
            &root.join("work"),
            plan.action(action).unwrap(),
            key,
            ActionOutcome::Succeeded { exit_code: 0 },
            ActionCacheProvenance::miss(CacheMissReason::NoLocalActionRecord),
        )
        .unwrap();
    fs::write(root.join("work/build/out.txt"), "stale").unwrap();

    cas.restore_action_outputs(&root.join("work"), plan.action(action).unwrap(), &record)
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.join("work/build/out.txt")).unwrap(),
        "compiled bytes"
    );
    assert_eq!(record.outputs[0].byte_len, "compiled bytes".len() as u64);

    let hex = digest.as_str().strip_prefix("sha256:").unwrap();
    let blob = cas
        .root()
        .join("blobs")
        .join("sha256")
        .join(&hex[..2])
        .join(&hex[2..]);
    fs::write(&blob, "corrupt").unwrap();
    let err = cas.read_blob(&digest).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    cas.put_blob(b"hello cache").unwrap();
    assert_eq!(cas.read_blob(&digest).unwrap(), b"hello cache");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cas_rejects_malformed_digests_without_panicking() {
    for malformed in ["", "sha256:", "sha256:abc", "md5:0000", &format!("sha256:{}", "g".repeat(64))] {
        assert!(ContentDigest::parse(malformed).is_err());
    }
}

#[cfg(unix)]
#[test]
fn cache_restore_rejects_symlinked_parent_without_outside_write() {
    use std::os::unix::fs::symlink;
    let root = std::env::temp_dir().join(format!("jet_build_cache_{}_parent_symlink", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("work/real")).unwrap();
    fs::create_dir_all(root.join("outside")).unwrap();
    let cas = LocalCas::new(root.join("cache"));
    let mut b = BuildContext::new();
    let action = b.action("emit", ActionSpec::cached(["jetc"]).with_outputs(["real/out.txt"])).unwrap();
    let plan = b.plan().unwrap();
    fs::write(root.join("work/real/out.txt"), "safe").unwrap();
    let record = cas.capture_declared_outputs(
        &root.join("work"),
        plan.action(action).unwrap(),
        plan.action_key(action).unwrap(),
        ActionOutcome::Succeeded { exit_code: 0 },
        ActionCacheProvenance::miss(CacheMissReason::NoLocalActionRecord),
    ).unwrap();
    fs::remove_dir_all(root.join("work/real")).unwrap();
    symlink(root.join("outside"), root.join("work/real")).unwrap();
    assert!(cas.restore_action_outputs(&root.join("work"), plan.action(action).unwrap(), &record).is_err());
    assert!(!root.join("outside/out.txt").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn action_key_changes_when_declared_input_contents_change() {
    let root = std::env::temp_dir().join(format!(
        "jet_build_cache_{}_{}",
        std::process::id(),
        "inputs"
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("work/src")).unwrap();

    let cas = LocalCas::new(root.join("cache"));
    let mut b = BuildContext::new();
    let action = b
        .action(
            "compile",
            ActionSpec::cached(["jetc", "src/main.jet"])
                .with_inputs(["src/main.jet"])
                .with_outputs(["build/main.o"]),
        )
        .unwrap();
    let plan = b.plan().unwrap();
    let action_ref = plan.action(action).unwrap();

    fs::write(root.join("work/src/main.jet"), "fn run() { print(1) }").unwrap();
    let first_inputs = cas
        .snapshot_declared_inputs(&root.join("work"), action_ref)
        .unwrap();
    let first = plan.action_key_with_inputs(action, &first_inputs).unwrap();

    fs::write(root.join("work/src/main.jet"), "fn run() { print(2) }").unwrap();
    let second_inputs = cas
        .snapshot_declared_inputs(&root.join("work"), action_ref)
        .unwrap();
    let second = plan.action_key_with_inputs(action, &second_inputs).unwrap();

    assert_ne!(first, second);
    assert_eq!(first_inputs[0].path.as_str(), "src/main.jet");
    assert_ne!(first_inputs[0].digest, second_inputs[0].digest);
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn cache_restore_replaces_output_symlink_instead_of_following_it() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "jet_build_cache_{}_{}",
        std::process::id(),
        "symlink"
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("work/build")).unwrap();
    fs::write(root.join("outside.txt"), "outside").unwrap();

    let cas = LocalCas::new(root.join("cache"));
    let mut b = BuildContext::new();
    let action = b
        .action(
            "emit",
            ActionSpec::cached(["jetc", "src/main.jet"]).with_outputs(["build/out.txt"]),
        )
        .unwrap();
    let plan = b.plan().unwrap();
    let key = plan.action_key(action).unwrap();
    fs::write(root.join("work/build/out.txt"), "compiled bytes").unwrap();
    let record = cas
        .capture_declared_outputs(
            &root.join("work"),
            plan.action(action).unwrap(),
            key,
            ActionOutcome::Succeeded { exit_code: 0 },
            ActionCacheProvenance::miss(CacheMissReason::NoLocalActionRecord),
        )
        .unwrap();

    fs::remove_file(root.join("work/build/out.txt")).unwrap();
    symlink(root.join("outside.txt"), root.join("work/build/out.txt")).unwrap();
    cas.restore_declared_outputs(&root.join("work"), &record)
        .unwrap();

    assert_eq!(
        fs::read_to_string(root.join("outside.txt")).unwrap(),
        "outside"
    );
    assert_eq!(
        fs::read_to_string(root.join("work/build/out.txt")).unwrap(),
        "compiled bytes"
    );
    assert!(!fs::symlink_metadata(root.join("work/build/out.txt"))
        .unwrap()
        .file_type()
        .is_symlink());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cache_provenance_records_hit_miss_and_remote_denial() {
    let hit = ActionCacheProvenance::hit(CacheHitReason::LocalActionRecordMatched);
    assert_eq!(
        hit.status,
        ActionCacheStatus::Hit(CacheHitReason::LocalActionRecordMatched)
    );
    let miss = ActionCacheProvenance::miss(CacheMissReason::ActionKeyChanged);
    assert_eq!(
        miss.status,
        ActionCacheStatus::Miss(CacheMissReason::ActionKeyChanged)
    );

    let policy = RemoteCachePolicy::disabled_until_grant_and_sandbox_proof();
    for request in [
        RemoteActionRequest::CacheRead,
        RemoteActionRequest::CacheWrite,
        RemoteActionRequest::Execute,
    ] {
        let denial = policy.check(request).unwrap_err();
        assert_eq!(denial.request, request);
        assert_eq!(
            denial.reason,
            RemoteDeniedReason::MissingGrantAndSandboxProof
        );
    }
}

#[test]
fn remote_transport_round_trips_blobs_records_and_execution_provenance() {
    let root = std::env::temp_dir().join(format!(
        "jet_remote_transport_{}_{}",
        std::process::id(),
        "roundtrip"
    ));
    let _ = fs::remove_dir_all(&root);
    let binding = RemoteBuildBinding::new("builder-roundtrip", &root, b"remote-test-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_worker_id("worker-roundtrip")
        .with_platform("linux-x86_64")
        .with_abi("native");
    let transport = RemoteCacheTransport::for_binding(&binding).unwrap();
    let key = ActionKey::new("compile:app");
    let provenance_digest = ContentDigest::from_bytes(b"provenance");
    let sandbox = transport
        .sandbox_proof(
            "remote:builder-roundtrip:trusted:sandbox-1",
            "attempt-roundtrip-1",
            key.as_str(),
            provenance_digest.clone(),
        )
        .unwrap();
    let policy = RemoteCachePolicy::granted(sandbox.clone());
    let output_digest = transport.upload_blob(b"compiled", &policy).unwrap();
    assert_eq!(transport.download_blob(&output_digest, &policy).unwrap(), b"compiled");

    let output = ActionOutputRecord {
        path: BuildPath::new("build/app").unwrap(),
        digest: output_digest.clone(),
        byte_len: 8,
    };
    let record = ActionResultRecord {
        key: key.clone(),
        outcome: ActionOutcome::Succeeded { exit_code: 0 },
        outputs: vec![output.clone()],
        provenance: ActionCacheProvenance::hit(CacheHitReason::LocalActionRecordMatched),
    };
    transport.upload_action_record(&record, &policy).unwrap();
    assert_eq!(transport.download_action_record(&key, &policy).unwrap(), record);
    let mut wrong_length = record.clone();
    wrong_length.outputs[0].byte_len = 7;
    assert!(matches!(
        transport.upload_action_record(&wrong_length, &policy),
        Err(RemoteCacheError::InvalidRecord(_))
    ));

    let request = RemoteExecutionRequest {
        key: key.clone(),
        attempt_id: sandbox.attempt_id.clone(),
        argv: vec!["jetc".to_string(), "src/main.jet".to_string()],
        inputs: vec![ActionInputSnapshot {
            path: BuildPath::new("src/main.jet").unwrap(),
            digest: transport
                .upload_execution_blob(b"source", &policy)
                .unwrap(),
            byte_len: 6,
        }],
        outputs: vec![output.path.clone()],
        toolchain_digest: ContentDigest::from_bytes(b"toolchain"),
        sandbox: sandbox.clone(),
    };
    transport.submit_execution(&request, &policy).unwrap();
    let result = RemoteExecutionResult {
        key: key.clone(),
        attempt_id: request.attempt_id.clone(),
        outcome: ActionOutcome::Succeeded { exit_code: 0 },
        outputs: vec![output],
        toolchain_digest: request.toolchain_digest.clone(),
        sandbox,
    };
    transport.publish_execution_result(&result, &policy).unwrap();
    assert_eq!(transport.download_execution_result(&key, &policy).unwrap(), result);

    let wrong_policy = RemoteCachePolicy::granted(
        transport
            .sandbox_proof(
                "remote:builder-roundtrip:trusted:sandbox-other",
                "attempt-roundtrip-other",
                "other-action",
                provenance_digest,
            )
            .unwrap(),
    );
    let error = transport.download_action_record(&key, &wrong_policy).unwrap_err();
    assert!(matches!(
        error,
        jet::Comptime::Build::RemoteCacheError::Denied(denied)
            if denied.reason == RemoteDeniedReason::ProofDoesNotMatchAction
    ));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn remote_worker_identity_and_cancellation_reject_late_or_mismatched_results() {
    let root = std::env::temp_dir().join(format!(
        "jet_remote_identity_{}_{}",
        std::process::id(),
        "proof"
    ));
    let _ = fs::remove_dir_all(&root);
    let binding = RemoteBuildBinding::new("builder-proof", &root, b"identity-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_worker_id("worker-a")
        .with_platform("linux-x86_64")
        .with_abi("native");
    let transport = RemoteCacheTransport::for_binding(&binding).unwrap();
    let key = ActionKey::new("proof-action");
    let proof = transport
        .sandbox_proof(
            "remote:builder-proof:trusted:sandbox-1",
            "attempt-proof-1",
            key.as_str(),
            ContentDigest::from_bytes(b"provenance"),
        )
        .unwrap();
    let policy = RemoteCachePolicy::with_grants(false, false, true, proof.clone());
    let request = RemoteExecutionRequest {
        key: key.clone(),
        attempt_id: proof.attempt_id.clone(),
        argv: vec!["remote-tool".to_string()],
        inputs: Vec::new(),
        outputs: Vec::new(),
        toolchain_digest: ContentDigest::from_bytes(b"toolchain"),
        sandbox: proof.clone(),
    };

    let mutations: [fn(&mut RemoteSandboxProof); 4] = [
        |proof: &mut RemoteSandboxProof| proof.worker_id = "worker-b".to_string(),
        |proof: &mut RemoteSandboxProof| proof.platform = "windows-x86_64".to_string(),
        |proof: &mut RemoteSandboxProof| proof.abi = "foreign".to_string(),
        |proof: &mut RemoteSandboxProof| proof.worker_receipt = "forged".to_string(),
    ];
    for mutate in mutations {
        let mut wrong_proof = proof.clone();
        mutate(&mut wrong_proof);
        let wrong_policy = RemoteCachePolicy::with_grants(
            false,
            false,
            true,
            wrong_proof.clone(),
        );
        let mut wrong_request = request.clone();
        wrong_request.sandbox = wrong_proof;
        assert!(matches!(
            transport.submit_execution(&wrong_request, &wrong_policy),
            Err(RemoteCacheError::InvalidRecord(_))
        ));
    }

    transport.submit_execution(&request, &policy).unwrap();
    transport.cancel_execution(&key, &policy).unwrap();
    assert!(matches!(
        transport.submit_execution(&request, &policy),
        Err(RemoteCacheError::InvalidRecord(message))
            if message.contains("attempt id was already cancelled")
    ));
    let late = RemoteExecutionResult {
        key: key.clone(),
        attempt_id: request.attempt_id.clone(),
        outcome: ActionOutcome::Succeeded { exit_code: 0 },
        outputs: Vec::new(),
        toolchain_digest: request.toolchain_digest.clone(),
        sandbox: proof,
    };
    assert!(matches!(
        transport.publish_execution_result(&late, &policy),
        Err(RemoteCacheError::InvalidRecord(_))
    ));
    assert!(matches!(
        transport.download_execution_result(&key, &policy),
        Err(RemoteCacheError::Io(error))
            if error.kind() == std::io::ErrorKind::NotFound
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn remote_cancelled_attempt_history_rejects_replay_after_later_submission() {
    let root = std::env::temp_dir().join(format!(
        "jet_remote_cancel_history_{}_{}",
        std::process::id(),
        "replay"
    ));
    let _ = fs::remove_dir_all(&root);
    let binding = RemoteBuildBinding::new("builder-history", &root, b"history-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_worker_id("worker-history")
        .with_platform("linux-x86_64")
        .with_abi("native");
    let transport = RemoteCacheTransport::for_binding(&binding).unwrap();
    let key = ActionKey::new("history-action");
    let proof_a = transport
        .sandbox_proof(
            "remote:builder-history:trusted:sandbox-a",
            "attempt-history-a",
            key.as_str(),
            ContentDigest::from_bytes(b"history-provenance"),
        )
        .unwrap();
    let policy_a = RemoteCachePolicy::with_grants(false, false, true, proof_a.clone());
    let request_a = RemoteExecutionRequest {
        key: key.clone(),
        attempt_id: proof_a.attempt_id.clone(),
        argv: vec!["remote-tool".to_string()],
        inputs: Vec::new(),
        outputs: Vec::new(),
        toolchain_digest: ContentDigest::from_bytes(b"history-toolchain"),
        sandbox: proof_a.clone(),
    };
    transport.submit_execution(&request_a, &policy_a).unwrap();
    transport.cancel_execution(&key, &policy_a).unwrap();

    let proof_b = transport
        .sandbox_proof(
            "remote:builder-history:trusted:sandbox-b",
            "attempt-history-b",
            key.as_str(),
            ContentDigest::from_bytes(b"history-provenance"),
        )
        .unwrap();
    let policy_b = RemoteCachePolicy::with_grants(false, false, true, proof_b.clone());
    let request_b = RemoteExecutionRequest {
        attempt_id: proof_b.attempt_id.clone(),
        sandbox: proof_b.clone(),
        ..request_a.clone()
    };
    transport.submit_execution(&request_b, &policy_b).unwrap();
    assert!(matches!(
        transport.submit_execution(&request_a, &policy_a),
        Err(RemoteCacheError::InvalidRecord(message))
            if message.contains("attempt id was already cancelled")
    ));

    let result_b = RemoteExecutionResult {
        key: key.clone(),
        attempt_id: request_b.attempt_id.clone(),
        outcome: ActionOutcome::Succeeded { exit_code: 0 },
        outputs: Vec::new(),
        toolchain_digest: request_b.toolchain_digest.clone(),
        sandbox: proof_b,
    };
    transport
        .publish_execution_result(&result_b, &policy_b)
        .unwrap();
    assert_eq!(
        transport.download_execution_result(&key, &policy_b).unwrap(),
        result_b
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn remote_cancel_and_publish_are_one_commit_race() {
    let root = std::env::temp_dir().join(format!(
        "jet_remote_cancel_race_{}_{}",
        std::process::id(),
        "commit"
    ));
    let _ = fs::remove_dir_all(&root);
    let binding = RemoteBuildBinding::new("builder-race", &root, b"race-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_worker_id("worker-race")
        .with_platform("linux-x86_64")
        .with_abi("native");
    let transport = RemoteCacheTransport::for_binding(&binding).unwrap();
    let key = ActionKey::new("race-action");
    let proof = transport
        .sandbox_proof(
            "remote:builder-race:trusted:race",
            "attempt-race-1",
            key.as_str(),
            ContentDigest::from_bytes(b"provenance"),
        )
        .unwrap();
    let policy = RemoteCachePolicy::with_grants(false, false, true, proof.clone());
    let request = RemoteExecutionRequest {
        key: key.clone(),
        attempt_id: proof.attempt_id.clone(),
        argv: vec!["remote-tool".to_string()],
        inputs: Vec::new(),
        outputs: Vec::new(),
        toolchain_digest: ContentDigest::from_bytes(b"toolchain"),
        sandbox: proof.clone(),
    };
    transport.submit_execution(&request, &policy).unwrap();
    let result = RemoteExecutionResult {
        key: key.clone(),
        attempt_id: request.attempt_id.clone(),
        outcome: ActionOutcome::Succeeded { exit_code: 0 },
        outputs: Vec::new(),
        toolchain_digest: request.toolchain_digest.clone(),
        sandbox: proof,
    };

    let barrier = Arc::new(Barrier::new(3));
    let publish_transport = transport.clone();
    let publish_policy = policy.clone();
    let publish_result = result.clone();
    let publish_barrier = barrier.clone();
    let cancel_transport = transport.clone();
    let cancel_policy = policy.clone();
    let cancel_key = key.clone();
    let cancel_barrier = barrier.clone();
    let (publish, cancel) = std::thread::scope(|scope| {
        let publish = scope.spawn(move || {
            publish_barrier.wait();
            publish_transport.publish_execution_result(&publish_result, &publish_policy)
        });
        let cancel = scope.spawn(move || {
            cancel_barrier.wait();
            cancel_transport.cancel_execution(&cancel_key, &cancel_policy)
        });
        barrier.wait();
        (publish.join().unwrap(), cancel.join().unwrap())
    });
    assert!(publish.is_ok() || matches!(publish, Err(RemoteCacheError::InvalidRecord(_))));
    assert!(cancel.is_ok());
    assert!(matches!(
        transport.download_execution_result(&key, &policy),
        Err(RemoteCacheError::Io(error))
            if error.kind() == std::io::ErrorKind::NotFound
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn remote_host_binding_registry_round_trips_without_storing_secret() {
    let _guard = REMOTE_HOST_ENV_LOCK.lock().unwrap();
    let root = std::env::temp_dir().join(format!(
        "jet_remote_registry_{}_{}",
        std::process::id(),
        "binding"
    ));
    let config = root.join("config");
    let remote_root = root.join("remote");
    let credential = root.join("credential");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&remote_root).unwrap();
    fs::write(&credential, b"registry-secret").unwrap();
    let previous_config = std::env::var_os("XDG_CONFIG_HOME");
    std::env::set_var("XDG_CONFIG_HOME", &config);

    let binding = RemoteBuildBinding::bind_host(
        "registry-builder",
        &remote_root,
        &credential,
        "trusted",
        "worker-a",
        "linux-x86_64",
        "native",
        true,
        true,
        true,
        true,
        4321,
    )
    .unwrap();
    let record = config
        .join("jet")
        .join("remote-bindings")
        .join("registry-builder.conf");
    let record_text = fs::read_to_string(record).unwrap();
    assert!(!record_text.contains("registry-secret"));
    assert_eq!(RemoteBuildBinding::list_host().unwrap(), vec!["registry-builder"]);
    assert_eq!(RemoteBuildBinding::load_host("registry-builder").unwrap(), binding);
    RemoteBuildBinding::remove_host("registry-builder").unwrap();
    assert!(RemoteBuildBinding::list_host().unwrap().is_empty());

    match previous_config {
        Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
        None => std::env::remove_var("XDG_CONFIG_HOME"),
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn remote_driver_consumes_authenticated_worker_result() {
    let project_root = std::env::temp_dir().join(format!(
        "jet_remote_driver_{}_{}",
        std::process::id(),
        "worker"
    ));
    let _ = fs::remove_dir_all(&project_root);
    fs::create_dir_all(&project_root).unwrap();
    let remote_root = project_root.join("remote-transport");
    let mut b = BuildContext::new();
    let action = b
        .action(
            "remote-action",
            ActionSpec::cached(["remote-tool"])
                .with_outputs(["build/remote-app"])
                .with_cap(BuildCapability::Net),
        )
        .unwrap();
    let target = b
        .add_executable("remote-app", TargetSpec::new().with_action(action))
        .unwrap();
    let plan = b.plan_with_default(target).unwrap();
    let grants = [BuildCapability::Net].into_iter().collect();
    let key = plan
        .effective_action_key(
            action,
            &[],
            &grants,
            std::path::Path::new("remote-tool"),
            &ContentDigest::from_bytes(b"remote-tool"),
            &[],
        )
        .unwrap();
    let binding = RemoteBuildBinding::new("builder-a", &remote_root, b"driver-worker-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_worker_id("worker-a")
        .with_platform(format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH))
        .with_abi("native")
        .with_execute(true)
        .with_timeout_ms(2_000);

    let worker_key = key.clone();
    let worker_binding = binding.clone();
    let worker = std::thread::spawn(move || {
        let transport = RemoteCacheTransport::for_binding(&worker_binding).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match transport.read_execution_request(&worker_key) {
                Ok(request) => {
                    let policy = RemoteCachePolicy::with_grants(
                        false,
                        false,
                        true,
                        request.sandbox.clone(),
                    );
                    let bytes = b"remote worker output";
                    let digest = transport.upload_execution_blob(bytes, &policy).unwrap();
                    transport
                        .publish_execution_result(
                            &RemoteExecutionResult {
                                key: request.key.clone(),
                                attempt_id: request.attempt_id.clone(),
                                outcome: ActionOutcome::Succeeded { exit_code: 0 },
                                outputs: vec![ActionOutputRecord {
                                    path: request.outputs[0].clone(),
                                    digest,
                                    byte_len: bytes.len() as u64,
                                }],
                                toolchain_digest: request.toolchain_digest.clone(),
                                sandbox: request.sandbox,
                            },
                            &policy,
                        )
                        .unwrap();
                    return;
                }
                Err(RemoteCacheError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(error) => panic!("remote worker could not read request: {error}"),
            }
        }
    });

    let execution = execute_build_plan_with_front_end_and_remote(
        &plan,
        &project_root,
        &grants,
        FrontEndCompletion::all_complete(),
        Some(&binding),
    )
    .unwrap();
    worker.join().unwrap();
    assert!(execution.report.events.iter().any(|event| {
        matches!(
            event,
            BuildExecutionEvent::Finished {
                action: finished,
                outcome: ActionOutcome::Succeeded { exit_code: 0 },
            } if *finished == action.id()
        )
    }));
    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn remote_execution_timeout_uses_declared_local_fallback() {
    let project_root = std::env::temp_dir().join(format!(
        "jet_remote_fallback_{}_{}",
        std::process::id(),
        "local"
    ));
    let _ = fs::remove_dir_all(&project_root);
    fs::create_dir_all(&project_root).unwrap();
    let remote_root = project_root.join("remote-transport");
    let mut b = BuildContext::new();
    let action = b
        .action(
            "local-fallback",
            ActionSpec::cached([
                "sh",
                "-c",
                "printf fallback > build/fallback.txt",
            ])
            .with_outputs(["build/fallback.txt"])
            .with_cap(BuildCapability::Exec)
            .with_cap(BuildCapability::FS)
            .with_cap(BuildCapability::Net),
        )
        .unwrap();
    let target = b
        .add_executable("fallback", TargetSpec::new().with_action(action))
        .unwrap();
    let plan = b.plan_with_default(target).unwrap();
    let grants = [
        BuildCapability::Exec,
        BuildCapability::FS,
        BuildCapability::Net,
    ]
    .into_iter()
    .collect();
    let binding = RemoteBuildBinding::new("builder-fallback", &remote_root, b"fallback-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_execute(true)
        .with_local_fallback(true)
        .with_timeout_ms(20);

    let execution = execute_build_plan_with_front_end_and_remote(
        &plan,
        &project_root,
        &grants,
        FrontEndCompletion::all_complete(),
        Some(&binding),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(project_root.join("build/fallback.txt")).unwrap(),
        "fallback"
    );
    assert!(execution.report.events.iter().any(|event| {
        matches!(
            event,
            BuildExecutionEvent::Finished {
                action: finished,
                outcome: ActionOutcome::Succeeded { exit_code: 0 },
            } if *finished == action.id()
        )
    }));
    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn remote_execution_grant_carries_blobs_without_cache_authority() {
    let root = std::env::temp_dir().join(format!(
        "jet_remote_execute_only_{}_{}",
        std::process::id(),
        "roundtrip"
    ));
    let _ = fs::remove_dir_all(&root);
    let binding = RemoteBuildBinding::new("builder-execute-only", &root, b"remote-test-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_worker_id("worker-execute-only")
        .with_platform("linux-x86_64")
        .with_abi("native");
    let transport = RemoteCacheTransport::for_binding(&binding).unwrap();
    let key = ActionKey::new("execute-only");
    let sandbox = transport
        .sandbox_proof(
            "remote:builder-execute-only:trusted:sandbox-execute-only",
            "attempt-execute-only-1",
            key.as_str(),
            ContentDigest::from_bytes(b"toolchain"),
        )
        .unwrap();
    let policy = RemoteCachePolicy::with_grants(false, false, true, sandbox.clone());
    let input_digest = transport
        .upload_execution_blob(b"source", &policy)
        .unwrap();
    assert_eq!(
        transport.download_execution_blob(&input_digest, &policy).unwrap(),
        b"source"
    );
    assert!(transport.upload_blob(b"cache-forbidden", &policy).is_err());

    let input = ActionInputSnapshot {
        path: BuildPath::new("src/main.jet").unwrap(),
        digest: input_digest,
        byte_len: 6,
    };
    let output_path = BuildPath::new("build/app").unwrap();
    let request = RemoteExecutionRequest {
        key: key.clone(),
        attempt_id: sandbox.attempt_id.clone(),
        argv: vec!["jetc".to_string(), "src/main.jet".to_string()],
        inputs: vec![input],
        outputs: vec![output_path.clone()],
        toolchain_digest: ContentDigest::from_bytes(b"toolchain"),
        sandbox: sandbox.clone(),
    };
    transport.submit_execution(&request, &policy).unwrap();
    assert_eq!(transport.read_execution_request(&key).unwrap(), request);

    let output_digest = transport
        .upload_execution_blob(b"compiled", &policy)
        .unwrap();
    let result = RemoteExecutionResult {
        key: key.clone(),
        attempt_id: request.attempt_id.clone(),
        outcome: ActionOutcome::Succeeded { exit_code: 0 },
        outputs: vec![ActionOutputRecord {
            path: output_path,
            digest: output_digest,
            byte_len: 8,
        }],
        toolchain_digest: request.toolchain_digest.clone(),
        sandbox,
    };
    transport.publish_execution_result(&result, &policy).unwrap();
    assert_eq!(transport.download_execution_result(&key, &policy).unwrap(), result);
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn remote_transport_rejects_a_symlinked_store_root() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "jet_remote_symlink_{}_{}",
        std::process::id(),
        "root"
    ));
    let outside = root.with_extension("outside");
    let _ = fs::remove_file(&root);
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&outside);
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, &root).unwrap();
    let key = ActionKey::new("remote-symlink-key");
    let binding = RemoteBuildBinding::new("builder-symlink", &root, b"remote-test-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_worker_id("worker-symlink")
        .with_platform("linux-x86_64")
        .with_abi("native");
    let transport = RemoteCacheTransport::for_binding(&binding).unwrap();
    let policy = RemoteCachePolicy::granted(
        transport
            .sandbox_proof(
                "remote:builder-symlink:trusted:symlink",
                "attempt-symlink-1",
                key.as_str(),
                ContentDigest::from_bytes(b"provenance"),
            )
            .unwrap(),
    );
    assert!(matches!(
        transport.upload_blob(b"must not follow", &policy),
        Err(RemoteCacheError::Io(_))
    ));
    assert!(!outside.join("cas").exists());
    let _ = fs::remove_file(&root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn authenticated_cache_only_transport_cannot_publish_without_worker_identity() {
    let root = std::env::temp_dir().join(format!(
        "jet_remote_cache_only_{}_{}",
        std::process::id(),
        "identity"
    ));
    let _ = fs::remove_dir_all(&root);
    let transport = RemoteCacheTransport::authenticated(&root, b"remote-test-key").unwrap();
    let policy = RemoteCachePolicy::with_grants(
        true,
        true,
        false,
        RemoteSandboxProof::new(
            "cache-only",
            "cache-only-key",
            ContentDigest::from_bytes(b"provenance"),
        ),
    );
    assert!(matches!(
        transport.upload_blob(b"must bind a worker", &policy),
        Err(RemoteCacheError::InvalidRecord(message)) if message.contains("worker identity")
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn remote_transport_authenticates_workers_and_rejects_tampered_or_stale_records() {
    let root = std::env::temp_dir().join(format!(
        "jet_remote_hostile_{}_{}",
        std::process::id(),
        "exchange"
    ));
    let _ = fs::remove_dir_all(&root);
    let key = ActionKey::new("hostile-worker");
    let worker_binding = RemoteBuildBinding::new("builder-a", &root, b"worker-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_worker_id("worker-a")
        .with_platform("linux-x86_64")
        .with_abi("native");
    let client = RemoteCacheTransport::for_binding(&worker_binding).unwrap();
    let worker = RemoteCacheTransport::for_binding(&worker_binding).unwrap();
    let sandbox = client
        .sandbox_proof(
            "remote:builder-a:trusted:job-1",
            "attempt-job-1",
            key.as_str(),
            ContentDigest::from_bytes(b"provenance"),
        )
        .unwrap();
    let policy = RemoteCachePolicy::with_grants(false, false, true, sandbox.clone());
    let unauthenticated = RemoteCacheTransport::new(&root);
    let auth_error = unauthenticated
        .upload_execution_blob(b"must-not-write", &policy)
        .unwrap_err();
    assert!(matches!(
        auth_error,
        RemoteCacheError::Denied(denied)
            if denied.reason == RemoteDeniedReason::MissingAuthentication
    ));

    let input_digest = client.upload_execution_blob(b"source", &policy).unwrap();
    let request = RemoteExecutionRequest {
        key: key.clone(),
        attempt_id: sandbox.attempt_id.clone(),
        argv: vec!["jetc".to_string(), "src/main.jet".to_string()],
        inputs: vec![ActionInputSnapshot {
            path: BuildPath::new("src/main.jet").unwrap(),
            digest: input_digest,
            byte_len: 6,
        }],
        outputs: vec![BuildPath::new("build/app").unwrap()],
        toolchain_digest: ContentDigest::from_bytes(b"toolchain"),
        sandbox: sandbox.clone(),
    };
    client.submit_execution(&request, &policy).unwrap();
    let wrong_binding = RemoteBuildBinding::new("builder-a", &root, b"wrong-worker-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_worker_id("worker-a")
        .with_platform("linux-x86_64")
        .with_abi("native");
    let wrong_key = RemoteCacheTransport::for_binding(&wrong_binding).unwrap();
    assert!(matches!(
        wrong_key.read_execution_request(&key),
        Err(RemoteCacheError::InvalidRecord(_))
    ));
    assert_eq!(worker.read_execution_request(&key).unwrap(), request);

    let output_digest = worker.upload_execution_blob(b"compiled", &policy).unwrap();
    let result = RemoteExecutionResult {
        key: key.clone(),
        attempt_id: request.attempt_id.clone(),
        outcome: ActionOutcome::Succeeded { exit_code: 0 },
        outputs: vec![ActionOutputRecord {
            path: request.outputs[0].clone(),
            digest: output_digest.clone(),
            byte_len: 8,
        }],
        toolchain_digest: request.toolchain_digest.clone(),
        sandbox: sandbox.clone(),
    };
    worker.publish_execution_result(&result, &policy).unwrap();
    assert_eq!(client.download_execution_result(&key, &policy).unwrap(), result);

    // A second submission for the same action is a new remote attempt. The
    // old result is removed before the new request is visible, so a delayed
    // worker cannot replay a successful result into a changed sandbox.
    let new_sandbox = client
        .sandbox_proof(
            "remote:builder-a:trusted:job-2",
            "attempt-job-2",
            key.as_str(),
            ContentDigest::from_bytes(b"provenance"),
        )
        .unwrap();
    let new_policy = RemoteCachePolicy::with_grants(false, false, true, new_sandbox.clone());
    let mut new_request = request.clone();
    new_request.sandbox = new_sandbox.clone();
    new_request.attempt_id = new_sandbox.attempt_id.clone();
    client.submit_execution(&new_request, &new_policy).unwrap();
    let stale_error = client
        .download_execution_result(&key, &new_policy)
        .unwrap_err();
    assert!(matches!(
        stale_error,
        RemoteCacheError::Io(error) if error.kind() == std::io::ErrorKind::NotFound
    ));
    let replay_error = worker
        .publish_execution_result(&result, &policy)
        .unwrap_err();
    assert!(matches!(replay_error, RemoteCacheError::InvalidRecord(_)));

    let output_path = root
        .join("cas")
        .join("blobs")
        .join("sha256")
        .join(&output_digest.as_str()[7..9])
        .join(&output_digest.as_str()[9..]);
    let mut tampered = fs::read(&output_path).unwrap();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    fs::write(&output_path, tampered).unwrap();
    let tampered_error = client
        .download_execution_blob(&output_digest, &policy)
        .unwrap_err();
    assert!(matches!(tampered_error, RemoteCacheError::InvalidRecord(_)));

    let binding = RemoteBuildBinding::new("builder-a", root.clone(), b"worker-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_execute(true);
    assert!(binding.is_enabled());
    assert!(format!("{binding:?}").contains("configured: true"));
    let bound = RemoteCacheTransport::for_binding(&binding).unwrap();
    let foreign_policy = RemoteCachePolicy::with_grants(
        false,
        false,
        true,
        RemoteSandboxProof::new(
            "remote:builder-a:other-trust:job-1",
            key.as_str(),
            ContentDigest::from_bytes(b"provenance"),
        ),
    );
    assert!(matches!(
        bound.upload_execution_blob(b"foreign", &foreign_policy),
        Err(RemoteCacheError::InvalidRecord(_))
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scheduler_records_parallel_ready_graph_pools_cancellation_and_metrics() {
    let mut b = BuildContext::new();
    let core = b
        .action(
            "compile-core",
            ActionSpec::cached(["jetc", "src/core.jet"])
                .with_outputs(["build/core.o"])
                .with_pool(BuildResourcePool::Cpu),
        )
        .unwrap();
    let ui = b
        .action(
            "compile-ui",
            ActionSpec::cached(["jetc", "src/ui.jet"])
                .with_outputs(["build/ui.o"])
                .with_pool(BuildResourcePool::Cpu),
        )
        .unwrap();
    let link = b
        .action(
            "link-app",
            ActionSpec::cached(["ld", "-o", "build/app"])
                .with_inputs(["build/core.o", "build/ui.o"])
                .with_outputs(["build/app"])
                .with_pool(BuildResourcePool::Linker)
                .with_pool(BuildResourcePool::Console),
        )
        .unwrap();
    let core_target = b
        .add_library("core", TargetSpec::new().with_action(core))
        .unwrap();
    let ui_target = b
        .add_library("ui", TargetSpec::new().with_action(ui))
        .unwrap();
    let app = b
        .add_executable(
            "app",
            TargetSpec::new()
                .with_dep(core_target)
                .with_dep(ui_target)
                .with_action(link),
        )
        .unwrap();
    let plan = b.plan_with_default(app).unwrap();

    let model = plan.execution_model().unwrap();
    assert_eq!(model.stages[0].actions, vec![core.id(), ui.id()]);
    assert_eq!(model.stages[1].actions, vec![link.id()]);
    assert_eq!(model.metrics.max_parallel_actions, 2);
    assert_eq!(model.metrics.cacheable_actions, 3);
    assert_eq!(
        model
            .pools
            .iter()
            .map(|pool| pool.pool.as_str())
            .collect::<Vec<_>>(),
        vec!["cpu", "memory", "linker", "console", "gpu"]
    );
    assert_eq!(
        model
            .nodes
            .iter()
            .find(|node| node.action == link.id())
            .unwrap()
            .prerequisites,
        vec![core.id(), ui.id()]
    );
    assert_eq!(model.console_order, vec![core.id(), ui.id(), link.id()]);

    let report = plan
        .execution_report(&[
            (core, ActionOutcome::Failed { exit_code: 1 }),
            (ui, ActionOutcome::Succeeded { exit_code: 0 }),
        ])
        .unwrap();
    assert!(report.events.contains(&BuildExecutionEvent::Cancelled {
        action: link.id(),
        failed_prereq: core.id(),
    }));
    assert_eq!(report.metrics.failed_actions, 1);
    assert_eq!(report.metrics.cancelled_actions, 1);
}

#[test]
fn scheduler_orders_file_producer_before_consumer_without_target_dep() {
    let mut b = BuildContext::new();
    let gen = b
        .action(
            "generate",
            ActionSpec::cached(["gen"])
                .with_outputs([".jet/generated/schema.jet"])
                .with_pool(BuildResourcePool::Cpu),
        )
        .unwrap();
    let compile = b
        .action(
            "compile",
            ActionSpec::cached(["jetc", ".jet/generated/schema.jet"])
                .with_inputs([".jet/generated/schema.jet"])
                .with_outputs(["build/schema.o"])
                .with_pool(BuildResourcePool::Cpu),
        )
        .unwrap();
    b.add_library("gen", TargetSpec::new().with_action(gen))
        .unwrap();
    b.add_library("compile", TargetSpec::new().with_action(compile))
        .unwrap();
    let plan = b.plan().unwrap();

    let model = plan.execution_model().unwrap();
    assert_eq!(model.stages[0].actions, vec![gen.id()]);
    assert_eq!(model.stages[1].actions, vec![compile.id()]);
    assert_eq!(
        model
            .nodes
            .iter()
            .find(|node| node.action == compile.id())
            .unwrap()
            .prerequisites,
        vec![gen.id()]
    );
}

#[test]
fn graph_query_explain_and_rebuild_reasons_share_plan_provenance() {
    let mut b = BuildContext::new();
    let gen = b
        .action(
            "generate-schema",
            ActionSpec::cached(["schema-gen", "schema/app.sql"])
                .with_inputs(["schema/app.sql"])
                .with_outputs([".jet/generated/schema.jet"])
                .with_cap(BuildCapability::FS),
        )
        .unwrap();
    let lib = b
        .add_library(
            "db",
            TargetSpec::new()
                .with_source(".jet/generated/schema.jet")
                .with_action(gen),
        )
        .unwrap();
    let plan = b.plan_with_default(lib).unwrap();

    let graph = plan.graph();
    assert_eq!(graph.targets[0].name, "db");
    assert_eq!(graph.actions[0].outputs, vec![".jet/generated/schema.jet"]);
    let ownership = plan.file_ownership(".jet/generated/schema.jet");
    assert_eq!(ownership.owner, Some(gen.id()));
    assert_eq!(ownership.targets, vec![lib.id()]);

    let target_explain = plan.explain_target(lib).unwrap();
    assert_eq!(target_explain.subject, BuildGraphSubject::Target(lib.id()));
    assert!(target_explain
        .provenance
        .iter()
        .any(|line| line == "actions=1"));
    let action_explain = plan.explain_action(gen).unwrap();
    assert_eq!(action_explain.subject, BuildGraphSubject::Action(gen.id()));
    assert!(action_explain
        .provenance
        .iter()
        .any(|line| line == "inputs=1"));
    let file_explain = plan.explain_file(".jet/generated/schema.jet");
    assert!(
        file_explain
            .provenance
            .iter()
            .any(|line| line == "owner=Some(ActionId(0))"),
        "generated action ownership must remain visible"
    );
    let rebuilt = plan
        .why_rebuilt(
            gen,
            ActionCacheStatus::Miss(CacheMissReason::ActionKeyChanged),
        )
        .unwrap();
    assert_eq!(rebuilt.action, gen.id());
    assert_eq!(rebuilt.reason, "action key changed");
}

#[test]
fn legacy_wrappers_are_typed_declared_and_policy_denied_without_ambient_authority() {
    let policy = BuildPolicy::allow_all();
    for (kind, spec) in [
        (
            LegacyWrapperKind::CMake,
            LegacyWrapperSpec::cmake(["cmake", "--build", "build"]),
        ),
        (LegacyWrapperKind::Make, LegacyWrapperSpec::make(["make"])),
        (
            LegacyWrapperKind::Gradle,
            LegacyWrapperSpec::gradle(["gradle", "assemble"]),
        ),
        (
            LegacyWrapperKind::Npm,
            LegacyWrapperSpec::npm(["npm", "run", "build"]),
        ),
        (
            LegacyWrapperKind::Cargo,
            LegacyWrapperSpec::cargo(["cargo", "build"]),
        ),
    ] {
        let action = spec
            .with_inputs(["legacy/project"])
            .with_outputs([format!("build/{}", kind.as_str())])
            .with_cap(BuildCapability::Exec)
            .with_cap(BuildCapability::FS)
            .into_action_spec(&policy)
            .unwrap();
        assert_eq!(action.legacy_wrapper, Some(kind));
        assert!(action.caps.contains(&BuildCapability::Exec));
        assert_eq!(action.inputs[0].as_str(), "legacy/project");
        assert_eq!(
            action.labels.get("legacy.wrapper").map(String::as_str),
            Some(kind.as_str())
        );
    }

    let denial = LegacyWrapperSpec::cargo(["cargo", "build"])
        .with_inputs(["Cargo.toml"])
        .with_outputs(["target/debug/app"])
        .with_cap(BuildCapability::Exec)
        .explain(&BuildPolicy::deny_legacy_wrappers(
            "CI forbids legacy build tools",
        ));
    assert!(!denial.allowed);
    assert_eq!(denial.reason, "CI forbids legacy build tools");

    let err = LegacyWrapperSpec::make(["make"])
        .with_inputs(["Makefile"])
        .with_outputs(["build/app"])
        .into_action_spec(&policy)
        .unwrap_err();
    assert_eq!(
        err,
        BuildError::LegacyWrapperWithoutCaps(LegacyWrapperKind::Make)
    );
}

#[test]
fn legacy_project_import_reads_one_canonical_file_and_ci_denies_the_wrapper() {
    let root = std::env::temp_dir().join(format!(
        "jet_legacy_import_{}_{}",
        std::process::id(),
        "cargo"
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("Cargo.toml"), b"[package]\nname = \"imported\"\n").unwrap();

    let imported = LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Cargo).unwrap();
    assert_eq!(
        imported.labels.get("legacy.project-file").map(String::as_str),
        Some("Cargo.toml")
    );
    assert_eq!(imported.inputs[0].as_str(), "Cargo.toml");
    assert_eq!(imported.argv[0], "cargo");
    let action = imported
        .clone()
        .with_outputs(["target/debug/imported"])
        .with_cap(BuildCapability::Exec)
        .into_action_spec(&BuildPolicy::allow_all())
        .unwrap();
    assert_eq!(action.legacy_wrapper, Some(LegacyWrapperKind::Cargo));

    let ci = imported
        .with_outputs(["target/debug/imported"])
        .with_cap(BuildCapability::Exec)
        .into_action_spec(&BuildPolicy::ci_default())
        .unwrap_err();
    assert!(matches!(ci, BuildError::PolicyDenied(_)));

    fs::remove_file(root.join("Cargo.toml")).unwrap();
    assert_eq!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Cargo).unwrap_err(),
        BuildError::LegacyProjectFileMissing(LegacyWrapperKind::Cargo)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn legacy_project_import_rejects_ambiguous_targets_and_unmodeled_dependencies() {
    let root = std::env::temp_dir().join(format!(
        "jet_legacy_import_strict_{}_{}",
        std::process::id(),
        "all"
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    fs::write(
        root.join("CMakeLists.txt"),
        "project(app)\nadd_executable(app main.cc)\nadd_dependencies(app generated)\n",
    )
    .unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::CMake),
        Err(BuildError::LegacyProjectFileInvalid(message)) if message.contains("add_dependencies")
    ));

    fs::write(root.join("Makefile"), "all: main.o\nother:\n").unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Make),
        Err(BuildError::LegacyProjectFileInvalid(message)) if message.contains("multiple or ambiguous")
    ));

    fs::write(
        root.join("build.gradle"),
        "tasks.register(\"build\")\ndependencies { implementation(\"x:y:1\") }\n",
    )
    .unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Gradle),
        Err(BuildError::LegacyProjectFileInvalid(message)) if message.contains("depends") || message.contains("dependencies")
    ));

    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"build":"tool"},"dependencies":"not-an-object"}"#,
    )
    .unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Npm),
        Err(BuildError::LegacyProjectFileInvalid(message)) if message.contains("dependencies")
    ));

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"app\"\n[[bin]]\nname = \"one\"\n[[bin]]\nname = \"two\"\n",
    )
    .unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Cargo),
        Err(BuildError::LegacyProjectFileInvalid(message)) if message.contains("multiple binary")
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn legacy_project_import_captures_source_closure_and_rejects_unmodeled_recipes() {
    let root = std::env::temp_dir().join(format!(
        "jet_legacy_import_closure_{}_{}",
        std::process::id(),
        "all"
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("include")).unwrap();
    fs::write(root.join("src/main.c"), "int main(void) { return 0; }\n").unwrap();
    fs::write(root.join("include/app.h"), "#pragma once\n").unwrap();

    fs::write(
        root.join("CMakeLists.txt"),
        "project(app)\nadd_executable(app src/main.c)\n# jet: output=build/app\n",
    )
    .unwrap();
    let cmake = LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::CMake).unwrap();
    assert!(cmake.inputs.iter().any(|path| path.as_str() == "src/main.c"));
    assert!(cmake.inputs.iter().any(|path| path.as_str() == "include/app.h"));
    assert!(cmake
        .labels
        .get("legacy.source-closure")
        .is_some_and(|value| value.starts_with("project-files-v1:")));

    fs::write(
        root.join("CMakeLists.txt"),
        "project(app)\nadd_executable(app src/main.c)\n",
    )
    .unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::CMake),
        Err(BuildError::LegacyProjectFileInvalid(message))
            if message.contains("exact build output")
    ));

    fs::write(root.join("Makefile"), "app: src/main.c\n\tcc -o app src/main.c\n").unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Make),
        Err(BuildError::LegacyProjectFileInvalid(message))
            if message.contains("recipe bodies")
    ));

    fs::write(
        root.join("build.gradle"),
        "rootProject.name = \"app\"\ntasks.register(\"build\")\n# jet: output=build/libs/app.jar\n",
    )
    .unwrap();
    let gradle = LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Gradle).unwrap();
    assert!(gradle
        .outputs
        .iter()
        .any(|path| path.as_str() == "build/libs/app.jar"));

    fs::write(root.join("build.gradle"), "tasks.register(\"build\") { dependsOn \"x\" }\n")
        .unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Gradle),
        Err(BuildError::LegacyProjectFileInvalid(message))
            if message.contains("task body")
    ));

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"app\"\n[dependencies]\nserde = \"1\"\n",
    )
    .unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Cargo),
        Err(BuildError::LegacyProjectFileInvalid(message))
            if message.contains("Cargo.lock")
    ));

    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"build":"tool"}}"#,
    )
    .unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Npm),
        Err(BuildError::LegacyProjectFileInvalid(message))
            if message.contains("exact build output")
    ));

    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"build":"tool"},"main":"dist/index.js","dependencies":{"vite":"5"}}"#,
    )
    .unwrap();
    fs::write(root.join("package-lock.json"), b"{}").unwrap();
    let npm = LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Npm).unwrap();
    assert!(npm
        .outputs
        .iter()
        .any(|path| path.as_str() == "dist/index.js"));
    assert_eq!(
        npm.labels
            .get("legacy.dependency.dependencies.vite")
            .map(String::as_str),
        Some("5")
    );

    fs::remove_file(root.join("package-lock.json")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"scripts":{"build":"tool"},"dependencies":{"vite":"5"}}"#,
    )
    .unwrap();
    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Npm),
        Err(BuildError::LegacyProjectFileInvalid(message))
            if message.contains("package-lock") || message.contains("shrinkwrap")
    ));
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn legacy_project_import_rejects_a_symlinked_canonical_file() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "jet_legacy_import_symlink_{}_{}",
        std::process::id(),
        "cargo"
    ));
    let outside = root.with_extension("outside");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&outside);
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("Cargo.toml"), b"[package]\nname = \"outside\"\n").unwrap();
    symlink(outside.join("Cargo.toml"), root.join("Cargo.toml")).unwrap();

    assert!(matches!(
        LegacyWrapperSpec::from_project_file(&root, LegacyWrapperKind::Cargo),
        Err(BuildError::LegacyProjectFileInvalid(path)) if path == "Cargo.toml"
    ));
    let _ = fs::remove_file(root.join("Cargo.toml"));
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn wasm_build_plugins_handshake_grants_policy_and_return_plan_contributions() {
    let mut b = BuildContext::new();
    let plugin = WasmComponentPluginSpec::new("shader-tools", "1.2.0", "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .with_capability(BuildCapability::FS)
        .with_capability(BuildCapability::Exec);
    let policy = BuildPolicy::allow_all()
        .with_plugin_grant("shader-tools", BuildCapability::FS)
        .with_plugin_grant("shader-tools", BuildCapability::Exec);
    let contribution = PluginContribution::new()
        .with_action(
            "compile-shaders",
            ActionSpec::cached(["shaderc", "assets/main.glsl"])
                .with_inputs(["assets/main.glsl"])
                .with_outputs(["build/shaders.bin"])
                .with_cap(BuildCapability::FS)
                .with_cap(BuildCapability::Exec),
        )
        .with_target(
            TargetKind::Library,
            "shader-lib",
            TargetSpec::new().with_source(".jet/generated/shaders.jet"),
        )
        .with_generated_module(GeneratedModuleSpec::new(
            "shaders",
            ".jet/generated/shaders.jet",
            "fn shader_count() => Int { return 1 }",
        ));

    let applied = b
        .apply_wasm_component_plugin(plugin, contribution, &policy)
        .unwrap();
    let plan = b.plan().unwrap();
    assert_eq!(plan.plugins()[0].name, "shader-tools");
    assert_eq!(plan.plugins()[0].api_version, BUILD_PLUGIN_API_VERSION);
    assert_eq!(
        plan.action(applied.actions[0]).unwrap().plugin,
        Some(applied.plugin)
    );
    assert_eq!(
        plan.target(applied.targets[0]).unwrap().plugin,
        Some(applied.plugin)
    );
    assert_eq!(plan.generated_modules()[0].name, "shaders");
    assert!(plan.generated_modules()[0]
        .source_digest
        .as_str()
        .starts_with("sha256:"));
    let generated = plan.explain_file(".jet/generated/shaders.jet");
    assert!(generated.provenance.iter().any(|fact| fact == "generated=shaders"));
    assert!(generated.provenance.iter().any(|fact| fact.starts_with("digest=sha256:")));

    let denied = BuildContext::new()
        .apply_wasm_component_plugin(
            WasmComponentPluginSpec::new("net-plugin", "1.0.0", "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
                .with_capability(BuildCapability::Net),
            PluginContribution::new(),
            &BuildPolicy::allow_all(),
        )
        .unwrap_err();
    assert!(matches!(denied, BuildError::PolicyDenied(_)));

    let version_err = BuildContext::new()
        .apply_wasm_component_plugin(
            WasmComponentPluginSpec::new("old-plugin", "0.1.0", "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc")
                .with_api_version("jet.build.plugin.v0"),
            PluginContribution::new(),
            &BuildPolicy::allow_all(),
        )
        .unwrap_err();
    assert!(matches!(
        version_err,
        BuildError::PluginVersionMismatch { .. }
    ));

    let contributed_cap_denied = BuildContext::new()
        .apply_wasm_component_plugin(
            WasmComponentPluginSpec::new("net-plugin", "1.0.0", "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
                .with_capability(BuildCapability::FS),
            PluginContribution::new().with_action(
                "probe-network",
                ActionSpec::cached(["curl", "https://example.invalid"])
                    .with_outputs(["build/net.txt"])
                    .with_cap(BuildCapability::Net),
            ),
            &BuildPolicy::allow_all().with_plugin_grant("net-plugin", BuildCapability::FS),
        )
        .unwrap_err();
    assert!(matches!(
        contributed_cap_denied,
        BuildError::PolicyDenied(_)
    ));
}

#[test]
fn packaged_build_plugins_verify_bytes_and_roll_back_rejected_contributions() {
    let root = std::env::temp_dir().join(format!(
        "jet_build_plugin_package_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let component = b"\0asm\x0d\0\x01\0";
    let digest = ContentDigest::from_bytes(component);
    let manifest = root.join("plugin.manifest");
    let component_path = root.join("plugin.wasm");
    fs::write(
        &manifest,
        format!(
            "name = \"packaged\"\nversion = \"1.0.0\"\napi_version = \"{BUILD_PLUGIN_API_VERSION}\"\ncomponent_digest = \"{}\"\ncapabilities = [\"FS\"]\n",
            digest.as_str()
        ),
    )
    .unwrap();
    fs::write(&component_path, component).unwrap();

    let spec = WasmComponentPluginSpec::load_packaged(&manifest, &component_path).unwrap();
    assert_eq!(spec.component_digest, digest.as_str());

    let invalid_component = b"not a component";
    let invalid_manifest = root.join("invalid.manifest");
    let invalid_component_path = root.join("invalid.wasm");
    fs::write(
        &invalid_manifest,
        format!(
            "name = \"invalid\"\nversion = \"1.0.0\"\napi_version = \"{BUILD_PLUGIN_API_VERSION}\"\ncomponent_digest = \"{}\"\ncapabilities = []\n",
            ContentDigest::from_bytes(invalid_component).as_str()
        ),
    )
    .unwrap();
    fs::write(&invalid_component_path, invalid_component).unwrap();
    let invalid = WasmComponentPluginSpec::load_packaged(&invalid_manifest, &invalid_component_path)
        .expect_err("digest-matching arbitrary bytes must not be a component");
    assert!(invalid.contains("Component Model binary"), "{invalid}");

    let policy = BuildPolicy::allow_all().with_plugin_grant("packaged", BuildCapability::FS);
    let rejected = PluginContribution::new()
        .with_action(
            "partial-action",
            ActionSpec::cached(["tool"])
                .with_outputs(["build/out"])
                .with_cap(BuildCapability::FS),
        )
        .with_generated_module(GeneratedModuleSpec::new(
            "bad-module",
            "bad-module.jet",
            "fn bad() {}",
        ));
    let mut build = BuildContext::new();
    assert!(matches!(
        build.apply_packaged_wasm_component_plugin(&manifest, &component_path, rejected, &policy),
        Err(BuildError::InvalidGeneratedModulePath(_))
    ));
    assert!(build.plan().unwrap().actions().is_empty());
    assert!(build.plan().unwrap().plugins().is_empty());

    let accepted = PluginContribution::new().with_action(
        "packaged-action",
        ActionSpec::cached(["tool"])
            .with_outputs(["build/packaged.out"])
            .with_cap(BuildCapability::FS),
    );
    let applied = build
        .apply_packaged_wasm_component_plugin(&manifest, &component_path, accepted, &policy)
        .unwrap();
    assert_eq!(applied.actions.len(), 1);
    assert_eq!(build.plan().unwrap().plugins().len(), 1);

    fs::write(&component_path, b"tampered").unwrap();
    assert!(WasmComponentPluginSpec::load_packaged(&manifest, &component_path).is_err());
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn packaged_plugin_host_reads_are_bounded_and_reject_links() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "jet_build_plugin_bounds_{}_{}",
        std::process::id(),
        "hostile"
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let oversized = root.join("oversized.manifest");
    fs::write(&oversized, vec![b'x'; 65 * 1024]).unwrap();
    let error = read_packaged_file_bounded(&oversized, "manifest", 64 * 1024).unwrap_err();
    assert!(error.contains("exceeds 65536"), "{error}");

    let target = root.join("real.manifest");
    fs::write(&target, b"safe").unwrap();
    let link = root.join("linked.manifest");
    symlink(&target, &link).unwrap();
    let error = read_packaged_file_bounded(&link, "manifest", 64 * 1024).unwrap_err();
    assert!(error.contains("regular non-symlink"), "{error}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn action_environment_allowlist_cannot_name_undeclared_values() {
    let mut build = BuildContext::new();
    let error = build
        .action(
            "env-check",
            ActionSpec::cached(["tool"])
                .with_outputs(["build/out"])
                .with_env_allowlist(["MISSING"]),
        )
        .unwrap_err();
    assert_eq!(
        error,
        BuildError::UndeclaredEnvName {
            action: "env-check".to_string(),
            key: "MISSING".to_string(),
        }
    );
}

#[test]
fn plugin_action_and_generated_module_cannot_share_one_path() {
    let result = BuildContext::new().apply_wasm_component_plugin(
        WasmComponentPluginSpec::new(
            "collision-plugin",
            "1.0.0",
            "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        )
        .with_capability(BuildCapability::Exec),
        PluginContribution::new()
            .with_action(
                "emit-source",
                ActionSpec::cached(["generator"])
                    .with_outputs([".jet/generated/collision.jet"])
                    .with_cap(BuildCapability::Exec),
            )
            .with_generated_module(GeneratedModuleSpec::new(
                "collision",
                ".jet/generated/collision.jet",
                "fn collision() {}",
            )),
        &BuildPolicy::allow_all().with_plugin_grant("collision-plugin", BuildCapability::Exec),
    );
    assert!(matches!(result, Err(BuildError::GeneratedModuleCycle { .. })));
}

#[test]
fn action_kinds_have_distinct_identities() {
    use jet::Comptime::Build::ActionKind;
    fn key(kind: ActionKind) -> String {
        let mut b = BuildContext::new();
        let action = b
            .action(
                "same",
                ActionSpec::cached(["jetc", "src/main.jet"])
                    .with_kind(kind)
                    .with_inputs(["src/main.jet"])
                    .with_outputs(["build/out"]),
            )
            .unwrap();
        b.plan()
            .unwrap()
            .action_key(action)
            .unwrap()
            .as_str()
            .to_string()
    }
    let compile = key(ActionKind::Compile);
    let docs = key(ActionKind::Docs);
    let debug = key(ActionKind::Debug);
    let archive = key(ActionKind::SourceArchive);
    assert_ne!(compile, docs);
    assert_ne!(compile, debug);
    assert_ne!(compile, archive);
    assert_ne!(docs, debug);
    assert_ne!(docs, archive);
    assert_ne!(debug, archive);
}

#[test]
fn complete_action_key_covers_dep_outputs_env_allowlist_helpers_and_exact_source() {
    use jet::Comptime::Build::{ActionInputSnapshot, ActionKind, ContentDigest};

    let mut b = BuildContext::new();
    let dep_action = b
        .action(
            "dep-compile",
            ActionSpec::cached(["jetc", "lib.jet"])
                .with_kind(ActionKind::Compile)
                .with_outputs(["build/lib.o"]),
        )
        .unwrap();
    let dep = b
        .add_library(
            "lib",
            TargetSpec::new()
                .with_source("lib.jet")
                .with_action(dep_action)
                .with_output("build/lib.o"),
        )
        .unwrap();
    let main = b
        .action(
            "main-compile",
            ActionSpec::cached(["jetc", "main.jet"])
                .with_kind(ActionKind::Compile)
                .with_inputs(["main.jet"])
                .with_outputs(["build/main"])
                .with_env("JET_PROFILE", "release")
                .with_env("HOME", "/tmp/leak")
                .with_env_allowlist(["JET_PROFILE"])
                .with_helper_version("docgen", "1.2.3")
                .with_label("profile", "release"),
        )
        .unwrap();
    b.add_executable(
        "app",
        TargetSpec::new()
            .with_source("main.jet")
            .with_dep(dep)
            .with_action(main)
            .with_metadata("profile", "release"),
    )
    .unwrap();
    let plan = b.plan().unwrap();
    let snap = [ActionInputSnapshot {
        path: jet::Comptime::Build::BuildPath::new("main.jet").unwrap(),
        digest: ContentDigest::from_bytes(b"fn run() {}"),
        byte_len: 11,
    }];
    let base = plan.action_key_with_inputs(main, &snap).unwrap();

    // Ambient HOME is outside allowlist — changing declared HOME must not
    // affect the key once allowlist filters it.
    let mut b2 = BuildContext::new();
    let dep2 = {
        let dep_action = b2
            .action(
                "dep-compile",
                ActionSpec::cached(["jetc", "lib.jet"])
                    .with_kind(ActionKind::Compile)
                    .with_outputs(["build/lib.o"]),
            )
            .unwrap();
        b2.add_library(
            "lib",
            TargetSpec::new()
                .with_source("lib.jet")
                .with_action(dep_action)
                .with_output("build/lib.o"),
        )
        .unwrap()
    };
    let main2 = b2
        .action(
            "main-compile",
            ActionSpec::cached(["jetc", "main.jet"])
                .with_kind(ActionKind::Compile)
                .with_inputs(["main.jet"])
                .with_outputs(["build/main"])
                .with_env("JET_PROFILE", "release")
                .with_env("HOME", "/other/home")
                .with_env_allowlist(["JET_PROFILE"])
                .with_helper_version("docgen", "1.2.3")
                .with_label("profile", "release"),
        )
        .unwrap();
    b2.add_executable(
        "app",
        TargetSpec::new()
            .with_source("main.jet")
            .with_dep(dep2)
            .with_action(main2)
            .with_metadata("profile", "release"),
    )
    .unwrap();
    assert_eq!(
        base,
        b2.plan()
            .unwrap()
            .action_key_with_inputs(main2, &snap)
            .unwrap()
    );

    // Helper version change flips key.
    let mut b3 = BuildContext::new();
    let dep3 = {
        let dep_action = b3
            .action(
                "dep-compile",
                ActionSpec::cached(["jetc", "lib.jet"])
                    .with_kind(ActionKind::Compile)
                    .with_outputs(["build/lib.o"]),
            )
            .unwrap();
        b3.add_library(
            "lib",
            TargetSpec::new()
                .with_source("lib.jet")
                .with_action(dep_action)
                .with_output("build/lib.o"),
        )
        .unwrap()
    };
    let main3 = b3
        .action(
            "main-compile",
            ActionSpec::cached(["jetc", "main.jet"])
                .with_kind(ActionKind::Compile)
                .with_inputs(["main.jet"])
                .with_outputs(["build/main"])
                .with_env("JET_PROFILE", "release")
                .with_env_allowlist(["JET_PROFILE"])
                .with_helper_version("docgen", "9.9.9")
                .with_label("profile", "release"),
        )
        .unwrap();
    b3.add_executable(
        "app",
        TargetSpec::new()
            .with_source("main.jet")
            .with_dep(dep3)
            .with_action(main3)
            .with_metadata("profile", "release"),
    )
    .unwrap();
    assert_ne!(
        base,
        b3.plan()
            .unwrap()
            .action_key_with_inputs(main3, &snap)
            .unwrap()
    );

    // Exact source byte change flips key for observing kinds.
    let snap2 = [ActionInputSnapshot {
        path: jet::Comptime::Build::BuildPath::new("main.jet").unwrap(),
        digest: ContentDigest::from_bytes(b"fn run() { print(1) }"),
        byte_len: 20,
    }];
    assert_ne!(base, plan.action_key_with_inputs(main, &snap2).unwrap());

    // Dep output path change flips key.
    let mut b4 = BuildContext::new();
    let dep4 = {
        let dep_action = b4
            .action(
                "dep-compile",
                ActionSpec::cached(["jetc", "lib.jet"])
                    .with_kind(ActionKind::Compile)
                    .with_outputs(["build/lib-renamed.o"]),
            )
            .unwrap();
        b4.add_library(
            "lib",
            TargetSpec::new()
                .with_source("lib.jet")
                .with_action(dep_action)
                .with_output("build/lib-renamed.o"),
        )
        .unwrap()
    };
    let main4 = b4
        .action(
            "main-compile",
            ActionSpec::cached(["jetc", "main.jet"])
                .with_kind(ActionKind::Compile)
                .with_inputs(["main.jet"])
                .with_outputs(["build/main"])
                .with_env("JET_PROFILE", "release")
                .with_env_allowlist(["JET_PROFILE"])
                .with_helper_version("docgen", "1.2.3")
                .with_label("profile", "release"),
        )
        .unwrap();
    b4.add_executable(
        "app",
        TargetSpec::new()
            .with_source("main.jet")
            .with_dep(dep4)
            .with_action(main4)
            .with_metadata("profile", "release"),
    )
    .unwrap();
    assert_ne!(
        base,
        b4.plan()
            .unwrap()
            .action_key_with_inputs(main4, &snap)
            .unwrap()
    );
}

#[test]
fn front_end_completion_gates_cache_lookup() {
    use jet::Comptime::Build::{CacheBypassDenied, FrontEndCompletion};

    assert!(FrontEndCompletion::all_complete()
        .authorize_cache_lookup()
        .is_ok());
    assert_eq!(
        FrontEndCompletion {
            parsed: false,
            ..FrontEndCompletion::all_complete()
        }
        .authorize_cache_lookup()
        .unwrap_err(),
        CacheBypassDenied::Parser
    );
    assert_eq!(
        FrontEndCompletion {
            sema_checked: false,
            ..FrontEndCompletion::all_complete()
        }
        .authorize_cache_lookup()
        .unwrap_err(),
        CacheBypassDenied::Sema
    );
    assert_eq!(
        FrontEndCompletion {
            policy_checked: false,
            ..FrontEndCompletion::all_complete()
        }
        .authorize_cache_lookup()
        .unwrap_err(),
        CacheBypassDenied::Policy
    );
    assert_eq!(
        FrontEndCompletion {
            diagnostics_complete: false,
            ..FrontEndCompletion::all_complete()
        }
        .authorize_cache_lookup()
        .unwrap_err(),
        CacheBypassDenied::Diagnostics
    );
}
