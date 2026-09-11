//! D-TARGET-* typed target machine facts.
//!
use crate::Layout::LayoutCapabilityFacts;
use crate::RingLayer::{classify_prelude_closure, RuntimeLayer};
use crate::Report::{StatusFields, StatusValue};
use std::fmt::Write;
use crate::Effects::EffectSet;
use crate::Facts::TargetDossier;
use std::collections::BTreeSet;


/// Compiler-owned provenance for the vendor SVD which generated a target's
/// hardware facts.  Register access is safe at the Jet surface because this
/// provenance is selected by the target profile, rather than supplied by a
/// user `#Unsafe` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvdProvenance {
    pub source: String,
    pub sha256: String,
}

impl SvdProvenance {
    pub fn new(source: impl Into<String>, sha256: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            sha256: sha256.into(),
        }
    }

    pub fn from_source(source: impl Into<String>, contents: &[u8]) -> Self {
        Self::new(
            source,
            format!("sha256:{}", crate::SHA256::sha256_hex(contents)),
        )
    }

    pub fn is_valid(&self) -> bool {
        !self.source.trim().is_empty()
            && self
                .sha256
                .strip_prefix("sha256:")
                .is_some_and(|digest| !digest.trim().is_empty())
    }

    fn audit_json(&self) -> String {
        format!(
            "{{\"source\":{},\"sha256\":{}}}",
            json_str(&self.source),
            json_str(&self.sha256)
        )
    }
}

/// Width of one generated memory-mapped register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegisterWidth {
    U8,
    U16,
    U32,
    U64,
}

impl RegisterWidth {

    pub const fn bytes(self) -> u64 {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::U64 => 8,
        }
    }

    pub const fn bits(self) -> u16 {
        (self.bytes() * 8) as u16
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
        }
    }
}

/// Access mode emitted by the SVD for one register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRegisterAccessMode {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

impl TargetRegisterAccessMode {

    pub const fn can_read(self) -> bool {
        matches!(self, Self::ReadOnly | Self::ReadWrite)
    }

    pub const fn can_write(self) -> bool {
        matches!(self, Self::WriteOnly | Self::ReadWrite)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WriteOnly => "write-only",
            Self::ReadWrite => "read-write",
        }
    }
}

/// One generated register inside a [`TargetRegisterBlockFact`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRegisterFact {
    pub name: String,
    pub offset: u64,
    pub width: RegisterWidth,
    pub access: TargetRegisterAccessMode,
    pub volatile: bool,
}

impl TargetRegisterFact {
    pub fn new(
        name: impl Into<String>,
        offset: u64,
        width: RegisterWidth,
        access: TargetRegisterAccessMode,
        volatile: bool,
    ) -> Self {
        Self {
            name: name.into(),
            offset,
            width,
            access,
            volatile,
        }
    }

}

/// One SVD-derived peripheral register block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRegisterBlockFact {
    pub name: String,
    pub base: u64,
    pub size: ByteSize,
    pub registers: Vec<TargetRegisterFact>,
}

impl TargetRegisterBlockFact {
    pub fn new(
        name: impl Into<String>,
        base: u64,
        size: ByteSize,
        registers: Vec<TargetRegisterFact>,
    ) -> Self {
        Self {
            name: name.into(),
            base,
            size,
            registers,
        }
    }

    pub fn register(&self, name: &str) -> Option<&TargetRegisterFact> {
        self.registers.iter().find(|register| register.name == name)
    }
}

/// Register operation facts projected by sema and consumed by a later MIR
/// lowering.  There is deliberately no unsafe gate: the enclosing target
/// profile owns the SVD provenance and has already established the MMIO range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRegisterOperation {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRegisterAccessFact {
    pub block: String,
    pub register: String,
    pub operation: TargetRegisterOperation,
    pub width: RegisterWidth,
}

impl TargetRegisterAccessFact {
    pub fn read(
        block: impl Into<String>,
        register: impl Into<String>,
        width: RegisterWidth,
    ) -> Self {
        Self {
            block: block.into(),
            register: register.into(),
            operation: TargetRegisterOperation::Read,
            width,
        }
    }

    pub fn write(
        block: impl Into<String>,
        register: impl Into<String>,
        width: RegisterWidth,
    ) -> Self {
        Self {
            block: block.into(),
            register: register.into(),
            operation: TargetRegisterOperation::Write,
            width,
        }
    }
}

/// One vector-table entry and its bounded-handler contract. `forbidden_effects`
/// is the negative effect row from `#Interrupt`; sema also always enforces the
/// two baseline ISR prohibitions (`Mem.Alloc` and `Time.Wait`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInterruptFact {
    pub name: String,
    pub vector: u16,
    pub forbidden_effects: EffectSet,
    pub bounded: bool,
}

impl TargetInterruptFact {
    pub fn new(name: impl Into<String>, vector: u16) -> Self {
        let mut forbidden_effects = EffectSet::new();
        forbidden_effects.insert("Mem.Alloc".to_string());
        forbidden_effects.insert("Time.Wait".to_string());
        Self {
            name: name.into(),
            vector,
            forbidden_effects,
            bounded: true,
        }
    }

    pub fn with_effects<I, S>(
        name: impl Into<String>,
        vector: u16,
        forbidden_effects: I,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            name: name.into(),
            vector,
            forbidden_effects: forbidden_effects
                .into_iter()
                .map(|effect| effect.as_ref().to_string())
                .collect(),
            bounded: true,
        }
    }
}

/// Semantic handler binding facts. The effect set is the existing sema
/// inference result for the handler body; `bounded` is the checker result
/// consumed by the target boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInterruptHandlerFact {
    pub interrupt: String,
    pub handler: String,
    pub effects: EffectSet,
    /// Per-handler forbidden effects declared by the interrupt marker.
    pub forbidden_effects: EffectSet,
    pub bounded: bool,
}

impl TargetInterruptHandlerFact {
    pub fn new(
        interrupt: impl Into<String>,
        handler: impl Into<String>,
        effects: EffectSet,
        bounded: bool,
    ) -> Self {
        Self {
            interrupt: interrupt.into(),
            handler: handler.into(),
            effects,
            forbidden_effects: EffectSet::new(),
            bounded,
        }
    }
}

/// Whether a DMA channel borrows a buffer or takes ownership until completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetDmaOwnership {
    Borrowed,
    Transfer,
}

impl TargetDmaOwnership {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Borrowed => "borrowed",
            Self::Transfer => "transfer",
        }
    }
}

/// One DMA channel fact from the target profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDmaChannelFact {
    pub name: String,
    pub channel: u16,
    pub transfer_width: RegisterWidth,
    pub ownership: TargetDmaOwnership,
    pub max_transfer_bytes: Option<u64>,
}

impl TargetDmaChannelFact {
    pub fn new(name: impl Into<String>, channel: u16) -> Self {
        Self {
            name: name.into(),
            channel,
            transfer_width: RegisterWidth::U8,
            ownership: TargetDmaOwnership::Transfer,
            max_transfer_bytes: None,
        }
    }
}

/// Ordered DMA ownership facts projected by sema. For a transfer channel,
/// `Start` moves the buffer CPU → device, `Wait` moves it device → CPU, and
/// `UseBuffer` requires CPU ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetDmaOperation {
    Start,
    Wait,
    UseBuffer,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetDmaOwner {
    Cpu,
    Device,
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDmaOperationFact {
    pub channel: String,
    pub buffer: String,
    pub operation: TargetDmaOperation,
    /// Owner immediately before this operation.
    pub owner: TargetDmaOwner,
}

impl TargetDmaOperationFact {
    pub fn start(channel: impl Into<String>, buffer: impl Into<String>) -> Self {
        Self {
            channel: channel.into(),
            buffer: buffer.into(),
            operation: TargetDmaOperation::Start,
            owner: TargetDmaOwner::Cpu,
        }
    }

    pub fn wait(channel: impl Into<String>, buffer: impl Into<String>) -> Self {
        Self {
            channel: channel.into(),
            buffer: buffer.into(),
            operation: TargetDmaOperation::Wait,
            owner: TargetDmaOwner::Device,
        }
    }

    pub fn use_buffer(buffer: impl Into<String>) -> Self {
        Self {
            channel: String::new(),
            buffer: buffer.into(),
            operation: TargetDmaOperation::UseBuffer,
            owner: TargetDmaOwner::Cpu,
        }
    }
}

/// Explicit external programmer selected by a target profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TargetProgrammerAdapter {
    Emulator,
    ProbeRs,
    OpenOcd,
}

impl TargetProgrammerAdapter {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Emulator => "emulator",
            Self::ProbeRs => "probe-rs",
            Self::OpenOcd => "openocd",
        }
    }
}

/// Typed facts needed to invoke one profile-declared programmer.  Missing
/// adapter-specific facts remain explicit and are rejected by the flash
/// command instead of being inferred from an SVD, target name, or triple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetProgrammerFacts {
    pub adapter: TargetProgrammerAdapter,
    pub executable: String,
    pub chip: Option<String>,
    pub interface: Option<String>,
    pub config: Vec<String>,
    pub speed_khz: Option<u32>,
    pub reset: bool,
    pub machine: Option<String>,
    pub cpu: Option<String>,
}

impl TargetProgrammerFacts {
    pub fn emulator(
        executable: impl Into<String>,
        machine: impl Into<String>,
        cpu: impl Into<String>,
    ) -> Self {
        Self {
            adapter: TargetProgrammerAdapter::Emulator,
            executable: executable.into(),
            chip: None,
            interface: None,
            config: Vec::new(),
            speed_khz: None,
            reset: false,
            machine: Some(machine.into()),
            cpu: Some(cpu.into()),
        }
    }

    pub fn missing_fact(&self) -> Option<&'static str> {
        if self.executable.trim().is_empty() {
            return Some("executable");
        }
        if self.config.iter().any(|value| value.trim().is_empty()) {
            return Some("config");
        }
        match self.adapter {
            TargetProgrammerAdapter::Emulator => {
                if self
                    .machine
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    return Some("machine");
                }
                if self
                    .cpu
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    return Some("cpu");
                }
            }
            TargetProgrammerAdapter::ProbeRs => {
                if self
                    .chip
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    return Some("chip");
                }
            }
            TargetProgrammerAdapter::OpenOcd => {
                if self.interface.as_deref().is_none_or(|value| value.trim().is_empty())
                    && self.config.is_empty()
                {
                    return Some("interface or config");
                }
            }
        }
        None
    }

    pub fn audit_json(&self) -> String {
        let config = self
            .config
            .iter()
            .map(|value| json_str(value))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"adapter\":{},\"chip\":{},\"config\":[{}],\"cpu\":{},\"executable\":{},\"interface\":{},\"machine\":{},\"reset\":{},\"speed_khz\":{}}}",
            json_str(self.adapter.as_str()),
            self.chip
                .as_deref()
                .map_or_else(|| "null".to_string(), json_str),
            config,
            self.cpu
                .as_deref()
                .map_or_else(|| "null".to_string(), json_str),
            json_str(&self.executable),
            self.interface
                .as_deref()
                .map_or_else(|| "null".to_string(), json_str),
            self.machine
                .as_deref()
                .map_or_else(|| "null".to_string(), json_str),
            if self.reset { "true" } else { "false" },
            self.speed_khz
                .map_or_else(|| "null".to_string(), |speed| speed.to_string()),
        )
    }
}

/// All target-profile hardware facts and all backend-neutral hardware uses
/// projected by sema. This is the single fact plane later MIR tiers consume.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetHardwareFacts {
    pub svd: Option<SvdProvenance>,
    pub register_blocks: Vec<TargetRegisterBlockFact>,
    pub interrupts: Vec<TargetInterruptFact>,
    pub dma_channels: Vec<TargetDmaChannelFact>,
}

impl TargetHardwareFacts {
    pub fn from_svd(source: impl Into<String>, sha256: impl Into<String>) -> Self {
        Self {
            svd: Some(SvdProvenance::new(source, sha256)),
            ..Self::default()
        }
    }

    pub fn register_block(&self, name: &str) -> Option<&TargetRegisterBlockFact> {
        self.register_blocks
            .iter()
            .find(|block| block.name == name)
    }

    pub fn interrupt(&self, selector: &str) -> Option<&TargetInterruptFact> {
        self.interrupts.iter().find(|interrupt| {
            interrupt.name == selector || interrupt.vector.to_string() == selector
        })
    }

    pub fn dma_channel(&self, name: &str) -> Option<&TargetDmaChannelFact> {
        self.dma_channels.iter().find(|channel| channel.name == name)
    }

    pub fn validate(&self, memory: &[MemoryRegion]) -> Vec<TargetMachineError> {
        let mut errors = Vec::new();
        self.validate_into(memory, &mut errors);
        errors
    }

    fn validate_into(&self, memory: &[MemoryRegion], errors: &mut Vec<TargetMachineError>) {
        if let Some(svd) = &self.svd {
            if !svd.is_valid() {
                errors.push(TargetMachineError::InvalidHardwareSvd {
                    source: svd.source.clone(),
                    sha256: svd.sha256.clone(),
                });
            }
        }

        let mut block_names = BTreeSet::new();
        for block in &self.register_blocks {
            if block.name.trim().is_empty() {
                errors.push(TargetMachineError::HardwareFactEmptyName {
                    kind: "register block".to_string(),
                });
            }
            if !block_names.insert(block.name.clone()) {
                errors.push(TargetMachineError::DuplicateRegisterBlock {
                    name: block.name.clone(),
                });
            }
            if block.size.bytes == 0 {
                errors.push(TargetMachineError::RegisterBlockEmpty {
                    name: block.name.clone(),
                });
            }
            if block.base.checked_add(block.size.bytes).is_none() {
                errors.push(TargetMachineError::RegisterBlockAddressOverflow {
                    name: block.name.clone(),
                });
            } else if !memory
                .iter()
                .any(|region| region.kind == MemoryKind::Mmio && region.contains(block.base, block.size))
            {
                errors.push(TargetMachineError::RegisterBlockOutsideRegion {
                    name: block.name.clone(),
                    address: block.base,
                    size_bytes: block.size.bytes,
                });
            }

            let mut register_names = BTreeSet::new();
            for register in &block.registers {
                if register.name.trim().is_empty() {
                    errors.push(TargetMachineError::HardwareFactEmptyName {
                        kind: format!("register in `{}`", block.name),
                    });
                }
                if !register_names.insert(register.name.clone()) {
                    errors.push(TargetMachineError::DuplicateRegister {
                        block: block.name.clone(),
                        register: register.name.clone(),
                    });
                }
                if register.offset.checked_add(register.width.bytes()).is_none() {
                    errors.push(TargetMachineError::RegisterAddressOverflow {
                        block: block.name.clone(),
                        register: register.name.clone(),
                    });
                } else if register.offset.saturating_add(register.width.bytes()) > block.size.bytes {
                    errors.push(TargetMachineError::RegisterOutsideBlock {
                        block: block.name.clone(),
                        register: register.name.clone(),
                    });
                }
            }
        }

        let mut vectors = BTreeSet::new();
        let mut interrupt_names = BTreeSet::new();
        for interrupt in &self.interrupts {
            if interrupt.name.trim().is_empty() {
                errors.push(TargetMachineError::HardwareFactEmptyName {
                    kind: "interrupt".to_string(),
                });
            }
            if !interrupt_names.insert(interrupt.name.clone()) {
                errors.push(TargetMachineError::DuplicateInterrupt {
                    name: interrupt.name.clone(),
                });
            }
            if !vectors.insert(interrupt.vector) {
                errors.push(TargetMachineError::DuplicateInterruptVector {
                    vector: interrupt.vector,
                });
            }
            for effect in &interrupt.forbidden_effects {
                if crate::Authority::parse_right(effect).is_none() {
                    errors.push(TargetMachineError::InvalidInterruptEffect {
                        interrupt: interrupt.name.clone(),
                        effect: effect.clone(),
                    });
                }
            }
        }

        let mut channels = BTreeSet::new();
        let mut channel_numbers = BTreeSet::new();
        for channel in &self.dma_channels {
            if channel.name.trim().is_empty() {
                errors.push(TargetMachineError::HardwareFactEmptyName {
                    kind: "DMA channel".to_string(),
                });
            }
            if !channels.insert(channel.name.clone()) {
                errors.push(TargetMachineError::DuplicateDmaChannel {
                    name: channel.name.clone(),
                });
            }
            if !channel_numbers.insert(channel.channel) {
                errors.push(TargetMachineError::DuplicateDmaChannelNumber {
                    channel: channel.channel,
                });
            }
            if channel.max_transfer_bytes == Some(0) {
                errors.push(TargetMachineError::DmaTransferSizeZero {
                    channel: channel.name.clone(),
                });
            }
        }
    }

