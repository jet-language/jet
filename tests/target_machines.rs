//! Card #239: typed target machines — validation, firmware artifacts, QEMU.

mod common;

use jet::Driver::{
    build_target_machine_firmware, qemu_target_machine_smoke, TargetMachineCompileError,
};
use jet::Syntax::RuntimeLayer;
use jet::TargetMachine::{
    AllocatorPolicy, ByteSize, ByteSinkPolicy, ClockPolicy, EntropyPolicy, ExecutionTier,
    LinkerInput, MemoryAccess, MemoryKind, MemoryRegion, MmioAccess, MmioPolicy, PanicPolicy,
    ProviderContract, SchedulerPolicy, StartupPolicy, TargetCapability, TargetMachine,
    TargetMachineError, TargetMachineUse, UnsafeGate,
};

fn checked_program_object(machine: &TargetMachine, dir: &std::path::Path) -> std::path::PathBuf {
    use std::process::Command;
    std::fs::create_dir_all(dir).expect("create program fixture directory");
    let source = dir.join("program.c");
    let object = dir.join("program.o");
    std::fs::write(
        &source,
        concat!(
            "typedef __SIZE_TYPE__ size_t;\n",
            "extern int __jet_target_write(const unsigned char *, size_t, size_t *);\n",
            "void __jet_program_entry(void) {\n",
            "    const unsigned char text[] = \"program-linked\\n\";\n",
            "    size_t used = 0;\n",
            "    __jet_target_write(text, sizeof(text) - 1, &used);\n",
            "}\n",
        ),
    )
    .expect("write program fixture");
    let clang = std::env::var_os("CC").unwrap_or_else(|| "clang".into());
    let output = Command::new(&clang)
        .arg(format!("--target={}", machine.triple))
        .arg("-nostdlib")
        .arg("-ffreestanding")
        .arg("-fno-builtin")
        .arg("-c")
        .arg(&source)
        .arg("-o")
        .arg(&object)
        .output()
        .expect("spawn clang for checked program fixture");
    assert!(
        output.status.success(),
        "clang fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    object
}

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
    machine.panic = PanicPolicy::Abort;
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
        required_capabilities: Vec::new(),
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
    machine.allocator = AllocatorPolicy::Counting {
        cap: Some(ByteSize::mib(8)),
    };
    let errors = machine.validate(&TargetMachineUse::default());
    assert!(errors.contains(&TargetMachineError::HostedAllocatorRequiresOs));
}

