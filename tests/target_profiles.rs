//! Card #239: typed target profiles — validation, firmware artifacts, QEMU.

use jet::Driver::{
    build_target_profile_firmware, qemu_virt_aarch64_smoke, TargetProfileCompileError,
};
use jet::Syntax::RuntimeLayer;
use jet::TargetProfile::{
    AllocatorPolicy, ByteSize, ExecutionTier, LinkerInput, MemoryAccess, MemoryKind, MemoryRegion,
    MmioAccess, PanicPolicy, TargetProfile, TargetProfileError, TargetProfileUse, UnsafeGate,
};

fn sensor_profile() -> TargetProfile {
    let mut profile = TargetProfile::board_sensor_v1();
    // Keep MMIO range used by compile-path volatile tests.
    profile.memory = vec![
        MemoryRegion::new(
            "flash",
            0x0800_0000,
            ByteSize::kib(512),
            MemoryKind::Flash,
            MemoryAccess::Rx,
        ),
        MemoryRegion::new(
            "ram",
            0x2000_0000,
            ByteSize::kib(64),
            MemoryKind::Ram,
            MemoryAccess::Rw,
        ),
        MemoryRegion::new(
            "peripherals",
            0x4000_0000,
            ByteSize::mib(1),
            MemoryKind::Mmio,
            MemoryAccess::Rw,
        ),
    ];
    profile.linker = LinkerInput::File {
        path: "vendor/memory.x".to_string(),
        sha256: "sha256:abc123".to_string(),
    };
    profile.allocator = AllocatorPolicy::Fixed {
        region: "ram".to_string(),
        size: ByteSize::kib(8),
    };
    profile.panic = PanicPolicy::Report {
        sink: "semihosting".to_string(),
    };
    profile
}

#[test]
fn hosted_jet_sees_no_target_ceremony() {
    let profile = TargetProfile::hosted("x86_64-unknown-linux-gnu");
    let usage = TargetProfileUse {
        core_apis: vec!["core.files".to_string(), "core.http.client".to_string()],
        heap_required: true,
        ..TargetProfileUse::default()
    };
    assert!(!profile.no_os);
    assert!(matches!(profile.linker, LinkerInput::HostedDefault));
    assert!(matches!(profile.allocator, AllocatorPolicy::HostedDefault));
    assert!(matches!(profile.panic, PanicPolicy::HostedDefault));
    assert!(profile.memory.is_empty());
    assert_eq!(profile.max_runtime_layer(), RuntimeLayer::Std);
    assert!(profile.validate(&usage).is_empty());
    assert!(profile.supports_execution_tier(ExecutionTier::Dev).is_ok());
    assert!(profile.supports_execution_tier(ExecutionTier::Jit).is_ok());
    let audit = profile.audit_json(&usage);
    assert!(audit.contains("\"environment\":\"hosted\""));
    assert!(audit.contains("\"execution\":{\"aot\":true,\"dev\":true,\"jit\":true}"));
}

#[test]
fn typed_target_profile_accepts_complete_board_facts() {
    let usage = TargetProfileUse {
        stack_bytes: ByteSize::kib(4).bytes,
        static_ram_bytes: ByteSize::kib(12).bytes,
        heap_required: true,
        core_apis: vec!["core.encoding.json".to_string()],
        mmio: vec![MmioAccess {
            address: 0x4000_0100,
            size: ByteSize::bytes(4),
            unsafe_gate: Some(UnsafeGate {
                reason: "timer status register".to_string(),
            }),
        }],
    };
    assert_eq!(sensor_profile().validate(&usage), Vec::new());
    let review = sensor_profile().safety_review(&usage);
    assert!(review.passes(), "{review:?}");
}

#[test]
fn no_os_allocator_and_core_limits_are_data_errors() {
    let mut profile = sensor_profile();
    profile.allocator = AllocatorPolicy::None;
    let usage = TargetProfileUse {
        heap_required: true,
        core_apis: vec!["core.files".to_string()],
        ..TargetProfileUse::default()
    };
    let errors = profile.validate(&usage);
    assert!(errors.contains(&TargetProfileError::HeapRequiresAllocator));
    assert!(errors.contains(&TargetProfileError::CoreApiUnavailable {
        api: "core.files".to_string(),
        required: RuntimeLayer::Std,
        available: RuntimeLayer::Core
    }));
}

#[test]
fn linker_override_requires_hashed_provenance() {
    let mut profile = sensor_profile();
    profile.linker = LinkerInput::File {
        path: "vendor/memory.x".to_string(),
        sha256: String::new(),
    };
    let errors = profile.validate(&TargetProfileUse::default());
    assert!(errors.contains(&TargetProfileError::LinkerFileMissingHash {
        path: "vendor/memory.x".to_string()
    }));
}

