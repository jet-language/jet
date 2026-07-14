//! E4-JP15 / D-JPK-VARIANT1 — typed variants + cross-compile identity in
//! action keys and the semantic lock.

use jet::Comptime::Build::{
    ActionSpec, BuildCapability, BuildContext, BuildProvenance, LinkerIdentity, LockRecord,
    SdkIdentity, SigningIdentitySpec, SysrootIdentity, ToolchainSpec,
};
use jetpack::Platform::PlatformKey;
use jetpack::SemanticLock::{
    self, locked_variant_domains, record_selected_variant, SemanticLockFile,
};
use jetpack::Variant::{
    select_variant, ArtifactKind, PackageVariant, VariantCandidate, VariantLibc, VariantLinkage,
    VariantOs, VariantArch, VariantRole, VariantSelectError,
};

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::assert_jetos_stderr_snapshot;

#[test]
fn closed_axes_defaults_and_identity_key() {
    let v = PackageVariant::defaults_for_context(&PlatformKey {
        arch: "aarch64".into(),
        os: "linux".into(),
    });
    assert_eq!(v.role, VariantRole::Host);
    assert_eq!(v.os, VariantOs::Linux);
    assert_eq!(v.arch, VariantArch::Aarch64);
    assert_eq!(v.libc, VariantLibc::Gnu);
    assert_eq!(v.linkage, VariantLinkage::Shared);
    assert_eq!(v.artifact, ArtifactKind::Library);
    let key = v.identity_key();
    assert!(key.contains("role=host"));
    assert!(key.contains("os=linux"));
    assert!(key.contains("arch=aarch64"));
    assert!(key.contains("libc=gnu"));
}

#[test]
fn select_exact_cross_static_musl() {
    let need = PackageVariant::cross_target(
        VariantOs::Linux,
        VariantArch::Aarch64,
        VariantLibc::Musl,
    )
    .with_linkage(VariantLinkage::Static);

    let winner = VariantCandidate::new("openssl-static-musl", need.clone());
    let other = VariantCandidate::new(
        "openssl-shared-gnu",
        PackageVariant::cross_target(VariantOs::Linux, VariantArch::Aarch64, VariantLibc::Gnu)
            .with_linkage(VariantLinkage::Shared),
    );
    let got = select_variant(&need, &[winner, other], &[]).unwrap();
    assert_eq!(got.label, "openssl-static-musl");
}

#[test]
fn ambiguous_variant_is_e1316_with_snapshot() {
    let need = PackageVariant::cross_target(
        VariantOs::Linux,
        VariantArch::Aarch64,
        VariantLibc::Musl,
    )
    .with_linkage(VariantLinkage::Static);

    let a = VariantCandidate::new("openssl-a", need.clone())
        .with_provider_fact("conan.compiler.runtime", "libstdc++11");
    let b = VariantCandidate::new("openssl-b", need.clone())
        .with_provider_fact("conan.compiler.runtime", "libstdc++");

    let err = select_variant(&need, &[a, b], &[]).unwrap_err();
    let diag = match &err {
        VariantSelectError::Ambiguous { .. } => err.to_diagnostic(),
        other => panic!("expected Ambiguous, got {other:?}"),
    };
    assert_eq!(diag.code, "E1316");
    let rendered = format!(
        "\n  error[{}]: {}\n    {}\n    fix: {}\n",
        diag.code, diag.what, diag.why, diag.fix
    );
    assert_jetos_stderr_snapshot("variant_ambiguous", &rendered);
}