#[test]
fn no_os_core_mem_try_allocation_compiles_with_fixed_allocator() {
    let dir = std::env::temp_dir().join(format!(
        "jet_target_machine_try_allocation_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("try_allocation.jet");
    std::fs::write(
        &file,
        include_str!("../examples/features/memory/try_allocation.jet"),
    )
    .unwrap();
    jet::Driver::compile_bundle_path_with_target_machine(
        &file.to_string_lossy(),
        jet::Sema::CompileMode::Run,
        &sensor_machine(),
    )
    .expect("no-os core.mem fallible allocation should compile before codegen");
    let _ = std::fs::remove_dir_all(&dir);
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
        TargetMachineCompileError::Diagnostics(diags) => {
            assert!(diags.iter().any(|diagnostic| diagnostic.code == "E3310"));
        }
        TargetMachineCompileError::Machine(errors) => {
            panic!("expected registered diagnostics, got machine errors: {errors:?}")
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
        TargetMachineCompileError::Diagnostics(diags) => {
            assert!(diags.iter().any(|diagnostic| diagnostic.code == "E3313"));
        }
        TargetMachineCompileError::Machine(errors) => {
            panic!("expected registered diagnostics, got machine errors: {errors:?}")
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
    let program = checked_program_object(&machine, &dir);
    let arts = build_target_machine_firmware(&machine, &usage, &program, &dir)
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
    assert!(
        map.contains("Reset_Handler") || map.contains(".text"),
        "{map}"
    );
    let linker = std::fs::read_to_string(&arts.linker_script).unwrap();
    assert!(linker.contains("MEMORY {"));
    assert!(linker.contains("flash (rx)"));
    assert!(arts.size_budget.ok());
    assert!(arts.audit.contains("\"size_budget\""));
    assert!(arts.audit.contains("\"environment\":\"no-os\""));
    let bytes = std::fs::metadata(&arts.elf).unwrap().len();
    assert!(
        bytes > 0
            && bytes
                <= machine
                    .memory
                    .iter()
                    .filter(|r| r.kind == MemoryKind::Flash)
                    .map(|r| r.size.bytes)
                    .sum::<u64>()
    );
    let serial = qemu_target_machine_smoke(&machine, &arts.elf)
        .expect("QEMU MPS2 should run the linked program");
    assert!(serial.contains("program-linked\n"), "{serial}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn linux_freestanding_qemu_virt_boots_with_audit() {
    let machine = TargetMachine::board_virt_aarch64();
    let usage = TargetMachineUse::default();
    let dir = std::env::temp_dir().join(format!("jet_virt_fw_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let program = checked_program_object(&machine, &dir);
    let arts = build_target_machine_firmware(&machine, &usage, &program, &dir)
        .expect("virt aarch64 firmware build should succeed");
    assert!(arts.map.is_file());
    assert!(arts.audit.contains("board.virt_aarch64"));
    let serial = qemu_target_machine_smoke(&machine, &arts.elf)
        .expect("QEMU virt should run the linked program");
    assert!(serial.contains("program-linked\n"), "{serial}");
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
fn wasm_profiles_are_explicit_and_provider_bound() {
    let browser = TargetMachine::wasm_browser();
    assert_eq!(browser.environment_identity(), "browser");
    assert!(browser.is_browser_target());
    assert!(browser.is_web_target());
    assert!(browser.validate(&TargetMachineUse::default()).is_empty());

    let wasi = TargetMachine::wasm_wasi();
    assert_eq!(wasi.environment_identity(), "wasi");
    assert!(wasi.is_wasi_target());
    assert!(!wasi.is_web_target());
    assert!(wasi.validate(&TargetMachineUse::default()).is_empty());

    let no_os = TargetMachine::wasm_no_os();
    assert_eq!(no_os.environment_identity(), "no-os");
    assert!(!no_os.is_web_target());
    assert_eq!(no_os.max_runtime_layer(), RuntimeLayer::Core);
    assert!(no_os.validate(&TargetMachineUse::default()).is_empty());
}

#[test]
fn provider_changes_invalidate_target_artifact_identity() {
    let usage = TargetMachineUse::from_core_apis(["core.ui"]);
    let browser = TargetMachine::wasm_browser();
    let before = browser.target_dossier(&usage, ExecutionTier::Aot, "compiler", "deps");
    let mut changed = browser.clone();
    changed.byte_sink = jet::TargetMachine::ByteSinkPolicy::Provider {
        read: None,
        write: Some(ProviderContract::new("other.write", "sha256:other")),
        report: Some(ProviderContract::new("other.report", "sha256:other")),
    };
    let after = changed.target_dossier(&usage, ExecutionTier::Aot, "compiler", "deps");
    assert_ne!(before.provider_identity, after.provider_identity);
    assert_ne!(
        before.cache_bytes(&browser.triple),
        after.cache_bytes(&changed.triple)
    );
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
#[derive(Clone, Copy)]
struct ProviderFactCase {
    name: &'static str,
    capability: TargetCapability,
    set: fn(&mut TargetMachine, ProviderContract),
    clear: fn(&mut TargetMachine),
}

fn target_test_provider(name: &str) -> ProviderContract {
    ProviderContract::new(
        format!("tests.target.{name}"),
        format!("sha256:tests-target-{name}"),
    )
}

fn set_mmio_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.mmio = MmioPolicy::Provider { provider };
}

fn clear_mmio_provider(machine: &mut TargetMachine) {
    machine.mmio = MmioPolicy::None;
}

fn set_wall_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.wall_clock = ClockPolicy::Provider { provider };
}

fn clear_wall_provider(machine: &mut TargetMachine) {
    machine.wall_clock = ClockPolicy::None;
}

fn set_monotonic_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.monotonic_clock = ClockPolicy::Provider { provider };
}

fn clear_monotonic_provider(machine: &mut TargetMachine) {
    machine.monotonic_clock = ClockPolicy::None;
}

fn set_zone_data_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.zone_data = ClockPolicy::Provider { provider };
}

fn clear_zone_data_provider(machine: &mut TargetMachine) {
    machine.zone_data = ClockPolicy::None;
}

fn set_sleep_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.sleep = ClockPolicy::Provider { provider };
}

fn clear_sleep_provider(machine: &mut TargetMachine) {
    machine.sleep = ClockPolicy::None;
}

fn set_entropy_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.entropy = EntropyPolicy::Provider { provider };
}

fn clear_entropy_provider(machine: &mut TargetMachine) {
    machine.entropy = EntropyPolicy::None;
}

fn set_scheduler_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.scheduler = SchedulerPolicy::Cooperative { provider };
}

fn clear_scheduler_provider(machine: &mut TargetMachine) {
    machine.scheduler = SchedulerPolicy::None;
}

fn set_io_read_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.byte_sink = ByteSinkPolicy::Provider {
        read: Some(provider),
        write: Some(target_test_provider("io-write")),
        report: Some(target_test_provider("panic-report")),
    };
}

fn clear_io_read_provider(machine: &mut TargetMachine) {
    machine.byte_sink = ByteSinkPolicy::Provider {
        read: None,
        write: Some(target_test_provider("io-write")),
        report: Some(target_test_provider("panic-report")),
    };
}

fn set_io_write_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.byte_sink = ByteSinkPolicy::Provider {
        read: Some(target_test_provider("io-read")),
        write: Some(provider),
        report: Some(target_test_provider("panic-report")),
    };
}