#[test]
fn allocator_size_must_fit_named_ram_region() {
    let mut profile = sensor_profile();
    profile.memory.push(MemoryRegion::new(
        "scratch",
        0x2001_0000,
        ByteSize::kib(4),
        MemoryKind::Ram,
        MemoryAccess::Rw,
    ));
    profile.allocator = AllocatorPolicy::Fixed {
        region: "scratch".to_string(),
        size: ByteSize::kib(8),
    };
    let errors = profile.validate(&TargetProfileUse::default());
    assert!(
        errors.contains(&TargetProfileError::AllocatorRegionTooSmall {
            region: "scratch".to_string(),
            requested_bytes: ByteSize::kib(8).bytes,
            available_bytes: ByteSize::kib(4).bytes,
        })
    );
}

#[test]
fn selected_target_profile_rejects_unavailable_core_api_before_codegen() {
    let dir = std::env::temp_dir().join(format!("jet_target_profile_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        r#"
use core.files as fs

fn run() {
    print(fs.read("missing.txt") ?? "missing")
}
"#,
    )
    .unwrap();
    let err = jet::Driver::compile_bundle_path_with_target_profile(
        &file.to_string_lossy(),
        jet::Sema::CompileMode::Run,
        &sensor_profile(),
    )
    .expect_err("no-os target profile should reject core.files before codegen");
    match err {
        TargetProfileCompileError::Profile(errors) => assert!(errors.iter().any(|e| matches!(
            e,
            TargetProfileError::CoreApiUnavailable { api, .. } if api.starts_with("core.files")
        ))),
        TargetProfileCompileError::Diagnostics(diags) => {
            panic!("expected profile errors, got diagnostics: {diags:?}")
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn selected_target_profile_validates_direct_mmio_accesses() {
    let dir = std::env::temp_dir().join(format!("jet_target_profile_mmio_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        r#"
use core.mem as mem

fn run() {
    #Unsafe("timer register is mapped by board.sensor_v1") {
        p :: mem.Ptr<Int>.from_addr(0x40000100)
        mem.volatile_write(p, 7)
        _seen :: mem.volatile_read(p)
    }
}
"#,
    )
    .unwrap();
    jet::Driver::compile_bundle_path_with_target_profile(
        &file.to_string_lossy(),
        jet::Sema::CompileMode::Run,
        &sensor_profile(),
    )
    .expect("profile should accept direct volatile access inside MMIO range");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn selected_target_profile_rejects_direct_mmio_outside_region() {
    let dir = std::env::temp_dir().join(format!(
        "jet_target_profile_bad_mmio_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        r#"
use core.mem as mem

fn run() {
    #Unsafe("this address is intentionally outside the board MMIO region") {
        p :: mem.Ptr<Int>.from_addr(0x50000000)
        mem.volatile_write(p, 7)
    }
}
"#,
    )
    .unwrap();
    let err = jet::Driver::compile_bundle_path_with_target_profile(
        &file.to_string_lossy(),
        jet::Sema::CompileMode::Run,
        &sensor_profile(),
    )
    .expect_err("target profile should reject direct volatile access outside MMIO range");
    match err {
        TargetProfileCompileError::Profile(errors) => assert!(errors.iter().any(|e| matches!(
            e,
            TargetProfileError::MmioOutsideRegion { address, .. } if *address == 0x5000_0000
        ))),
        TargetProfileCompileError::Diagnostics(diags) => {
            panic!("expected profile errors, got diagnostics: {diags:?}")
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn audit_report_is_machine_stable() {
    let usage = TargetProfileUse {
        core_apis: vec!["core.files".to_string()],
        ..TargetProfileUse::default()
    };
    let audit = sensor_profile().audit_json(&usage);
    assert!(audit.contains("\"environment\":\"no-os\""));
    assert!(audit.contains("\"linker\":{\"kind\":\"file\""));
    assert!(audit.contains("\"allocator\":{\"kind\":\"fixed\""));
    assert!(audit.contains("\"panic\":{\"kind\":\"report\""));
    assert!(audit.contains("\"unavailable_core_apis\":[\"core.files\"]"));
    assert!(audit.contains("\"execution\":{\"aot\":true,\"dev\":false,\"jit\":false}"));
}

#[test]
fn no_os_profile_rejects_dev_and_jit_explicitly() {
    let profile = TargetProfile::board_sensor_v1();
    assert!(profile.supports_execution_tier(ExecutionTier::Aot).is_ok());
    match profile.supports_execution_tier(ExecutionTier::Dev) {
        Err(TargetProfileError::ExecutionTierUnsupported { tier, .. }) => {
            assert_eq!(tier, "dev")
        }
        other => panic!("expected Dev rejection, got {other:?}"),
    }
    match profile.supports_execution_tier(ExecutionTier::Jit) {
        Err(TargetProfileError::ExecutionTierUnsupported { tier, .. }) => {
            assert_eq!(tier, "jit")
        }
        other => panic!("expected Jit rejection, got {other:?}"),
    }
}

#[test]
fn hostile_profiles_fail_closed() {
    let mut overlapping = TargetProfile::board_sensor_v1();
    overlapping.memory.push(MemoryRegion::new(
        "clash",
        0x0000_0100,
        ByteSize::kib(1),
        MemoryKind::Flash,
        MemoryAccess::Rx,
    ));
    let errors = overlapping.validate(&TargetProfileUse::default());
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, TargetProfileError::OverlappingMemoryRegions { .. })),
        "{errors:?}"
    );

    let mut bad_heap = TargetProfile::board_sensor_v1();
    bad_heap.allocator = AllocatorPolicy::None;
    let usage = TargetProfileUse {
        heap_required: true,
        ..TargetProfileUse::default()
    };
    assert!(bad_heap
        .validate(&usage)
        .contains(&TargetProfileError::HeapRequiresAllocator));

    let mut missing_panic = TargetProfile::board_sensor_v1();
    missing_panic.panic = PanicPolicy::Unspecified;
    assert!(missing_panic
        .validate(&TargetProfileUse::default())
        .contains(&TargetProfileError::MissingPanicPolicy));
}

#[test]
fn mcu_firmware_build_writes_elf_map_audit_and_size_budget() {
    let profile = TargetProfile::board_sensor_v1();
    let usage = TargetProfileUse {
        stack_bytes: ByteSize::kib(2).bytes,
        static_ram_bytes: ByteSize::kib(1).bytes,
        ..TargetProfileUse::default()
    };
    let dir = std::env::temp_dir().join(format!("jet_mcu_fw_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let arts = build_target_profile_firmware(&profile, &usage, &dir)
        .expect("MCU firmware build should succeed");
    assert!(arts.elf.is_file(), "missing elf {}", arts.elf.display());
    assert!(arts.map.is_file(), "missing map {}", arts.map.display());
    assert!(
        arts.linker_script.is_file(),
        "missing linker {}",
        arts.linker_script.display()
    );
    assert!(
        arts.audit_json.is_file(),
        "missing audit {}",
        arts.audit_json.display()
    );
    let map = std::fs::read_to_string(&arts.map).unwrap();
    assert!(map.contains("Reset_Handler") || map.contains(".text"), "{map}");
    let linker = std::fs::read_to_string(&arts.linker_script).unwrap();
    assert!(linker.contains("MEMORY {"));
    assert!(linker.contains("flash (rx)"));
    assert!(arts.size_budget.ok());
    assert!(arts.audit.contains("\"size_budget\""));
    assert!(arts.audit.contains("\"environment\":\"no-os\""));
    let bytes = std::fs::metadata(&arts.elf).unwrap().len();
    assert!(bytes > 0 && bytes <= profile.memory.iter().filter(|r| r.kind == MemoryKind::Flash).map(|r| r.size.bytes).sum::<u64>());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn linux_freestanding_qemu_virt_boots_with_audit() {
    let profile = TargetProfile::board_virt_aarch64();
    let usage = TargetProfileUse::default();
    let dir = std::env::temp_dir().join(format!("jet_virt_fw_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let arts = build_target_profile_firmware(&profile, &usage, &dir)
        .expect("virt aarch64 firmware build should succeed");
    assert!(arts.map.is_file());
    assert!(arts.audit.contains("board.virt_aarch64"));
    let serial = qemu_virt_aarch64_smoke(&arts.elf).expect("QEMU virt should print OK");
    assert!(serial.contains("OK"), "serial={serial:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dossier_target_lens_returns_stable_json() {
    let sensor = jet::Driver::target_profile_dossier_json("board.sensor_v1").unwrap();
    assert!(sensor.contains("\"name\":\"board.sensor_v1\""));
    assert!(sensor.contains("\"environment\":\"no-os\""));
    let hosted = jet::Driver::target_profile_dossier_json("hosted").unwrap();
    assert!(hosted.contains("\"environment\":\"hosted\""));
    assert!(jet::Driver::target_profile_dossier_json("nope").is_err());
}

#[test]
fn independent_safety_review_covers_gates() {
    let profile = TargetProfile::board_sensor_v1();
    let usage = TargetProfileUse {
        mmio: vec![MmioAccess {
            address: 0x4000_0100,
            size: ByteSize::bytes(4),
            unsafe_gate: Some(UnsafeGate {
                reason: "UART TX register is mapped by the target profile".to_string(),
            }),
        }],
        ..TargetProfileUse::default()
    };
    let review = profile.safety_review(&usage);
    assert!(review.no_os);
    assert!(review.panic_explicit);
    assert!(review.allocator_explicit);
    assert!(review.linker_explicit);
    assert!(review.mmio_requires_unsafe);
    assert!(review.mmio_inside_declared_regions);
    assert!(review.aot_only);
    assert!(review.passes());
}
