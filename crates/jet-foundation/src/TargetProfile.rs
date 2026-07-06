//! D-TARGET-* typed target profile facts.
//!
//! This is the internal model for embedded/freestanding builds. It deliberately
//! carries validation errors as data, not user diagnostics: surfacing new error
//! codes or command/manifest spellings remains owner-gated.

use crate::RingLayer::{core_module_layer, core_usage_layer, RuntimeLayer};
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetProfile {
    pub name: String,
    pub triple: String,
    pub no_os: bool,
    pub memory: Vec<MemoryRegion>,
    pub linker: LinkerInput,
    pub allocator: AllocatorPolicy,
    pub panic: PanicPolicy,
    pub audit: AuditPolicy,
}

impl TargetProfile {
    pub fn hosted(triple: impl Into<String>) -> Self {
        Self {
            name: "hosted".to_string(),
            triple: triple.into(),
            no_os: false,
            memory: Vec::new(),
            linker: LinkerInput::HostedDefault,
            allocator: AllocatorPolicy::HostedDefault,
            panic: PanicPolicy::HostedDefault,
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

    pub fn validate(&self, usage: &TargetProfileUse) -> Vec<TargetProfileError> {
        let mut errors = Vec::new();

        if self.triple.trim().is_empty() {
            errors.push(TargetProfileError::MissingTargetTriple);
        }

        validate_memory_regions(&self.memory, &mut errors);
        validate_linker(&self.linker, self.no_os, &mut errors);
        validate_allocator(self, &mut errors);
        validate_panic(self, &mut errors);
        validate_ram_budget(self, usage, &mut errors);
        validate_core_usage(self, usage, &mut errors);
        validate_mmio(self, usage, &mut errors);

        errors
    }

    pub fn audit_json(&self, usage: &TargetProfileUse) -> String {
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
        push_field(
            &mut out,
            "unavailable_core_apis",
            &unavailable_core_json(self, usage),
            false,
        );
        push_field(&mut out, "mmio", &mmio_json(&usage.mmio), false);
        out.push('}');
        out
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocatorPolicy {
    HostedDefault,
    Unspecified,
    None,
    Fixed { region: String, size: ByteSize },
}

impl AllocatorPolicy {
    pub fn provides_heap(&self) -> bool {
        matches!(
            self,
            AllocatorPolicy::HostedDefault | AllocatorPolicy::Fixed { .. }
        )
    }

    fn fixed_size(&self) -> u64 {
        match self {
            AllocatorPolicy::Fixed { size, .. } => size.bytes,
            _ => 0,
        }
    }

    fn audit_json(&self) -> String {
        match self {
            AllocatorPolicy::HostedDefault => "{\"kind\":\"hosted-default\"}".to_string(),
            AllocatorPolicy::Unspecified => "{\"kind\":\"unspecified\"}".to_string(),
            AllocatorPolicy::None => "{\"kind\":\"none\"}".to_string(),
            AllocatorPolicy::Fixed { region, size } => format!(
                "{{\"kind\":\"fixed\",\"region\":{},\"size_bytes\":{}}}",
                json_str(region),
                size.bytes
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetProfileUse {
    pub stack_bytes: u64,
    pub static_ram_bytes: u64,
    pub heap_required: bool,
    pub core_apis: Vec<String>,
    pub mmio: Vec<MmioAccess>,
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
pub enum TargetProfileError {
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
    MissingPanicPolicy,
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
}

fn validate_memory_regions(regions: &[MemoryRegion], errors: &mut Vec<TargetProfileError>) {
    let mut names: Vec<&str> = Vec::new();
    for region in regions {
        if region.size.bytes == 0 {
            errors.push(TargetProfileError::EmptyMemoryRegion {
                name: region.name.clone(),
            });
        }
        if region.end().is_none() {
            errors.push(TargetProfileError::MemoryAddressOverflow {
                name: region.name.clone(),
            });
        }
        if names.contains(&region.name.as_str()) {
            errors.push(TargetProfileError::DuplicateMemoryRegion {
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
            errors.push(TargetProfileError::OverlappingMemoryRegions {
                first: first.name.clone(),
                second: second.name.clone(),
            });
        }
    }
}

fn validate_linker(linker: &LinkerInput, no_os: bool, errors: &mut Vec<TargetProfileError>) {
    match linker {
        LinkerInput::Unspecified if no_os => errors.push(TargetProfileError::MissingLinkerInput),
        LinkerInput::File { path, sha256 } => {
            if path.trim().is_empty() {
                errors.push(TargetProfileError::LinkerFileMissingPath);
            }
            if !sha256.starts_with("sha256:") || sha256.len() <= "sha256:".len() {
                errors.push(TargetProfileError::LinkerFileMissingHash { path: path.clone() });
            }
        }
        _ => {}
    }
}

fn validate_allocator(profile: &TargetProfile, errors: &mut Vec<TargetProfileError>) {
    match &profile.allocator {
        AllocatorPolicy::Unspecified if profile.no_os => {
            errors.push(TargetProfileError::MissingAllocatorPolicy)
        }
        AllocatorPolicy::Fixed { region, .. } => {
            match profile.memory.iter().find(|r| r.name == *region) {
                Some(r) if r.kind == MemoryKind::Ram => {}
                Some(_) => errors.push(TargetProfileError::AllocatorRegionNotRam {
                    region: region.clone(),
                }),
                None => errors.push(TargetProfileError::AllocatorRegionUnknown {
                    region: region.clone(),
                }),
            }
        }
        _ => {}
    }
}

fn validate_panic(profile: &TargetProfile, errors: &mut Vec<TargetProfileError>) {
    if profile.no_os
        && matches!(
            profile.panic,
            PanicPolicy::Unspecified | PanicPolicy::HostedDefault
        )
    {
        errors.push(TargetProfileError::MissingPanicPolicy);
    }
}

fn validate_ram_budget(
    profile: &TargetProfile,
    usage: &TargetProfileUse,
    errors: &mut Vec<TargetProfileError>,
) {
    if !profile.no_os {
        return;
    }
    for kind in [MemoryKind::Flash, MemoryKind::Ram] {
        if !profile.memory.iter().any(|r| r.kind == kind) {
            errors.push(TargetProfileError::MissingMemoryKind { kind });
        }
    }
    let ram_bytes: u64 = profile
        .memory
        .iter()
        .filter(|r| r.kind == MemoryKind::Ram)
        .map(|r| r.size.bytes)
        .sum();
    let used_bytes = usage
        .stack_bytes
        .saturating_add(usage.static_ram_bytes)
        .saturating_add(profile.allocator.fixed_size());
    if ram_bytes > 0 && used_bytes > ram_bytes {
        errors.push(TargetProfileError::RamOverflow {
            used_bytes,
            ram_bytes,
        });
    }
}

fn validate_core_usage(
    profile: &TargetProfile,
    usage: &TargetProfileUse,
    errors: &mut Vec<TargetProfileError>,
) {
    if usage.heap_required && !profile.allocator.provides_heap() {
        errors.push(TargetProfileError::HeapRequiresAllocator);
    }

    let available = profile.max_runtime_layer();
    for api in &usage.core_apis {
        let required = core_usage_layer(api)
            .or_else(|| core_module_layer(api))
            .unwrap_or(RuntimeLayer::Std);
        if required > available {
            errors.push(TargetProfileError::CoreApiUnavailable {
                api: api.clone(),
                required,
                available,
            });
        }
    }
}

fn validate_mmio(
    profile: &TargetProfile,
    usage: &TargetProfileUse,
    errors: &mut Vec<TargetProfileError>,
) {
    for access in &usage.mmio {
        let inside_mmio = profile
            .memory
            .iter()
            .any(|r| r.kind == MemoryKind::Mmio && r.contains(access.address, access.size));
        if !inside_mmio {
            errors.push(TargetProfileError::MmioOutsideRegion {
                address: access.address,
                size_bytes: access.size.bytes,
            });
        }
        match &access.unsafe_gate {
            Some(gate) if gate.reason.trim().is_empty() => {
                errors.push(TargetProfileError::MmioEmptyUnsafeReason {
                    address: access.address,
                });
            }
            Some(_) => {}
            None => errors.push(TargetProfileError::MmioMissingUnsafeGate {
                address: access.address,
            }),
        }
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

fn unavailable_core_json(profile: &TargetProfile, usage: &TargetProfileUse) -> String {
    let available = profile.max_runtime_layer();
    let mut unavailable = Vec::new();
    for api in &usage.core_apis {
        let required = core_usage_layer(api)
            .or_else(|| core_module_layer(api))
            .unwrap_or(RuntimeLayer::Std);
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

    fn valid_profile() -> TargetProfile {
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
        profile.allocator = AllocatorPolicy::Fixed {
            region: "ram".to_string(),
            size: ByteSize::kib(16),
        };
        profile.panic = PanicPolicy::Abort;
        profile
    }

    #[test]
    fn hosted_default_keeps_full_runtime() {
        let profile = TargetProfile::hosted("x86_64-unknown-linux-gnu");
        let usage = TargetProfileUse {
            core_apis: vec!["core.files".to_string(), "core.http.client".to_string()],
            heap_required: true,
            ..TargetProfileUse::default()
        };
        assert_eq!(profile.max_runtime_layer(), RuntimeLayer::Std);
        assert!(profile.validate(&usage).is_empty());
    }

    #[test]
    fn valid_freestanding_profile_passes() {
        let usage = TargetProfileUse {
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
        assert!(valid_profile().validate(&usage).is_empty());
    }

    #[test]
    fn validation_reports_missing_required_no_os_facts() {
        let profile = TargetProfile::freestanding("", "");
        let errors = profile.validate(&TargetProfileUse::default());
        assert!(errors.contains(&TargetProfileError::MissingTargetTriple));
        assert!(errors.contains(&TargetProfileError::MissingMemoryKind {
            kind: MemoryKind::Flash
        }));
        assert!(errors.contains(&TargetProfileError::MissingMemoryKind {
            kind: MemoryKind::Ram
        }));
        assert!(errors.contains(&TargetProfileError::MissingAllocatorPolicy));
        assert!(errors.contains(&TargetProfileError::MissingPanicPolicy));
    }

    #[test]
    fn validation_reports_ram_heap_core_and_mmio_errors() {
        let mut profile = valid_profile();
        profile.allocator = AllocatorPolicy::None;
        let usage = TargetProfileUse {
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
        let errors = profile.validate(&usage);
        assert!(errors.contains(&TargetProfileError::RamOverflow {
            used_bytes: ByteSize::kib(160).bytes,
            ram_bytes: ByteSize::kib(128).bytes
        }));
        assert!(errors.contains(&TargetProfileError::HeapRequiresAllocator));
        assert!(errors.contains(&TargetProfileError::CoreApiUnavailable {
            api: "core.files".to_string(),
            required: RuntimeLayer::Std,
            available: RuntimeLayer::Core
        }));
        assert!(errors.contains(&TargetProfileError::MmioOutsideRegion {
            address: 0x5000_0000,
            size_bytes: 4
        }));
        assert!(errors.contains(&TargetProfileError::MmioMissingUnsafeGate {
            address: 0x5000_0000
        }));
    }

    #[test]
    fn audit_json_is_stable() {
        let usage = TargetProfileUse {
            core_apis: vec!["core.files".to_string()],
            mmio: vec![MmioAccess {
                address: 0x4002_0000,
                size: ByteSize::bytes(4),
                unsafe_gate: Some(UnsafeGate {
                    reason: "GPIO register write".to_string(),
                }),
            }],
            ..TargetProfileUse::default()
        };
        let json = valid_profile().audit_json(&usage);
        assert_eq!(
            json,
            "{\"name\":\"board.sensor_v1\",\"triple\":\"thumbv7em-none-eabihf\",\"environment\":\"no-os\",\"linker\":{\"kind\":\"generated\"},\"allocator\":{\"kind\":\"fixed\",\"region\":\"ram\",\"size_bytes\":16384},\"panic\":{\"kind\":\"abort\"},\"memory\":[{\"name\":\"flash\",\"origin\":134217728,\"size_bytes\":524288,\"kind\":\"flash\",\"access\":\"rx\"},{\"name\":\"ram\",\"origin\":536870912,\"size_bytes\":131072,\"kind\":\"ram\",\"access\":\"rw\"},{\"name\":\"gpio\",\"origin\":1073872896,\"size_bytes\":1024,\"kind\":\"mmio\",\"access\":\"rw\"}],\"unavailable_core_apis\":[\"core.files\"],\"mmio\":[{\"address\":1073872896,\"size_bytes\":4,\"unsafe_reason\":\"GPIO register write\"}]}"
        );
    }
}