fn clear_io_write_provider(machine: &mut TargetMachine) {
    machine.byte_sink = ByteSinkPolicy::Provider {
        read: Some(target_test_provider("io-read")),
        write: None,
        report: Some(target_test_provider("panic-report")),
    };
}

fn set_panic_report_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.byte_sink = ByteSinkPolicy::Provider {
        read: Some(target_test_provider("io-read")),
        write: Some(target_test_provider("io-write")),
        report: Some(provider),
    };
}

fn clear_panic_report_provider(machine: &mut TargetMachine) {
    machine.byte_sink = ByteSinkPolicy::Provider {
        read: Some(target_test_provider("io-read")),
        write: Some(target_test_provider("io-write")),
        report: None,
    };
}

fn set_allocator_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.allocator = AllocatorPolicy::Provider { provider };
}

fn clear_allocator_provider(machine: &mut TargetMachine) {
    machine.allocator = AllocatorPolicy::None;
}

fn set_startup_provider(machine: &mut TargetMachine, provider: ProviderContract) {
    machine.startup = StartupPolicy::Generated { provider };
}

fn clear_startup_provider(machine: &mut TargetMachine) {
    machine.startup = StartupPolicy::Unspecified;
}

fn provider_fact_cases() -> Vec<ProviderFactCase> {
    vec![
        ProviderFactCase {
            name: "MMIO",
            capability: TargetCapability::Mmio,
            set: set_mmio_provider,
            clear: clear_mmio_provider,
        },
        ProviderFactCase {
            name: "Time.Wall",
            capability: TargetCapability::TimeWall,
            set: set_wall_provider,
            clear: clear_wall_provider,
        },
        ProviderFactCase {
            name: "Time.Monotonic",
            capability: TargetCapability::TimeMonotonic,
            set: set_monotonic_provider,
            clear: clear_monotonic_provider,
        },
        ProviderFactCase {
            name: "Time.ZoneData",
            capability: TargetCapability::TimeZoneData,
            set: set_zone_data_provider,
            clear: clear_zone_data_provider,
        },
        ProviderFactCase {
            name: "Time.Sleep",
            capability: TargetCapability::TimeSleep,
            set: set_sleep_provider,
            clear: clear_sleep_provider,
        },
        ProviderFactCase {
            name: "Rand.Entropy",
            capability: TargetCapability::Entropy,
            set: set_entropy_provider,
            clear: clear_entropy_provider,
        },
        ProviderFactCase {
            name: "Target.Scheduler",
            capability: TargetCapability::Scheduler,
            set: set_scheduler_provider,
            clear: clear_scheduler_provider,
        },
        ProviderFactCase {
            name: "IO.Read",
            capability: TargetCapability::IoRead,
            set: set_io_read_provider,
            clear: clear_io_read_provider,
        },
        ProviderFactCase {
            name: "IO.Write",
            capability: TargetCapability::IoWrite,
            set: set_io_write_provider,
            clear: clear_io_write_provider,
        },
        ProviderFactCase {
            name: "Panic.Report",
            capability: TargetCapability::PanicReport,
            set: set_panic_report_provider,
            clear: clear_panic_report_provider,
        },
        ProviderFactCase {
            name: "Target.Allocator",
            capability: TargetCapability::Allocator,
            set: set_allocator_provider,
            clear: clear_allocator_provider,
        },
        ProviderFactCase {
            name: "Target.Startup",
            capability: TargetCapability::Startup,
            set: set_startup_provider,
            clear: clear_startup_provider,
        },
    ]
}

