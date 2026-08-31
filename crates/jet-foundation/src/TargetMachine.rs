//! D-TARGET-* typed target machine facts.
//!
//! Internal model for embedded/freestanding builds. Validation errors stay data
//! (not new user diagnostics) until a follow-up surface ballot lands. Hosted
//! Jet keeps hidden defaults; selecting a no-OS machine exposes memory, linker,
//! allocator, panic, volatile/MMIO, and audit facts.

use crate::RingLayer::{core_module_layer, core_usage_layer, RuntimeLayer};
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetMachine {
    pub name: String,
    pub triple: String,
    pub no_os: bool,
    pub memory: Vec<MemoryRegion>,
    pub linker: LinkerInput,
    pub allocator: AllocatorPolicy,
    pub panic: PanicPolicy,
    /// D-FREESTAND-FACTS1=A: typed provider for device memory access.
    pub mmio: MmioPolicy,
    /// D-FREESTAND-TIME1=A: each time service is an independent fact.
    pub wall_clock: ClockPolicy,
    pub monotonic_clock: ClockPolicy,
    pub zone_data: ClockPolicy,
    pub sleep: ClockPolicy,
    /// D-FREESTAND-FACTS1=A: cryptographic entropy is distinct from seeded Rng.
    pub entropy: EntropyPolicy,
    /// D-FREESTAND-SCHED1=A: target-selected task runtime.
    pub scheduler: SchedulerPolicy,
    /// D-FREESTAND-SINK1=B: typed byte input/output/report providers.
    pub byte_sink: ByteSinkPolicy,
    /// D-FREESTAND-START1=A: generated startup provider.
    pub startup: StartupPolicy,
    pub audit: AuditPolicy,
}

impl TargetMachine {
    pub fn hosted(triple: impl Into<String>) -> Self {
        Self {
            name: "hosted".to_string(),
            triple: triple.into(),
            no_os: false,
            memory: Vec::new(),
            linker: LinkerInput::HostedDefault,
            allocator: AllocatorPolicy::HostedDefault,
            panic: PanicPolicy::HostedDefault,
            mmio: MmioPolicy::HostedDefault,
            wall_clock: ClockPolicy::HostedDefault,
            monotonic_clock: ClockPolicy::HostedDefault,
            zone_data: ClockPolicy::HostedDefault,
            sleep: ClockPolicy::HostedDefault,
            entropy: EntropyPolicy::HostedDefault,
            scheduler: SchedulerPolicy::HostedDefault,
            byte_sink: ByteSinkPolicy::HostedDefault,
            startup: StartupPolicy::HostedDefault,
            audit: AuditPolicy::default(),
        }
    }

