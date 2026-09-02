use crate::AST::ProgramBundle;
use crate::Diagnostics::Diagnostic;
use jet_foundation::RingLayer::classify_prelude_closure;
use jet_foundation::TargetMachine::{TargetMachine, TargetMachineError, TargetMachineUse};

/// Project the complete sema-owned Prelude closure into target requirements.
///
/// `used_core` contains direct calls and sema-generated roots. The target pass
/// must see the transitive closure, not only imports visible to one checker, so
/// it expands the same RingLayer graph before validating target facts.
pub fn target_machine_use(bundle: &ProgramBundle) -> TargetMachineUse {
    let closure = classify_prelude_closure(bundle.used_core.iter());
    let mut usage = TargetMachineUse::from_core_apis(closure.keys());
    usage.core_apis = closure.into_keys().collect();
    usage
}

/// Admit a selected target before codegen. TargetMachine remains the typed fact
/// validator; this seam projects its data errors through registered sema rows
/// so human and machine output share one diagnostic contract.
pub fn check_target_machine(
    machine: &TargetMachine,
    usage: &TargetMachineUse,
) -> Vec<Diagnostic> {
    machine
        .validate(usage)
        .iter()
        .map(|error| diagnostic_for_error(machine, error))
        .collect()
}

fn diagnostic_for_error(machine: &TargetMachine, error: &TargetMachineError) -> Diagnostic {
    let target = target_name(machine);
    match error {
        TargetMachineError::CoreApiUnavailable {
            api,
            required,
            available,
        } => {
            let required = required.as_str();
            let available = available.as_str();
            Diagnostic::from_row(
                "E3310",
                &[
                    ("api", api),
                    ("required", required),
                    ("available", available),
                    ("target", target),
                ],
                None,
            )
        }
        TargetMachineError::HeapRequiresAllocator
        | TargetMachineError::MissingAllocatorPolicy => {
            Diagnostic::from_row("E3303", &[], None)
        }
        TargetMachineError::MissingTargetCapability { capability }
        | TargetMachineError::HostedCapabilityRequiresOs { capability } => {
            Diagnostic::from_row(
                "E3311",
                &[("target", target), ("capability", capability)],
                None,
            )
        }
        TargetMachineError::InvalidProviderContract {
            capability,
            provider,
            sha256,
        } => Diagnostic::from_row(
            "E3312",
            &[
                ("target", target),
                ("capability", capability),
                ("provider", provider),
                ("sha256", sha256),
            ],
            None,
        ),
        TargetMachineError::MmioOutsideRegion { address, size_bytes } => {
            let detail = format!(
                "address 0x{address:X} ({size_bytes} bytes) is outside a declared MMIO region"
            );
            mmio_diagnostic(target, &detail)
        }
        TargetMachineError::MmioMissingUnsafeGate { address } => {
            let detail = format!("address 0x{address:X} has no #Unsafe gate");
            mmio_diagnostic(target, &detail)
        }
        TargetMachineError::MmioEmptyUnsafeReason { address } => {
            let detail = format!("address 0x{address:X} has an empty #Unsafe reason");
            mmio_diagnostic(target, &detail)
        }
        _ => {
            let (fact, detail) = generic_detail(error);
            Diagnostic::from_row(
                "E3314",
                &[("target", target), ("fact", fact), ("detail", detail.as_str())],
                None,
            )
        }
    }
}

fn target_name(machine: &TargetMachine) -> &str {
    if machine.name.trim().is_empty() {
        &machine.triple
    } else {
        &machine.name
    }
}

fn mmio_diagnostic(target: &str, detail: &str) -> Diagnostic {
    Diagnostic::from_row(
        "E3313",
        &[("target", target), ("detail", detail)],
        None,
    )
}