fn required_capability_use(capability: TargetCapability) -> TargetMachineUse {
    let mut usage = TargetMachineUse {
        required_capabilities: vec![capability],
        ..TargetMachineUse::default()
    };
    if capability == TargetCapability::Allocator {
        usage.heap_required = true;
    }
    usage
}

fn has_missing_capability(
    errors: &[TargetMachineError],
    capability: TargetCapability,
) -> bool {
    errors.iter().any(|error| match error {
        TargetMachineError::MissingTargetCapability { capability: found } => {
            found == capability.as_str()
        }
        TargetMachineError::HeapRequiresAllocator => capability == TargetCapability::Allocator,
        _ => false,
    })
}

fn has_invalid_provider(errors: &[TargetMachineError], capability: TargetCapability) -> bool {
    errors.iter().any(|error| {
        matches!(
            error,
            TargetMachineError::InvalidProviderContract {
                capability: found,
                ..
            } if found == capability.as_str()
        )
    })
}

#[test]
fn target_provider_fact_matrix_covers_present_missing_and_malformed() {
    for case in provider_fact_cases() {
        let usage = required_capability_use(case.capability);

        let mut present = sensor_machine();
        (case.set)(&mut present, target_test_provider(case.name));
        let errors = present.validate(&usage);
        assert!(
            errors.is_empty(),
            "{} present provider rejected: {errors:?}",
            case.name
        );
        assert!(
            jet::Sema::check_target_machine(&present, &usage).is_empty(),
            "{} present provider reached a sema diagnostic",
            case.name
        );

        let mut missing = sensor_machine();
        (case.clear)(&mut missing);
        let errors = missing.validate(&usage);
        assert!(
            has_missing_capability(&errors, case.capability),
            "{} missing provider was admitted: {errors:?}",
            case.name
        );
        let diagnostics = jet::Sema::check_target_machine(&missing, &usage);
        let expected_code = if case.capability == TargetCapability::Allocator {
            "E3303"
        } else {
            "E3311"
        };
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == expected_code),
            "{} missing provider did not use {expected_code}: {diagnostics:?}",
            case.name
        );

        for (form, provider) in [
            (
                "missing identity",
                ProviderContract::new("", "not-a-digest"),
            ),
            (
                "missing digest",
                ProviderContract::new("tests.target.invalid", ""),
            ),
            (
                "empty sha256 payload",
                ProviderContract::new("tests.target.invalid", "sha256:"),
            ),
        ] {
            let mut malformed = sensor_machine();
            (case.set)(&mut malformed, provider);
            let errors = malformed.validate(&usage);
            assert!(
                has_invalid_provider(&errors, case.capability),
                "{} {form} provider was admitted: {errors:?}",
                case.name
            );
            let diagnostics = jet::Sema::check_target_machine(&malformed, &usage);
            assert!(
                diagnostics.iter().any(|diagnostic| diagnostic.code == "E3312"),
                "{} {form} provider did not use E3312: {diagnostics:?}",
                case.name
            );
        }
    }
}

