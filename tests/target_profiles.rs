//! Card #239: internal typed target profile model.

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