fn generic_detail(error: &TargetMachineError) -> (&'static str, String) {
    match error {
        TargetMachineError::MissingTargetTriple => {
            ("target triple", "the selected target triple is empty".to_string())
        }
        TargetMachineError::MissingMemoryKind { kind } => (
            "memory",
            format!("the target is missing a {} memory region", memory_kind_name(*kind)),
        ),
        TargetMachineError::DuplicateMemoryRegion { name } => {
            ("memory", format!("memory region `{name}` is declared more than once"))
        }
        TargetMachineError::EmptyMemoryRegion { name } => {
            ("memory", format!("memory region `{name}` has zero size"))
        }
        TargetMachineError::MemoryAddressOverflow { name } => {
            ("memory", format!("memory region `{name}` overflows the address space"))
        }
        TargetMachineError::OverlappingMemoryRegions { first, second } => (
            "memory",
            format!("memory regions `{first}` and `{second}` overlap"),
        ),
        TargetMachineError::MissingLinkerInput => {
            ("linker", "the no-OS target has no linker input".to_string())
        }
        TargetMachineError::LinkerFileMissingPath => {
            ("linker", "the linker file path is empty".to_string())
        }
        TargetMachineError::LinkerFileMissingHash { path } => (
            "linker",
            format!("linker file `{path}` has no valid sha256 provenance"),
        ),
        TargetMachineError::AllocatorRegionUnknown { region } => (
            "allocator",
            format!("allocator region `{region}` does not exist"),
        ),
        TargetMachineError::AllocatorRegionNotRam { region } => (
            "allocator",
            format!("allocator region `{region}` is not RAM"),
        ),
        TargetMachineError::AllocatorRegionTooSmall {
            region,
            requested_bytes,
            available_bytes,
        } => (
            "allocator",
            format!(
                "allocator region `{region}` has {available_bytes} bytes but needs {requested_bytes}"
            ),
        ),
        TargetMachineError::HostedAllocatorRequiresOs => (
            "allocator",
            "a hosted system allocator cannot be selected on a no-OS target".to_string(),
        ),
        TargetMachineError::MissingPanicPolicy => (
            "panic",
            "the no-OS target has no panic policy".to_string(),
        ),
        TargetMachineError::RamOverflow {
            used_bytes,
            ram_bytes,
        } => (
            "memory",
            format!("RAM use ({used_bytes} bytes) exceeds {ram_bytes} bytes"),
        ),
        TargetMachineError::HostedHasNoLinkerScript => (
            "linker",
            "hosted targets do not have a generated linker script".to_string(),
        ),
        TargetMachineError::HostedHasNoStartup => (
            "startup",
            "hosted targets do not have generated startup glue".to_string(),
        ),
        TargetMachineError::UnsupportedStartupTriple { triple } => (
            "startup",
            format!("target triple `{triple}` has no startup generator"),
        ),
        TargetMachineError::ExecutionTierUnsupported { tier, machine } => (
            "execution",
            format!("target machine `{machine}` does not support the `{tier}` tier"),
        ),
        TargetMachineError::FirmwareToolchainMissing { tool } => (
            "toolchain",
            format!("required firmware tool `{tool}` is unavailable"),
        ),
        TargetMachineError::FirmwareBuildFailed { detail } => ("firmware", detail.clone()),
        TargetMachineError::SizeBudgetExceeded { report } => (
            "size budget",
            format!(
                "artifact={} flash={} ram={} used_ram={}",
                report.artifact_bytes, report.flash_bytes, report.ram_bytes, report.ram_used_bytes
            ),
        ),
        _ => ("target", "the selected target fact is invalid".to_string()),
    }
}