#[test]
fn panic_policy_matrix_covers_explicit_missing_and_malformed() {
    let mut present = sensor_machine();
    present.panic = PanicPolicy::Abort;
    assert!(
        present.validate(&TargetMachineUse::default()).is_empty(),
        "explicit abort panic policy was rejected"
    );
    assert!(
        jet::Sema::check_target_machine(&present, &TargetMachineUse::default()).is_empty(),
        "explicit abort panic policy reached a diagnostic"
    );

    let mut missing = sensor_machine();
    missing.panic = PanicPolicy::Unspecified;
    let errors = missing.validate(&TargetMachineUse::default());
    assert!(
        errors.contains(&TargetMachineError::MissingPanicPolicy),
        "missing panic policy was admitted: {errors:?}"
    );
    let diagnostics = jet::Sema::check_target_machine(&missing, &TargetMachineUse::default());
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3314"),
        "missing panic policy did not use E3314: {diagnostics:?}"
    );

    let mut malformed = sensor_machine();
    malformed.panic = PanicPolicy::Report {
        provider: ProviderContract::new("", "not-a-digest"),
    };
    let errors = malformed.validate(&TargetMachineUse::default());
    assert!(
        has_invalid_provider(&errors, TargetCapability::PanicReport),
        "malformed panic provider was admitted: {errors:?}"
    );
    let diagnostics = jet::Sema::check_target_machine(&malformed, &TargetMachineUse::default());
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3312"),
        "malformed panic provider did not use E3312: {diagnostics:?}"
    );
}

#[test]
fn startup_provider_covers_entry_vector_abi_and_placement() {
    let sensor = sensor_machine();
    let startup = sensor
        .generate_startup_source()
        .expect("sensor startup source should be generated");
    assert_eq!(startup.filename, "startup.c");
    for marker in [
        "typedef void (*vec_t)(void);",
        "void __jet_program_entry(void);",
        "__attribute__((section(\".vectors\"), used))",
        "vec_t const vectors[]",
        "__jet_data_load",
        "__jet_bss_start",
        "__jet_program_entry();",
        "wfi",
    ] {
        assert!(startup.contents.contains(marker), "missing startup marker {marker}");
    }
    let linker = sensor
        .generate_linker_script()
        .expect("sensor linker should be generated");
    for marker in [
        "ENTRY(Reset_Handler)",
        "KEEP(*(.vectors))",
        "__jet_stack_top",
        "__jet_data_start",
        "__jet_data_end",
        "__jet_data_load",
        "__jet_bss_start",
        "__jet_bss_end",
        "__jet_heap_start",
        "__jet_heap_end",
        "> ram AT > flash",
    ] {
        assert!(linker.contains(marker), "missing linker placement marker {marker}");
    }

    let virt = TargetMachine::board_virt_aarch64();
    let startup = virt
        .generate_startup_source()
        .expect("aarch64 startup source should be generated");
    assert_eq!(startup.filename, "startup.S");
    for marker in [
        ".global _start",
        "_start:",
        "ldr x0, =__jet_stack_top",
        "bl __jet_program_entry",
        "wfe",
    ] {
        assert!(startup.contents.contains(marker), "missing aarch64 marker {marker}");
    }
    assert!(
        virt
            .generate_linker_script()
            .expect("aarch64 linker should be generated")
            .contains("ENTRY(_start)")
    );
}