    pub fn freestanding(name: impl Into<String>, triple: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            triple: triple.into(),
            no_os: true,
            memory: Vec::new(),
            linker: LinkerInput::Generated,
            allocator: AllocatorPolicy::Unspecified,
            panic: PanicPolicy::Unspecified,
            mmio: MmioPolicy::Unspecified,
            wall_clock: ClockPolicy::Unspecified,
            monotonic_clock: ClockPolicy::Unspecified,
            zone_data: ClockPolicy::Unspecified,
            sleep: ClockPolicy::Unspecified,
            entropy: EntropyPolicy::Unspecified,
            scheduler: SchedulerPolicy::Unspecified,
            byte_sink: ByteSinkPolicy::Unspecified,
            startup: StartupPolicy::Unspecified,
            audit: AuditPolicy::default(),
        }
    }

    pub fn max_runtime_layer(&self) -> RuntimeLayer {
        if !self.no_os {
            RuntimeLayer::Std
        } else if self.allocator.provides_heap() {
            RuntimeLayer::Alloc
        } else {
            RuntimeLayer::Core
        }
    }

    pub fn validate(&self, usage: &TargetMachineUse) -> Vec<TargetMachineError> {
        let mut errors = Vec::new();

        if self.triple.trim().is_empty() {
            errors.push(TargetMachineError::MissingTargetTriple);
        }

        validate_memory_regions(&self.memory, &mut errors);
        validate_linker(&self.linker, self.no_os, &mut errors);
        validate_allocator(self, &mut errors);
        validate_panic(self, &mut errors);
        validate_ram_budget(self, usage, &mut errors);
        validate_core_usage(self, usage, &mut errors);
        validate_target_capabilities(self, usage, &mut errors);
        validate_mmio(self, usage, &mut errors);

        errors
    }

    pub fn audit_json(&self, usage: &TargetMachineUse) -> String {
        self.audit_json_with_budget(usage, None)
    }

    pub fn audit_json_with_budget(
        &self,
        usage: &TargetMachineUse,
        budget: Option<&SizeBudgetReport>,
    ) -> String {
        let mut out = String::new();
        out.push('{');
        push_field(&mut out, "name", &json_str(&self.name), true);
        push_field(&mut out, "triple", &json_str(&self.triple), false);
        push_field(
            &mut out,
            "environment",
            if self.no_os {
                "\"no-os\""
            } else {
                "\"hosted\""
            },
            false,
        );
        push_field(&mut out, "linker", &self.linker.audit_json(), false);
        push_field(&mut out, "allocator", &self.allocator.audit_json(), false);
        push_field(&mut out, "panic", &self.panic.audit_json(), false);
        push_field(&mut out, "memory", &memory_json(&self.memory), false);
        push_field(&mut out, "mmio_capability", &self.mmio.audit_json(), false);
        push_field(&mut out, "time_wall", &self.wall_clock.audit_json(), false);
        push_field(
            &mut out,
            "time_monotonic",
            &self.monotonic_clock.audit_json(),
            false,
        );
        push_field(&mut out, "time_zone_data", &self.zone_data.audit_json(), false);
        push_field(&mut out, "time_sleep", &self.sleep.audit_json(), false);
        push_field(&mut out, "entropy", &self.entropy.audit_json(), false);
        push_field(&mut out, "scheduler", &self.scheduler.audit_json(), false);
        push_field(&mut out, "byte_sink", &self.byte_sink.audit_json(), false);
        push_field(&mut out, "startup", &self.startup.audit_json(), false);
        push_field(
            &mut out,
            "unavailable_core_apis",
            &unavailable_core_json(self, usage),
            false,
        );
        push_field(&mut out, "mmio", &mmio_json(&usage.mmio), false);
        push_field(&mut out, "execution", &execution_json(self), false);
        if let Some(budget) = budget {
            push_field(&mut out, "size_budget", &budget.to_json(), false);
        }
        out.push('}');
        out
    }

    /// D-TARGET-LINKER1=A: generate linker input from typed memory regions.
    pub fn generate_linker_script(&self) -> Result<String, TargetMachineError> {
        if !self.no_os {
            return Err(TargetMachineError::HostedHasNoLinkerScript);
        }
        if self.memory.is_empty() {
            return Err(TargetMachineError::MissingMemoryKind {
                kind: MemoryKind::Flash,
            });
        }
        let mut out = String::from("/* generated by Jet target machine */\nMEMORY {\n");
        for region in &self.memory {
            let attrs = match region.kind {
                MemoryKind::Flash => "rx",
                MemoryKind::Ram => "rwx",
                MemoryKind::Mmio => "rw",
                MemoryKind::Reserved => "r",
            };
            let _ = write!(
                out,
                "  {} ({attrs}) : ORIGIN = 0x{:08X}, LENGTH = {}\n",
                region.name,
                region.origin,
                format_length(region.size.bytes)
            );
        }
        out.push_str("}\n");
        let entry = if self.triple.contains("thumb") || self.triple.contains("armv") {
            "Reset_Handler"
        } else {
            "_start"
        };
        let _ = write!(out, "ENTRY({entry})\nSECTIONS {{\n");
        if let Some(flash) = self.memory.iter().find(|r| r.kind == MemoryKind::Flash) {
            let _ = write!(
                out,
                "  .text : {{\n    KEEP(*(.vectors))\n    *(.text*)\n    *(.rodata*)\n  }} > {}\n",
                flash.name
            );
        }
        if let Some(ram) = self.memory.iter().find(|r| r.kind == MemoryKind::Ram) {
            let flash = self
                .memory
                .iter()
                .find(|r| r.kind == MemoryKind::Flash)
                .map(|r| r.name.as_str())
                .unwrap_or(ram.name.as_str());
            let _ = write!(
                out,
                "  .data : {{ *(.data*) }} > {} AT > {}\n  .bss : {{ *(.bss*) *(COMMON) }} > {}\n",
                ram.name, flash, ram.name
            );
        }
        out.push_str("}\n");
        Ok(out)
    }

    /// Startup source that matches the machine triple (AOT firmware smoke).
    pub fn generate_startup_source(&self) -> Result<StartupSource, TargetMachineError> {
        if !self.no_os {
            return Err(TargetMachineError::HostedHasNoStartup);
        }
        if self.triple.contains("thumb") || self.triple.starts_with("arm") {
            let ram = self
                .memory
                .iter()
                .find(|r| r.kind == MemoryKind::Ram)
                .ok_or(TargetMachineError::MissingMemoryKind {
                    kind: MemoryKind::Ram,
                })?;
            let stack_top = ram.origin.saturating_add(ram.size.bytes);
            let mark = ram.origin;
            Ok(StartupSource {
                filename: "startup.c".to_string(),
                contents: format!(
                    concat!(
                        "/* generated by Jet target machine `{name}` */\n",
                        "typedef void (*vec_t)(void);\n",
                        "void Reset_Handler(void);\n",
                        "void Default_Handler(void) {{ for(;;){{}} }}\n",
                        "__attribute__((section(\".vectors\"), used))\n",
                        "vec_t const vectors[] = {{ (vec_t)0x{stack_top:08X}u, Reset_Handler }};\n",
                        "void Reset_Handler(void) {{\n",
                        "  volatile unsigned char *mark = (volatile unsigned char *)0x{mark:08X}u;\n",
                        "  mark[0] = 0x4F; mark[1] = 0x4B;\n",
                        "  for(;;){{}}\n",
                        "}}\n"
                    ),
                    name = self.name,
                    stack_top = stack_top,
                    mark = mark
                ),
            })
        } else if self.triple.contains("aarch64") {
            // QEMU virt UART0 at 0x09000000 — print "OK\n" then idle.
            // Split the UART base into mov+lsl so clang's aarch64 asm accepts it.
            Ok(StartupSource {
                filename: "startup.S".to_string(),
                contents: format!(
                    concat!(
                        "/* generated by Jet target machine `{name}` */\n",
                        ".global _start\n",
                        "_start:\n",
                        "  mov x1, #0x0900\n",
                        "  lsl x1, x1, #16\n",
                        "  mov w0, #79\n",
                        "  str w0, [x1]\n",
                        "  mov w0, #75\n",
                        "  str w0, [x1]\n",
                        "  mov w0, #10\n",
                        "  str w0, [x1]\n",
                        "1: wfe\n",
                        "  b 1b\n"
                    ),
                    name = self.name
                ),
            })
        } else {
            Err(TargetMachineError::UnsupportedStartupTriple {
                triple: self.triple.clone(),
            })
        }
    }

    pub fn size_budget(&self, usage: &TargetMachineUse, artifact_bytes: u64) -> SizeBudgetReport {
        let flash_bytes: u64 = self
            .memory
            .iter()
            .filter(|r| r.kind == MemoryKind::Flash)
            .map(|r| r.size.bytes)
            .sum();
        let ram_bytes: u64 = self
            .memory
            .iter()
            .filter(|r| r.kind == MemoryKind::Ram)
            .map(|r| r.size.bytes)
            .sum();
        let ram_used = usage
            .stack_bytes
            .saturating_add(usage.static_ram_bytes)
            .saturating_add(self.allocator.fixed_size());
        SizeBudgetReport {
            artifact_bytes,
            flash_bytes,
            flash_ok: flash_bytes == 0 || artifact_bytes <= flash_bytes,
            ram_bytes,
            ram_used_bytes: ram_used,
            ram_ok: ram_bytes == 0 || ram_used <= ram_bytes,
        }
    }

    /// No-OS machines are AOT-only; hosted keeps Dev/JIT.
    pub fn supports_execution_tier(&self, tier: ExecutionTier) -> Result<(), TargetMachineError> {
        if self.no_os && matches!(tier, ExecutionTier::Dev | ExecutionTier::Jit) {
            return Err(TargetMachineError::ExecutionTierUnsupported {
                tier: tier.as_str().to_string(),
                machine: self.name.clone(),
            });
        }
        Ok(())
    }

    /// Safety checklist for independent review of a selected machine.
    pub fn safety_review(&self, usage: &TargetMachineUse) -> SafetyReview {
        let mmio_gated = usage.mmio.iter().all(|a| {
            a.unsafe_gate
                .as_ref()
                .is_some_and(|g| !g.reason.trim().is_empty())
        });
        let mmio_in_region = usage.mmio.iter().all(|a| {
            self.memory
                .iter()
                .any(|r| r.kind == MemoryKind::Mmio && r.contains(a.address, a.size))
        });
        SafetyReview {
            no_os: self.no_os,
            panic_explicit: !matches!(
                self.panic,
                PanicPolicy::Unspecified | PanicPolicy::HostedDefault
            ),
            allocator_explicit: !matches!(
                self.allocator,
                AllocatorPolicy::Unspecified | AllocatorPolicy::HostedDefault
            ),
            linker_explicit: !matches!(self.linker, LinkerInput::Unspecified),
            mmio_requires_unsafe: mmio_gated,
            mmio_inside_declared_regions: mmio_in_region,
            aot_only: self.no_os,
            transitive_memory_regions: self.memory.len(),
        }
    }

    /// Representative MCU board used by card #239 proofs.
    pub fn board_sensor_v1() -> Self {
        let mut machine = Self::freestanding("board.sensor_v1", "thumbv7em-none-eabihf");
        machine.memory = vec![
            MemoryRegion::new(
                "flash",
                0x0000_0000,
                ByteSize::kib(256),
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
        machine.linker = LinkerInput::Generated;
        machine.allocator = AllocatorPolicy::None;
        machine.panic = PanicPolicy::Abort;
        machine.mmio = MmioPolicy::Provider {
            provider: ProviderContract::new("board.sensor_v1.mmio", "sha256:board-sensor-v1-mmio"),
        };
        machine.wall_clock = ClockPolicy::None;
        machine.monotonic_clock = ClockPolicy::Provider {
            provider: ProviderContract::new(
                "board.sensor_v1.systick",
                "sha256:board-sensor-v1-systick",
            ),
        };
        machine.zone_data = ClockPolicy::None;
        machine.sleep = ClockPolicy::Provider {
            provider: ProviderContract::new(
                "board.sensor_v1.systick",
                "sha256:board-sensor-v1-systick",
            ),
        };
        machine.entropy = EntropyPolicy::None;
        machine.scheduler = SchedulerPolicy::Cooperative {
            provider: ProviderContract::new(
                "board.sensor_v1.cooperative",
                "sha256:board-sensor-v1-scheduler",
            ),
        };
        machine.byte_sink = ByteSinkPolicy::Provider {
            read: Some(ProviderContract::new(
                "board.sensor_v1.uart_rx",
                "sha256:board-sensor-v1-uart-rx",
            )),
            write: Some(ProviderContract::new(
                "board.sensor_v1.uart_tx",
                "sha256:board-sensor-v1-uart-tx",
            )),
            report: Some(ProviderContract::new(
                "board.sensor_v1.report_uart",
                "sha256:board-sensor-v1-report",
            )),
        };
        machine.startup = StartupPolicy::Generated {
            provider: ProviderContract::new(
                "board.sensor_v1.startup",
                "sha256:board-sensor-v1-startup",
            ),
        };
        machine
    }

    /// Linux freestanding / QEMU virt proof board.
    pub fn board_virt_aarch64() -> Self {
        let mut machine = Self::freestanding("board.virt_aarch64", "aarch64-unknown-none");
        machine.memory = vec![
            MemoryRegion::new(
                "flash",
                0x4000_0000,
                ByteSize::mib(1),
                MemoryKind::Flash,
                MemoryAccess::Rx,
            ),
            MemoryRegion::new(
                "ram",
                0x4010_0000,
                ByteSize::mib(1),
                MemoryKind::Ram,
                MemoryAccess::Rw,
            ),
            MemoryRegion::new(
                "uart0",
                0x0900_0000,
                ByteSize::kib(4),
                MemoryKind::Mmio,
                MemoryAccess::Rw,
            ),
        ];
        machine.linker = LinkerInput::Generated;
        machine.allocator = AllocatorPolicy::None;
        machine.panic = PanicPolicy::Abort;
        machine.mmio = MmioPolicy::Provider {
            provider: ProviderContract::new("board.virt_aarch64.mmio", "sha256:board-virt-mmio"),
        };
        machine.wall_clock = ClockPolicy::None;
        machine.monotonic_clock = ClockPolicy::None;
        machine.zone_data = ClockPolicy::None;
        machine.sleep = ClockPolicy::None;
        machine.entropy = EntropyPolicy::None;
        machine.scheduler = SchedulerPolicy::None;
        machine.byte_sink = ByteSinkPolicy::Provider {
            read: None,
            write: Some(ProviderContract::new(
                "board.virt_aarch64.uart0",
                "sha256:board-virt-uart0",
            )),
            report: Some(ProviderContract::new(
                "board.virt_aarch64.uart0",
                "sha256:board-virt-uart0",
            )),
        };
        machine.startup = StartupPolicy::Generated {
            provider: ProviderContract::new(
                "board.virt_aarch64.startup",
                "sha256:board-virt-startup",
            ),
        };
        machine
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupSource {
    pub filename: String,
    pub contents: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionTier {
    Aot,
    Dev,
    Jit,
}

impl ExecutionTier {
    pub fn as_str(self) -> &'static str {
        match self {
            ExecutionTier::Aot => "aot",
            ExecutionTier::Dev => "dev",
            ExecutionTier::Jit => "jit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeBudgetReport {
    pub artifact_bytes: u64,
    pub flash_bytes: u64,
    pub flash_ok: bool,
    pub ram_bytes: u64,
    pub ram_used_bytes: u64,
    pub ram_ok: bool,
}

impl SizeBudgetReport {
    pub fn ok(&self) -> bool {
        self.flash_ok && self.ram_ok
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"artifact_bytes\":{},\"flash_bytes\":{},\"flash_ok\":{},\"ram_bytes\":{},\"ram_used_bytes\":{},\"ram_ok\":{}}}",
            self.artifact_bytes,
            self.flash_bytes,
            if self.flash_ok { "true" } else { "false" },
            self.ram_bytes,
            self.ram_used_bytes,
            if self.ram_ok { "true" } else { "false" }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyReview {
    pub no_os: bool,
    pub panic_explicit: bool,
    pub allocator_explicit: bool,
    pub linker_explicit: bool,
    pub mmio_requires_unsafe: bool,
    pub mmio_inside_declared_regions: bool,
    pub aot_only: bool,
    pub transitive_memory_regions: usize,
}

impl SafetyReview {
    pub fn passes(&self) -> bool {
        if !self.no_os {
            return true;
        }
        self.panic_explicit
            && self.allocator_explicit
            && self.linker_explicit
            && self.mmio_requires_unsafe
            && self.mmio_inside_declared_regions
            && self.aot_only
            && self.transitive_memory_regions > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRegion {
    pub name: String,
    pub origin: u64,
    pub size: ByteSize,
    pub kind: MemoryKind,
    pub access: MemoryAccess,
}

impl MemoryRegion {
    pub fn new(
        name: impl Into<String>,
        origin: u64,
        size: ByteSize,
        kind: MemoryKind,
        access: MemoryAccess,
    ) -> Self {
        Self {
            name: name.into(),
            origin,
            size,
            kind,
            access,
        }
    }

    fn end(&self) -> Option<u64> {
        self.origin.checked_add(self.size.bytes)
    }

    fn contains(&self, origin: u64, size: ByteSize) -> bool {
        match (self.end(), origin.checked_add(size.bytes)) {
            (Some(region_end), Some(access_end)) => {
                self.origin <= origin && access_end <= region_end
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteSize {
    pub bytes: u64,
}

impl ByteSize {
    pub const fn bytes(bytes: u64) -> Self {
        Self { bytes }
    }

    pub const fn kib(value: u64) -> Self {
        Self {
            bytes: value * 1024,
        }
    }

    pub const fn mib(value: u64) -> Self {
        Self {
            bytes: value * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    Flash,
    Ram,
    Mmio,
    Reserved,
}

impl MemoryKind {
    fn as_str(self) -> &'static str {
        match self {
            MemoryKind::Flash => "flash",
            MemoryKind::Ram => "ram",
            MemoryKind::Mmio => "mmio",
            MemoryKind::Reserved => "reserved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAccess {
    R,
    Rw,
    Rx,
    Rwx,
}

impl MemoryAccess {
    fn as_str(self) -> &'static str {
        match self {
            MemoryAccess::R => "r",
            MemoryAccess::Rw => "rw",
            MemoryAccess::Rx => "rx",
            MemoryAccess::Rwx => "rwx",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkerInput {
    HostedDefault,
    Unspecified,
    Generated,
    File { path: String, sha256: String },
}

impl LinkerInput {
    fn audit_json(&self) -> String {
        match self {
            LinkerInput::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            LinkerInput::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            LinkerInput::Generated => "{\"kind\":\"generated\"}".to_string(),
            LinkerInput::File { path, sha256 } => format!(
                "{{\"kind\":\"file\",\"path\":{},\"sha256\":{}}}",
                json_str(path),
                json_str(sha256)
            ),
        }
    }
}

/// D-TARGET-ALLOC1 / D-ALLOC-PROGRAM1=A: one typed allocator fact for
/// freestanding targets and hosted programs. Hosted programs may wrap the
/// hidden system heap with the built-in counting allocator and an optional
/// hard cap; no fact keeps the existing hidden heap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocatorPolicy {
    HostedDefault,
    Unspecified,
    None,
    Fixed { region: String, size: ByteSize },
    Counting { cap: Option<ByteSize> },
}

impl Default for AllocatorPolicy {
    fn default() -> Self {
        Self::HostedDefault
    }
}

impl AllocatorPolicy {
    pub fn provides_heap(&self) -> bool {
        matches!(
            self,
            AllocatorPolicy::HostedDefault
                | AllocatorPolicy::Fixed { .. }
                | AllocatorPolicy::Counting { .. }
        )
    }

    fn fixed_size(&self) -> u64 {
        match self {
            AllocatorPolicy::Fixed { size, .. } => size.bytes,
            _ => 0,
        }
    }

    pub fn audit_json(&self) -> String {
        match self {
            AllocatorPolicy::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            AllocatorPolicy::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            AllocatorPolicy::None => "{\"kind\":\"none\"}".to_string(),
            AllocatorPolicy::Fixed { region, size } => format!(
                "{{\"kind\":\"fixed\",\"region\":{},\"size_bytes\":{}}}",
                json_str(region),
                size.bytes
            ),
            AllocatorPolicy::Counting { cap } => format!(
                "{{\"kind\":\"counting\",\"wraps\":\"system\",\"cap_bytes\":{}}}",
                cap.map_or_else(|| "null".to_string(), |size| size.bytes.to_string())
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanicPolicy {
    HostedDefault,
    Unspecified,
    Abort,
    Report { sink: String },
}

impl PanicPolicy {
    fn audit_json(&self) -> String {
        match self {
            PanicPolicy::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            PanicPolicy::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            PanicPolicy::Abort => "{\"kind\":\"abort\"}".to_string(),
            PanicPolicy::Report { sink } => {
                format!("{{\"kind\":\"report\",\"sink\":{}}}", json_str(sink))
            }
        }
    }
}
/// D-FREESTAND-FACTS1=A: every selected target provider carries an explicit
/// identity and digest. A target triple never supplies either value implicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderContract {
    pub provider: String,
    pub sha256: String,
}

impl ProviderContract {
    pub fn new(provider: impl Into<String>, sha256: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            sha256: sha256.into(),
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.provider.trim().is_empty()
            && self
                .sha256
                .strip_prefix("sha256:")
                .is_some_and(|digest| !digest.trim().is_empty())
    }

    fn audit_json(&self) -> String {
        format!(
            "{{\"provider\":{},\"sha256\":{}}}",
            json_str(&self.provider),
            json_str(&self.sha256)
        )
    }
}

/// One independently selectable time provider. The four TargetMachine fields
/// using this type are distinct facts: Time.Wall, Time.Monotonic,
/// Time.ZoneData, and Time.Sleep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockPolicy {
    HostedDefault,
    Unspecified,
    None,
    Provider { provider: ProviderContract },
}

impl Default for ClockPolicy {
    fn default() -> Self {
        Self::Unspecified
    }
}

impl ClockPolicy {
    fn is_declared(&self) -> bool {
        !matches!(self, Self::Unspecified)
    }

    fn provider(&self) -> Option<&ProviderContract> {
        match self {
            Self::Provider { provider } => Some(provider),
            _ => None,
        }
    }

    fn provides(&self, hosted: bool) -> bool {
        hosted && matches!(self, Self::HostedDefault) || self.provider().is_some()
    }

    fn audit_json(&self) -> String {
        match self {
            Self::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            Self::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            Self::None => "{\"kind\":\"none\"}".to_string(),
            Self::Provider { provider } => {
                format!("{{\"kind\":\"provider\",\"contract\":{}}}", provider.audit_json())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntropyPolicy {
    HostedDefault,
    Unspecified,
    None,
    Provider { provider: ProviderContract },
}

impl Default for EntropyPolicy {
    fn default() -> Self {
        Self::Unspecified
    }
}

impl EntropyPolicy {
    fn is_declared(&self) -> bool {
        !matches!(self, Self::Unspecified)
    }

    fn provider(&self) -> Option<&ProviderContract> {
        match self {
            Self::Provider { provider } => Some(provider),
            _ => None,
        }
    }

    fn provides(&self, hosted: bool) -> bool {
        hosted && matches!(self, Self::HostedDefault) || self.provider().is_some()
    }

    fn audit_json(&self) -> String {
        match self {
            Self::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            Self::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            Self::None => "{\"kind\":\"none\"}".to_string(),
            Self::Provider { provider } => {
                format!("{{\"kind\":\"provider\",\"contract\":{}}}", provider.audit_json())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerPolicy {
    HostedDefault,
    Unspecified,
    None,
    Cooperative { provider: ProviderContract },
    InterruptDriven { provider: ProviderContract },
    BoardRuntime { provider: ProviderContract },
}

impl Default for SchedulerPolicy {
    fn default() -> Self {
        Self::Unspecified
    }
}

impl SchedulerPolicy {
    fn is_declared(&self) -> bool {
        !matches!(self, Self::Unspecified)
    }

    fn provider(&self) -> Option<&ProviderContract> {
        match self {
            Self::Cooperative { provider }
            | Self::InterruptDriven { provider }
            | Self::BoardRuntime { provider } => Some(provider),
            _ => None,
        }
    }

    fn provides(&self, hosted: bool) -> bool {
        hosted && matches!(self, Self::HostedDefault) || self.provider().is_some()
    }

    fn audit_json(&self) -> String {
        match self {
            Self::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            Self::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            Self::None => "{\"kind\":\"none\"}".to_string(),
            Self::Cooperative { provider } => {
                format!("{{\"kind\":\"cooperative\",\"contract\":{}}}", provider.audit_json())
            }
            Self::InterruptDriven { provider } => format!(
                "{{\"kind\":\"interrupt-driven\",\"contract\":{}}}",
                provider.audit_json()
            ),
            Self::BoardRuntime { provider } => {
                format!("{{\"kind\":\"board-runtime\",\"contract\":{}}}", provider.audit_json())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MmioPolicy {
    HostedDefault,
    Unspecified,
    None,
    Provider { provider: ProviderContract },
}

impl Default for MmioPolicy {
    fn default() -> Self {
        Self::Unspecified
    }
}

impl MmioPolicy {
    fn is_declared(&self) -> bool {
        !matches!(self, Self::Unspecified)
    }

    fn provider(&self) -> Option<&ProviderContract> {
        match self {
            Self::Provider { provider } => Some(provider),
            _ => None,
        }
    }

    fn provides(&self, hosted: bool) -> bool {
        hosted && matches!(self, Self::HostedDefault) || self.provider().is_some()
    }

    fn audit_json(&self) -> String {
        match self {
            Self::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            Self::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            Self::None => "{\"kind\":\"none\"}".to_string(),
            Self::Provider { provider } => {
                format!("{{\"kind\":\"provider\",\"contract\":{}}}", provider.audit_json())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ByteSinkPolicy {
    HostedDefault,
    Unspecified,
    None,
    Provider {
        read: Option<ProviderContract>,
        write: Option<ProviderContract>,
        report: Option<ProviderContract>,
    },
}

impl Default for ByteSinkPolicy {
    fn default() -> Self {
        Self::Unspecified
    }
}

impl ByteSinkPolicy {
    fn is_declared(&self) -> bool {
        !matches!(self, Self::Unspecified)
    }

    fn provides_read(&self, hosted: bool) -> bool {
        match self {
            Self::HostedDefault => hosted,
            Self::Provider { read, .. } => read.is_some(),
            _ => false,
        }
    }

    fn provides_write(&self, hosted: bool) -> bool {
        match self {
            Self::HostedDefault => hosted,
            Self::Provider { write, .. } => write.is_some(),
            _ => false,
        }
    }

    fn provides_report(&self, hosted: bool) -> bool {
        match self {
            Self::HostedDefault => hosted,
            Self::Provider { report, .. } => report.is_some(),
            _ => false,
        }
    }

    fn audit_json(&self) -> String {
        match self {
            Self::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            Self::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            Self::None => "{\"kind\":\"none\"}".to_string(),
            Self::Provider {
                read,
                write,
                report,
            } => format!(
                "{{\"kind\":\"provider\",\"read\":{},\"write\":{},\"report\":{}}}",
                optional_contract_json(read.as_ref()),
                optional_contract_json(write.as_ref()),
                optional_contract_json(report.as_ref())
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPolicy {
    HostedDefault,
    Unspecified,
    Generated { provider: ProviderContract },
}

impl Default for StartupPolicy {
    fn default() -> Self {
        Self::Unspecified
    }
}

impl StartupPolicy {
    fn is_declared(&self) -> bool {
        !matches!(self, Self::Unspecified)
    }

    fn provider(&self) -> Option<&ProviderContract> {
        match self {
            Self::Generated { provider } => Some(provider),
            _ => None,
        }
    }

    fn provides(&self, hosted: bool) -> bool {
        hosted && matches!(self, Self::HostedDefault) || self.provider().is_some()
    }

    fn audit_json(&self) -> String {
        match self {
            Self::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            Self::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            Self::Generated { provider } => {
                format!("{{\"kind\":\"generated\",\"contract\":{}}}", provider.audit_json())
            }
        }
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditPolicy {
    pub build_artifact: bool,
    pub dossier_lens: bool,
}

impl Default for AuditPolicy {
    fn default() -> Self {
        Self {
            build_artifact: true,
            dossier_lens: true,
        }
    }
}

/// One reachable Prelude requirement against the target fact plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetCapability {
    Mmio,
    TimeWall,
    TimeMonotonic,
    TimeZoneData,
    TimeSleep,
    Entropy,
    Scheduler,
    IoRead,
    IoWrite,
    PanicReport,
    Startup,
}

impl TargetCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mmio => "MMIO",
            Self::TimeWall => "Time.Wall",
            Self::TimeMonotonic => "Time.Monotonic",
            Self::TimeZoneData => "Time.ZoneData",
            Self::TimeSleep => "Time.Sleep",
            Self::Entropy => "Rand.Entropy",
            Self::Scheduler => "Scheduler",
            Self::IoRead => "IO.Read",
            Self::IoWrite => "IO.Write",
            Self::PanicReport => "Panic.Report",
            Self::Startup => "Startup",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetMachineUse {
    pub stack_bytes: u64,
    pub static_ram_bytes: u64,
    pub heap_required: bool,
    pub core_apis: Vec<String>,
    pub mmio: Vec<MmioAccess>,
    pub required_capabilities: Vec<TargetCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmioAccess {
    pub address: u64,
    pub size: ByteSize,
    pub unsafe_gate: Option<UnsafeGate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsafeGate {
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetMachineError {
    MissingTargetTriple,
    MissingMemoryKind {
        kind: MemoryKind,
    },
    DuplicateMemoryRegion {
        name: String,
    },
    EmptyMemoryRegion {
        name: String,
    },
    MemoryAddressOverflow {
        name: String,
    },
    OverlappingMemoryRegions {
        first: String,
        second: String,
    },
    MissingLinkerInput,
    LinkerFileMissingPath,
    LinkerFileMissingHash {
        path: String,
    },
    MissingAllocatorPolicy,
    AllocatorRegionUnknown {
        region: String,
    },
    AllocatorRegionNotRam {
        region: String,
    },
    AllocatorRegionTooSmall {
        region: String,
        requested_bytes: u64,
        available_bytes: u64,
    },
    HostedAllocatorRequiresOs,
    MissingPanicPolicy,
    MissingTargetCapability {
        capability: String,
    },
    HostedCapabilityRequiresOs {
        capability: String,
    },
    InvalidProviderContract {
        capability: String,
        provider: String,
        sha256: String,
    },
    RamOverflow {
        used_bytes: u64,
        ram_bytes: u64,
    },
    HeapRequiresAllocator,
    CoreApiUnavailable {
        api: String,
        required: RuntimeLayer,
        available: RuntimeLayer,
    },
    MmioOutsideRegion {
        address: u64,
        size_bytes: u64,
    },
    MmioMissingUnsafeGate {
        address: u64,
    },
    MmioEmptyUnsafeReason {
        address: u64,
    },
    HostedHasNoLinkerScript,
    HostedHasNoStartup,
    UnsupportedStartupTriple {
        triple: String,
    },
    ExecutionTierUnsupported {
        tier: String,
        machine: String,
    },
    FirmwareToolchainMissing {
        tool: String,
    },
    FirmwareBuildFailed {
        detail: String,
    },
    SizeBudgetExceeded {
        report: SizeBudgetReport,
    },
}

fn validate_memory_regions(regions: &[MemoryRegion], errors: &mut Vec<TargetMachineError>) {
    let mut names: Vec<&str> = Vec::new();
    for region in regions {
        if region.size.bytes == 0 {
            errors.push(TargetMachineError::EmptyMemoryRegion {
                name: region.name.clone(),
            });
        }
        if region.end().is_none() {
            errors.push(TargetMachineError::MemoryAddressOverflow {
                name: region.name.clone(),
            });
        }
        if names.contains(&region.name.as_str()) {
            errors.push(TargetMachineError::DuplicateMemoryRegion {
                name: region.name.clone(),
            });
        } else {
            names.push(&region.name);
        }
    }

    let mut sorted: Vec<&MemoryRegion> = regions.iter().collect();
    sorted.sort_by_key(|r| r.origin);
    for pair in sorted.windows(2) {
        let first = pair[0];
        let second = pair[1];
        if first.end().is_some_and(|end| end > second.origin) {
            errors.push(TargetMachineError::OverlappingMemoryRegions {
                first: first.name.clone(),
                second: second.name.clone(),
            });
        }
    }
}

fn validate_linker(linker: &LinkerInput, no_os: bool, errors: &mut Vec<TargetMachineError>) {
    match linker {
        LinkerInput::Unspecified if no_os => errors.push(TargetMachineError::MissingLinkerInput),
        LinkerInput::File { path, sha256 } => {
            if path.trim().is_empty() {
                errors.push(TargetMachineError::LinkerFileMissingPath);
            }
            if !sha256.starts_with("sha256:") || sha256.len() <= "sha256:".len() {
                errors.push(TargetMachineError::LinkerFileMissingHash { path: path.clone() });
            }
        }
        _ => {}
    }
}

fn validate_allocator(machine: &TargetMachine, errors: &mut Vec<TargetMachineError>) {
    match &machine.allocator {
        AllocatorPolicy::Unspecified if machine.no_os => {
            errors.push(TargetMachineError::MissingAllocatorPolicy)
        }
        AllocatorPolicy::Counting { .. } if machine.no_os => {
            errors.push(TargetMachineError::HostedAllocatorRequiresOs)
        }
        AllocatorPolicy::Fixed { region, size } => {
            match machine.memory.iter().find(|r| r.name == *region) {
                Some(r) if r.kind == MemoryKind::Ram && size.bytes <= r.size.bytes => {}
                Some(r) if r.kind == MemoryKind::Ram => {
                    errors.push(TargetMachineError::AllocatorRegionTooSmall {
                        region: region.clone(),
                        requested_bytes: size.bytes,
                        available_bytes: r.size.bytes,
                    })
                }
                Some(_) => errors.push(TargetMachineError::AllocatorRegionNotRam {
                    region: region.clone(),
                }),
                None => errors.push(TargetMachineError::AllocatorRegionUnknown {
                    region: region.clone(),
                }),
            }
        }
        _ => {}
    }
}

fn validate_panic(machine: &TargetMachine, errors: &mut Vec<TargetMachineError>) {
    if machine.no_os
        && matches!(
            machine.panic,
            PanicPolicy::Unspecified | PanicPolicy::HostedDefault
        )
    {
        errors.push(TargetMachineError::MissingPanicPolicy);
    }
}

fn validate_ram_budget(
    machine: &TargetMachine,
    usage: &TargetMachineUse,
    errors: &mut Vec<TargetMachineError>,
) {
    if !machine.no_os {
        return;
    }
    for kind in [MemoryKind::Flash, MemoryKind::Ram] {
        if !machine.memory.iter().any(|r| r.kind == kind) {
            errors.push(TargetMachineError::MissingMemoryKind { kind });
        }
    }
    let ram_bytes: u64 = machine
        .memory
        .iter()
        .filter(|r| r.kind == MemoryKind::Ram)
        .map(|r| r.size.bytes)
        .sum();
    let used_bytes = usage
        .stack_bytes
        .saturating_add(usage.static_ram_bytes)
        .saturating_add(machine.allocator.fixed_size());
    if ram_bytes > 0 && used_bytes > ram_bytes {
        errors.push(TargetMachineError::RamOverflow {
            used_bytes,
            ram_bytes,
        });
    }
}

fn validate_core_usage(
    machine: &TargetMachine,
    usage: &TargetMachineUse,
    errors: &mut Vec<TargetMachineError>,
) {
    if usage.heap_required && !machine.allocator.provides_heap() {
        errors.push(TargetMachineError::HeapRequiresAllocator);
    }

    let available = machine.max_runtime_layer();
    for api in &usage.core_apis {
        let Some(required) = core_api_runtime_layer(api) else {
            continue;
        };
        if required > available {
            errors.push(TargetMachineError::CoreApiUnavailable {
                api: api.clone(),
                required,
                available,
            });
        }
    }
}
fn validate_target_capabilities(
    machine: &TargetMachine,
    usage: &TargetMachineUse,
    errors: &mut Vec<TargetMachineError>,
) {
    validate_simple_capability(
        machine,
        TargetCapability::Mmio,
        machine.mmio.is_declared(),
        matches!(machine.mmio, MmioPolicy::HostedDefault),
        machine.mmio.provider(),
        errors,
    );
    validate_simple_capability(
        machine,
        TargetCapability::TimeWall,
        machine.wall_clock.is_declared(),
        matches!(machine.wall_clock, ClockPolicy::HostedDefault),
        machine.wall_clock.provider(),
        errors,
    );
    validate_simple_capability(
        machine,
        TargetCapability::TimeMonotonic,
        machine.monotonic_clock.is_declared(),
        matches!(machine.monotonic_clock, ClockPolicy::HostedDefault),
        machine.monotonic_clock.provider(),
        errors,
    );
    validate_simple_capability(
        machine,
        TargetCapability::TimeZoneData,
        machine.zone_data.is_declared(),
        matches!(machine.zone_data, ClockPolicy::HostedDefault),
        machine.zone_data.provider(),
        errors,
    );
    validate_simple_capability(
        machine,
        TargetCapability::TimeSleep,
        machine.sleep.is_declared(),
        matches!(machine.sleep, ClockPolicy::HostedDefault),
        machine.sleep.provider(),
        errors,
    );
    validate_simple_capability(
        machine,
        TargetCapability::Entropy,
        machine.entropy.is_declared(),
        matches!(machine.entropy, EntropyPolicy::HostedDefault),
        machine.entropy.provider(),
        errors,
    );
    validate_simple_capability(
        machine,
        TargetCapability::Scheduler,
        machine.scheduler.is_declared(),
        matches!(machine.scheduler, SchedulerPolicy::HostedDefault),
        machine.scheduler.provider(),
        errors,
    );
    validate_simple_capability(
        machine,
        TargetCapability::Startup,
        machine.startup.is_declared(),
        matches!(machine.startup, StartupPolicy::HostedDefault),
        machine.startup.provider(),
        errors,
    );

    match &machine.byte_sink {
        ByteSinkPolicy::HostedDefault if machine.no_os => {
            push_unique(
                errors,
                TargetMachineError::HostedCapabilityRequiresOs {
                    capability: "IO.Read/IO.Write/Panic.Report".to_string(),
                },
            );
        }
        ByteSinkPolicy::Unspecified if machine.no_os => {
            push_unique(
                errors,
                TargetMachineError::MissingTargetCapability {
                    capability: "IO.Read/IO.Write/Panic.Report".to_string(),
                },
            );
        }
        ByteSinkPolicy::Provider {
            read,
            write,
            report,
        } => {
            if let Some(provider) = read {
                validate_provider_contract(TargetCapability::IoRead, provider, errors);
            }
            if let Some(provider) = write {
                validate_provider_contract(TargetCapability::IoWrite, provider, errors);
            }
            if let Some(provider) = report {
                validate_provider_contract(TargetCapability::PanicReport, provider, errors);
            }
        }
        _ => {}
    }
    if machine.no_os
        && matches!(machine.panic, PanicPolicy::Report { .. })
        && !machine.byte_sink.provides_report(false)
    {
        push_unique(
            errors,
            TargetMachineError::MissingTargetCapability {
                capability: TargetCapability::PanicReport.as_str().to_string(),
            },
        );
    }

    if !usage.mmio.is_empty() && !machine.provides_capability(TargetCapability::Mmio) {
        push_unique(
            errors,
            TargetMachineError::MissingTargetCapability {
                capability: TargetCapability::Mmio.as_str().to_string(),
            },
        );
    }
    for capability in &usage.required_capabilities {
        if !machine.provides_capability(*capability) {
            push_unique(
                errors,
                TargetMachineError::MissingTargetCapability {
                    capability: capability.as_str().to_string(),
                },
            );
        }
    }
}

fn validate_simple_capability(
    machine: &TargetMachine,
    capability: TargetCapability,
    declared: bool,
    hosted_default: bool,
    provider: Option<&ProviderContract>,
    errors: &mut Vec<TargetMachineError>,
) {
    if machine.no_os {
        if hosted_default {
            push_unique(
                errors,
                TargetMachineError::HostedCapabilityRequiresOs {
                    capability: capability.as_str().to_string(),
                },
            );
        } else if !declared {
            push_unique(
                errors,
                TargetMachineError::MissingTargetCapability {
                    capability: capability.as_str().to_string(),
                },
            );
        }
    }
    if let Some(provider) = provider {
        validate_provider_contract(capability, provider, errors);
    }
}

fn validate_provider_contract(
    capability: TargetCapability,
    provider: &ProviderContract,
    errors: &mut Vec<TargetMachineError>,
) {
    if !provider.is_valid() {
        push_unique(
            errors,
            TargetMachineError::InvalidProviderContract {
                capability: capability.as_str().to_string(),
                provider: provider.provider.clone(),
                sha256: provider.sha256.clone(),
            },
        );
    }
}

fn push_unique(errors: &mut Vec<TargetMachineError>, error: TargetMachineError) {
    if !errors.contains(&error) {
        errors.push(error);
    }
}

/// Sema adds closure markers for Core source/intrinsic reachability. They are
/// codegen provenance, not additional machine capabilities; their direct API
/// entries already carry the required runtime layer.
fn core_api_runtime_layer(api: &str) -> Option<RuntimeLayer> {
    if api.starts_with("__core_source::") || api.starts_with("__core_intrinsic::") {
        return None;
    }
    Some(
        core_usage_layer(api)
            .or_else(|| core_module_layer(api))
            .unwrap_or(RuntimeLayer::Std),
    )
}

fn validate_mmio(
    machine: &TargetMachine,
    usage: &TargetMachineUse,
    errors: &mut Vec<TargetMachineError>,
) {
    for access in &usage.mmio {
        let inside_mmio = machine
            .memory
            .iter()
            .any(|r| r.kind == MemoryKind::Mmio && r.contains(access.address, access.size));
        if !inside_mmio {
            errors.push(TargetMachineError::MmioOutsideRegion {
                address: access.address,
                size_bytes: access.size.bytes,
            });
        }
        match &access.unsafe_gate {
            Some(gate) if gate.reason.trim().is_empty() => {
                errors.push(TargetMachineError::MmioEmptyUnsafeReason {
                    address: access.address,
                });
            }
            Some(_) => {}
            None => errors.push(TargetMachineError::MmioMissingUnsafeGate {
                address: access.address,
            }),
        }
    }
}

fn format_length(bytes: u64) -> String {
    if bytes % (1024 * 1024) == 0 {
        format!("{}M", bytes / (1024 * 1024))
    } else if bytes % 1024 == 0 {
        format!("{}K", bytes / 1024)
    } else {
        format!("{bytes}")
    }
}

fn execution_json(machine: &TargetMachine) -> String {
    if machine.no_os {
        "{\"aot\":true,\"dev\":false,\"jit\":false}".to_string()
    } else {
        "{\"aot\":true,\"dev\":true,\"jit\":true}".to_string()
    }
}

fn memory_json(regions: &[MemoryRegion]) -> String {
    let mut out = String::from("[");
    for (idx, region) in regions.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"name\":{},\"origin\":{},\"size_bytes\":{},\"kind\":\"{}\",\"access\":\"{}\"}}",
            json_str(&region.name),
            region.origin,
            region.size.bytes,
            region.kind.as_str(),
            region.access.as_str()
        );
    }
    out.push(']');
    out
}

fn unavailable_core_json(machine: &TargetMachine, usage: &TargetMachineUse) -> String {
    let available = machine.max_runtime_layer();
    let mut unavailable = Vec::new();
    for api in &usage.core_apis {
        let Some(required) = core_api_runtime_layer(api) else {
            continue;
        };
        if required > available {
            unavailable.push(api.as_str());
        }
    }
    string_array_json(&unavailable)
}

fn mmio_json(accesses: &[MmioAccess]) -> String {
    let mut out = String::from("[");
    for (idx, access) in accesses.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        let reason = access
            .unsafe_gate
            .as_ref()
            .map(|g| json_str(&g.reason))
            .unwrap_or_else(|| "null".to_string());
        let _ = write!(
            out,
            "{{\"address\":{},\"size_bytes\":{},\"unsafe_reason\":{}}}",
            access.address, access.size.bytes, reason
        );
    }
    out.push(']');
    out
}

fn string_array_json(values: &[&str]) -> String {
    let mut out = String::from("[");
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_str(value));
    }
    out.push(']');
    out
}

fn push_field(out: &mut String, key: &str, value: &str, first: bool) {
    if !first {
        out.push(',');
    }
    let _ = write!(out, "\"{key}\":{value}");
}

fn json_str(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_machine() -> TargetMachine {
        let mut machine = TargetMachine::freestanding("board.sensor_v1", "thumbv7em-none-eabihf");
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
                ByteSize::kib(128),
                MemoryKind::Ram,
                MemoryAccess::Rw,
            ),
            MemoryRegion::new(
                "gpio",
                0x4002_0000,
                ByteSize::kib(1),
                MemoryKind::Mmio,
                MemoryAccess::Rw,
            ),
        ];
        machine.allocator = AllocatorPolicy::Fixed {
            region: "ram".to_string(),
            size: ByteSize::kib(16),
        };
        machine.panic = PanicPolicy::Abort;
        machine
    }

    #[test]
    fn hosted_default_keeps_full_runtime() {
        let machine = TargetMachine::hosted("x86_64-unknown-linux-gnu");
        let usage = TargetMachineUse {
            core_apis: vec!["core.files".to_string(), "core.http.client".to_string()],
            heap_required: true,
            ..TargetMachineUse::default()
        };
        assert_eq!(machine.max_runtime_layer(), RuntimeLayer::Std);
        assert!(machine.validate(&usage).is_empty());
    }

    #[test]
    fn hosted_counting_allocator_is_typed_and_auditable() {
        let mut machine = TargetMachine::hosted("x86_64-unknown-linux-gnu");
        machine.allocator = AllocatorPolicy::Counting {
            cap: Some(ByteSize::bytes(2 * 1024 * 1024 * 1024)),
        };
        assert!(machine.validate(&TargetMachineUse::default()).is_empty());
        assert_eq!(
            machine.allocator.audit_json(),
            "{\"kind\":\"counting\",\"wraps\":\"system\",\"cap_bytes\":2147483648}"
        );
    }

    #[test]
    fn freestanding_machine_rejects_hosted_counting_wrapper() {
        let mut machine = valid_machine();
        machine.allocator = AllocatorPolicy::Counting { cap: None };
        let errors = machine.validate(&TargetMachineUse::default());
        assert!(errors.contains(&TargetMachineError::HostedAllocatorRequiresOs));
    }

    #[test]
    fn valid_freestanding_machine_passes() {
        let usage = TargetMachineUse {
            stack_bytes: ByteSize::kib(4).bytes,
            static_ram_bytes: ByteSize::kib(8).bytes,
            heap_required: true,
            core_apis: vec!["core.encoding.json".to_string()],
            mmio: vec![MmioAccess {
                address: 0x4002_0000,
                size: ByteSize::bytes(4),
                unsafe_gate: Some(UnsafeGate {
                    reason: "GPIO register write".to_string(),
                }),
            }],
        };
        assert!(valid_machine().validate(&usage).is_empty());
    }

    #[test]
    fn validation_reports_missing_required_no_os_facts() {
        let machine = TargetMachine::freestanding("", "");
        let errors = machine.validate(&TargetMachineUse::default());
        assert!(errors.contains(&TargetMachineError::MissingTargetTriple));
        assert!(errors.contains(&TargetMachineError::MissingMemoryKind {
            kind: MemoryKind::Flash
        }));
        assert!(errors.contains(&TargetMachineError::MissingMemoryKind {
            kind: MemoryKind::Ram
        }));
        assert!(errors.contains(&TargetMachineError::MissingAllocatorPolicy));
        assert!(errors.contains(&TargetMachineError::MissingPanicPolicy));
    }

    #[test]
    fn validation_reports_ram_heap_core_and_mmio_errors() {
        let mut machine = valid_machine();
        machine.allocator = AllocatorPolicy::None;
        let usage = TargetMachineUse {
            stack_bytes: ByteSize::kib(96).bytes,
            static_ram_bytes: ByteSize::kib(64).bytes,
            heap_required: true,
            core_apis: vec!["core.files".to_string()],
            mmio: vec![MmioAccess {
                address: 0x5000_0000,
                size: ByteSize::bytes(4),
                unsafe_gate: None,
            }],
        };
        let errors = machine.validate(&usage);
        assert!(errors.contains(&TargetMachineError::RamOverflow {
            used_bytes: ByteSize::kib(160).bytes,
            ram_bytes: ByteSize::kib(128).bytes
        }));
        assert!(errors.contains(&TargetMachineError::HeapRequiresAllocator));
        assert!(errors.contains(&TargetMachineError::CoreApiUnavailable {
            api: "core.files".to_string(),
            required: RuntimeLayer::Std,
            available: RuntimeLayer::Core
        }));
        assert!(errors.contains(&TargetMachineError::MmioOutsideRegion {
            address: 0x5000_0000,
            size_bytes: 4
        }));
        assert!(errors.contains(&TargetMachineError::MmioMissingUnsafeGate {
            address: 0x5000_0000
        }));
    }

    #[test]
    fn audit_json_is_stable() {
        let usage = TargetMachineUse {
            core_apis: vec!["core.files".to_string()],
            mmio: vec![MmioAccess {
                address: 0x4002_0000,
                size: ByteSize::bytes(4),
                unsafe_gate: Some(UnsafeGate {
                    reason: "GPIO register write".to_string(),
                }),
            }],
            ..TargetMachineUse::default()
        };
        let json = valid_machine().audit_json(&usage);
        assert_eq!(
            json,
            "{\"name\":\"board.sensor_v1\",\"triple\":\"thumbv7em-none-eabihf\",\"environment\":\"no-os\",\"linker\":{\"kind\":\"generated\"},\"allocator\":{\"kind\":\"fixed\",\"region\":\"ram\",\"size_bytes\":16384},\"panic\":{\"kind\":\"abort\"},\"memory\":[{\"name\":\"flash\",\"origin\":134217728,\"size_bytes\":524288,\"kind\":\"flash\",\"access\":\"rx\"},{\"name\":\"ram\",\"origin\":536870912,\"size_bytes\":131072,\"kind\":\"ram\",\"access\":\"rw\"},{\"name\":\"gpio\",\"origin\":1073872896,\"size_bytes\":1024,\"kind\":\"mmio\",\"access\":\"rw\"}],\"unavailable_core_apis\":[\"core.files\"],\"mmio\":[{\"address\":1073872896,\"size_bytes\":4,\"unsafe_reason\":\"GPIO register write\"}],\"execution\":{\"aot\":true,\"dev\":false,\"jit\":false}}"
        );
    }

    #[test]
    fn generated_linker_script_is_deterministic() {
        let script = valid_machine().generate_linker_script().unwrap();
        assert!(script.contains("MEMORY {"));
        assert!(script.contains("flash (rx) : ORIGIN = 0x08000000, LENGTH = 512K"));
        assert!(script.contains("ram (rwx) : ORIGIN = 0x20000000, LENGTH = 128K"));
        assert!(script.contains("ENTRY(Reset_Handler)"));
        assert_eq!(script, valid_machine().generate_linker_script().unwrap());
    }

    #[test]
    fn no_os_rejects_dev_and_jit_tiers() {
        let machine = valid_machine();
        assert!(machine.supports_execution_tier(ExecutionTier::Aot).is_ok());
        assert!(matches!(
            machine.supports_execution_tier(ExecutionTier::Dev),
            Err(TargetMachineError::ExecutionTierUnsupported { .. })
        ));
        assert!(matches!(
            machine.supports_execution_tier(ExecutionTier::Jit),
            Err(TargetMachineError::ExecutionTierUnsupported { .. })
        ));
        assert!(TargetMachine::hosted("x86_64-unknown-linux-gnu")
            .supports_execution_tier(ExecutionTier::Jit)
            .is_ok());
    }
}