    fn validate_register_accesses(
        &self,
        accesses: &[TargetRegisterAccessFact],
        errors: &mut Vec<TargetMachineError>,
    ) {
        for access in accesses {
            let Some(block) = self.register_block(&access.block) else {
                errors.push(TargetMachineError::UnknownRegisterBlock {
                    block: access.block.clone(),
                });
                continue;
            };
            let Some(register) = block.register(&access.register) else {
                errors.push(TargetMachineError::UnknownRegister {
                    block: access.block.clone(),
                    register: access.register.clone(),
                });
                continue;
            };
            if register.width != access.width {
                errors.push(TargetMachineError::RegisterWidthMismatch {
                    block: access.block.clone(),
                    register: access.register.clone(),
                    expected: register.width,
                    actual: access.width,
                });
            }
            match access.operation {
                TargetRegisterOperation::Read if !register.access.can_read() => {
                    errors.push(TargetMachineError::RegisterReadDenied {
                        block: access.block.clone(),
                        register: access.register.clone(),
                    });
                }
                TargetRegisterOperation::Write if !register.access.can_write() => {
                    errors.push(TargetMachineError::RegisterWriteDenied {
                        block: access.block.clone(),
                        register: access.register.clone(),
                    });
                }
                _ => {}
            }
        }
    }

    pub fn audit_json(&self) -> String {
        let svd = self
            .svd
            .as_ref()
            .map(SvdProvenance::audit_json)
            .unwrap_or_else(|| "null".to_string());
        let mut out = format!("{{\"svd\":{},\"register_blocks\":[", svd);
        for (index, block) in self.register_blocks.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"name\":{},\"base\":{},\"size_bytes\":{},\"registers\":[",
                json_str(&block.name),
                block.base,
                block.size.bytes
            );
            for (register_index, register) in block.registers.iter().enumerate() {
                if register_index > 0 {
                    out.push(',');
                }
                let _ = write!(
                    out,
                    "{{\"name\":{},\"offset\":{},\"width\":\"{}\",\"access\":\"{}\",\"volatile\":{}}}",
                    json_str(&register.name),
                    register.offset,
                    register.width.as_str(),
                    register.access.as_str(),
                    if register.volatile { "true" } else { "false" }
                );
            }
            out.push_str("]}");
        }
        out.push_str("],\"interrupts\":[");
        for (index, interrupt) in self.interrupts.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let effects = interrupt
                .forbidden_effects
                .iter()
                .map(|effect| json_str(effect))
                .collect::<Vec<_>>()
                .join(",");
            let _ = write!(
                out,
                "{{\"name\":{},\"vector\":{},\"forbidden_effects\":[{}],\"bounded\":{}}}",
                json_str(&interrupt.name),
                interrupt.vector,
                effects,
                if interrupt.bounded { "true" } else { "false" }
            );
        }
        out.push_str("],\"dma_channels\":[");
        for (index, channel) in self.dma_channels.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"name\":{},\"channel\":{},\"transfer_width\":\"{}\",\"ownership\":\"{}\",\"max_transfer_bytes\":{}}}",
                json_str(&channel.name),
                channel.channel,
                channel.transfer_width.as_str(),
                channel.ownership.as_str(),
                channel.max_transfer_bytes
                    .map_or_else(|| "null".to_string(), |bytes| bytes.to_string())
            );
        }
        out.push_str("]}");
        out
    }
}

/// A source hardware reference that could not be resolved against the selected
/// profile.  It is diagnostic-only metadata; unresolved names never become
/// typed register, interrupt, or DMA facts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetHardwareUnresolvedReference {
    RegisterBlock { block: String },
    Register { block: String, register: String },
    DmaChannel { channel: String },
    Interrupt { interrupt: String },
}

/// Sema-owned hardware uses. The target profile above is immutable; this
/// separate projection records operations and inferred handler/ownership facts
/// without adding parser or backend concepts to the profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetHardwareUse {
    pub register_accesses: Vec<TargetRegisterAccessFact>,
    pub interrupt_handlers: Vec<TargetInterruptHandlerFact>,
    pub dma_operations: Vec<TargetDmaOperationFact>,
    /// Source references which failed profile lookup and therefore cannot be
    /// admitted to any typed hardware operation row.
    pub unresolved_references: Vec<TargetHardwareUnresolvedReference>,
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetMachine {
    pub name: String,
    pub triple: String,
    /// Canonical target/profile layout support facts. This is independent of
    /// host guesses and participates in provider/artifact identity.
    pub layout: LayoutCapabilityFacts,
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
    /// D-FREESTAND-START1=A: generated startup provider and typed startup ABI.
    pub startup: StartupPolicy,
    pub startup_entry: String,
    pub startup_vectors: String,
    pub startup_abi: ProviderAbi,
    pub startup_placement: String,
    /// D-FOUND-BOARD1=A: compiler-owned SVD-derived register, interrupt, and
    /// DMA facts. This is the only hardware profile plane.
    pub hardware: TargetHardwareFacts,
    /// Explicit external programmer facts. An absent list is not inferred from
    /// the target name, triple, or hardware SVD.
    pub programmers: Vec<TargetProgrammerFacts>,
    pub audit: AuditPolicy,
}

impl TargetMachine {
    pub fn hosted(triple: impl Into<String>) -> Self {
        let triple = triple.into();
        Self {
            name: "hosted".to_string(),
            triple: triple.clone(),
            layout: crate::Layout::TargetLayout::from_triple(&triple).layout_facts,
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
            startup_entry: String::new(),
            startup_vectors: String::new(),
            startup_abi: ProviderAbi::default(),
            startup_placement: String::new(),
            hardware: TargetHardwareFacts::default(),
            programmers: Vec::new(),
            audit: AuditPolicy::default(),
        }
    }

    /// Construct a named no-OS machine with all boundary facts explicit.
    pub fn bare_metal(name: impl Into<String>, triple: impl Into<String>) -> Self {
        let triple = triple.into();
        Self {
            name: name.into(),
            triple: triple.clone(),
            layout: crate::Layout::TargetLayout::from_triple(&triple).layout_facts,
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
            // These are profile-owned facts, not triple-derived fallbacks.
            // Board constructors override them when their ABI differs.
            startup_entry: "Reset_Handler".to_string(),
            startup_vectors: "vectors".to_string(),
            startup_abi: ProviderAbi::c(),
            startup_placement: ".vectors".to_string(),
            hardware: TargetHardwareFacts::default(),
            programmers: Vec::new(),
            audit: AuditPolicy::default(),
        }
    }
    /// Override layout support with a complete compiler-owned target profile.
    pub fn with_layout_facts(mut self, facts: LayoutCapabilityFacts) -> Self {
        self.layout = facts;
        self
    }


    /// Override the generated startup facts as one typed contract.
    pub fn with_startup_facts(
        mut self,
        entry: impl Into<String>,
        vectors: impl Into<String>,
        abi: ProviderAbi,
        placement: impl Into<String>,
    ) -> Self {
        self.startup_entry = entry.into();
        self.startup_vectors = vectors.into();
        self.startup_abi = abi;
        self.startup_placement = placement.into();
        self
    }

    /// Add one complete, owner-approved programmer profile. Adapter-specific
    /// fields are validated before the profile can be used by `jet flash`.
    pub fn with_programmer(
        mut self,
        programmer: TargetProgrammerFacts,
    ) -> Result<Self, String> {
        if let Some(missing) = programmer.missing_fact() {
            return Err(format!(
                "target programmer `{}` is missing explicit `{missing}` fact",
                programmer.adapter.as_str()
            ));
        }
        self.programmers.push(programmer);
        Ok(self)
    }