fn write_target_fixture(
    tag: &str,
    source: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "jet_target_machine_witness_{tag}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create target witness directory");
    let file = dir.join(format!("{tag}.jet"));
    std::fs::write(&file, source).expect("write target witness");
    (dir, file)
}

fn compile_target_fixture(
    tag: &str,
    source: &str,
    machine: &TargetMachine,
) -> Result<jet::CompileOutput, TargetMachineCompileError> {
    let (dir, file) = write_target_fixture(tag, source);
    let shown = file.to_string_lossy();
    let result = jet::Driver::compile_bundle_path_with_target_machine(
        &shown,
        jet::Sema::CompileMode::Run,
        machine,
    );
    let _ = std::fs::remove_dir_all(dir);
    result
}

#[test]
fn malformed_target_provider_is_rejected_before_codegen() {
    let mut machine = sensor_machine();
    machine.wall_clock = ClockPolicy::Provider {
        provider: ProviderContract::new("tests.target.clock", ""),
    };
    let result = compile_target_fixture(
        "malformed_provider",
        include_str!("target_witness/heap_free_core.jet"),
        &machine,
    );
    match result {
        Err(TargetMachineCompileError::Diagnostics(diagnostics)) => {
            assert!(
                diagnostics.iter().any(|diagnostic| diagnostic.code == "E3312"),
                "expected E3312 before codegen: {diagnostics:?}"
            );
        }
        Err(TargetMachineCompileError::Machine(errors)) => {
            panic!("target provider failed outside sema diagnostics: {errors:?}");
        }
        Ok(_) => panic!("malformed target provider reached codegen"),
    }
}

#[test]
fn heap_free_core_target_witness() {
    let source = include_str!("target_witness/heap_free_core.jet");
    let machine = TargetMachine::wasm_no_os();
    assert_eq!(machine.max_runtime_layer(), RuntimeLayer::Core);
    // Applicable tier: AOT. Dev/JIT are excluded by the no-OS execution contract.
    assert!(machine.supports_execution_tier(ExecutionTier::Aot).is_ok());
    assert!(machine.supports_execution_tier(ExecutionTier::Dev).is_err());
    assert!(machine.supports_execution_tier(ExecutionTier::Jit).is_err());
    let output = compile_target_fixture("heap_free_core", source, &machine)
        .expect("heap-free Core witness should compile");
    assert_eq!(output.inferred_layer, RuntimeLayer::Core);
    assert!(output.web.is_none());
    assert!(
        output
            .rust
            .contains("// jet:target-dossier layer=core provider=target-providers-v1:")
    );
}

