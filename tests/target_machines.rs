//! Card #239: typed target machines — validation, firmware artifacts, QEMU.

mod common;

use jet::Driver::{
    build_target_machine_firmware, qemu_virt_aarch64_smoke, TargetMachineCompileError,
};
use jet::Syntax::RuntimeLayer;
use jet::TargetMachine::{
    AllocatorPolicy, ByteSize, ExecutionTier, LinkerInput, MemoryAccess, MemoryKind, MemoryRegion,
    MmioAccess, PanicPolicy, TargetMachine, TargetMachineError, TargetMachineUse, UnsafeGate,
};

fn sensor_machine() -> TargetMachine {
    let mut machine = TargetMachine::board_sensor_v1();
    // Keep MMIO range used by compile-path volatile tests.
    machine.memory = vec![
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
    machine.linker = LinkerInput::File {
        path: "vendor/memory.x".to_string(),
        sha256: "sha256:abc123".to_string(),
    };
    machine.allocator = AllocatorPolicy::Fixed {
        region: "ram".to_string(),
        size: ByteSize::kib(8),
    };
    machine.panic = PanicPolicy::Report {
        sink: "semihosting".to_string(),
    };
    machine
}

#[test]
fn hosted_jet_sees_no_target_ceremony() {
    let machine = TargetMachine::hosted("x86_64-unknown-linux-gnu");
    let usage = TargetMachineUse {
        core_apis: vec!["core.files".to_string(), "core.http.client".to_string()],
        heap_required: true,
        ..TargetMachineUse::default()
    };
    assert!(!machine.no_os);
    assert!(matches!(machine.linker, LinkerInput::HostedDefault));
    assert!(matches!(machine.allocator, AllocatorPolicy::HostedDefault));
    assert!(matches!(machine.panic, PanicPolicy::HostedDefault));
    assert!(machine.memory.is_empty());
    assert_eq!(machine.max_runtime_layer(), RuntimeLayer::Std);
    assert!(machine.validate(&usage).is_empty());
    assert!(machine.supports_execution_tier(ExecutionTier::Dev).is_ok());
    assert!(machine.supports_execution_tier(ExecutionTier::Jit).is_ok());
    let audit = machine.audit_json(&usage);
    assert!(audit.contains("\"environment\":\"hosted\""));
    assert!(audit.contains("\"execution\":{\"aot\":true,\"dev\":true,\"jit\":true}"));
}

#[test]
fn typed_target_machine_accepts_complete_board_facts() {
    let usage = TargetMachineUse {
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
    assert_eq!(sensor_machine().validate(&usage), Vec::new());
    let review = sensor_machine().safety_review(&usage);
    assert!(review.passes(), "{review:?}");
}

#[test]
fn no_os_allocator_and_core_limits_are_data_errors() {
    let mut machine = sensor_machine();
    machine.allocator = AllocatorPolicy::None;
    let usage = TargetMachineUse {
        heap_required: true,
        core_apis: vec!["core.files".to_string(), "core.mem".to_string()],
        ..TargetMachineUse::default()
    };
    let errors = machine.validate(&usage);
    assert!(errors.contains(&TargetMachineError::HeapRequiresAllocator));
    assert!(errors.contains(&TargetMachineError::CoreApiUnavailable {
        api: "core.files".to_string(),
        required: RuntimeLayer::Std,
        available: RuntimeLayer::Core
    }));
    assert!(errors.contains(&TargetMachineError::CoreApiUnavailable {
        api: "core.mem".to_string(),
        required: RuntimeLayer::Alloc,
        available: RuntimeLayer::Core
    }));
}

#[test]
fn linker_override_requires_hashed_provenance() {
    let mut machine = sensor_machine();
    machine.linker = LinkerInput::File {
        path: "vendor/memory.x".to_string(),
        sha256: String::new(),
    };
    let errors = machine.validate(&TargetMachineUse::default());
    assert!(errors.contains(&TargetMachineError::LinkerFileMissingHash {
        path: "vendor/memory.x".to_string()
    }));
}

#[test]
fn allocator_size_must_fit_named_ram_region() {
    let mut machine = sensor_machine();
    machine.memory.push(MemoryRegion::new(
        "scratch",
        0x2001_0000,
        ByteSize::kib(4),
        MemoryKind::Ram,
        MemoryAccess::Rw,
    ));
    machine.allocator = AllocatorPolicy::Fixed {
        region: "scratch".to_string(),
        size: ByteSize::kib(8),
    };
    let errors = machine.validate(&TargetMachineUse::default());
    assert!(
        errors.contains(&TargetMachineError::AllocatorRegionTooSmall {
            region: "scratch".to_string(),
            requested_bytes: ByteSize::kib(8).bytes,
            available_bytes: ByteSize::kib(4).bytes,
        })
    );
}

#[test]
fn selected_target_machine_rejects_unavailable_core_api_before_codegen() {
    let dir = std::env::temp_dir().join(format!("jet_target_machine_{}", std::process::id()));
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
    let err = jet::Driver::compile_bundle_path_with_target_machine(
        &file.to_string_lossy(),
        jet::Sema::CompileMode::Run,
        &sensor_machine(),
    )
    .expect_err("no-os target machine should reject core.files before codegen");
    match err {
        TargetMachineCompileError::Machine(errors) => assert!(errors.iter().any(|e| matches!(
            e,
            TargetMachineError::CoreApiUnavailable { api, .. } if api.starts_with("core.files")
        ))),
        TargetMachineCompileError::Diagnostics(diags) => {
            panic!("expected machine errors, got diagnostics: {diags:?}")
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn selected_target_machine_validates_direct_mmio_accesses() {
    let dir = std::env::temp_dir().join(format!("jet_target_machine_mmio_{}", std::process::id()));
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
    jet::Driver::compile_bundle_path_with_target_machine(
        &file.to_string_lossy(),
        jet::Sema::CompileMode::Run,
        &sensor_machine(),
    )
    .expect("machine should accept direct volatile access inside MMIO range");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn selected_target_machine_rejects_direct_mmio_outside_region() {
    let dir = std::env::temp_dir().join(format!(
        "jet_target_machine_bad_mmio_{}",
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
    let err = jet::Driver::compile_bundle_path_with_target_machine(
        &file.to_string_lossy(),
        jet::Sema::CompileMode::Run,
        &sensor_machine(),
    )
    .expect_err("target machine should reject direct volatile access outside MMIO range");
    match err {
        TargetMachineCompileError::Machine(errors) => assert!(errors.iter().any(|e| matches!(
            e,
            TargetMachineError::MmioOutsideRegion { address, .. } if *address == 0x5000_0000
        ))),
        TargetMachineCompileError::Diagnostics(diags) => {
            panic!("expected machine errors, got diagnostics: {diags:?}")
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn audit_report_is_machine_stable() {
    let usage = TargetMachineUse {
        core_apis: vec!["core.files".to_string()],
        ..TargetMachineUse::default()
    };
    let audit = sensor_machine().audit_json(&usage);
    assert!(audit.contains("\"environment\":\"no-os\""));
    assert!(audit.contains("\"linker\":{\"kind\":\"file\""));
    assert!(audit.contains("\"allocator\":{\"kind\":\"fixed\""));
    assert!(audit.contains("\"panic\":{\"kind\":\"report\""));
    assert!(audit.contains("\"unavailable_core_apis\":[\"core.files\"]"));
    assert!(audit.contains("\"execution\":{\"aot\":true,\"dev\":false,\"jit\":false}"));
}

#[test]
fn no_os_machine_rejects_dev_and_jit_explicitly() {
    let machine = TargetMachine::board_sensor_v1();
    assert!(machine.supports_execution_tier(ExecutionTier::Aot).is_ok());
    match machine.supports_execution_tier(ExecutionTier::Dev) {
        Err(TargetMachineError::ExecutionTierUnsupported { tier, .. }) => {
            assert_eq!(tier, "dev")
        }
        other => panic!("expected Dev rejection, got {other:?}"),
    }
    match machine.supports_execution_tier(ExecutionTier::Jit) {
        Err(TargetMachineError::ExecutionTierUnsupported { tier, .. }) => {
            assert_eq!(tier, "jit")
        }
        other => panic!("expected Jit rejection, got {other:?}"),
    }
}

#[test]
fn hostile_machines_fail_closed() {
    let mut overlapping = TargetMachine::board_sensor_v1();
    overlapping.memory.push(MemoryRegion::new(
        "clash",
        0x0000_0100,
        ByteSize::kib(1),
        MemoryKind::Flash,
        MemoryAccess::Rx,
    ));
    let errors = overlapping.validate(&TargetMachineUse::default());
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, TargetMachineError::OverlappingMemoryRegions { .. })),
        "{errors:?}"
    );

    let mut bad_heap = TargetMachine::board_sensor_v1();
    bad_heap.allocator = AllocatorPolicy::None;
    let usage = TargetMachineUse {
        heap_required: true,
        ..TargetMachineUse::default()
    };
    assert!(bad_heap
        .validate(&usage)
        .contains(&TargetMachineError::HeapRequiresAllocator));

    let mut missing_panic = TargetMachine::board_sensor_v1();
    missing_panic.panic = PanicPolicy::Unspecified;
    assert!(missing_panic
        .validate(&TargetMachineUse::default())
        .contains(&TargetMachineError::MissingPanicPolicy));
}

#[test]
fn mcu_firmware_build_writes_elf_map_audit_and_size_budget() {
    let machine = TargetMachine::board_sensor_v1();
    let usage = TargetMachineUse {
        stack_bytes: ByteSize::kib(2).bytes,
        static_ram_bytes: ByteSize::kib(1).bytes,
        ..TargetMachineUse::default()
    };
    let dir = std::env::temp_dir().join(format!("jet_mcu_fw_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let arts = build_target_machine_firmware(&machine, &usage, &dir)
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
    assert!(bytes > 0 && bytes <= machine.memory.iter().filter(|r| r.kind == MemoryKind::Flash).map(|r| r.size.bytes).sum::<u64>());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn linux_freestanding_qemu_virt_boots_with_audit() {
    let machine = TargetMachine::board_virt_aarch64();
    let usage = TargetMachineUse::default();
    let dir = std::env::temp_dir().join(format!("jet_virt_fw_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let arts = build_target_machine_firmware(&machine, &usage, &dir)
        .expect("virt aarch64 firmware build should succeed");
    assert!(arts.map.is_file());
    assert!(arts.audit.contains("board.virt_aarch64"));
    let serial = qemu_virt_aarch64_smoke(&arts.elf).expect("QEMU virt should print OK");
    assert!(serial.contains("OK"), "serial={serial:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dossier_target_lens_returns_stable_json() {
    let sensor = jet::Driver::target_machine_dossier_json("board.sensor_v1").unwrap();
    assert!(sensor.contains("\"name\":\"board.sensor_v1\""));
    assert!(sensor.contains("\"environment\":\"no-os\""));
    let hosted = jet::Driver::target_machine_dossier_json("hosted").unwrap();
    assert!(hosted.contains("\"environment\":\"hosted\""));
    assert!(jet::Driver::target_machine_dossier_json("nope").is_err());
}

#[test]
fn independent_safety_review_covers_gates() {
    let machine = TargetMachine::board_sensor_v1();
    let usage = TargetMachineUse {
        mmio: vec![MmioAccess {
            address: 0x4000_0100,
            size: ByteSize::bytes(4),
            unsafe_gate: Some(UnsafeGate {
                reason: "UART TX register is mapped by the target machine".to_string(),
            }),
        }],
        ..TargetMachineUse::default()
    };
    let review = machine.safety_review(&usage);
    assert!(review.no_os);
    assert!(review.panic_explicit);
    assert!(review.allocator_explicit);
    assert!(review.linker_explicit);
    assert!(review.mmio_requires_unsafe);
    assert!(review.mmio_inside_declared_regions);
    assert!(review.aot_only);
    assert!(review.passes());
}
