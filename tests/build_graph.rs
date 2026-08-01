use jet::Comptime::Build::{
    ActionCache, ActionCacheProvenance, ActionCacheStatus, ActionOutcome, ActionSpec,
    ActionInputSnapshot, ActionKey, ActionOutputRecord, ActionResultRecord, BuildCapability,
    BuildContext, BuildError, BuildExecutionEvent, BuildGraphSubject, BuildPath, BuildPolicy,
    BuildProvenance, BuildResourcePool, CacheHitReason, CacheMissReason, ContentDigest,
    GeneratedModuleSpec, LegacyWrapperKind, LegacyWrapperSpec, LinkerIdentity, LocalCas, LockRecord,
    PluginContribution, ProbeKind, ProbeSpec, ProvenanceSource, RemoteActionRequest,
    RemoteCachePolicy, RemoteCacheTransport, RemoteDeniedReason, RemoteExecutionRequest,
    RemoteExecutionResult, RemoteSandboxProof, ReproducibilityClass, SdkIdentity,
    SigningIdentitySpec, TargetKind, TargetSpec, ToolchainRole, ToolchainSpec,
    WasmComponentPluginSpec, BUILD_PLUGIN_API_VERSION,
};
use std::fs;

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
    let transport = RemoteCacheTransport::new(&root);
    let key = ActionKey::new("compile:app");
    let provenance_digest = ContentDigest::from_bytes(b"provenance");
    let sandbox = RemoteSandboxProof::new("sandbox-1", key.as_str(), provenance_digest.clone());
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

    let request = RemoteExecutionRequest {
        key: key.clone(),
        argv: vec!["jetc".to_string(), "src/main.jet".to_string()],
        inputs: vec![ActionInputSnapshot {
            path: BuildPath::new("src/main.jet").unwrap(),
            digest: ContentDigest::from_bytes(b"source"),
            byte_len: 6,
        }],
        outputs: vec![output.path.clone()],
        toolchain_digest: ContentDigest::from_bytes(b"toolchain"),
        sandbox: sandbox.clone(),
    };
    transport.submit_execution(&request, &policy).unwrap();
    let result = RemoteExecutionResult {
        key: key.clone(),
        outcome: ActionOutcome::Succeeded { exit_code: 0 },
        outputs: vec![output],
        toolchain_digest: request.toolchain_digest.clone(),
        sandbox,
    };
    transport.publish_execution_result(&result, &policy).unwrap();
    assert_eq!(transport.download_execution_result(&key, &policy).unwrap(), result);

    let wrong_policy = RemoteCachePolicy::granted(RemoteSandboxProof::new(
        "sandbox-1",
        "other-action",
        provenance_digest,
    ));
    let error = transport.download_action_record(&key, &wrong_policy).unwrap_err();
    assert!(matches!(
        error,
        jet::Comptime::Build::RemoteCacheError::Denied(denied)
            if denied.reason == RemoteDeniedReason::ProofDoesNotMatchAction
    ));
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
    let policy = RemoteCachePolicy::granted(RemoteSandboxProof::new(
        "sandbox",
        key.as_str(),
        ContentDigest::from_bytes(b"provenance"),
    ));
    let transport = RemoteCacheTransport::new(&root);
    assert!(transport.upload_blob(b"must not follow", &policy).is_err());
    assert!(!outside.join("cas").exists());
    let _ = fs::remove_file(&root);
    let _ = fs::remove_dir_all(outside);
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
        ),
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