#[test]
fn allocator_firmware_target_witness() {
    let source = include_str!("target_witness/allocator_firmware.jet");
    let machine = sensor_machine();
    assert_eq!(machine.max_runtime_layer(), RuntimeLayer::Alloc);
    // Applicable tier: AOT firmware. Dev/JIT are excluded for no-OS machines.
    assert!(machine.supports_execution_tier(ExecutionTier::Aot).is_ok());
    assert!(machine.supports_execution_tier(ExecutionTier::Dev).is_err());
    assert!(machine.supports_execution_tier(ExecutionTier::Jit).is_err());

    let output = compile_target_fixture("allocator_firmware", source, &machine)
        .expect("allocator firmware witness should compile");
    assert_eq!(output.inferred_layer, RuntimeLayer::Alloc);
    assert!(
        output
            .rust
            .contains("// jet:target-dossier layer=alloc provider=target-providers-v1:")
    );

    let usage = TargetMachineUse::from_core_apis(["core.mem"]);
    let out_dir = std::env::temp_dir().join(format!(
        "jet_allocator_firmware_witness_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out_dir);
    let program = checked_program_object(&machine, &out_dir);
    let artifacts = build_target_machine_firmware(&machine, &usage, &program, &out_dir)
        .expect("allocator firmware target should emit an AOT image");
    assert!(artifacts.elf.is_file());
    assert!(artifacts.map.is_file());
    assert!(artifacts.audit.contains("\"allocator\":{\"kind\":\"fixed\""));
    assert!(artifacts.audit.contains("\"execution\":{\"aot\":true"));
    let _ = std::fs::remove_dir_all(out_dir);
}

#[test]
fn browser_wasm_target_witness() {
    let source = include_str!("target_witness/browser_wasm.jet");
    let machine = TargetMachine::wasm_browser();
    assert!(machine.is_browser_target());
    assert!(machine.is_web_target());
    // Browser web output is AOT; the hosted machine remains valid for Dev/JIT.
    for tier in [ExecutionTier::Aot, ExecutionTier::Dev, ExecutionTier::Jit] {
        assert!(
            machine.supports_execution_tier(tier).is_ok(),
            "browser machine rejected hosted tier {}",
            tier.as_str()
        );
    }
    let output = compile_target_fixture("browser_wasm", source, &machine)
        .expect("browser Wasm witness should compile");
    let web = output.web.expect("browser machine should emit web artifacts");
    for marker in [
        "\"wasmTriple\": \"wasm32-unknown-unknown\"",
        "\"runtimeLayer\": \"hosted\"",
        "\"providerIdentity\": \"target-providers-v1:",
        "\"targetEnvironment\": \"browser\"",
    ] {
        assert!(web.manifest_json.contains(marker), "missing browser marker {marker}");
    }
    assert!(web.js_app.contains("const JET_TARGET_DOSSIER"));
    assert!(web.wasm_rust.contains("jet_target_artifact_identity_ptr"));
}

#[test]
fn wasi_target_witness() {
    let source = include_str!("target_witness/wasi.jet");
    let machine = TargetMachine::wasm_wasi();
    assert!(machine.is_wasi_target());
    assert!(!machine.is_web_target());
    // WASI uses the hosted AOT provider set here; hosted Dev/JIT remain allowed.
    for tier in [ExecutionTier::Aot, ExecutionTier::Dev, ExecutionTier::Jit] {
        assert!(
            machine.supports_execution_tier(tier).is_ok(),
            "WASI machine rejected hosted tier {}",
            tier.as_str()
        );
    }
    let output =
        compile_target_fixture("wasi", source, &machine).expect("WASI witness should compile");
    assert!(output.web.is_none());
    assert_eq!(output.inferred_layer, RuntimeLayer::Alloc);
    assert!(
        output
            .rust
            .contains("// jet:target-dossier layer=alloc provider=target-providers-v1:")
    );
}

#[test]
fn hosted_default_target_witness() {
    let source = include_str!("target_witness/hosted_default.jet");
    let machine = TargetMachine::hosted("x86_64-unknown-linux-gnu");
    assert!(!machine.no_os);
    // Hosted default applies to AOT, Dev, and JIT; no no-OS tier is excluded.
    for tier in [ExecutionTier::Aot, ExecutionTier::Dev, ExecutionTier::Jit] {
        assert!(
            machine.supports_execution_tier(tier).is_ok(),
            "hosted machine rejected tier {}",
            tier.as_str()
        );
    }
    let output = compile_target_fixture("hosted_default", source, &machine)
        .expect("hosted default witness should compile");
    assert_eq!(output.inferred_layer, RuntimeLayer::Alloc);
    assert!(output.web.is_none());
    assert!(
        output
            .rust
            .contains("// jet:target-dossier layer=alloc provider=target-providers-v1:")
    );
    let (status, stdout, stderr) = common::build_and_run(
        "target_machine_hosted_witness",
        "hosted_default",
        source,
    );
    assert_eq!(status, 0, "hosted runner failed: {stderr}");
    assert_eq!(stdout, "hosted default\n");
}
