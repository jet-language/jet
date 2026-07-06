use jet::Comptime::Build::{
    ActionCache, ActionSpec, BuildCapability, BuildContext, BuildError, BuildProvenance,
    LinkerIdentity, LockRecord, ProbeKind, ProbeSpec, ProvenanceSource, ReproducibilityClass,
    SdkIdentity, SigningIdentitySpec, TargetKind, TargetSpec, ToolchainRole, ToolchainSpec,
};

#[test]
fn registers_typed_targets_and_default_plan() {
    let mut b = BuildContext::new();
    let gen = b
        .action(
            "generate-assets",
            ActionSpec::cached(["jet-tool", "pack"])
                .with_inputs(["assets/sprite.png"])
                .with_outputs([".jet/generated/assets.jet"])
                .with_cap(BuildCapability::Fs),
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
                .with_cap(BuildCapability::Fs),
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