fn memory_kind_name(kind: jet_foundation::TargetMachine::MemoryKind) -> &'static str {
    match kind {
        jet_foundation::TargetMachine::MemoryKind::Flash => "flash",
        jet_foundation::TargetMachine::MemoryKind::Ram => "RAM",
        jet_foundation::TargetMachine::MemoryKind::Mmio => "MMIO",
        jet_foundation::TargetMachine::MemoryKind::Reserved => "reserved",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jet_foundation::TargetMachine::{
        ByteSize, ClockPolicy, MmioAccess, ProviderContract, TargetCapability, UnsafeGate,
    };

    fn row<'a>(diagnostics: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
        diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .unwrap_or_else(|| panic!("missing {code} diagnostic: {diagnostics:?}"))
    }

    fn assert_row(
        diagnostics: &[Diagnostic],
        code: &str,
        what: &str,
        why: &str,
        fix: &str,
    ) {
        let diagnostic = row(diagnostics, code);
        assert_eq!(diagnostic.what, what);
        assert_eq!(diagnostic.why, why);
        assert_eq!(diagnostic.fix, fix);
    }

    #[test]
    fn target_machine_rows_are_registered_and_stable() {
        let machine = TargetMachine::wasm_no_os();
        let diagnostics =
            check_target_machine(&machine, &TargetMachineUse::from_core_apis(["core.files"]));
        assert_row(
            &diagnostics,
            "E3310",
            "prelude part `core.files` needs the `hosted` runtime layer, but target `wasm.no-os` provides `core`.",
            "the complete semantic Prelude closure includes `core.files`; a target cannot emit a higher layer or substitute a second Prelude.",
            "select a typed target with `hosted` support, or remove `core.files` from the reachable closure.",
        );

        let usage = TargetMachineUse {
            required_capabilities: vec![TargetCapability::TimeWall],
            ..TargetMachineUse::default()
        };
        assert_row(
            &check_target_machine(&machine, &usage),
            "E3311",
            "Target `wasm.no-os` lacks the required `Time.Wall` capability.",
            "the selected target profile must state each machine service separately; a target triple cannot imply a UART, clock, entropy source, scheduler, allocator, startup provider, or MMIO device.",
            "select a typed target profile with a `Time.Wall` provider, or remove the reachable operation.",
        );

        let mut invalid_provider = machine.clone();
        invalid_provider.monotonic_clock = ClockPolicy::Provider {
            provider: ProviderContract::new("", "not-a-digest"),
        };
        assert_row(
            &check_target_machine(
                &invalid_provider,
                &TargetMachineUse {
                    required_capabilities: vec![TargetCapability::TimeMonotonic],
                    ..TargetMachineUse::default()
                },
            ),
            "E3312",
            "provider `` for `Time.Monotonic` on target `wasm.no-os` has invalid digest `not-a-digest`.",
            "every selected provider needs a stable identity and sha256 provenance so artifacts cannot reuse changed target behavior.",
            "declare a nonempty provider identity and a `sha256:` digest for `Time.Monotonic`.",
        );

        assert_row(
            &check_target_machine(
                &machine,
                &TargetMachineUse {
                    mmio: vec![MmioAccess {
                        address: 0x10,
                        size: ByteSize::bytes(4),
                        unsafe_gate: Some(UnsafeGate {
                            reason: "probe".to_string(),
                        }),
                    }],
                    ..TargetMachineUse::default()
                },
            ),
            "E3313",
            "Target `wasm.no-os` has an MMIO fact failure: address 0x10 (4 bytes) is outside a declared MMIO region.",
            "MMIO addresses must lie inside a declared MMIO region and every access must carry a nonempty `#Unsafe` reason.",
            "declare a matching MMIO provider and region, then place the access inside `#Unsafe(\"reason\")`.",
        );

        let broken = TargetMachine::bare_metal("broken", "");
        assert_row(
            &check_target_machine(&broken, &TargetMachineUse::default()),
            "E3314",
            "Target `broken` has an invalid `target triple` fact: the selected target triple is empty.",
            "Target memory, linker, allocator, panic, startup, and execution facts are explicit inputs to sema and cannot be recovered safely from a triple.",
            "select a complete typed target profile and correct the `target triple` declaration.",
        );
    }
}
