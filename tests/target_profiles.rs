//! Card #239: internal typed target profile model.

use jet::Driver::TargetProfileCompileError;
use jet::Syntax::RuntimeLayer;
use jet::TargetProfile::{
    AllocatorPolicy, ByteSize, LinkerInput, MemoryAccess, MemoryKind, MemoryRegion, MmioAccess,
    PanicPolicy, TargetProfile, TargetProfileError, TargetProfileUse, UnsafeGate,
};

fn sensor_profile() -> TargetProfile {
    let mut profile = TargetProfile::freestanding("board.sensor_v1", "thumbv7em-none-eabihf");
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
    @Unsafe("timer register is mapped by board.sensor_v1") {
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
    @Unsafe("this address is intentionally outside the board MMIO region") {
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
}