    pub fn environment_identity(&self) -> &'static str {
        if self.no_os {
            "no-os"
        } else if self.triple.contains("wasip") {
            "wasi"
        } else if self.triple == "wasm32-unknown-unknown" {
            "browser"
        } else {
            "hosted"
        }
    }

    /// Whether this machine selects the browser WebAssembly boundary.
    ///
    /// The target triple and the explicit machine facts are authoritative. A
    /// profile name never turns an otherwise hosted or no-OS target into a
    /// browser target.
    pub fn is_browser_target(&self) -> bool {
        self.environment_identity() == "browser" && !self.no_os
    }

    /// Whether this machine selects a WASI component boundary.
    pub fn is_wasi_target(&self) -> bool {
        self.environment_identity() == "wasi" && !self.no_os
    }

    /// Whether this machine emits the browser web artifact rather than a
    /// native Rust artifact.
    pub fn is_web_target(&self) -> bool {
        self.is_browser_target()
    }
    /// Whether this target has the compiler-supported atomic-word primitive.
    /// The fact is target-triple based and deliberately has no hosted fallback:
    /// unsupported targets must be rejected before the Rust backend sees
    /// `Atomic<T>`.
    pub fn supports_atomic_word(&self) -> bool {
        self.triple.starts_with("x86_64-")
            || self.triple.starts_with("aarch64-")
            || self.triple.starts_with("wasm32-")
    }

    /// Ordered, explicit target facts used by both audit output and the
    /// provider identity digest. Policy absence stays represented by the
    /// `unspecified`/`none` value; `Atomic64` is the compiler's target fact
    /// derived from its supported target triple set.
    pub fn provider_fact_list(&self) -> Vec<(TargetCapability, String)> {
        TargetCapability::ALL
            .into_iter()
            .map(|capability| {
                let fact = match capability {
                    TargetCapability::Allocator => self.allocator.audit_json(),
                    TargetCapability::Atomic64 => format!(
                        "{{\"triple\":{},\"supported\":{}}}",
                        json_str(&self.triple),
                        self.supports_atomic_word()
                    ),
                    TargetCapability::Mmio => self.mmio.audit_json(),
                    TargetCapability::Hardware => self.hardware.audit_json(),
                    TargetCapability::TimeWall => self.wall_clock.audit_json(),
                    TargetCapability::TimeMonotonic => self.monotonic_clock.audit_json(),
                    TargetCapability::TimeZoneData => self.zone_data.audit_json(),
                    TargetCapability::TimeSleep => self.sleep.audit_json(),
                    TargetCapability::Entropy => self.entropy.audit_json(),
                    TargetCapability::Scheduler => self.scheduler.audit_json(),
                    TargetCapability::IoRead => {
                        self.byte_sink.audit_component_json(TargetCapability::IoRead)
                    }
                    TargetCapability::IoWrite => {
                        self.byte_sink.audit_component_json(TargetCapability::IoWrite)
                    }
                    TargetCapability::PanicReport => format!(
                        "{{\"panic\":{},\"sink\":{}}}",
                        self.panic.audit_json(),
                        self.byte_sink
                            .audit_component_json(TargetCapability::PanicReport)
                    ),
                    TargetCapability::Startup => format!(
                        "{{\"policy\":{},\"entry\":{},\"vectors\":{},\"abi\":{},\"placement\":{}}}",
                        self.startup.audit_json(),
                        json_str(&self.startup_entry),
                        json_str(&self.startup_vectors),
                        self.startup_abi.audit_json(),
                        json_str(&self.startup_placement)
                    ),
                };
                (capability, fact)
            })
            .collect()
    }

    fn provider_facts_json(&self) -> String {
        let mut out = String::from("[");
        for (index, (capability, fact)) in self.provider_fact_list().iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"capability\":{},\"fact\":{}}}",
                json_str(capability.as_str()),
                fact
            );
        }
        out.push(']');
        out
    }
    fn programmer_facts_json(&self) -> String {
        let mut out = String::from("[");
        for (index, programmer) in self.programmers.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&programmer.audit_json());
        }
        out.push(']');
        out
    }

    /// Stable identity of every selected provider and target boundary input.
    pub fn provider_identity(&self) -> String {
        let mut bytes = Vec::new();
        append_identity_frame(&mut bytes, "environment", self.environment_identity());
        append_identity_frame(&mut bytes, "linker", &self.linker.audit_json());
        append_identity_frame(&mut bytes, "allocator", &self.allocator.audit_json());
        append_identity_frame(&mut bytes, "panic", &self.panic.audit_json());
        append_identity_frame(&mut bytes, "memory", &memory_json(&self.memory));
        append_identity_frame(&mut bytes, "mmio", &self.mmio.audit_json());
        append_identity_frame(&mut bytes, "time_wall", &self.wall_clock.audit_json());
        append_identity_frame(
            &mut bytes,
            "time_monotonic",
            &self.monotonic_clock.audit_json(),
        );
        append_identity_frame(&mut bytes, "time_zone_data", &self.zone_data.audit_json());
        append_identity_frame(&mut bytes, "time_sleep", &self.sleep.audit_json());
        append_identity_frame(&mut bytes, "entropy", &self.entropy.audit_json());
        append_identity_frame(&mut bytes, "scheduler", &self.scheduler.audit_json());
        append_identity_frame(&mut bytes, "byte_sink", &self.byte_sink.audit_json());
        append_identity_frame(&mut bytes, "startup", &self.startup.audit_json());
        append_identity_frame(&mut bytes, "provider_facts", &self.provider_facts_json());
        append_identity_frame(&mut bytes, "hardware", &self.hardware.audit_json());
        append_identity_frame(&mut bytes, "programmers", &self.programmer_facts_json());
        append_identity_frame(&mut bytes, "layout_facts", &self.layout.cache_identity());
        format!(
            "target-providers-v1:{}",
            crate::SHA256::sha256_hex(&bytes)
        )
    }
    /// Stable identity of the linker implementation selected by this machine.
    /// Generated scripts are content-bound to the typed memory, allocator, and
    /// startup facts; they must not collapse to one profile-wide label.
    pub fn linker_identity(&self) -> String {
        match &self.linker {
            LinkerInput::Generated => self
                .generate_linker_script()
                .map(|script| {
                    format!(
                        "linker-generated-v2:{}",
                        crate::SHA256::sha256_hex(script.as_bytes())
                    )
                })
                .unwrap_or_else(|_| self.linker.identity()),
            _ => self.linker.identity(),
        }
    }

    /// Build the complete target dossier consumed by artifact and Prelude
    /// cache keys. `compiler_identity` and `dependency_identity` are supplied
    /// by the enclosing compiler session because the machine cannot discover
    /// those inputs itself.
    pub fn target_dossier(
        &self,
        usage: &TargetMachineUse,
        tier: ExecutionTier,
        compiler_identity: impl AsRef<str>,
        dependency_identity: impl AsRef<str>,
    ) -> TargetDossier {
        let closure = classify_prelude_closure(usage.core_apis.iter());
        let mut closure_bytes = Vec::new();
        for (api, layer) in &closure {
            append_identity_frame(&mut closure_bytes, "api", api);
            append_identity_frame(&mut closure_bytes, "layer", layer.as_str());
        }
        let closure_identity = format!(
            "prelude-closure-v1:{}",
            crate::SHA256::sha256_hex(&closure_bytes)
        );
        TargetDossier {
            machine: Some(Box::new(self.clone())),
            layer: self.max_runtime_layer(),
            provider_identity: self.provider_identity(),
            closure_identity,
            linker_identity: self.linker_identity(),
            tier_identity: tier.as_str().to_string(),
            compiler_identity: compiler_identity.as_ref().to_string(),
            environment_identity: self.environment_identity().to_string(),
            dependency_identity: dependency_identity.as_ref().to_string(),
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
    /// Return whether this machine supplies one explicit target capability.
    /// Hosted defaults are available only on the hosted environment; named
    /// browser/WASI/no-OS profiles must name their own positive providers.
    pub fn provides_capability(&self, capability: TargetCapability) -> bool {
        let hosted = self.environment_identity() == "hosted";
        match capability {
            TargetCapability::Allocator => match &self.allocator {
                AllocatorPolicy::HostedDefault | AllocatorPolicy::Counting { .. } => hosted,
                AllocatorPolicy::Provider { .. } | AllocatorPolicy::Fixed { .. } => true,
                AllocatorPolicy::Unspecified | AllocatorPolicy::None => false,
            },
            TargetCapability::Atomic64 => self.supports_atomic_word(),
            TargetCapability::Mmio => self.mmio.provides(hosted),
            TargetCapability::Hardware => {
                !self.hardware.register_blocks.is_empty()
                    || !self.hardware.interrupts.is_empty()
                    || !self.hardware.dma_channels.is_empty()
            },
            TargetCapability::TimeWall => self.wall_clock.provides(hosted),
            TargetCapability::TimeMonotonic => self.monotonic_clock.provides(hosted),
            TargetCapability::TimeZoneData => self.zone_data.provides(hosted),
            TargetCapability::TimeSleep => self.sleep.provides(hosted),
            TargetCapability::Entropy => self.entropy.provides(hosted),
            TargetCapability::Scheduler => self.scheduler.provides(hosted),
            TargetCapability::IoRead => self.byte_sink.provides_read(hosted),
            TargetCapability::IoWrite => self.byte_sink.provides_write(hosted),
            TargetCapability::PanicReport => self.byte_sink.provides_report(hosted),
            TargetCapability::Startup => self.startup.provides(hosted),
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
        validate_startup_facts(self, &mut errors);
        validate_mmio(self, usage, &mut errors);
        errors.extend(self.hardware.validate(&self.memory));

        errors
    }

    /// Validate backend-neutral hardware operations projected by sema. The
    /// sema boundary performs effect and ownership sequencing; this method
    /// supplies the profile and typed register checks it shares with MIR.
    pub fn validate_hardware(&self, usage: &TargetHardwareUse) -> Vec<TargetMachineError> {
        let mut errors = self.hardware.validate(&self.memory);
        for reference in &usage.unresolved_references {
            match reference {
                TargetHardwareUnresolvedReference::RegisterBlock { block } => {
                    errors.push(TargetMachineError::UnknownRegisterBlock {
                        block: block.clone(),
                    });
                }
                TargetHardwareUnresolvedReference::Register { block, register } => {
                    errors.push(TargetMachineError::UnknownRegister {
                        block: block.clone(),
                        register: register.clone(),
                    });
                }
                TargetHardwareUnresolvedReference::DmaChannel { channel } => {
                    errors.push(TargetMachineError::UnknownDmaChannel {
                        channel: channel.clone(),
                    });
                }
                TargetHardwareUnresolvedReference::Interrupt { interrupt } => {
                    errors.push(TargetMachineError::UnknownInterrupt {
                        interrupt: interrupt.clone(),
                    });
                }
            }
        }
        self.hardware
            .validate_register_accesses(&usage.register_accesses, &mut errors);
        errors
    }

    /// Typed target audit used by command status producers. This preserves the
    /// audit document shape without routing in-repo facts through rendered JSON.
    pub fn audit_value(&self, usage: &TargetMachineUse) -> StatusValue {
        let dossier = self.target_dossier(usage, ExecutionTier::Aot, "unspecified", "unspecified");
        StatusValue::object(
            StatusFields::new()
                .with("name", self.name.as_str())
                .with("triple", self.triple.as_str())
                .with("environment", self.environment_identity())
                .with("provider_identity", self.provider_identity())
                .with("linker", linker_value(&self.linker))
                .with("allocator", allocator_value(&self.allocator))
                .with("panic", panic_value(&self.panic))
                .with("memory", memory_value(&self.memory))
                .with("mmio_capability", mmio_policy_value(&self.mmio))
                .with("time_wall", clock_value(&self.wall_clock))
                .with("time_monotonic", clock_value(&self.monotonic_clock))
                .with("time_zone_data", clock_value(&self.zone_data))
                .with("time_sleep", clock_value(&self.sleep))
                .with("entropy", entropy_value(&self.entropy))
                .with("scheduler", scheduler_value(&self.scheduler))
                .with("byte_sink", byte_sink_value(&self.byte_sink))
                .with("startup", startup_value(&self.startup))
                .with("provider_facts", provider_facts_value(self))
                .with("hardware", hardware_value(&self.hardware))
                .with("programmers", programmers_value(&self.programmers))
                .with("target_dossier", target_dossier_value(&dossier, &self.triple))
                .with("unavailable_core_apis", unavailable_core_value(self, usage))
                .with("mmio", mmio_value(&usage.mmio))
                .with(
                    "execution",
                    StatusValue::object(
                        StatusFields::new()
                            .with("aot", true)
                            .with("dev", !self.no_os)
                            .with("jit", !self.no_os),
                    ),
                ),
        )
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
            &json_str(self.environment_identity()),
            false,
        );
        push_field(
            &mut out,
            "provider_identity",
            &json_str(&self.provider_identity()),
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
        push_field(&mut out, "provider_facts", &self.provider_facts_json(), false);
        push_field(&mut out, "hardware", &self.hardware.audit_json(), false);
        push_field(&mut out, "programmers", &self.programmer_facts_json(), false);
        let dossier = self.target_dossier(usage, ExecutionTier::Aot, "unspecified", "unspecified");
        push_field(
            &mut out,
            "target_dossier",
            &target_dossier_json(&dossier, &self.triple),
            false,
        );
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
    ///
    /// The exported symbols are part of the portable Prelude ABI. Every value
    /// comes from the checked memory and allocator facts; no target triple
    /// guesses section placement or heap bounds.
    pub fn generate_linker_script(&self) -> Result<String, TargetMachineError> {
        if !self.no_os {
            return Err(TargetMachineError::HostedHasNoLinkerScript);
        }
        let flash = self
            .memory
            .iter()
            .find(|region| region.kind == MemoryKind::Flash)
            .ok_or(TargetMachineError::MissingMemoryKind {
                kind: MemoryKind::Flash,
            })?;
        let ram = self
            .memory
            .iter()
            .find(|region| region.kind == MemoryKind::Ram)
            .ok_or(TargetMachineError::MissingMemoryKind {
                kind: MemoryKind::Ram,
            })?;

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
        let _ = write!(out, "ENTRY({})\nSECTIONS {{\n", self.startup_entry);
        let _ = write!(
            out,
            "  .text : {{\n    KEEP(*({}))\n    *(.text*)\n    *(.rodata*)\n  }} > {}\n",
            self.startup_placement, flash.name
        );
        let _ = write!(
            out,
            "  .data : ALIGN(4) {{\n    __jet_data_start = .;\n    *(.data*)\n    __jet_data_end = .;\n  }} > {} AT > {}\n  __jet_data_load = LOADADDR(.data);\n",
            ram.name, flash.name
        );
        let _ = write!(
            out,
            "  .bss (NOLOAD) : ALIGN(4) {{\n    __jet_bss_start = .;\n    *(.bss*)\n    *(COMMON)\n    __jet_bss_end = .;\n  }} > {}\n",
            ram.name
        );
        let _ = write!(
            out,
            "  __jet_stack_top = ORIGIN({}) + LENGTH({});\n",
            ram.name, ram.name
        );
        match &self.allocator {
            AllocatorPolicy::Fixed { region, size } => {
                let _ = write!(
                    out,
                    "  __jet_heap_start = ORIGIN({region});\n  __jet_heap_end = ORIGIN({region}) + {};\n",
                    size.bytes
                );
            }
            AllocatorPolicy::Provider { .. } => {
                let _ = write!(
                    out,
                    "  __jet_heap_start = __jet_bss_end;\n  __jet_heap_end = ORIGIN({}) + LENGTH({});\n",
                    ram.name, ram.name
                );
            }
            AllocatorPolicy::None
            | AllocatorPolicy::Unspecified
            | AllocatorPolicy::HostedDefault
            | AllocatorPolicy::Counting { .. } => {
                out.push_str(
                    "  __jet_heap_start = __jet_bss_end;\n  __jet_heap_end = __jet_bss_end;\n",
                );
            }
        }
        out.push_str("}\n");
        Ok(out)
    }

    /// Startup source that matches the machine triple (AOT firmware smoke).
    ///
    /// Startup is deliberately only machine plumbing. The checked Jet program
    /// enters through `__jet_program_entry`; no marker output or canned
    /// success path is emitted here.
    pub fn generate_startup_source(&self) -> Result<StartupSource, TargetMachineError> {
        if !self.no_os {
            return Err(TargetMachineError::HostedHasNoStartup);
        }
        if self.triple.contains("thumb") || self.triple.starts_with("arm") {
            self.memory
                .iter()
                .find(|region| region.kind == MemoryKind::Ram)
                .ok_or(TargetMachineError::MissingMemoryKind {
                    kind: MemoryKind::Ram,
                })?;
            Ok(StartupSource {
                filename: "startup.c".to_string(),
                contents: format!(
                    concat!(
                        "/* generated by Jet target machine `{name}` */\n",
                        "/* ABI: {abi} {abi_version} */\n",
                        "typedef void (*vec_t)(void);\n",
                        "typedef __UINTPTR_TYPE__ uintptr_t;\n",
                        "typedef __UINT32_TYPE__ uint32_t;\n",
                        "extern unsigned char __jet_stack_top;\n",
                        "extern unsigned char __jet_data_load;\n",
                        "extern unsigned char __jet_data_start;\n",
                        "extern unsigned char __jet_data_end;\n",
                        "extern unsigned char __jet_bss_start;\n",
                        "extern unsigned char __jet_bss_end;\n",
                        "#if defined(__ARM_FP) && (__ARM_FP != 0)\n",
                        "static void __jet_enable_fpu(void) {{\n",
                        "  volatile uint32_t *cpacr = (volatile uint32_t *)(uintptr_t)0xE000ED88u;\n",
                        "  *cpacr |= (uint32_t)(0xFu << 20);\n",
                        "  __asm__ volatile (\"dsb\" ::: \"memory\");\n",
                        "  __asm__ volatile (\"isb\" ::: \"memory\");\n",
                        "}}\n",
                        "#else\n",
                        "static void __jet_enable_fpu(void) {{}}\n",
                        "#endif\n",
                        "void __jet_target_init(void);\n",
                        "void __jet_program_entry(void);\n",
                        "void {entry}(void);\n",
                        "void SysTick_Handler(void);\n",
                        "void Default_Handler(void) {{ for (;;) {{ __asm__ volatile (\"wfi\"); }} }}\n",
                        "__attribute__((section(\"{placement}\"), used, aligned(128)))\n",
                        "vec_t const {vectors}[] = {{\n",
                        "  (vec_t)&__jet_stack_top,\n",
                        "  (vec_t){entry},\n",
                        "  (vec_t)Default_Handler,\n",
                        "  (vec_t)Default_Handler,\n",
                        "  (vec_t)Default_Handler,\n",
                        "  (vec_t)Default_Handler,\n",
                        "  (vec_t)Default_Handler,\n",
                        "  (vec_t)0,\n",
                        "  (vec_t)0,\n",
                        "  (vec_t)0,\n",
                        "  (vec_t)0,\n",
                        "  (vec_t)Default_Handler,\n",
                        "  (vec_t)Default_Handler,\n",
                        "  (vec_t)0,\n",
                        "  (vec_t)Default_Handler,\n",
                        "  (vec_t)SysTick_Handler\n",
                        "}};\n",
                        "void {entry}(void) {{\n",
                        "  unsigned char *src = &__jet_data_load;\n",
                        "  unsigned char *dst = &__jet_data_start;\n",
                        "  while (dst < &__jet_data_end) {{ *dst++ = *src++; }}\n",
                        "  dst = &__jet_bss_start;\n",
                        "  while (dst < &__jet_bss_end) {{ *dst++ = 0; }}\n",
                        "  __jet_enable_fpu();\n",
                        "  __jet_target_init();\n",
                        "  __jet_program_entry();\n",
                        "  for (;;) {{ __asm__ volatile (\"wfi\"); }}\n",
                        "}}\n"
                    ),
                    name = self.name,
                    abi = self.startup_abi.calling_convention,
                    abi_version = self.startup_abi.version,
                    entry = self.startup_entry,
                    vectors = self.startup_vectors,
                    placement = self.startup_placement,
                ),
            })
        } else if self.triple.contains("aarch64") {
            Ok(StartupSource {
                filename: "startup.S".to_string(),
                contents: format!(
                    concat!(
                        "/* generated by Jet target machine `{name}` */\n",
                        "/* ABI: {abi} {abi_version}; placement: {placement} */\n",
                        ".text\n",
                        ".global {entry}\n",
                        ".type {entry}, %function\n",
                        ".extern __jet_target_init\n",
                        ".extern __jet_program_entry\n",
                        ".extern __jet_stack_top\n",
                        ".extern __jet_data_load\n",
                        ".extern __jet_data_start\n",
                        ".extern __jet_data_end\n",
                        ".extern __jet_bss_start\n",
                        ".extern __jet_bss_end\n",
                        "{entry}:\n",
                        "  ldr x0, =__jet_stack_top\n",
                        "  mov sp, x0\n",
                        "  ldr x0, =__jet_data_load\n",
                        "  ldr x1, =__jet_data_start\n",
                        "  ldr x2, =__jet_data_end\n",
                        "1:\n",
                        "  cmp x1, x2\n",
                        "  b.hs 2f\n",
                        "  ldrb w3, [x0], #1\n",
                        "  strb w3, [x1], #1\n",
                        "  b 1b\n",
                        "2:\n",
                        "  ldr x1, =__jet_bss_start\n",
                        "  ldr x2, =__jet_bss_end\n",
                        "3:\n",
                        "  cmp x1, x2\n",
                        "  b.hs 4f\n",
                        "  strb wzr, [x1], #1\n",
                        "  b 3b\n",
                        "4:\n",
                        "  bl __jet_target_init\n",
                        "  bl __jet_program_entry\n",
                        "5: wfe\n",
                        "  b 5b\n",
                        ".size {entry}, .-{entry}\n"
                    ),
                    name = self.name,
                    abi = self.startup_abi.calling_convention,
                    abi_version = self.startup_abi.version,
                    entry = self.startup_entry,
                    placement = self.startup_placement,
                ),
            })
        } else {
            Err(TargetMachineError::UnsupportedStartupTriple {
                triple: self.triple.clone(),
            })
        }
    }
    /// Emit the actual C adapter object for this no-OS profile.
    ///
    /// Only profiles with an implementation in this module are accepted.
    /// Provider labels alone never cause a target function to be invented.
    pub fn generate_provider_source(
        &self,
    ) -> Result<TargetProviderSource, TargetMachineError> {
        if !self.no_os {
            return Err(TargetMachineError::FirmwareBuildFailed {
                detail: "hosted targets do not emit portable target adapters".to_string(),
            });
        }
        let contents = match self.name.as_str() {
            "board.sensor_v1" => {
                let uart = self
                    .hardware
                    .register_blocks
                    .iter()
                    .find(|block| block.name == "UART0")
                    .ok_or_else(|| TargetMachineError::FirmwareBuildFailed {
                        detail: "board.sensor_v1 requires UART0 hardware facts".to_string(),
                    })?;
                let register_address = |name: &str| {
                    uart.register(name)
                        .map(|register| uart.base.saturating_add(register.offset))
                };
                let data = register_address("data").ok_or_else(|| {
                    TargetMachineError::FirmwareBuildFailed {
                        detail: "board.sensor_v1 requires UART0.data hardware fact".to_string(),
                    }
                })?;
                let state = register_address("state").ok_or_else(|| {
                    TargetMachineError::FirmwareBuildFailed {
                        detail: "board.sensor_v1 requires UART0.state hardware fact".to_string(),
                    }
                })?;
                let ctrl = register_address("ctrl").ok_or_else(|| {
                    TargetMachineError::FirmwareBuildFailed {
                        detail: "board.sensor_v1 requires UART0.ctrl hardware fact".to_string(),
                    }
                })?;
                let bauddiv = register_address("bauddiv").ok_or_else(|| {
                    TargetMachineError::FirmwareBuildFailed {
                        detail: "board.sensor_v1 requires UART0.bauddiv hardware fact".to_string(),
                    }
                })?;
                let systick = 0xE000_E018u64;
                sensor_provider_source_contents(ctrl, state, data, bauddiv, systick)
            }
            "board.virt_aarch64" => {
                let uart = self
                    .memory
                    .iter()
                    .find(|region| region.name == "uart0")
                    .map(|region| region.origin)
                    .ok_or_else(|| TargetMachineError::FirmwareBuildFailed {
                        detail: "board.virt_aarch64 requires uart0 memory fact".to_string(),
                    })?;
                virt_provider_source_contents(uart)
            }
            _ => {
                return Err(TargetMachineError::FirmwareBuildFailed {
                    detail: format!(
                        "no checked target adapter implementation for `{}`",
                        self.name
                    ),
                })
            }
        };

        self.verify_provider_bindings(&contents)?;
        Ok(TargetProviderSource {
            filename: "target_providers.c".to_string(),
            contents,
        })
    }

    fn verify_provider_bindings(&self, source: &str) -> Result<(), TargetMachineError> {
        let source = source.as_bytes();
        let check = |capability: TargetCapability, provider: &ProviderContract| {
            if provider.provenance.starts_with("builtin:") && provider.matches_source(source) {
                Ok(())
            } else {
                Err(TargetMachineError::InvalidProviderContract {
                    capability: capability.as_str().to_string(),
                    provider: provider.provider.clone(),
                    sha256: provider.sha256.clone(),
                })
            }
        };
        match self.name.as_str() {
            "board.sensor_v1" => {
                if let MmioPolicy::Provider { provider } = &self.mmio {
                    check(TargetCapability::Mmio, provider)?;
                }
                if let ClockPolicy::Provider { provider } = &self.monotonic_clock {
                    check(TargetCapability::TimeMonotonic, provider)?;
                }
                if let ClockPolicy::Provider { provider } = &self.sleep {
                    check(TargetCapability::TimeSleep, provider)?;
                }
                if let SchedulerPolicy::Cooperative { provider } = &self.scheduler {
                    check(TargetCapability::Scheduler, provider)?;
                }
                if let Some(provider) = self.panic.provider() {
                    check(TargetCapability::PanicReport, provider)?;
                }
                if let ByteSinkPolicy::Provider {
                    read,
                    write,
                    report,
                } = &self.byte_sink
                {
                    if let Some(provider) = read {
                        check(TargetCapability::IoRead, provider)?;
                    }
                    if let Some(provider) = write {
                        check(TargetCapability::IoWrite, provider)?;
                    }
                    if let Some(provider) = report {
                        check(TargetCapability::PanicReport, provider)?;
                    }
                }
            }
            "board.virt_aarch64" => {
                if let MmioPolicy::Provider { provider } = &self.mmio {
                    check(TargetCapability::Mmio, provider)?;
                }
                if let ByteSinkPolicy::Provider {
                    read: _,
                    write,
                    report,
                } = &self.byte_sink
                {
                    if let Some(provider) = write {
                        check(TargetCapability::IoWrite, provider)?;
                    }
                    if let Some(provider) = report {
                        check(TargetCapability::PanicReport, provider)?;
                    }
                }
            }
            _ => unreachable!("provider implementation matched above"),
        }
        if let StartupPolicy::Generated { provider } = &self.startup {
            let startup = self.generate_startup_source()?.contents;
            if !provider.provenance.starts_with("builtin:")
                || !provider.matches_source(startup.as_bytes())
            {
                return Err(TargetMachineError::InvalidProviderContract {
                    capability: TargetCapability::Startup.as_str().to_string(),
                    provider: provider.provider.clone(),
                    sha256: provider.sha256.clone(),
                });
            }
        }
        Ok(())
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

    /// QEMU MPS2 AN386 (Cortex-M4) proof board used by card #239 proofs.
    pub fn board_sensor_v1() -> Self {
        let mut machine = Self::bare_metal("board.sensor_v1", "thumbv7em-none-eabihf");
        machine.memory = vec![
            MemoryRegion::new(
                "flash",
                0x0000_0000,
                ByteSize::mib(4),
                MemoryKind::Flash,
                MemoryAccess::Rx,
            ),
            MemoryRegion::new(
                "ram",
                0x2000_0000,
                ByteSize::mib(4),
                MemoryKind::Ram,
                MemoryAccess::Rw,
            ),
            MemoryRegion::new(
                "peripherals",
                0x4000_0000,
                ByteSize::kib(64),
                MemoryKind::Mmio,
                MemoryAccess::Rw,
            ),
        ];
        machine.linker = LinkerInput::Generated;
        machine.allocator = AllocatorPolicy::None;
        machine.panic = PanicPolicy::Abort;
        let adapter_source = sensor_provider_source_contents(
            0x4000_4008,
            0x4000_4004,
            0x4000_4000,
            0x4000_4010,
            0xE000_E018,
        );
        machine.mmio = MmioPolicy::Provider {
            provider: builtin_provider("board.sensor_v1.mmio", &adapter_source),
        };
        machine.wall_clock = ClockPolicy::None;
        machine.monotonic_clock = ClockPolicy::Provider {
            provider: builtin_provider("board.sensor_v1.systick", &adapter_source),
        };
        machine.zone_data = ClockPolicy::None;
        machine.sleep = ClockPolicy::Provider {
            provider: builtin_provider("board.sensor_v1.systick", &adapter_source),
        };
        machine.entropy = EntropyPolicy::None;
        machine.scheduler = SchedulerPolicy::Cooperative {
            provider: builtin_provider("board.sensor_v1.cooperative", &adapter_source),
        };
        machine.byte_sink = ByteSinkPolicy::Provider {
            read: Some(builtin_provider("board.sensor_v1.uart_rx", &adapter_source)),
            write: Some(builtin_provider("board.sensor_v1.uart_tx", &adapter_source)),
            report: Some(builtin_provider(
                "board.sensor_v1.report_uart",
                &adapter_source,
            )),
        };
        machine.startup = StartupPolicy::Generated {
            provider: builtin_provider(
                "board.sensor_v1.startup",
                &machine
                    .generate_startup_source()
                    .expect("board sensor startup facts are complete")
                    .contents,
            ),
        };
        machine.hardware = TargetHardwareFacts {
            svd: None,
            register_blocks: vec![TargetRegisterBlockFact::new(
                "UART0",
                0x4000_4000,
                ByteSize::bytes(0x14),
                vec![
                    TargetRegisterFact::new(
                        "data",
                        0x00,
                        RegisterWidth::U32,
                        TargetRegisterAccessMode::ReadWrite,
                        true,
                    ),
                    TargetRegisterFact::new(
                        "state",
                        0x04,
                        RegisterWidth::U32,
                        TargetRegisterAccessMode::ReadWrite,
                        true,
                    ),
                    TargetRegisterFact::new(
                        "ctrl",
                        0x08,
                        RegisterWidth::U32,
                        TargetRegisterAccessMode::ReadWrite,
                        true,
                    ),
                    TargetRegisterFact::new(
                        "bauddiv",
                        0x10,
                        RegisterWidth::U32,
                        TargetRegisterAccessMode::ReadWrite,
                        true,
                    ),
                ],
            )],
            interrupts: Vec::new(),
            dma_channels: Vec::new(),
        };
        machine
    }

    /// Linux no-OS / QEMU virt proof board.
    pub fn board_virt_aarch64() -> Self {
        let mut machine = Self::bare_metal("board.virt_aarch64", "aarch64-unknown-none")
            .with_startup_facts(
                "_start",
                "vectors",
                ProviderAbi::new("AArch64", "1"),
                ".vectors",
            );
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
        let adapter_source = virt_provider_source_contents(0x0900_0000);
        machine.mmio = MmioPolicy::Provider {
            provider: builtin_provider("board.virt_aarch64.mmio", &adapter_source),
        };
        machine.wall_clock = ClockPolicy::None;
        machine.monotonic_clock = ClockPolicy::None;
        machine.zone_data = ClockPolicy::None;
        machine.sleep = ClockPolicy::None;
        machine.entropy = EntropyPolicy::None;
        machine.scheduler = SchedulerPolicy::None;
        machine.byte_sink = ByteSinkPolicy::Provider {
            read: None,
            write: Some(builtin_provider(
                "board.virt_aarch64.uart0",
                &adapter_source,
            )),
            report: Some(builtin_provider(
                "board.virt_aarch64.uart0",
                &adapter_source,
            )),
        };
        machine.startup = StartupPolicy::Generated {
            provider: builtin_provider(
                "board.virt_aarch64.startup",
                &machine
                    .generate_startup_source()
                    .expect("board virt startup facts are complete")
                    .contents,
            ),
        };
        machine.programmers = vec![TargetProgrammerFacts::emulator(
            "qemu-system-aarch64",
            "virt",
            "cortex-a57",
        )];
        machine
    }
    /// Browser Wasm profile: browser/JS adapters are explicit target facts.
    pub fn wasm_browser() -> Self {
        let mut machine = Self::hosted("wasm32-unknown-unknown");
        machine.name = "wasm.browser".to_string();
        machine.allocator = AllocatorPolicy::Provider {
            provider: wasm_provider("wasm.browser.allocator", "wasm-browser-allocator"),
        };
        machine.panic = PanicPolicy::Report {
            provider: wasm_provider("wasm.browser.report", "wasm-browser-report"),
        };
        machine.mmio = MmioPolicy::None;
        machine.wall_clock = ClockPolicy::Provider {
            provider: wasm_provider("wasm.browser.wall_clock", "wasm-browser-wall"),
        };
        machine.monotonic_clock = ClockPolicy::Provider {
            provider: wasm_provider("wasm.browser.monotonic_clock", "wasm-browser-mono"),
        };
        machine.zone_data = ClockPolicy::Provider {
            provider: wasm_provider("wasm.browser.zone_data", "wasm-browser-zone"),
        };
        machine.sleep = ClockPolicy::Provider {
            provider: wasm_provider("wasm.browser.sleep", "wasm-browser-sleep"),
        };
        machine.entropy = EntropyPolicy::Provider {
            provider: wasm_provider("wasm.browser.entropy", "wasm-browser-entropy"),
        };
        machine.scheduler = SchedulerPolicy::Cooperative {
            provider: wasm_provider("wasm.browser.scheduler", "wasm-browser-scheduler"),
        };
        machine.byte_sink = ByteSinkPolicy::Provider {
            read: Some(wasm_provider("wasm.browser.io_read", "wasm-browser-read")),
            write: Some(wasm_provider("wasm.browser.io_write", "wasm-browser-write")),
            report: Some(wasm_provider("wasm.browser.report", "wasm-browser-report")),
        };
        machine.startup = StartupPolicy::Generated {
            provider: wasm_provider("wasm.browser.startup", "wasm-browser-startup"),
        };
        machine
    }

    /// WASI Preview 2 profile. The supported component target is
    /// `wasm32-wasip2`; every host boundary remains named and digestable.
    pub fn wasm_wasi() -> Self {
        let mut machine = Self::hosted("wasm32-wasip2");
        machine.name = "wasm.wasi".to_string();
        machine.allocator = AllocatorPolicy::Provider {
            provider: wasm_provider("wasm.wasi.allocator", "wasm-wasi-allocator"),
        };
        machine.panic = PanicPolicy::Report {
            provider: wasm_provider("wasm.wasi.report", "wasm-wasi-report"),
        };
        machine.mmio = MmioPolicy::None;
        machine.wall_clock = ClockPolicy::Provider {
            provider: wasm_provider("wasm.wasi.wall_clock", "wasm-wasi-wall"),
        };
        machine.monotonic_clock = ClockPolicy::Provider {
            provider: wasm_provider("wasm.wasi.monotonic_clock", "wasm-wasi-mono"),
        };
        machine.zone_data = ClockPolicy::Provider {
            provider: wasm_provider("wasm.wasi.zone_data", "wasm-wasi-zone"),
        };
        machine.sleep = ClockPolicy::Provider {
            provider: wasm_provider("wasm.wasi.sleep", "wasm-wasi-sleep"),
        };
        machine.entropy = EntropyPolicy::Provider {
            provider: wasm_provider("wasm.wasi.entropy", "wasm-wasi-entropy"),
        };
        machine.scheduler = SchedulerPolicy::BoardRuntime {
            provider: wasm_provider("wasm.wasi.scheduler", "wasm-wasi-scheduler"),
        };
        machine.byte_sink = ByteSinkPolicy::Provider {
            read: Some(wasm_provider("wasm.wasi.io_read", "wasm-wasi-read")),
            write: Some(wasm_provider("wasm.wasi.io_write", "wasm-wasi-write")),
            report: Some(wasm_provider("wasm.wasi.report", "wasm-wasi-report")),
        };
        machine.startup = StartupPolicy::Generated {
            provider: wasm_provider("wasm.wasi.startup", "wasm-wasi-startup"),
        };
        machine
    }

    /// No-OS Wasm profile. It is a heap-free Core target and is AOT-only.
    pub fn wasm_no_os() -> Self {
        let mut machine = Self::bare_metal("wasm.no-os", "wasm32-unknown-unknown");
        machine.memory = vec![
            MemoryRegion::new(
                "flash",
                0,
                ByteSize::mib(16),
                MemoryKind::Flash,
                MemoryAccess::Rx,
            ),
            MemoryRegion::new(
                "ram",
                0x0100_0000,
                ByteSize::mib(16),
                MemoryKind::Ram,
                MemoryAccess::Rw,
            ),
        ];
        machine.allocator = AllocatorPolicy::None;
        machine.panic = PanicPolicy::Abort;
        machine.mmio = MmioPolicy::None;
        machine.wall_clock = ClockPolicy::None;
        machine.monotonic_clock = ClockPolicy::None;
        machine.zone_data = ClockPolicy::None;
        machine.sleep = ClockPolicy::None;
        machine.entropy = EntropyPolicy::None;
        machine.scheduler = SchedulerPolicy::None;
        machine.byte_sink = ByteSinkPolicy::None;
        machine.startup = StartupPolicy::Generated {
            provider: wasm_provider("wasm.no-os.startup", "wasm-no-os-startup"),
        };
        machine
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupSource {
    pub filename: String,
    pub contents: String,
}

/// Target adapter source emitted from the checked provider facts.
///
/// The source is compiled and linked as a separate object. Its digest is also
/// carried by the provider contracts, so changing an implementation changes
/// the target dossier and firmware identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetProviderSource {
    pub filename: String,
    pub contents: String,
}

fn sensor_provider_source_contents(
    control_register: u64,
    state_register: u64,
    data_register: u64,
    bauddiv_register: u64,
    systick_register: u64,
) -> String {
    format!(
        concat!(
            "/* checked board.sensor_v1 target adapters */\n",
            "/* board contract: QEMU MPS2 AN386 CMSDK APB UART0. */\n",
            "/* UART0 is at 0x40004000; MPS2 AN386 runs the Cortex-M4 at 25 MHz. */\n",
            "/* Sources: https://raw.githubusercontent.com/qemu/qemu/master/hw/arm/mps2.c; https://raw.githubusercontent.com/qemu/qemu/master/hw/char/cmsdk-apb-uart.c. */\n",
            "/* Provider source identity is bound by the checked ProviderContract. */\n",
            "typedef __SIZE_TYPE__ size_t;\n",
            "typedef __UINTPTR_TYPE__ uintptr_t;\n",
            "typedef __UINT8_TYPE__ uint8_t;\n",
            "typedef __INT32_TYPE__ int32_t;\n",
            "typedef __UINT32_TYPE__ uint32_t;\n",
            "typedef __UINT64_TYPE__ uint64_t;\n",
            "typedef __INT64_TYPE__ int64_t;\n",
            "#define JET_UART_DATA ((volatile uint32_t *)(uintptr_t)0x{data_register:08X}u)\n",
            "#define JET_UART_STATE ((volatile uint32_t *)(uintptr_t)0x{state_register:08X}u)\n",
            "#define JET_UART_CTRL ((volatile uint32_t *)(uintptr_t)0x{control_register:08X}u)\n",
            "#define JET_UART_BAUDDIV ((volatile uint32_t *)(uintptr_t)0x{bauddiv_register:08X}u)\n",
            "#define JET_SYSTICK_CVR ((volatile uint32_t *)(uintptr_t)0x{systick_register:08X}u)\n",
            "#define JET_SYSTICK_RVR ((volatile uint32_t *)(uintptr_t)(0x{systick_register:08X}u - 0x4u))\n",
            "#define JET_SYSTICK_CSR ((volatile uint32_t *)(uintptr_t)(0x{systick_register:08X}u - 0x8u))\n",
            "#define JET_UART_STATE_TXFULL (1u << 0)\n",
            "#define JET_UART_STATE_RXFULL (1u << 1)\n",
            "#define JET_UART_STATE_TXOVERRUN (1u << 2)\n",
            "#define JET_UART_STATE_RXOVERRUN (1u << 3)\n",
            "#define JET_UART_CTRL_TX_EN (1u << 0)\n",
            "#define JET_UART_CTRL_RX_EN (1u << 1)\n",
            "#define JET_UART_BAUDDIV_VALUE ((uint32_t)217u)\n",
            "#define JET_SYSTICK_ENABLE (1u << 0)\n",
            "#define JET_SYSTICK_TICKINT (1u << 1)\n",
            "#define JET_SYSTICK_CLKSOURCE (1u << 2)\n",
            "#define JET_SYSTICK_RELOAD ((uint32_t)24999u)\n",
            "#define JET_MMIO_START ((uint64_t)0x40000000u)\n",
            "#define JET_MMIO_END ((uint64_t)0x40010000u)\n",
            "#define JET_IO_LIMIT ((size_t)4096u)\n",
            "\n",
            "static volatile uint32_t jet_initialized;\n",
            "static volatile uint32_t jet_ticks_lo;\n",
            "static volatile uint32_t jet_ticks_hi;\n",
            "\n",
            "static int32_t jet_uart_error(void) {{\n",
            "  uint32_t errors = *JET_UART_STATE & (JET_UART_STATE_TXOVERRUN | JET_UART_STATE_RXOVERRUN);\n",
            "  if (errors == 0u) return 0;\n",
            "  *JET_UART_STATE = errors;\n",
            "  return -4;\n",
            "}}\n",
            "\n",
            "static int32_t jet_uart_wait_rx(void) {{\n",
            "  for (;;) {{\n",
            "    uint32_t state = *JET_UART_STATE;\n",
            "    if ((state & JET_UART_STATE_RXFULL) != 0u) return jet_uart_error();\n",
            "    int32_t error = jet_uart_error();\n",
            "    if (error != 0) return error;\n",
            "    __asm__ volatile (\"wfi\");\n",
            "  }}\n",
            "}}\n",
            "\n",
            "static int32_t jet_uart_wait_tx(void) {{\n",
            "  for (;;) {{\n",
            "    uint32_t state = *JET_UART_STATE;\n",
            "    if ((state & JET_UART_STATE_TXFULL) == 0u) return jet_uart_error();\n",
            "    int32_t error = jet_uart_error();\n",
            "    if (error != 0) return error;\n",
            "    __asm__ volatile (\"nop\");\n",
            "  }}\n",
            "}}\n",
            "\n",
            "void __jet_target_init(void) {{\n",
            "  if (jet_initialized != 0u) return;\n",
            "  *JET_UART_CTRL = 0u;\n",
            "  *JET_UART_STATE = JET_UART_STATE_TXOVERRUN | JET_UART_STATE_RXOVERRUN;\n",
            "  *JET_UART_BAUDDIV = JET_UART_BAUDDIV_VALUE;\n",
            "  *JET_UART_CTRL = JET_UART_CTRL_TX_EN | JET_UART_CTRL_RX_EN;\n",
            "  *JET_SYSTICK_RVR = JET_SYSTICK_RELOAD;\n",
            "  *JET_SYSTICK_CVR = 0u;\n",
            "  *JET_SYSTICK_CSR = JET_SYSTICK_ENABLE | JET_SYSTICK_TICKINT | JET_SYSTICK_CLKSOURCE;\n",
            "  jet_initialized = 1u;\n",
            "}}\n",
            "\n",
            "void SysTick_Handler(void) {{\n",
            "  uint32_t next = jet_ticks_lo + 1u;\n",
            "  if (next == 0u) ++jet_ticks_hi;\n",
            "  jet_ticks_lo = next;\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_read(uint8_t *dst, size_t cap, size_t *used) {{\n",
            "  if (used == (size_t *)0) return -1;\n",
            "  *used = 0;\n",
            "  if (cap > JET_IO_LIMIT || (cap != 0 && dst == (uint8_t *)0)) return -2;\n",
            "  __jet_target_init();\n",
            "  for (size_t i = 0; i < cap; ++i) {{\n",
            "    int32_t error = jet_uart_wait_rx();\n",
            "    if (error != 0) {{ *used = i; return error; }}\n",
            "    dst[i] = (uint8_t)(*JET_UART_DATA & 0xffu);\n",
            "    *used = i + 1u;\n",
            "  }}\n",
            "  return jet_uart_error();\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_write(const uint8_t *src, size_t len, size_t *used) {{\n",
            "  if (used == (size_t *)0) return -1;\n",
            "  *used = 0;\n",
            "  if (len > JET_IO_LIMIT || (len != 0 && src == (const uint8_t *)0)) return -2;\n",
            "  __jet_target_init();\n",
            "  for (size_t i = 0; i < len; ++i) {{\n",
            "    int32_t error = jet_uart_wait_tx();\n",
            "    if (error != 0) {{ *used = i; return error; }}\n",
            "    *JET_UART_DATA = (uint32_t)src[i];\n",
            "    *used = i + 1u;\n",
            "    error = jet_uart_error();\n",
            "    if (error != 0) return error;\n",
            "  }}\n",
            "  return 0;\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_report(const uint8_t *src, size_t len, size_t *used) {{\n",
            "  return __jet_target_write(src, len, used);\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_monotonic_clock(uint64_t *out) {{\n",
            "  uint32_t hi1;\n",
            "  uint32_t lo;\n",
            "  uint32_t hi2;\n",
            "  if (out == (uint64_t *)0) return -1;\n",
            "  __jet_target_init();\n",
            "  do {{\n",
            "    hi1 = jet_ticks_hi;\n",
            "    lo = jet_ticks_lo;\n",
            "    hi2 = jet_ticks_hi;\n",
            "  }} while (hi1 != hi2);\n",
            "  uint64_t ticks = ((uint64_t)hi1 << 32) | (uint64_t)lo;\n",
            "  if (ticks > (~(uint64_t)0 / 1000000u)) {{\n",
            "    *out = ~(uint64_t)0;\n",
            "  }} else {{\n",
            "    *out = ticks * 1000000u;\n",
            "  }}\n",
            "  return 0;\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_sleep(uint64_t nanoseconds) {{\n",
            "  uint64_t start;\n",
            "  if (nanoseconds == 0u) return 0;\n",
            "  __jet_target_init();\n",
            "  if (__jet_target_monotonic_clock(&start) != 0) return -1;\n",
            "  while (1) {{\n",
            "    uint64_t now;\n",
            "    if (__jet_target_monotonic_clock(&now) != 0) return -1;\n",
            "    if ((uint64_t)(now - start) >= nanoseconds) return 0;\n",
            "    __asm__ volatile (\"wfi\");\n",
            "  }}\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_mmio_read(uint64_t address, uint8_t *dst, size_t len, size_t *used) {{\n",
            "  if (used == (size_t *)0) return -1;\n",
            "  *used = 0;\n",
            "  if (address < JET_MMIO_START || address >= JET_MMIO_END || len > (size_t)(JET_MMIO_END - address)) return -2;\n",
            "  if (len != 0 && dst == (uint8_t *)0) return -3;\n",
            "  __jet_target_init();\n",
            "  for (size_t i = 0; i < len; ++i) dst[i] = *((volatile uint8_t *)(uintptr_t)(address + i));\n",
            "  *used = len;\n",
            "  return jet_uart_error();\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_mmio_write(uint64_t address, const uint8_t *src, size_t len, size_t *used) {{\n",
            "  if (used == (size_t *)0) return -1;\n",
            "  *used = 0;\n",
            "  if (address < JET_MMIO_START || address >= JET_MMIO_END || len > (size_t)(JET_MMIO_END - address)) return -2;\n",
            "  if (len != 0 && src == (const uint8_t *)0) return -3;\n",
            "  __jet_target_init();\n",
            "  for (size_t i = 0; i < len; ++i) *((volatile uint8_t *)(uintptr_t)(address + i)) = src[i];\n",
            "  *used = len;\n",
            "  return jet_uart_error();\n",
            "}}\n",
            "\n",
            "void __jet_target_scheduler_yield(void) {{\n",
            "  __jet_target_init();\n",
            "  __asm__ volatile (\"wfi\");\n",
            "}}\n",
            "void __jet_target_abort(void) {{ for (;;) __asm__ volatile (\"wfi\"); }}\n"
        ),
        control_register = control_register,
        state_register = state_register,
        data_register = data_register,
        bauddiv_register = bauddiv_register,
        systick_register = systick_register
    )
}

fn virt_provider_source_contents(uart: u64) -> String {
    format!(
        concat!(
            "/* checked board.virt_aarch64 target adapters */\n",
            "/* board contract: ARM PL011 UART at the declared uart0 base. */\n",
            "typedef __SIZE_TYPE__ size_t;\n",
            "typedef __UINTPTR_TYPE__ uintptr_t;\n",
            "typedef __UINT8_TYPE__ uint8_t;\n",
            "typedef __UINT32_TYPE__ uint32_t;\n",
            "typedef __UINT64_TYPE__ uint64_t;\n",
            "typedef __INT32_TYPE__ int32_t;\n",
            "#define JET_UART_DR ((volatile uint32_t *)(uintptr_t)0x{uart:08X}u)\n",
            "#define JET_UART_FR ((volatile uint32_t *)(uintptr_t)(0x{uart:08X}u + 0x18u))\n",
            "#define JET_UART_LCR_H ((volatile uint32_t *)(uintptr_t)(0x{uart:08X}u + 0x2Cu))\n",
            "#define JET_UART_CR ((volatile uint32_t *)(uintptr_t)(0x{uart:08X}u + 0x30u))\n",
            "#define JET_UART_FR_RXFE (1u << 4)\n",
            "#define JET_UART_FR_TXFF (1u << 5)\n",
            "#define JET_UART_LCR_H_FEN (1u << 4)\n",
            "#define JET_UART_LCR_H_WLEN_8 (3u << 5)\n",
            "#define JET_UART_CR_UARTEN (1u << 0)\n",
            "#define JET_UART_CR_TXE (1u << 8)\n",
            "#define JET_UART_CR_RXE (1u << 9)\n",
            "#define JET_MMIO_START ((uint64_t)0x09000000u)\n",
            "#define JET_MMIO_END ((uint64_t)0x09001000u)\n",
            "#define JET_IO_LIMIT ((size_t)4096u)\n",
            "\n",
            "static volatile uint32_t jet_initialized;\n",
            "\n",
            "void __jet_target_init(void) {{\n",
            "  if (jet_initialized != 0u) return;\n",
            "  *JET_UART_CR = 0u;\n",
            "  *JET_UART_LCR_H = JET_UART_LCR_H_FEN | JET_UART_LCR_H_WLEN_8;\n",
            "  *JET_UART_CR = JET_UART_CR_UARTEN | JET_UART_CR_TXE | JET_UART_CR_RXE;\n",
            "  jet_initialized = 1u;\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_write(const uint8_t *src, size_t len, size_t *used) {{\n",
            "  if (used == (size_t *)0) return -1;\n",
            "  *used = 0;\n",
            "  if (len > JET_IO_LIMIT || (len != 0 && src == (const uint8_t *)0)) return -2;\n",
            "  __jet_target_init();\n",
            "  for (size_t i = 0; i < len; ++i) {{\n",
            "    while ((*JET_UART_FR & JET_UART_FR_TXFF) != 0u) __asm__ volatile (\"nop\");\n",
            "    *JET_UART_DR = (uint32_t)src[i];\n",
            "  }}\n",
            "  *used = len;\n",
            "  return 0;\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_report(const uint8_t *src, size_t len, size_t *used) {{\n",
            "  return __jet_target_write(src, len, used);\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_mmio_read(uint64_t address, uint8_t *dst, size_t len, size_t *used) {{\n",
            "  if (used == (size_t *)0) return -1;\n",
            "  *used = 0;\n",
            "  if (address < JET_MMIO_START || address >= JET_MMIO_END || len > (size_t)(JET_MMIO_END - address)) return -2;\n",
            "  if (len != 0 && dst == (uint8_t *)0) return -3;\n",
            "  __jet_target_init();\n",
            "  for (size_t i = 0; i < len; ++i) dst[i] = *((volatile uint8_t *)(uintptr_t)(address + i));\n",
            "  *used = len;\n",
            "  return 0;\n",
            "}}\n",
            "\n",
            "int32_t __jet_target_mmio_write(uint64_t address, const uint8_t *src, size_t len, size_t *used) {{\n",
            "  if (used == (size_t *)0) return -1;\n",
            "  *used = 0;\n",
            "  if (address < JET_MMIO_START || address >= JET_MMIO_END || len > (size_t)(JET_MMIO_END - address)) return -2;\n",
            "  if (len != 0 && src == (const uint8_t *)0) return -3;\n",
            "  __jet_target_init();\n",
            "  for (size_t i = 0; i < len; ++i) *((volatile uint8_t *)(uintptr_t)(address + i)) = src[i];\n",
            "  *used = len;\n",
            "  return 0;\n",
            "}}\n",
            "\n",
            "void __jet_target_abort(void) {{ for (;;) __asm__ volatile (\"wfe\"); }}\n"
        ),
        uart = uart
    )
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

    /// Stable linker identity for target dossiers and artifact keys.
    pub fn identity(&self) -> String {
        let kind = match self {
            Self::HostedDefault => "hosted-default",
            Self::Unspecified => "unspecified",
            Self::Generated => "generated",
            Self::File { .. } => "file",
        };
        format!(
            "linker-{kind}-v1:{}",
            crate::SHA256::sha256_hex(self.audit_json().as_bytes())
        )
    }
}

/// D-TARGET-ALLOC1 / D-ALLOC-PROGRAM1=A: one typed allocator fact for
/// no-OS targets and hosted programs. Hosted programs may wrap the
/// hidden system heap with the built-in counting allocator and an optional
/// hard cap; no fact keeps the existing hidden heap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocatorPolicy {
    HostedDefault,
    Unspecified,
    None,
    /// A non-host allocator supplied by a named runtime (for example a Wasm
    /// linear-memory allocator).
    Provider { provider: ProviderContract },
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
                | AllocatorPolicy::Provider { .. }
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
            AllocatorPolicy::Provider { provider } => {
                format!("{{\"kind\":\"provider\",\"contract\":{}}}", provider.audit_json())
            }
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

    pub fn audit_value(&self) -> StatusValue {
        allocator_value(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanicPolicy {
    HostedDefault,
    Unspecified,
    Abort,
    /// A reporting panic provider is a target fact, not an untyped sink name.
    Report { provider: ProviderContract },
}

impl PanicPolicy {
    fn provider(&self) -> Option<&ProviderContract> {
        match self {
            Self::Report { provider } => Some(provider),
            _ => None,
        }
    }

    fn audit_json(&self) -> String {
        match self {
            PanicPolicy::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            PanicPolicy::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            PanicPolicy::Abort => "{\"kind\":\"abort\"}".to_string(),
            PanicPolicy::Report { provider } => {
                format!("{{\"kind\":\"report\",\"contract\":{}}}", provider.audit_json())
            }
        }
    }
}

/// D-FREESTAND-FACTS1=A: metadata common to every selected provider fact.
///
/// The calling convention and version are kept as data so a target profile
/// cannot silently inherit the host ABI. `ProviderContract::new` remains the
/// compatibility constructor for recorded provider identities and supplies
/// the explicit C/1 default; profiles that use another convention must name
/// it with [`ProviderContract::with_abi`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAbi {
    pub calling_convention: String,
    pub version: String,
}

impl ProviderAbi {
    pub fn new(
        calling_convention: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            calling_convention: calling_convention.into(),
            version: version.into(),
        }
    }

    pub fn c() -> Self {
        Self::new("C", "1")
    }

    fn is_valid(&self) -> bool {
        !self.calling_convention.trim().is_empty()
            && !self.version.trim().is_empty()
            && !self.calling_convention.chars().any(char::is_whitespace)
            && !self.version.chars().any(char::is_whitespace)
    }

    fn audit_json(&self) -> String {
        format!(
            "{{\"calling_convention\":{},\"version\":{}}}",
            json_str(&self.calling_convention),
            json_str(&self.version)
        )
    }
}

impl Default for ProviderAbi {
    fn default() -> Self {
        Self::c()
    }
}

/// Resource limits promised by a provider contract. `None` means that the
/// provider has no declared bound for that dimension; zero is never a valid
/// declared limit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderLimits {
    pub max_request_bytes: Option<u64>,
    pub max_response_bytes: Option<u64>,
    pub max_calls: Option<u64>,
}

impl ProviderLimits {
    pub fn unbounded() -> Self {
        Self::default()
    }

    fn is_valid(&self) -> bool {
        [self.max_request_bytes, self.max_response_bytes, self.max_calls]
            .into_iter()
            .flatten()
            .all(|limit| limit > 0)
    }

    fn audit_json(&self) -> String {
        format!(
            "{{\"max_request_bytes\":{},\"max_response_bytes\":{},\"max_calls\":{}}}",
            optional_u64_json(self.max_request_bytes),
            optional_u64_json(self.max_response_bytes),
            optional_u64_json(self.max_calls)
        )
    }
}

/// D-FREESTAND-FACTS1=A: every selected target provider carries an explicit
/// identity, digest, provenance, ABI, and resource limits. A target triple
/// never supplies any of these values implicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderContract {
    pub provider: String,
    pub sha256: String,
    pub provenance: String,
    pub abi: ProviderAbi,
    pub limits: ProviderLimits,
}

impl ProviderContract {
    pub fn new(provider: impl Into<String>, sha256: impl Into<String>) -> Self {
        let provider = provider.into();
        Self {
            provenance: format!("provider:{provider}"),
            provider,
            sha256: sha256.into(),
            abi: ProviderAbi::default(),
            limits: ProviderLimits::default(),
        }
    }

    /// Build a contract from the bytes that implement the provider boundary.
    ///
    /// Target profiles use this instead of treating a profile label as a
    /// provider digest. Callers supplying external providers may continue to
    /// pass their recorded `sha256:` value through [`Self::new`].
    pub fn from_source(provider: impl Into<String>, source: &[u8]) -> Self {
        Self::new(
            provider,
            format!("sha256:{}", crate::SHA256::sha256_hex(source)),
        )
    }

    /// Return whether this contract names exactly the supplied implementation
    /// bytes. Providers emitted by Jet use this before compilation so a stale
    /// digest cannot silently link under a current machine profile.
    pub fn matches_source(&self, source: &[u8]) -> bool {
        self.sha256
            == format!("sha256:{}", crate::SHA256::sha256_hex(source))
    }

    pub fn with_provenance(mut self, provenance: impl Into<String>) -> Self {
        self.provenance = provenance.into();
        self
    }

    pub fn with_abi(mut self, abi: ProviderAbi) -> Self {
        self.abi = abi;
        self
    }

    pub fn with_limits(mut self, limits: ProviderLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn is_valid(&self) -> bool {
        !self.provider.trim().is_empty()
            && self
                .sha256
                .strip_prefix("sha256:")
                .is_some_and(|digest| {
                    !digest.trim().is_empty() && !digest.chars().any(char::is_whitespace)
                })
            && !self.provenance.trim().is_empty()
            && self.abi.is_valid()
            && self.limits.is_valid()
    }

    pub fn audit_json(&self) -> String {
        format!(
            "{{\"provider\":{},\"sha256\":{},\"provenance\":{},\"abi\":{},\"limits\":{}}}",
            json_str(&self.provider),
            json_str(&self.sha256),
            json_str(&self.provenance),
            self.abi.audit_json(),
            self.limits.audit_json()
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

    pub fn audit_json(&self) -> String {
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

    pub fn audit_json(&self) -> String {
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

    pub fn audit_json(&self) -> String {
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

    pub fn audit_json(&self) -> String {
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

    fn audit_component_json(&self, capability: TargetCapability) -> String {
        let contract = match (self, capability) {
            (Self::Provider { read, .. }, TargetCapability::IoRead) => read.as_ref(),
            (Self::Provider { write, .. }, TargetCapability::IoWrite) => write.as_ref(),
            (Self::Provider { report, .. }, TargetCapability::PanicReport) => report.as_ref(),
            _ => None,
        };
        match self {
            Self::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            Self::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            Self::None => "{\"kind\":\"none\"}".to_string(),
            Self::Provider { .. } => format!(
                "{{\"kind\":\"provider\",\"contract\":{}}}",
                optional_contract_json(contract)
            ),
        }
    }

    pub fn audit_json(&self) -> String {
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

    pub fn audit_json(&self) -> String {
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
    Allocator,
    Atomic64,
    Mmio,
    Hardware,
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
    pub const ALL: [Self; 14] = [
        Self::Allocator,
        Self::Atomic64,
        Self::Mmio,
        Self::Hardware,
        Self::TimeWall,
        Self::TimeMonotonic,
        Self::TimeZoneData,
        Self::TimeSleep,
        Self::Entropy,
        Self::Scheduler,
        Self::IoRead,
        Self::IoWrite,
        Self::PanicReport,
        Self::Startup,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allocator => "Target.Allocator",
            Self::Atomic64 => "Target.Atomic64",
            Self::Mmio => "Target.MMIO",
            Self::Hardware => "Target.Hardware",
            Self::TimeWall => "Time.Wall",
            Self::TimeMonotonic => "Time.Monotonic",
            Self::TimeZoneData => "Time.ZoneData",
            Self::TimeSleep => "Time.Sleep",
            Self::Entropy => "Rand.Entropy",
            Self::Scheduler => "Target.Scheduler",
            Self::IoRead => "IO.Read",
            Self::IoWrite => "IO.Write",
            Self::PanicReport => "Panic.Report",
            Self::Startup => "Target.Startup",
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

impl TargetMachineUse {
    /// Derive target requirements from the complete semantic Prelude closure.
    /// Callers may still add stack, static-RAM, MMIO, or capability facts that
    /// are discovered outside the core-usage walk.
    pub fn from_core_apis<I, S>(core_apis: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut usage = Self {
            core_apis: core_apis
                .into_iter()
                .map(|api| api.as_ref().to_string())
                .collect(),
            ..Self::default()
        };
        usage.core_apis.sort();
        usage.core_apis.dedup();
        let closure = classify_prelude_closure(usage.core_apis.iter());
        usage.heap_required = closure.values().any(|layer| *layer >= RuntimeLayer::Alloc);
        for api in &usage.core_apis {
            for capability in capabilities_for_core_usage(api) {
                if !usage.required_capabilities.contains(&capability) {
                    usage.required_capabilities.push(capability);
                }
            }
        }
        usage
    }
}
fn capabilities_for_core_usage(api: &str) -> Vec<TargetCapability> {
    let (module, helper) = api
        .split_once("::")
        .map_or((api, ""), |(module, helper)| (module, helper));
    let mut capabilities = Vec::new();
    let add = |capabilities: &mut Vec<TargetCapability>, capability| {
        if !capabilities.contains(&capability) {
            capabilities.push(capability);
        }
    };
    match module {
        "core.mem" if matches!(helper, "Atomic" | "atomic") => {
            add(&mut capabilities, TargetCapability::Atomic64);
        }
        "core.term" => {
            if helper.is_empty()
                || matches!(
                    helper,
                    "input"
                        | "readline"
                        | "read_until"
                        | "read_all_input"
                        | "take"
                        | "buffered"
                        | "stdin"
                        | "binread"
                        | "read_key"
                        | "confirm"
                        | "choose"
                        | "select"
                )
            {
                add(&mut capabilities, TargetCapability::IoRead);
            }
            if helper.is_empty()
                || matches!(
                    helper,
                    "print"
                        | "eprint"
                        | "progress"
                        | "binwrite"
                        | "stdout"
                        | "stderr"
                        | "style"
                        | "style_force"
                )
            {
                add(&mut capabilities, TargetCapability::IoWrite);
            }
        }
        "core.concurrency" | "core.tasks" => {
            add(&mut capabilities, TargetCapability::Scheduler);
        }
        "core.time" => {
            if matches!(
                helper,
                "sleep" | "delay" | "yield" | "wait"
            ) {
                add(&mut capabilities, TargetCapability::TimeSleep);
            } else if matches!(
                helper,
                "instant" | "monotonic" | "elapsed" | "elapsed_millis"
            ) {
                add(&mut capabilities, TargetCapability::TimeMonotonic);
            } else if matches!(helper, "zone" | "zone_data" | "named_zone") {
                add(&mut capabilities, TargetCapability::TimeZoneData);
            } else if !matches!(helper, "" | "typed_head" | "__duration__" | "duration") {
                add(&mut capabilities, TargetCapability::TimeWall);
            }
        }
        "core.crypto.random" | "core.crypto.uuid" => {
            add(&mut capabilities, TargetCapability::Entropy);
        }
        "core.hardware" => {
            add(&mut capabilities, TargetCapability::Hardware);
        }
        _ => {}
    }
    capabilities
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
    HardwareFactsMissingSvd,
    InvalidHardwareSvd {
        source: String,
        sha256: String,
    },
    HardwareFactEmptyName {
        kind: String,
    },
    DuplicateRegisterBlock {
        name: String,
    },
    RegisterBlockEmpty {
        name: String,
    },
    RegisterBlockAddressOverflow {
        name: String,
    },
    RegisterBlockOutsideRegion {
        name: String,
        address: u64,
        size_bytes: u64,
    },
    DuplicateRegister {
        block: String,
        register: String,
    },
    RegisterAddressOverflow {
        block: String,
        register: String,
    },
    RegisterOutsideBlock {
        block: String,
        register: String,
    },
    UnknownRegisterBlock {
        block: String,
    },
    UnknownRegister {
        block: String,
        register: String,
    },
    RegisterWidthMismatch {
        block: String,
        register: String,
        expected: RegisterWidth,
        actual: RegisterWidth,
    },
    RegisterReadDenied {
        block: String,
        register: String,
    },
    RegisterWriteDenied {
        block: String,
        register: String,
    },
    DuplicateInterrupt {
        name: String,
    },
    DuplicateInterruptVector {
        vector: u16,
    },
    InvalidInterruptEffect {
        interrupt: String,
        effect: String,
    },
    UnknownInterrupt {
        interrupt: String,
    },
    InterruptEffectForbidden {
        interrupt: String,
        handler: String,
        effect: String,
    },
    InterruptHandlerUnbounded {
        interrupt: String,
        handler: String,
    },
    DuplicateDmaChannel {
        name: String,
    },
    DuplicateDmaChannelNumber {
        channel: u16,
    },
    DmaTransferSizeZero {
        channel: String,
    },
    UnknownDmaChannel {
        channel: String,
    },
    DmaOwnershipMismatch {
        channel: String,
        buffer: String,
        expected: TargetDmaOwner,
        actual: TargetDmaOwner,
    },
    DmaStartWhileInFlight {
        channel: String,
        buffer: String,
    },
    DmaWaitWithoutTransfer {
        channel: String,
        buffer: String,
    },
    DmaBufferUnavailable {
        channel: String,
        buffer: String,
    },
    DmaWaitChannelMismatch {
        channel: String,
        buffer: String,
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
        AllocatorPolicy::HostedDefault | AllocatorPolicy::Counting { .. } if machine.no_os => {
            errors.push(TargetMachineError::HostedAllocatorRequiresOs)
        }
        AllocatorPolicy::Provider { provider } => {
            validate_provider_contract(TargetCapability::Allocator, provider, errors);
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
    if let Some(provider) = machine.panic.provider() {
        validate_provider_contract(TargetCapability::PanicReport, provider, errors);
    }
}

fn validate_startup_facts(machine: &TargetMachine, errors: &mut Vec<TargetMachineError>) {
    if !machine.no_os || !matches!(machine.startup, StartupPolicy::Generated { .. }) {
        return;
    }
    let missing = machine.startup_entry.trim().is_empty()
        || machine.startup_vectors.trim().is_empty()
        || machine.startup_placement.trim().is_empty();
    if missing {
        push_unique(
            errors,
            TargetMachineError::MissingTargetCapability {
                capability: TargetCapability::Startup.as_str().to_string(),
            },
        );
    }
    if !machine.startup_abi.is_valid() {
        if let Some(provider) = machine.startup.provider() {
            push_unique(
                errors,
                TargetMachineError::InvalidProviderContract {
                    capability: TargetCapability::Startup.as_str().to_string(),
                    provider: provider.provider.clone(),
                    sha256: provider.sha256.clone(),
                },
            );
        }
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
    if usage.heap_required && !machine.provides_capability(TargetCapability::Allocator) {
        errors.push(TargetMachineError::HeapRequiresAllocator);
    }

    let available = machine.max_runtime_layer();
    for (api, required) in classify_prelude_closure(usage.core_apis.iter()) {
        if required > available {
            errors.push(TargetMachineError::CoreApiUnavailable {
                api,
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
            for capability in [
                TargetCapability::IoRead,
                TargetCapability::IoWrite,
                TargetCapability::PanicReport,
            ] {
                push_unique(
                    errors,
                    TargetMachineError::HostedCapabilityRequiresOs {
                        capability: capability.as_str().to_string(),
                    },
                );
            }
        }
        ByteSinkPolicy::Unspecified if machine.no_os => {
            for capability in [
                TargetCapability::IoRead,
                TargetCapability::IoWrite,
                TargetCapability::PanicReport,
            ] {
                push_unique(
                    errors,
                    TargetMachineError::MissingTargetCapability {
                        capability: capability.as_str().to_string(),
                    },
                );
            }
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
    if matches!(machine.panic, PanicPolicy::Report { .. })
        && !machine
            .byte_sink
            .provides_report(machine.environment_identity() == "hosted")
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
/// Render the stable target dossier projection used by inspect and artifact identity.
pub fn target_dossier_json(dossier: &TargetDossier, target_triple: &str) -> String {
    let artifact_key =
        crate::SHA256::sha256_hex(&dossier.cache_bytes(target_triple));
    let machine = dossier.machine.as_deref().map_or_else(
        || "null".to_string(),
        |machine| {
            format!(
                "{{\"name\":{},\"triple\":{},\"provider_identity\":{},\"linker_identity\":{}}}",
                json_str(&machine.name),
                json_str(&machine.triple),
                json_str(&machine.provider_identity()),
                json_str(&machine.linker_identity()),
            )
        },
    );
    format!(
        "{{\"machine\":{},\"layer\":{},\"provider_identity\":{},\"closure_identity\":{},\"linker_identity\":{},\"tier_identity\":{},\"compiler_identity\":{},\"environment_identity\":{},\"dependency_identity\":{},\"artifact_key\":{}}}",
        machine,
        json_str(dossier.layer.as_str()),
        json_str(&dossier.provider_identity),
        json_str(&dossier.closure_identity),
        json_str(&dossier.linker_identity),
        json_str(&dossier.tier_identity),
        json_str(&dossier.compiler_identity),
        json_str(&dossier.environment_identity),
        json_str(&dossier.dependency_identity),
        json_str(&artifact_key),
    )
}
trait StatusFieldsOptional {
    fn with_optional(self, name: &str, value: Option<StatusValue>) -> Self;
    fn with_optional_u64(self, name: &str, value: Option<u64>) -> Self;
}

impl StatusFieldsOptional for StatusFields {
    fn with_optional(self, name: &str, value: Option<StatusValue>) -> Self {
        self.with(name, value.unwrap_or(StatusValue::Null))
    }

    fn with_optional_u64(self, name: &str, value: Option<u64>) -> Self {
        self.with(name, value.map(StatusValue::from).unwrap_or(StatusValue::Null))
    }
}

pub fn target_dossier_value(dossier: &TargetDossier, target_triple: &str) -> StatusValue {
    let artifact_key = crate::SHA256::sha256_hex(&dossier.cache_bytes(target_triple));
    let machine = dossier
        .machine
        .as_deref()
        .map(target_machine_identity_value)
        .unwrap_or(StatusValue::Null);
    StatusValue::object(
        StatusFields::new()
            .with("machine", machine)
            .with("layer", dossier.layer.as_str())
            .with("provider_identity", dossier.provider_identity.as_str())
            .with("closure_identity", dossier.closure_identity.as_str())
            .with("linker_identity", dossier.linker_identity.as_str())
            .with("tier_identity", dossier.tier_identity.as_str())
            .with("compiler_identity", dossier.compiler_identity.as_str())
            .with("environment_identity", dossier.environment_identity.as_str())
            .with("dependency_identity", dossier.dependency_identity.as_str())
            .with("artifact_key", artifact_key),
    )
}

fn target_machine_identity_value(machine: &TargetMachine) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("name", machine.name.as_str())
            .with("triple", machine.triple.as_str())
            .with("provider_identity", machine.provider_identity())
            .with("linker_identity", machine.linker_identity()),
    )
}

fn linker_value(linker: &LinkerInput) -> StatusValue {
    match linker {
        LinkerInput::HostedDefault => {
            StatusValue::object(StatusFields::new().with("kind", "hosted-default"))
        }
        LinkerInput::Unspecified => {
            StatusValue::object(StatusFields::new().with("kind", "unspecified"))
        }
        LinkerInput::Generated => {
            StatusValue::object(StatusFields::new().with("kind", "generated"))
        }
        LinkerInput::File { path, sha256 } => StatusValue::object(
            StatusFields::new()
                .with("kind", "file")
                .with("path", path.as_str())
                .with("sha256", sha256.as_str()),
        ),
    }
}

fn provider_contract_value(provider: &ProviderContract) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("provider", provider.provider.as_str())
            .with("sha256", provider.sha256.as_str())
            .with("provenance", provider.provenance.as_str())
            .with(
                "abi",
                StatusValue::object(
                    StatusFields::new()
                        .with("calling_convention", provider.abi.calling_convention.as_str())
                        .with("version", provider.abi.version.as_str()),
                ),
            )
            .with(
                "limits",
                StatusValue::object(
                    StatusFields::new()
                        .with_optional_u64("max_request_bytes", provider.limits.max_request_bytes)
                        .with_optional_u64("max_response_bytes", provider.limits.max_response_bytes)
                        .with_optional_u64("max_calls", provider.limits.max_calls),
                ),
            ),
    )
}

fn allocator_value(policy: &AllocatorPolicy) -> StatusValue {
    match policy {
        AllocatorPolicy::HostedDefault => {
            StatusValue::object(StatusFields::new().with("kind", "hosted-default"))
        }
        AllocatorPolicy::Unspecified => {
            StatusValue::object(StatusFields::new().with("kind", "unspecified"))
        }
        AllocatorPolicy::None => StatusValue::object(StatusFields::new().with("kind", "none")),
        AllocatorPolicy::Provider { provider } => StatusValue::object(
            StatusFields::new()
                .with("kind", "provider")
                .with("contract", provider_contract_value(provider)),
        ),
        AllocatorPolicy::Fixed { region, size } => StatusValue::object(
            StatusFields::new()
                .with("kind", "fixed")
                .with("region", region.as_str())
                .with("size_bytes", size.bytes),
        ),
        AllocatorPolicy::Counting { cap } => StatusValue::object(
            StatusFields::new()
                .with("kind", "counting")
                .with("wraps", "system")
                .with_optional_u64("cap_bytes", cap.map(|size| size.bytes)),
        ),
    }
}

fn panic_value(policy: &PanicPolicy) -> StatusValue {
    match policy {
        PanicPolicy::HostedDefault => {
            StatusValue::object(StatusFields::new().with("kind", "hosted-default"))
        }
        PanicPolicy::Unspecified => {
            StatusValue::object(StatusFields::new().with("kind", "unspecified"))
        }
        PanicPolicy::Abort => StatusValue::object(StatusFields::new().with("kind", "abort")),
        PanicPolicy::Report { provider } => StatusValue::object(
            StatusFields::new()
                .with("kind", "report")
                .with("contract", provider_contract_value(provider)),
        ),
    }
}

fn clock_value(policy: &ClockPolicy) -> StatusValue {
    policy_value_with_provider(
        match policy {
            ClockPolicy::HostedDefault => "hosted-default",
            ClockPolicy::Unspecified => "unspecified",
            ClockPolicy::None => "none",
            ClockPolicy::Provider { .. } => "provider",
        },
        match policy {
            ClockPolicy::Provider { provider } => Some(provider),
            _ => None,
        },
    )
}

fn entropy_value(policy: &EntropyPolicy) -> StatusValue {
    policy_value_with_provider(
        match policy {
            EntropyPolicy::HostedDefault => "hosted-default",
            EntropyPolicy::Unspecified => "unspecified",
            EntropyPolicy::None => "none",
            EntropyPolicy::Provider { .. } => "provider",
        },
        match policy {
            EntropyPolicy::Provider { provider } => Some(provider),
            _ => None,
        },
    )
}

fn scheduler_value(policy: &SchedulerPolicy) -> StatusValue {
    let (kind, provider) = match policy {
        SchedulerPolicy::HostedDefault => ("hosted-default", None),
        SchedulerPolicy::Unspecified => ("unspecified", None),
        SchedulerPolicy::None => ("none", None),
        SchedulerPolicy::Cooperative { provider } => ("cooperative", Some(provider)),
        SchedulerPolicy::InterruptDriven { provider } => ("interrupt-driven", Some(provider)),
        SchedulerPolicy::BoardRuntime { provider } => ("board-runtime", Some(provider)),
    };
    policy_value_with_provider(kind, provider)
}

fn mmio_policy_value(policy: &MmioPolicy) -> StatusValue {
    policy_value_with_provider(
        match policy {
            MmioPolicy::HostedDefault => "hosted-default",
            MmioPolicy::Unspecified => "unspecified",
            MmioPolicy::None => "none",
            MmioPolicy::Provider { .. } => "provider",
        },
        match policy {
            MmioPolicy::Provider { provider } => Some(provider),
            _ => None,
        },
    )
}

fn policy_value_with_provider(kind: &str, provider: Option<&ProviderContract>) -> StatusValue {
    let fields = StatusFields::new().with("kind", kind);
    match provider {
        Some(provider) => StatusValue::object(fields.with("contract", provider_contract_value(provider))),
        None => StatusValue::object(fields),
    }
}

fn byte_sink_value(policy: &ByteSinkPolicy) -> StatusValue {
    match policy {
        ByteSinkPolicy::HostedDefault => {
            StatusValue::object(StatusFields::new().with("kind", "hosted-default"))
        }
        ByteSinkPolicy::Unspecified => {
            StatusValue::object(StatusFields::new().with("kind", "unspecified"))
        }
        ByteSinkPolicy::None => StatusValue::object(StatusFields::new().with("kind", "none")),
        ByteSinkPolicy::Provider {
            read,
            write,
            report,
        } => StatusValue::object(
            StatusFields::new()
                .with("kind", "provider")
                .with_optional("read", read.as_ref().map(provider_contract_value))
                .with_optional("write", write.as_ref().map(provider_contract_value))
                .with_optional("report", report.as_ref().map(provider_contract_value)),
        ),
    }
}

fn startup_value(policy: &StartupPolicy) -> StatusValue {
    let (kind, provider) = match policy {
        StartupPolicy::HostedDefault => ("hosted-default", None),
        StartupPolicy::Unspecified => ("unspecified", None),
        StartupPolicy::Generated { provider } => ("generated", Some(provider)),
    };
    policy_value_with_provider(kind, provider)
}

fn provider_facts_value(machine: &TargetMachine) -> StatusValue {
    StatusValue::array(TargetCapability::ALL.iter().map(|capability| {
        let fact = match capability {
            TargetCapability::Allocator => allocator_value(&machine.allocator),
            TargetCapability::Atomic64 => StatusValue::object(
                StatusFields::new()
                    .with("triple", machine.triple.as_str())
                    .with("supported", machine.supports_atomic_word()),
            ),
            TargetCapability::Mmio => mmio_policy_value(&machine.mmio),
            TargetCapability::Hardware => hardware_value(&machine.hardware),
            TargetCapability::TimeWall => clock_value(&machine.wall_clock),
            TargetCapability::TimeMonotonic => clock_value(&machine.monotonic_clock),
            TargetCapability::TimeZoneData => clock_value(&machine.zone_data),
            TargetCapability::TimeSleep => clock_value(&machine.sleep),
            TargetCapability::Entropy => entropy_value(&machine.entropy),
            TargetCapability::Scheduler => scheduler_value(&machine.scheduler),
            TargetCapability::IoRead => byte_sink_component_value(&machine.byte_sink, TargetCapability::IoRead),
            TargetCapability::IoWrite => byte_sink_component_value(&machine.byte_sink, TargetCapability::IoWrite),
            TargetCapability::PanicReport => StatusValue::object(
                StatusFields::new()
                    .with("panic", panic_value(&machine.panic))
                    .with(
                        "sink",
                        byte_sink_component_value(&machine.byte_sink, TargetCapability::PanicReport),
                    ),
            ),
            TargetCapability::Startup => StatusValue::object(
                StatusFields::new()
                    .with("policy", startup_value(&machine.startup))
                    .with("entry", machine.startup_entry.as_str())
                    .with("vectors", machine.startup_vectors.as_str())
                    .with(
                        "abi",
                        StatusValue::object(
                            StatusFields::new()
                                .with("calling_convention", machine.startup_abi.calling_convention.as_str())
                                .with("version", machine.startup_abi.version.as_str()),
                        ),
                    )
                    .with("placement", machine.startup_placement.as_str()),
            ),
        };
        StatusValue::object(
            StatusFields::new()
                .with("capability", capability.as_str())
                .with("fact", fact),
        )
    }))
}

fn byte_sink_component_value(policy: &ByteSinkPolicy, capability: TargetCapability) -> StatusValue {
    let contract = match (policy, capability) {
        (ByteSinkPolicy::Provider { read, .. }, TargetCapability::IoRead) => {
            read.as_ref().map(provider_contract_value)
        }
        (ByteSinkPolicy::Provider { write, .. }, TargetCapability::IoWrite) => {
            write.as_ref().map(provider_contract_value)
        }
        (ByteSinkPolicy::Provider { report, .. }, TargetCapability::PanicReport) => {
            report.as_ref().map(provider_contract_value)
        }
        _ => None,
    };
    let fields = StatusFields::new().with(
        "kind",
        match policy {
            ByteSinkPolicy::HostedDefault => "hosted-default",
            ByteSinkPolicy::Unspecified => "unspecified",
            ByteSinkPolicy::None => "none",
            ByteSinkPolicy::Provider { .. } => "provider",
        },
    );
    match policy {
        ByteSinkPolicy::Provider { .. } => {
            StatusValue::object(fields.with_optional("contract", contract))
        }
        _ => StatusValue::object(fields),
    }
}

fn memory_value(regions: &[MemoryRegion]) -> StatusValue {
    StatusValue::array(regions.iter().map(|region| {
        StatusValue::object(
            StatusFields::new()
                .with("name", region.name.as_str())
                .with("origin", region.origin)
                .with("size_bytes", region.size.bytes)
                .with("kind", region.kind.as_str())
                .with("access", region.access.as_str()),
        )
    }))
}

fn hardware_value(hardware: &TargetHardwareFacts) -> StatusValue {
    let svd = hardware
        .svd
        .as_ref()
        .map(|svd| {
            StatusValue::object(
                StatusFields::new()
                    .with("source", svd.source.as_str())
                    .with("sha256", svd.sha256.as_str()),
            )
        })
        .unwrap_or(StatusValue::Null);
    let register_blocks = StatusValue::array(hardware.register_blocks.iter().map(|block| {
        StatusValue::object(
            StatusFields::new()
                .with("name", block.name.as_str())
                .with("base", block.base)
                .with("size_bytes", block.size.bytes)
                .with(
                    "registers",
                    StatusValue::array(block.registers.iter().map(|register| {
                        StatusValue::object(
                            StatusFields::new()
                                .with("name", register.name.as_str())
                                .with("offset", register.offset)
                                .with("width", register.width.as_str())
                                .with("access", register.access.as_str())
                                .with("volatile", register.volatile),
                        )
                    })),
                ),
        )
    }));
    let interrupts = StatusValue::array(hardware.interrupts.iter().map(|interrupt| {
        StatusValue::object(
            StatusFields::new()
                .with("name", interrupt.name.as_str())
                .with("vector", u64::from(interrupt.vector))
                .with(
                    "forbidden_effects",
                    StatusValue::array(interrupt.forbidden_effects.iter().map(|effect| StatusValue::from(effect.as_str()))),
                )
                .with("bounded", interrupt.bounded),
        )
    }));
    let dma_channels = StatusValue::array(hardware.dma_channels.iter().map(|channel| {
        StatusValue::object(
            StatusFields::new()
                .with("name", channel.name.as_str())
                .with("channel", u64::from(channel.channel))
                .with("transfer_width", channel.transfer_width.as_str())
                .with("ownership", channel.ownership.as_str())
                .with_optional_u64("max_transfer_bytes", channel.max_transfer_bytes),
        )
    }));
    StatusValue::object(
        StatusFields::new()
            .with("svd", svd)
            .with("register_blocks", register_blocks)
            .with("interrupts", interrupts)
            .with("dma_channels", dma_channels),
    )
}

fn programmers_value(programmers: &[TargetProgrammerFacts]) -> StatusValue {
    StatusValue::array(programmers.iter().map(|programmer| {
        StatusValue::object(
            StatusFields::new()
                .with("adapter", programmer.adapter.as_str())
                .with_optional("chip", programmer.chip.as_ref().map(|value| StatusValue::from(value.as_str())))
                .with(
                    "config",
                    StatusValue::array(programmer.config.iter().map(|value| StatusValue::from(value.as_str()))),
                )
                .with_optional("cpu", programmer.cpu.as_ref().map(|value| StatusValue::from(value.as_str())))
                .with("executable", programmer.executable.as_str())
                .with_optional(
                    "interface",
                    programmer.interface.as_ref().map(|value| StatusValue::from(value.as_str())),
                )
                .with_optional(
                    "machine",
                    programmer.machine.as_ref().map(|value| StatusValue::from(value.as_str())),
                )
                .with("reset", programmer.reset)
                .with_optional_u64("speed_khz", programmer.speed_khz.map(u64::from)),
        )
    }))
}

fn unavailable_core_value(machine: &TargetMachine, usage: &TargetMachineUse) -> StatusValue {
    let available = machine.max_runtime_layer();
    let closure = classify_prelude_closure(usage.core_apis.iter());
    StatusValue::array(
        closure
            .iter()
            .filter_map(|(api, required)| (*required > available).then_some(StatusValue::from(api.as_str()))),
    )
}

fn mmio_value(accesses: &[MmioAccess]) -> StatusValue {
    StatusValue::array(accesses.iter().map(|access| {
        StatusValue::object(
            StatusFields::new()
                .with("address", access.address)
                .with("size_bytes", access.size.bytes)
                .with_optional(
                    "unsafe_reason",
                    access
                        .unsafe_gate
                        .as_ref()
                        .map(|gate| StatusValue::from(gate.reason.as_str())),
                ),
        )
    }))
}


fn append_identity_frame(bytes: &mut Vec<u8>, label: &str, value: &str) {
    bytes.extend_from_slice(&(label.len() as u64).to_le_bytes());
    bytes.extend_from_slice(label.as_bytes());
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn wasm_provider(name: &str, implementation: &str) -> ProviderContract {
    let mut source = Vec::new();
    append_identity_frame(&mut source, "provider", name);
    append_identity_frame(&mut source, "implementation", implementation);
    ProviderContract::from_source(name, &source)
}

fn builtin_provider(name: &str, source: &str) -> ProviderContract {
    let digest = crate::SHA256::sha256_hex(source.as_bytes());
    ProviderContract::from_source(name, source.as_bytes())
        .with_provenance(format!("builtin:{name};sha256:{digest}"))
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
    let closure = classify_prelude_closure(usage.core_apis.iter());
    let unavailable: Vec<&str> = closure
        .iter()
        .filter_map(|(api, required)| (*required > available).then_some(api.as_str()))
        .collect();
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

fn optional_contract_json(contract: Option<&ProviderContract>) -> String {
    contract
        .map(ProviderContract::audit_json)
        .unwrap_or_else(|| "null".to_string())
}
fn optional_u64_json(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_string(), |value| value.to_string())
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
        let mut machine = TargetMachine::bare_metal("board.sensor_v1", "thumbv7em-none-eabihf");
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
        machine.mmio = MmioPolicy::Provider {
            provider: ProviderContract::new("test.mmio", "sha256:test-mmio"),
        };
        machine.wall_clock = ClockPolicy::None;
        machine.monotonic_clock = ClockPolicy::Provider {
            provider: ProviderContract::new("test.monotonic", "sha256:test-monotonic"),
        };
        machine.zone_data = ClockPolicy::None;
        machine.sleep = ClockPolicy::Provider {
            provider: ProviderContract::new("test.sleep", "sha256:test-sleep"),
        };
        machine.entropy = EntropyPolicy::None;
        machine.scheduler = SchedulerPolicy::None;
        machine.byte_sink = ByteSinkPolicy::None;
        machine.startup = StartupPolicy::Generated {
            provider: ProviderContract::new("test.startup", "sha256:test-startup"),
        };
        machine
    }

    #[test]
    fn target_capability_facts_validate_provider_contracts() {
        let mut machine = valid_machine();
        assert!(machine.provides_capability(TargetCapability::Mmio));
        assert!(machine.provides_capability(TargetCapability::TimeMonotonic));
        assert!(!machine.provides_capability(TargetCapability::TimeWall));
        assert!(!machine.provides_capability(TargetCapability::Entropy));

        let usage = TargetMachineUse {
            required_capabilities: vec![
                TargetCapability::TimeMonotonic,
                TargetCapability::TimeWall,
            ],
            ..TargetMachineUse::default()
        };
        let errors = machine.validate(&usage);
        assert!(errors.contains(&TargetMachineError::MissingTargetCapability {
            capability: "Time.Wall".to_string(),
        }));

        machine.mmio = MmioPolicy::Provider {
            provider: ProviderContract::new("", "not-a-digest"),
        };
        let errors = machine.validate(&TargetMachineUse::default());
        assert!(errors.contains(&TargetMachineError::InvalidProviderContract {
            capability: "Target.MMIO".to_string(),
            provider: String::new(),
            sha256: "not-a-digest".to_string(),
        }));
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
    fn no_os_machine_rejects_hosted_counting_wrapper() {
        let mut machine = valid_machine();
        machine.allocator = AllocatorPolicy::Counting { cap: None };
        let errors = machine.validate(&TargetMachineUse::default());
        assert!(errors.contains(&TargetMachineError::HostedAllocatorRequiresOs));
    }

    #[test]
    fn valid_no_os_machine_passes() {
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
            required_capabilities: Vec::new(),
        };
        assert!(valid_machine().validate(&usage).is_empty());
    }

    #[test]
    fn validation_reports_missing_required_no_os_facts() {
        let machine = TargetMachine::bare_metal("", "");
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
            required_capabilities: Vec::new(),
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
        let repeat = valid_machine().audit_json(&usage);
        assert_eq!(json, repeat);
        assert!(json.contains("\"provider_identity\":\"target-providers-v1:"));
        assert!(json.contains("\"target_dossier\":"));
    }

    #[test]
    fn named_wasm_profiles_have_distinct_provider_identities() {
        let browser = TargetMachine::wasm_browser();
        let wasi = TargetMachine::wasm_wasi();
        let no_os = TargetMachine::wasm_no_os();
        assert_eq!(browser.environment_identity(), "browser");
        assert_eq!(wasi.environment_identity(), "wasi");
        assert_eq!(no_os.environment_identity(), "no-os");
        assert_eq!(browser.max_runtime_layer(), RuntimeLayer::Std);
        assert_eq!(wasi.max_runtime_layer(), RuntimeLayer::Std);
        assert_eq!(no_os.max_runtime_layer(), RuntimeLayer::Core);
        assert_ne!(browser.provider_identity(), wasi.provider_identity());
        assert_ne!(browser.provider_identity(), no_os.provider_identity());
        assert_ne!(wasi.provider_identity(), no_os.provider_identity());
    }

    #[test]
    fn target_dossier_carries_closure_and_execution_identity() {
        let machine = TargetMachine::wasm_no_os();
        let usage = TargetMachineUse::from_core_apis(["core.encoding.json"]);
        let dossier = machine.target_dossier(
            &usage,
            ExecutionTier::Aot,
            "jet@2300",
            "deps:none",
        );
        assert_eq!(dossier.layer, RuntimeLayer::Core);
        assert!(dossier.closure_identity.starts_with("prelude-closure-v1:"));
        assert_eq!(dossier.tier_identity, "aot");
        assert_eq!(dossier.compiler_identity, "jet@2300");
        assert_eq!(dossier.dependency_identity, "deps:none");
        assert_ne!(
            dossier.cache_bytes(&machine.triple),
            machine
                .target_dossier(&usage, ExecutionTier::Jit, "jet@2300", "deps:none")
                .cache_bytes(&machine.triple)
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