#[test]
fn action_key_includes_variant_sysroot_sdk_linker_signing() {
    fn key(
        variant: &str,
        sysroot_digest: &str,
        target_triple: &str,
        signer: &str,
    ) -> String {
        let mut b = BuildContext::new();
        let toolchain = b
            .toolchain(
                "cross-tc",
                ToolchainSpec::target(
                    target_triple,
                    BuildProvenance::jetpack_dependency(
                        format!("toolchain.{target_triple}#1"),
                        LockRecord::new(format!("toolchain:{target_triple}"), "sha256:tc"),
                    ),
                )
                .with_host_triple("x86_64-linux")
                .with_sdk(SdkIdentity::new(
                    "sdk",
                    "1",
                    BuildProvenance::jetpack_dependency(
                        "sdk#1",
                        LockRecord::new("sdk:1", "sha256:sdk"),
                    ),
                ))
                .with_linker(LinkerIdentity::new(
                    "lld",
                    BuildProvenance::jetpack_dependency(
                        "lld#1",
                        LockRecord::new("linker:lld", "sha256:lld"),
                    ),
                ))
                .with_sysroot(SysrootIdentity::new(
                    "sysroot",
                    sysroot_digest,
                    BuildProvenance::jetpack_dependency(
                        "sysroot#1",
                        LockRecord::new("sysroot:1", sysroot_digest),
                    ),
                )),
            )
            .unwrap();
        let signing = b
            .signing_identity(
                "signer",
                SigningIdentitySpec::new(
                    signer,
                    BuildProvenance::user_declared(
                        format!("keychain:{signer}"),
                        Some(LockRecord::new("signing:1", "sha256:sig")),
                    ),
                ),
            )
            .unwrap();
        let action = b
            .action(
                "compile",
                ActionSpec::cached(["jetc", "--emit", "obj"])
                    .with_outputs(["build/out.o"])
                    .with_cap(BuildCapability::Exec)
                    .with_toolchain(toolchain)
                    .with_signing_identity(signing)
                    .with_variant_identity(variant),
            )
            .unwrap();
        let plan = b.plan().unwrap();
        plan.action_key(action).unwrap().as_str().to_string()
    }

    let base = key(
        "role=target;os=linux;arch=aarch64;libc=musl;linkage=static;abi=sysv;artifact=library;features=",
        "sha256:sysroot-a",
        "aarch64-linux-musl",
        "developer-id:ACME",
    );
    // Deterministic.
    assert_eq!(
        base,
        key(
            "role=target;os=linux;arch=aarch64;libc=musl;linkage=static;abi=sysv;artifact=library;features=",
            "sha256:sysroot-a",
            "aarch64-linux-musl",
            "developer-id:ACME",
        )
    );
    // Variant identity is part of the key.
    assert_ne!(
        base,
        key(
            "role=host;os=linux;arch=x86_64;libc=gnu;linkage=shared;abi=sysv;artifact=library;features=",
            "sha256:sysroot-a",
            "aarch64-linux-musl",
            "developer-id:ACME",
        )
    );
    // Sysroot digest is part of the key.
    assert_ne!(
        base,
        key(
            "role=target;os=linux;arch=aarch64;libc=musl;linkage=static;abi=sysv;artifact=library;features=",
            "sha256:sysroot-b",
            "aarch64-linux-musl",
            "developer-id:ACME",
        )
    );
    // Signing identity is part of the key.
    assert_ne!(
        base,
        key(
            "role=target;os=linux;arch=aarch64;libc=musl;linkage=static;abi=sysv;artifact=library;features=",
            "sha256:sysroot-a",
            "aarch64-linux-musl",
            "developer-id:OTHER",
        )
    );
    // Target triple (cross identity) is part of the key.
    assert_ne!(
        base,
        key(
            "role=target;os=linux;arch=aarch64;libc=musl;linkage=static;abi=sysv;artifact=library;features=",
            "sha256:sysroot-a",
            "aarch64-linux-gnu",
            "developer-id:ACME",
        )
    );
}

#[test]
fn semantic_lock_covers_declared_variant_domains() {
    let need = PackageVariant::cross_target(
        VariantOs::Linux,
        VariantArch::Aarch64,
        VariantLibc::Musl,
    )
    .with_linkage(VariantLinkage::Static);
    let native = PackageVariant::defaults_for_context(&PlatformKey {
        arch: "x86_64".into(),
        os: "linux".into(),
    });

    let mut lock = SemanticLockFile::default();
    record_selected_variant(
        &mut lock,
        "openssl",
        &need.identity_key(),
        "sha256:openssl-cross",
        "exact match for static musl aarch64",
    );
    record_selected_variant(
        &mut lock,
        "zlib",
        &native.identity_key(),
        "sha256:zlib-native",
        "host default",
    );

    let domains = locked_variant_domains(&lock);
    assert!(domains.contains(&need.identity_key()));
    assert!(domains.contains(&native.identity_key()));

    let missing = jetpack::Variant::missing_lock_domains(&[need.clone(), native.clone()], &domains);
    assert!(missing.is_empty());

    let missing2 = jetpack::Variant::missing_lock_domains(
        &[
            need,
            PackageVariant::cross_target(VariantOs::Windows, VariantArch::X86_64, VariantLibc::Msvc),
        ],
        &domains,
    );
    assert_eq!(missing2.len(), 1);
    assert!(missing2[0].contains("os=windows"));

    let text = SemanticLock::write(&lock);
    assert!(text.contains("kind = \"variant\""));
    let parsed = SemanticLock::parse(&text);
    assert_eq!(parsed.records.len(), 2);
    assert!(parsed
        .records
        .iter()
        .all(|r| r.identity.kind == SemanticLock::LockRecordKind::Variant));
}

#[test]
fn native_cross_toolchain_building_remote_and_emulator_domains() {
    // Measurable JP15 coverage: five declared domains all lockable.
    let native = PackageVariant::host_defaults().with_role(VariantRole::Host);
    let cross = PackageVariant::cross_target(
        VariantOs::Linux,
        VariantArch::Aarch64,
        VariantLibc::Musl,
    );
    let toolchain_building = PackageVariant::host_defaults()
        .with_role(VariantRole::Build)
        .with_artifact(ArtifactKind::DevTool);
    let remote_target = PackageVariant::cross_target(
        VariantOs::Linux,
        VariantArch::X86_64,
        VariantLibc::Gnu,
    )
    .with_feature("remote-builder");
    let emulator = PackageVariant::cross_target(
        VariantOs::Linux,
        VariantArch::Aarch64,
        VariantLibc::Gnu,
    )
    .with_feature("emulator");

    let declared = [
        native.clone(),
        cross.clone(),
        toolchain_building.clone(),
        remote_target.clone(),
        emulator.clone(),
    ];
    let mut lock = SemanticLockFile::default();
    for (pkg, v) in [
        ("app", &native),
        ("openssl", &cross),
        ("gcc", &toolchain_building),
        ("remote-lib", &remote_target),
        ("qemu-user", &emulator),
    ] {
        record_selected_variant(
            &mut lock,
            pkg,
            &v.identity_key(),
            &format!("sha256:{pkg}"),
            "jp15 domain coverage",
        );
    }
    let domains = locked_variant_domains(&lock);
    assert_eq!(
        jetpack::Variant::missing_lock_domains(&declared, &domains),
        Vec::<String>::new()
    );
}
