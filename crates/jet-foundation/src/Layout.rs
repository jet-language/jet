//! Canonical target layout facts for compiler-owned physical layouts.
//!
//! This module deliberately does not ask rustc for answers. A layout fact is
//! concrete only when Jet owns the representation (`#Layout(c)` or
//! `#Layout(columnar)`); ordinary Rust-layout types remain unknown. The
//! comptime reflection layer is a formatter for this model, not another
//! layout implementation.

use crate::AST::{
    numeric_type_from_name, CEnumTag, EnumDef, Item, StructDef, StructLayout, Type, VariantPayload,
};
use crate::Syntax;

/// Version of the canonical target-layout fact schema.
pub const LAYOUT_FACT_VERSION: &str = "layout-facts-v2";

/// The portable alignment baseline is a language contract, not a target
/// capability. A target must still prove support through its profile facts.
pub const PORTABLE_ALIGNMENT_BASELINE_MAX: u64 = Syntax::LAYOUT_PORTABLE_ALIGNMENT_BASELINE_MAX;

/// Every portable alignment boundary is an exact power of two. Keeping the
/// baseline set explicit makes the profile intersection visible to cache and
/// tooling consumers instead of hiding it in a backend policy.
pub const PORTABLE_ALIGNMENT_BASELINE: &[u64] = &[
    1,
    2,
    4,
    8,
    16,
    32,
    64,
    128,
    256,
    512,
    1024,
    2048,
    4096,
    8192,
    16384,
    32768,
    65536,
    131072,
    262144,
    524288,
    1048576,
];

/// A placement boundary that a target profile has proved for one layout use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutSupportFacts {
    /// False means the profile did not provide an authoritative fact. An
    /// unknown fact is never treated as an unlimited capability.
    pub proven: bool,
    /// Exact supported alignment boundaries, sorted and deduplicated.
    pub alignments: Vec<u64>,
    /// Maximum representable object size, when the profile supplied one.
    pub max_size: Option<u64>,
    /// Maximum addressable end offset, when the profile supplied one.
    pub max_address: Option<u64>,
    /// Foreign ABI name for the by-value carrier, when applicable.
    pub abi: Option<String>,
    /// Human/tooling provenance for the evidence row.
    pub provenance: String,
}

impl LayoutSupportFacts {
    pub fn unknown() -> Self {
        Self {
            proven: false,
            alignments: Vec::new(),
            max_size: None,
            max_address: None,
            abi: None,
            provenance: String::new(),
        }
    }

    pub fn exact(
        alignments: impl IntoIterator<Item = u64>,
        max_size: Option<u64>,
        max_address: Option<u64>,
        provenance: impl Into<String>,
    ) -> Self {
        let mut alignments = alignments.into_iter().collect::<Vec<_>>();
        alignments.sort_unstable();
        alignments.dedup();
        Self {
            proven: !alignments.is_empty(),
            alignments,
            max_size,
            max_address,
            abi: None,
            provenance: provenance.into(),
        }
    }

    pub fn powers_through(
        max_alignment: u64,
        max_size: Option<u64>,
        max_address: Option<u64>,
        provenance: impl Into<String>,
    ) -> Self {
        if max_alignment == 0 || !max_alignment.is_power_of_two() {
            return Self::unknown();
        }
        let mut alignments = Vec::new();
        let mut alignment = 1u64;
        loop {
            alignments.push(alignment);
            if alignment >= max_alignment {
                break;
            }
            let Some(next) = alignment.checked_mul(2) else {
                break;
            };
            alignment = next;
        }
        Self::exact(alignments, max_size, max_address, provenance)
    }

    pub fn with_abi(mut self, abi: impl Into<String>) -> Self {
        self.abi = Some(abi.into());
        self
    }

    pub fn supports_alignment(&self, alignment: u64) -> bool {
        self.proven && self.alignments.binary_search(&alignment).is_ok()
    }

    fn intersect(left: &Self, right: &Self) -> Self {
        let alignments = if left.proven && right.proven {
            left.alignments
                .iter()
                .copied()
                .filter(|alignment| right.alignments.binary_search(alignment).is_ok())
                .collect()
        } else {
            Vec::new()
        };
        let max_size = left.max_size.zip(right.max_size).map(|(a, b)| a.min(b));
        let max_address = left
            .max_address
            .zip(right.max_address)
            .map(|(a, b)| a.min(b));
        let abi = (left.abi == right.abi)
            .then(|| left.abi.clone())
            .flatten();
        Self {
            proven: left.proven && right.proven && !alignments.is_empty(),
            alignments,
            max_size,
            max_address,
            abi,
            provenance: format!("intersection({}, {})", left.provenance, right.provenance),
        }
    }
}

/// One execution-mode row before profile intersection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutExecutionFacts {
    pub mode: String,
    pub type_representation: LayoutSupportFacts,
    pub stack_placement: LayoutSupportFacts,
    pub global_placement: LayoutSupportFacts,
    pub heap_allocator: LayoutSupportFacts,
    pub foreign_by_value_abi: LayoutSupportFacts,
}

impl LayoutExecutionFacts {
    pub fn uniform(mode: impl Into<String>, support: LayoutSupportFacts) -> Self {
        Self {
            mode: mode.into(),
            type_representation: support.clone(),
            stack_placement: support.clone(),
            global_placement: support.clone(),
            heap_allocator: support.clone(),
            foreign_by_value_abi: support,
        }
    }
}

/// Canonical target/profile intersection consumed by sema and every layout
/// adapter. The five support planes stay separate because a representable type
/// does not promise stack, global, heap, or foreign by-value placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutCapabilityFacts {
    pub fact_version: String,
    pub target_profile: String,
    pub execution_modes: Vec<String>,
    pub backend: String,
    pub toolchain: String,
    pub linker: String,
    pub allocator_provider: String,
    pub type_representation: LayoutSupportFacts,
    pub stack_placement: LayoutSupportFacts,
    pub global_placement: LayoutSupportFacts,
    pub heap_allocator: LayoutSupportFacts,
    pub foreign_by_value_abi: LayoutSupportFacts,
}

impl LayoutCapabilityFacts {
    pub fn from_execution_modes(
        target_profile: impl Into<String>,
        backend: impl Into<String>,
        toolchain: impl Into<String>,
        linker: impl Into<String>,
        allocator_provider: impl Into<String>,
        modes: impl IntoIterator<Item = LayoutExecutionFacts>,
    ) -> Self {
        let modes = modes.into_iter().collect::<Vec<_>>();
        let mut execution_modes = modes
            .iter()
            .map(|mode| mode.mode.clone())
            .collect::<Vec<_>>();
        execution_modes.sort();
        execution_modes.dedup();
        let first = modes.first();
        let mut facts = first
            .map(|mode| {
                (
                    mode.type_representation.clone(),
                    mode.stack_placement.clone(),
                    mode.global_placement.clone(),
                    mode.heap_allocator.clone(),
                    mode.foreign_by_value_abi.clone(),
                )
            })
            .unwrap_or_else(|| {
                (
                    LayoutSupportFacts::unknown(),
                    LayoutSupportFacts::unknown(),
                    LayoutSupportFacts::unknown(),
                    LayoutSupportFacts::unknown(),
                    LayoutSupportFacts::unknown(),
                )
            });
        for mode in modes.iter().skip(1) {
            facts.0 = LayoutSupportFacts::intersect(&facts.0, &mode.type_representation);
            facts.1 = LayoutSupportFacts::intersect(&facts.1, &mode.stack_placement);
            facts.2 = LayoutSupportFacts::intersect(&facts.2, &mode.global_placement);
            facts.3 = LayoutSupportFacts::intersect(&facts.3, &mode.heap_allocator);
            facts.4 =
                LayoutSupportFacts::intersect(&facts.4, &mode.foreign_by_value_abi);
        }
        Self {
            fact_version: LAYOUT_FACT_VERSION.to_string(),
            target_profile: target_profile.into(),
            execution_modes,
            backend: backend.into(),
            toolchain: toolchain.into(),
            linker: linker.into(),
            allocator_provider: allocator_provider.into(),
            type_representation: facts.0,
            stack_placement: facts.1,
            global_placement: facts.2,
            heap_allocator: facts.3,
            foreign_by_value_abi: facts.4,
        }
    }

    /// Build the compiler-owned baseline profile for a known target family.
    /// Unknown target families deliberately receive no evidence row.
    pub fn for_triple(triple: &str, pointer_size: u64) -> Self {
        let known = triple.starts_with("x86_64-")
            || triple.starts_with("aarch64-")
            || triple.starts_with("wasm32-")
            || triple.starts_with("i686-")
            || triple.starts_with("arm-")
            || triple.starts_with("armv7-")
            || triple.starts_with("thumb");
        let address_bound = if pointer_size <= 4 {
            u32::MAX as u64
        } else {
            u64::MAX
        };
        let support = if known {
            LayoutSupportFacts::exact(
                PORTABLE_ALIGNMENT_BASELINE.iter().copied(),
                Some(address_bound),
                Some(address_bound),
                format!("target-profile:{triple}:portable-alignment-baseline"),
            )
        } else {
            LayoutSupportFacts::unknown()
        };
        let foreign = support.clone().with_abi("C");
        let mode = LayoutExecutionFacts {
            mode: "aot".to_string(),
            type_representation: support.clone(),
            stack_placement: support.clone(),
            global_placement: support.clone(),
            heap_allocator: support,
            foreign_by_value_abi: foreign,
        };
        Self::from_execution_modes(
            triple,
            "jet-codegen",
            env!("CARGO_PKG_VERSION"),
            "target-profile",
            "target-profile",
            [mode],
        )
    }

    /// Replace only identity inputs while retaining the already-intersected
    /// support rows. Build/profile selection uses this at the front-end
    /// boundary so target identity cannot be guessed from the host.
    pub fn with_identity(
        mut self,
        target_profile: impl Into<String>,
        execution_modes: impl IntoIterator<Item = String>,
        backend: impl Into<String>,
        toolchain: impl Into<String>,
        linker: impl Into<String>,
        allocator_provider: impl Into<String>,
    ) -> Self {
        self.target_profile = target_profile.into();
        self.execution_modes = execution_modes.into_iter().collect();
        self.backend = backend.into();
        self.toolchain = toolchain.into();
        self.linker = linker.into();
        self.allocator_provider = allocator_provider.into();
        self
    }

    pub fn cache_identity(&self) -> String {
        let mut bytes = Vec::new();
        for value in [
            self.fact_version.as_str(),
            self.target_profile.as_str(),
            self.backend.as_str(),
            self.toolchain.as_str(),
            self.linker.as_str(),
            self.allocator_provider.as_str(),
        ] {
            append_layout_identity_frame(&mut bytes, value.as_bytes());
        }
        for mode in &self.execution_modes {
            append_layout_identity_frame(&mut bytes, mode.as_bytes());
        }
        for support in [
            &self.type_representation,
            &self.stack_placement,
            &self.global_placement,
            &self.heap_allocator,
            &self.foreign_by_value_abi,
        ] {
            append_layout_identity_frame(&mut bytes, &[u8::from(support.proven)]);
            for alignment in &support.alignments {
                append_layout_identity_frame(&mut bytes, &alignment.to_le_bytes());
            }
            append_layout_identity_frame(
                &mut bytes,
                &support.max_size.unwrap_or_default().to_le_bytes(),
            );
            append_layout_identity_frame(
                &mut bytes,
                &support.max_address.unwrap_or_default().to_le_bytes(),
            );
            append_layout_identity_frame(&mut bytes, support.abi.as_deref().unwrap_or("").as_bytes());
            append_layout_identity_frame(&mut bytes, support.provenance.as_bytes());
        }
        format!(
            "{LAYOUT_FACT_VERSION}:{}",
            crate::SHA256::sha256_hex(&bytes)
        )
    }

    pub fn check_alignment(
        &self,
        requested: u64,
        natural: u64,
        target_mode: bool,
    ) -> Result<LayoutAlignmentFact, LayoutAlignmentError> {
        if requested == 0 {
            return Err(LayoutAlignmentError::Zero);
        }
        if !requested.is_power_of_two() {
            return Err(LayoutAlignmentError::NotPowerOfTwo { requested });
        }
        if !target_mode && requested > PORTABLE_ALIGNMENT_BASELINE_MAX {
            return Err(LayoutAlignmentError::PortableBaseline {
                requested,
                baseline: PORTABLE_ALIGNMENT_BASELINE_MAX,
            });
        }
        if !self.type_representation.proven {
            return Err(LayoutAlignmentError::MissingTypeFacts {
                profile: self.target_profile.clone(),
            });
        }
        let effective = natural.max(requested);
        if !self.type_representation.supports_alignment(effective) {
            return Err(LayoutAlignmentError::Unsupported {
                requested,
                effective,
                profile: self.target_profile.clone(),
            });
        }
        Ok(LayoutAlignmentFact {
            requested_alignment: requested,
            effective_alignment: effective,
            target_mode,
            target_profile: self.target_profile.clone(),
            fact_identity: self.cache_identity(),
        })
    }

    pub fn check_storage(
        &self,
        kind: LayoutStorageKind,
        alignment: u64,
        size: u64,
        address: Option<u64>,
    ) -> Result<(), LayoutStorageError> {
        let support = match kind {
            LayoutStorageKind::Stack => &self.stack_placement,
            LayoutStorageKind::Global => &self.global_placement,
            LayoutStorageKind::Heap => &self.heap_allocator,
            LayoutStorageKind::ForeignByValue => &self.foreign_by_value_abi,
        };
        if !support.proven {
            return Err(LayoutStorageError::MissingFacts {
                kind,
                profile: self.target_profile.clone(),
            });
        }
        if !support.supports_alignment(alignment) {
            return Err(LayoutStorageError::UnsupportedAlignment {
                kind,
                alignment,
                profile: self.target_profile.clone(),
            });
        }
        let Some(max_size) = support.max_size else {
            return Err(LayoutStorageError::MissingSizeBound { kind });
        };
        if size > max_size {
            return Err(LayoutStorageError::SizeExceeded {
                kind,
                size,
                max_size,
            });
        }
        if let Some(address) = address {
            let Some(max_address) = support.max_address else {
                return Err(LayoutStorageError::MissingAddressBound { kind });
            };
            let Some(end) = address.checked_add(size) else {
                return Err(LayoutStorageError::AddressOverflow { kind });
            };
            if end > max_address {
                return Err(LayoutStorageError::AddressExceeded {
                    kind,
                    end,
                    max_address,
                });
            }
        }
        Ok(())
    }
}

/// Checked declaration metadata carried from sema into layout consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutAlignmentFact {
    pub requested_alignment: u64,
    pub effective_alignment: u64,
    pub target_mode: bool,
    pub target_profile: String,
    pub fact_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutAlignmentError {
    Zero,
    NotPowerOfTwo {
        requested: u64,
    },
    PortableBaseline {
        requested: u64,
        baseline: u64,
    },
    MissingTypeFacts {
        profile: String,
    },
    Unsupported {
        requested: u64,
        effective: u64,
        profile: String,
    },
}

impl LayoutAlignmentError {
    pub fn reason(&self) -> String {
        match self {
            Self::Zero => "zero does not establish an alignment boundary".to_string(),
            Self::NotPowerOfTwo { .. } => "the value is not a power of two".to_string(),
            Self::PortableBaseline { baseline, .. } => format!(
                "the portable declaration baseline is {baseline} bytes; use `align(target, N)` for a profile-specific request"
            ),
            Self::MissingTypeFacts { profile } => {
                format!("target profile `{profile}` has no proved type-representation alignment facts")
            }
            Self::Unsupported {
                requested,
                effective,
                profile,
            } => format!(
                "target profile `{profile}` does not support requested alignment {requested} (effective {effective})"
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutStorageKind {
    Stack,
    Global,
    Heap,
    ForeignByValue,
}

impl LayoutStorageKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stack => "stack",
            Self::Global => "global",
            Self::Heap => "heap",
            Self::ForeignByValue => "foreign by-value ABI",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutStorageError {
    MissingFacts {
        kind: LayoutStorageKind,
        profile: String,
    },
    UnsupportedAlignment {
        kind: LayoutStorageKind,
        alignment: u64,
        profile: String,
    },
    MissingSizeBound {
        kind: LayoutStorageKind,
    },
    SizeExceeded {
        kind: LayoutStorageKind,
        size: u64,
        max_size: u64,
    },
    MissingAddressBound {
        kind: LayoutStorageKind,
    },
    AddressOverflow {
        kind: LayoutStorageKind,
    },
    AddressExceeded {
        kind: LayoutStorageKind,
        end: u64,
        max_address: u64,
    },
}

impl LayoutStorageError {
    pub fn reason(&self) -> String {
        match self {
            Self::MissingFacts { kind, profile } => {
                format!("{kind:?} placement facts are missing for target profile `{profile}`")
            }
            Self::UnsupportedAlignment {
                kind,
                alignment,
                profile,
            } => format!(
                "{kind:?} placement does not support alignment {alignment} on target profile `{profile}`"
            ),
            Self::MissingSizeBound { kind } => {
                format!("{kind:?} placement has no proved size bound")
            }
            Self::SizeExceeded {
                kind,
                size,
                max_size,
            } => format!("{kind:?} object size {size} exceeds target bound {max_size}"),
            Self::MissingAddressBound { kind } => {
                format!("{kind:?} placement has no proved address bound")
            }
            Self::AddressOverflow { kind } => {
                format!("{kind:?} address plus object size overflows")
            }
            Self::AddressExceeded {
                kind,
                end,
                max_address,
            } => format!("{kind:?} object end {end} exceeds target address bound {max_address}"),
        }
    }
}

fn append_layout_identity_frame(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value);
}

/// The target properties needed by the Jet layout ABI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetLayout {
    /// Canonical target triple shown in reflection and tooling.
    pub triple: String,
    /// Width of a target pointer in bytes.
    pub pointer_size: u64,
    /// ABI alignment of a target pointer in bytes.
    pub pointer_alignment: u64,
    /// Profile-intersected support facts consumed by sema and adapters.
    pub layout_facts: LayoutCapabilityFacts,
}

impl TargetLayout {
    /// Build facts for the host target used by the current compiler process.
    pub fn host() -> Self {
        Self::from_triple(Self::host_triple())
    }

    /// Build the target ABI facts used by the layout engine.
    pub fn from_triple(triple: impl Into<String>) -> Self {
        let triple = triple.into();
        let pointer_size = if triple.starts_with("wasm32-")
            || triple.starts_with("i686-")
            || triple.starts_with("arm-")
            || triple.starts_with("armv7-")
            || triple.starts_with("thumb")
        {
            4
        } else {
            8
        };
        Self {
            layout_facts: LayoutCapabilityFacts::for_triple(&triple, pointer_size),
            triple,
            pointer_size,
            pointer_alignment: pointer_size,
        }
    }

    /// Resolve the target and all cache identity inputs from the folded build
    /// snapshot. Host details never replace an explicit target profile.
    pub fn from_build_facts(facts: &crate::Facts::BuildFactSnapshot) -> Self {
        let triple = if facts.target_triple.is_empty() {
            Self::host_triple()
        } else {
            facts.target_triple.clone()
        };
        let mut target = Self::from_triple(triple);
        if let Some(machine) = facts.target_dossier.machine.as_deref() {
            target.layout_facts = machine.layout.clone();
        }
        let mut modes = target.layout_facts.execution_modes.clone();
        if !modes.contains(&facts.target_dossier.tier_identity) {
            modes.push(facts.target_dossier.tier_identity.clone());
        }
        let target_profile = facts
            .target_dossier
            .machine
            .as_deref()
            .map(|machine| machine.layout.target_profile.clone())
            .unwrap_or_else(|| format!("{}:{}", facts.profile, target.triple));
        target.layout_facts = target.layout_facts.with_identity(
            target_profile,
            modes,
            "jet-codegen",
            facts.target_dossier.compiler_identity.clone(),
            facts.target_dossier.linker_identity.clone(),
            facts.target_dossier.provider_identity.clone(),
        );
        target
    }

    /// Whether the selected target has the ratified lock-free 64-bit atomic
    /// carrier used by `Atomic<T>`. This is an explicit compiler target fact,
    /// not a rustc fallback: unsupported targets must reject the declaration
    /// before backend emission.
    pub fn supports_atomic_word(&self) -> bool {
        self.triple.starts_with("x86_64-")
            || self.triple.starts_with("aarch64-")
            || self.triple.starts_with("wasm32-")
    }

    pub fn check_array_storage(
        &self,
        kind: LayoutStorageKind,
        count: u64,
        layout: ByteLayout,
        address: Option<u64>,
    ) -> Result<(), LayoutStorageError> {
        let size = layout
            .stride
            .checked_mul(count)
            .ok_or(LayoutStorageError::AddressOverflow { kind })?;
        self.layout_facts
            .check_storage(kind, layout.alignment, size, address)
    }

    /// Canonical host triple used when no explicit target is carried by the
    /// checked bundle.
    pub fn host_triple() -> String {
        match (std::env::consts::ARCH, std::env::consts::OS) {
            ("x86_64", "linux") => "x86_64-unknown-linux-gnu".to_string(),
            ("aarch64", "linux") => "aarch64-unknown-linux-gnu".to_string(),
            ("x86_64", "windows") => "x86_64-pc-windows-msvc".to_string(),
            ("aarch64", "windows") => "aarch64-pc-windows-msvc".to_string(),
            ("x86_64", "macos") => "x86_64-apple-darwin".to_string(),
            ("aarch64", "macos") => "aarch64-apple-darwin".to_string(),
            (arch, "wasi") => format!("{arch}-wasi"),
            (arch, os) => format!("{arch}-unknown-{os}"),
        }
    }
}

/// Size, alignment, and repeated-element stride for one physical value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteLayout {
    pub size: u64,
    pub alignment: u64,
    pub stride: u64,
}

/// Byte facts for one stored field or enum payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldLayoutFacts {
    pub name: String,
    pub offset: Option<u64>,
    pub size: Option<u64>,
    pub alignment: Option<u64>,
    pub stride: Option<u64>,
}

/// The one result returned by the target layout engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutFacts {
    pub bytes: Option<ByteLayout>,
    pub fields: Vec<FieldLayoutFacts>,
}

/// Physical facts plus the sema-checked alignment contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedLayoutFacts {
    pub physical: LayoutFacts,
    pub alignment: Option<LayoutAlignmentFact>,
}

impl LayoutFacts {
    fn unknown(fields: impl IntoIterator<Item = String>) -> Self {
        Self {
            bytes: None,
            fields: fields
                .into_iter()
                .map(|name| FieldLayoutFacts {
                    name,
                    offset: None,
                    size: None,
                    alignment: None,
                    stride: None,
                })
                .collect(),
        }
    }
}

/// Computes all compiler-owned layout facts for one target.
pub struct TargetLayoutEngine<'a> {
    target: TargetLayout,
    items: Vec<&'a Item>,
}

impl<'a> TargetLayoutEngine<'a> {
    pub fn new<I>(items: I, target: TargetLayout) -> Self
    where
        I: IntoIterator<Item = &'a Item>,
    {
        Self {
            target,
            items: items.into_iter().collect(),
        }
    }

    pub fn host<I>(items: I) -> Self
    where
        I: IntoIterator<Item = &'a Item>,
    {
        Self::new(items, TargetLayout::host())
    }

    pub fn target(&self) -> &TargetLayout {
        &self.target
    }

    /// Compute a struct's layout.  Default-layout structs intentionally return
    /// no byte facts even when every member is a scalar: rustc owns their
    /// field order and padding.
    pub fn struct_facts(&self, definition: &StructDef) -> LayoutFacts {
        let mut seen = Vec::new();
        self.struct_facts_inner(definition, &mut seen)
    }

    /// Return physical facts together with the exact sema-checked alignment
    /// request. Backends consume this carrier; they do not reinterpret marker
    /// syntax or apply a second numeric policy.
    pub fn checked_struct_facts(
        &self,
        definition: &StructDef,
    ) -> Result<CheckedLayoutFacts, LayoutAlignmentError> {
        let physical = self.struct_facts(definition);
        let alignment = match definition.layout.as_ref() {
            Some(StructLayout::CAligned {
                alignment,
                target,
            }) => Some(self.target.layout_facts.check_alignment(
                *alignment,
                physical
                    .bytes
                    .map(|bytes| bytes.alignment)
                    .unwrap_or(1),
                *target,
            )?),
            _ => None,
        };
        Ok(CheckedLayoutFacts {
            physical,
            alignment,
        })
    }

    /// Compute an enum's layout when it carries an explicit C tag.  Other
    /// enums retain the same unspecified guarantee as ordinary structs.
    pub fn enum_facts(&self, definition: &EnumDef) -> LayoutFacts {
        let fields = definition
            .variants
            .iter()
            .map(|variant| variant.name.clone())
            .collect::<Vec<_>>();
        let Some(tag) = definition.c_layout_tag() else {
            return LayoutFacts::unknown(fields);
        };
        let tag_layout = self.scalar_layout(self.enum_tag_size(tag));
        let mut seen = vec![definition.name.clone()];
        let mut payloads = Vec::with_capacity(definition.variants.len());
        let mut payloads_known = Vec::with_capacity(definition.variants.len());
        for variant in &definition.variants {
            let (payload, known) = match &variant.payload {
                VariantPayload::Unit => (None, true),
                VariantPayload::Single(ty, _) => {
                    let payload = self.type_layout_inner(ty, &mut seen);
                    (payload, payload.is_some())
                }
                VariantPayload::Named(payload_fields) => {
                    let payload = self
                        .aggregate_types(payload_fields.iter().map(|field| &field.ty), &mut seen);
                    (payload, payload.is_some())
                }
            };
            payloads.push(payload);
            payloads_known.push(known);
        }

        let payload_alignment = payloads
            .iter()
            .flatten()
            .map(|layout| layout.alignment)
            .max()
            .unwrap_or(1);
        let payload_size = payloads
            .iter()
            .flatten()
            .map(|layout| layout.size)
            .max()
            .unwrap_or(0);
        let alignment = tag_layout.alignment.max(payload_alignment);
        let Some(payload_offset) = align_up(tag_layout.size, payload_alignment) else {
            return LayoutFacts::unknown(fields);
        };
        let Some(size) = payload_offset
            .checked_add(payload_size)
            .and_then(|size| align_up(size, alignment))
        else {
            return LayoutFacts::unknown(fields);
        };
        let bytes = payloads_known
            .iter()
            .all(|known| *known)
            .then_some(ByteLayout {
                size,
                alignment,
                stride: size,
            });
        let fields = definition
            .variants
            .iter()
            .zip(payloads)
            .map(|(variant, payload)| {
                let (offset, size, alignment, stride) = match payload {
                    Some(layout) => (
                        Some(payload_offset),
                        Some(layout.size),
                        Some(layout.alignment),
                        Some(layout.stride),
                    ),
                    None => (
                        Some(0),
                        Some(tag_layout.size),
                        Some(tag_layout.alignment),
                        Some(tag_layout.stride),
                    ),
                };
                FieldLayoutFacts {
                    name: variant.name.clone(),
                    offset,
                    size,
                    alignment,
                    stride,
                }
            })
            .collect();
        LayoutFacts { bytes, fields }
    }

    fn struct_facts_inner(&self, definition: &StructDef, seen: &mut Vec<String>) -> LayoutFacts {
        let fields = definition
            .reflection_fields()
            .map(|field| field.name.clone())
            .collect::<Vec<_>>();
        let Some(layout) = definition.layout.as_ref() else {
            return LayoutFacts::unknown(fields);
        };
        if !layout.is_c() && !matches!(layout, StructLayout::Columnar) {
            return LayoutFacts::unknown(fields);
        }
        if seen.iter().any(|name| name == &definition.name) {
            return LayoutFacts::unknown(fields);
        }
        seen.push(definition.name.clone());
        let facts = self.aggregate_fields(
            definition
                .reflection_fields()
                .map(|field| (field.name.clone(), &field.ty)),
            seen,
        );
        seen.pop();
        match layout {
            StructLayout::CAligned {
                alignment,
                ..
            } => aligned_struct_facts(facts, *alignment),
            StructLayout::C | StructLayout::Columnar => facts,
        }
    }

    fn aggregate_fields<'b, I>(&self, fields: I, seen: &mut Vec<String>) -> LayoutFacts
    where
        I: IntoIterator<Item = (String, &'b Type)>,
    {
        let mut cursor = Some(0u64);
        let mut aggregate_alignment = Some(1u64);
        let mut facts = Vec::new();
        for (name, ty) in fields {
            let layout = self.type_layout_inner(ty, seen);
            let offset = match (cursor, layout) {
                (Some(cursor), Some(layout)) => align_up(cursor, layout.alignment),
                _ => None,
            };
            if let Some(layout) = layout {
                if let Some(next) = offset.and_then(|offset| offset.checked_add(layout.size)) {
                    cursor = Some(next);
                } else {
                    cursor = None;
                }
                aggregate_alignment =
                    aggregate_alignment.map(|alignment| alignment.max(layout.alignment));
                facts.push(FieldLayoutFacts {
                    name,
                    offset,
                    size: Some(layout.size),
                    alignment: Some(layout.alignment),
                    stride: Some(layout.stride),
                });
            } else {
                cursor = None;
                aggregate_alignment = None;
                facts.push(FieldLayoutFacts {
                    name,
                    offset: None,
                    size: None,
                    alignment: None,
                    stride: None,
                });
            }
        }
        let bytes = match (cursor, aggregate_alignment) {
            (Some(cursor), Some(alignment)) => align_up(cursor, alignment).map(|size| ByteLayout {
                size,
                alignment,
                stride: size,
            }),
            _ => None,
        };
        LayoutFacts {
            bytes,
            fields: facts,
        }
    }

    fn aggregate_types<'b, I>(&self, types: I, seen: &mut Vec<String>) -> Option<ByteLayout>
    where
        I: IntoIterator<Item = &'b Type>,
    {
        let fields = types
            .into_iter()
            .enumerate()
            .map(|(index, ty)| (index.to_string(), ty));
        self.aggregate_fields(fields, seen).bytes
    }

    fn type_layout_inner(&self, ty: &Type, seen: &mut Vec<String>) -> Option<ByteLayout> {
        match ty {
            Type::Int | Type::Float => Some(self.scalar_layout(8)),
            Type::Bool => Some(self.scalar_layout(1)),
            Type::Char => Some(self.scalar_layout(4)),
            Type::Float32 => Some(self.scalar_layout(4)),
            Type::IntN { bits, .. } => {
                let size = u64::from(*bits).checked_add(7)? / 8;
                Some(self.scalar_layout(size))
            }
            Type::String | Type::List(_) => Some(ByteLayout {
                size: self.target.pointer_size.checked_mul(3)?,
                alignment: self.target.pointer_alignment,
                stride: self.target.pointer_size.checked_mul(3)?,
            }),
            Type::FixedList { elem, len } => {
                let element = self.type_layout_inner(elem, seen)?;
                let count = len.literal_value()?;
                let size = element.stride.checked_mul(count)?;
                Some(ByteLayout {
                    size,
                    alignment: element.alignment,
                    stride: size,
                })
            }
            Type::InlineRange { base, .. }
            | Type::Tagged { inner: base, .. }
            | Type::Quantity { base, .. } => self.type_layout_inner(base, seen),
            Type::Named(name) => self.named_type_layout(name, seen),
            Type::Apply { name, args } if name == Syntax::TYPE_ATOMIC => {
                self.atomic_layout(args, seen)
            }
            Type::Apply { name, .. } => self.named_type_layout(name, seen),
            Type::Tuple(_)
            | Type::Map { .. }
            | Type::Shared(_)
            | Type::Option(_)
            | Type::Result { .. }
            | Type::Fn { .. }
            | Type::TraitObject(_)
            | Type::Union(_)
            | Type::Measure(_) => None,
        }
    }

    /// `Atomic<T>` is an inline atomic word in compiler-owned records.  Keep
    /// its physical facts independent of `T`'s source width: the runtime
    /// carrier is one 64-bit word on every ratified target.  The checker owns
    /// the public closed-set diagnostic; this guard keeps unknown generic
    /// applications and targets without the lock-free word from receiving
    /// accidental byte facts.
    fn atomic_layout(&self, args: &[Type], _seen: &mut Vec<String>) -> Option<ByteLayout> {
        if !self.target.supports_atomic_word() {
            return None;
        }
        let [inner] = args else {
            return None;
        };
        atomic_scalar_type(inner).then(|| self.scalar_layout(8))
    }

    fn named_type_layout(&self, name: &str, seen: &mut Vec<String>) -> Option<ByteLayout> {
        if let Some(numeric) = numeric_type_from_name(name) {
            return self.type_layout_inner(&numeric, seen);
        }
        match name {
            "Bool" => return Some(self.scalar_layout(1)),
            "Char" => return Some(self.scalar_layout(4)),
            "String" | "List" => {
                return Some(ByteLayout {
                    size: self.target.pointer_size.checked_mul(3)?,
                    alignment: self.target.pointer_alignment,
                    stride: self.target.pointer_size.checked_mul(3)?,
                })
            }
            _ => {}
        }
        if matches!(name, "Unit" | "()") {
            return Some(ByteLayout {
                size: 0,
                alignment: 1,
                stride: 0,
            });
        }
        let item = self.items.iter().find(|item| item_name(item) == name)?;
        match item {
            Item::Struct(definition) => self.struct_facts_inner(definition, seen).bytes,
            Item::Enum(definition) => self.enum_facts(definition).bytes,
            Item::Distinct(definition) => self.type_layout_inner(&definition.base, seen),
            Item::TypeAlias(definition) => self.type_layout_inner(&definition.target, seen),
            _ => None,
        }
    }

    fn scalar_layout(&self, size: u64) -> ByteLayout {
        ByteLayout {
            size,
            alignment: scalar_alignment(size),
            stride: size,
        }
    }

    fn enum_tag_size(&self, tag: CEnumTag) -> u64 {
        match tag {
            CEnumTag::CInt => 4,
            CEnumTag::U8 | CEnumTag::I8 => 1,
            CEnumTag::U16 | CEnumTag::I16 => 2,
            CEnumTag::U32 | CEnumTag::I32 => 4,

            CEnumTag::U64 | CEnumTag::I64 => 8,
        }
    }
}

fn aligned_struct_facts(mut facts: LayoutFacts, alignment: u64) -> LayoutFacts {
    if alignment == 0 || !alignment.is_power_of_two() {
        facts.bytes = None;
        return facts;
    }
    let Some(bytes) = facts.bytes else {
        return facts;
    };
    let Some(size) = align_up(bytes.size, alignment) else {
        facts.bytes = None;
        return facts;
    };
    let alignment = bytes.alignment.max(alignment);
    facts.bytes = Some(ByteLayout {
        size,
        alignment,
        stride: size,
    });
    facts
}

fn item_name(item: &Item) -> &str {
    match item {
        Item::Struct(definition) => &definition.name,
        Item::Enum(definition) => &definition.name,
        Item::Distinct(definition) => &definition.name,
        Item::TypeAlias(definition) => &definition.name,
        _ => "",
    }
}

/// Whether `T` belongs to D-PLACE1/D-ATOMIC-WIDTH1's closed atomic scalar
/// set. `IntN<64>` covers both the signed `I64` and unsigned `U64` spellings;
/// default `Int` remains the exact scalar route.
pub fn atomic_scalar_type(ty: &Type) -> bool {
    match ty {
        Type::Bool | Type::Int => true,
        Type::IntN { bits, .. } => *bits == 32 || *bits == 64,
        Type::Named(name) if name == "Bool" => true,
        Type::Named(name) => numeric_type_from_name(name)
            .is_some_and(|numeric| atomic_scalar_type(&numeric)),
        _ => false,
    }
}

/// Whether `T` supports D-PLACE1's numeric `Atomic.add` operation.
pub fn atomic_add_type(ty: &Type) -> bool {
    atomic_scalar_type(ty)
        && !matches!(ty, Type::Bool)
        && !matches!(ty, Type::Named(name) if name == "Bool")
}
fn scalar_alignment(size: u64) -> u64 {
    match size {
        0 => 1,
        1 | 2 | 4 | 8 => size,
        _ => 8,
    }
}

fn align_up(value: u64, alignment: u64) -> Option<u64> {
    let alignment = alignment.max(1);
    value
        .checked_add(alignment - 1)
        .map(|value| value / alignment * alignment)
}

#[cfg(test)]
mod tests {
    use super::{atomic_scalar_type, ByteLayout, TargetLayout, TargetLayoutEngine};
    use crate::Diagnostics::Span;
    use crate::AST::{Field, Item, StructDef, StructLayout, Type};

    fn field(name: &str, ty: Type) -> Field {
        Field {
            is_pub: true,
            is_package_pub: false,
            name: name.to_string(),
            name_span: Span::new(0, 0),
            ty,
            ty_span: Span::new(0, 0),
            serde_markers: Vec::new(),
            redact: false,
            computed: None,
            default: None,
            default_ct: None,
        }
    }

    fn structure(layout: StructLayout, fields: Vec<Field>) -> StructDef {
        StructDef {
            span: Span::new(0, 0),
            is_pub: true,
            is_package_pub: false,
            name: "Packet".to_string(),
            name_span: Span::new(0, 0),
            type_params: Vec::new(),
            fields,
            state: None,
            methods: Vec::new(),
            cli_bindings: Vec::new(),
            trait_impls: Vec::new(),
            derives: Vec::new(),
            auto_derive_default: false,
            is_published_schema: false,
            published_schema_span: None,
            is_single_use: false,
            single_use_span: None,
            is_must_use: false,
            must_use_span: None,
            layout: Some(layout),
            layout_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
            validate_block: Vec::new(),
            validate_span: None,
        }
    }

    #[test]
    fn c_layout_reports_padding_and_offsets() {
        let definition = structure(
            StructLayout::C,
            vec![
                field(
                    "tag",
                    Type::IntN {
                        signed: false,
                        bits: 8,
                    },
                ),
                field(
                    "value",
                    Type::IntN {
                        signed: false,
                        bits: 64,
                    },
                ),
                field(
                    "tail",
                    Type::IntN {
                        signed: false,
                        bits: 8,
                    },
                ),
            ],
        );
        let engine = TargetLayoutEngine::new(
            std::iter::empty::<&Item>(),
            TargetLayout::from_triple("x86_64-unknown-linux-gnu"),
        );
        let facts = engine.struct_facts(&definition);
        assert_eq!(
            facts.bytes,
            Some(ByteLayout {
                size: 24,
                alignment: 8,
                stride: 24,
            })
        );
        assert_eq!(facts.fields[0].offset, Some(0));
        assert_eq!(facts.fields[1].offset, Some(8));
        assert_eq!(facts.fields[2].offset, Some(16));
    }

    #[test]
    fn aligned_c_layout_uses_requested_alignment_for_size_and_stride() {
        let definition = structure(
            StructLayout::CAligned {
                alignment: 64,
                target: false,
            },
            vec![field(
                "value",
                Type::IntN {
                    signed: true,
                    bits: 32,
                },
            )],
        );
        let engine = TargetLayoutEngine::new(
            std::iter::empty::<&Item>(),
            TargetLayout::from_triple("x86_64-unknown-linux-gnu"),
        );
        let facts = engine.struct_facts(&definition);
        assert_eq!(
            facts.bytes,
            Some(ByteLayout {
                size: 64,
                alignment: 64,
                stride: 64,
            })
        );
        assert_eq!(facts.fields[0].offset, Some(0));
    }

    #[test]
    fn atomic_scalar_set_includes_signed_i64() {
        assert!(atomic_scalar_type(&Type::Int));
        assert!(atomic_scalar_type(&Type::IntN {
            signed: true,
            bits: 64,
        }));
        assert!(atomic_scalar_type(&Type::IntN {
            signed: false,
            bits: 64,
        }));
        assert!(!atomic_scalar_type(&Type::IntN {
            signed: true,
            bits: 16,
        }));
    }

    #[test]
    fn default_layout_keeps_byte_facts_absent() {
        let mut definition = structure(StructLayout::C, vec![field("value", Type::Int)]);
        definition.layout = None;
        let engine = TargetLayoutEngine::new(std::iter::empty::<&Item>(), TargetLayout::host());
        let facts = engine.struct_facts(&definition);
        assert_eq!(facts.bytes, None);
        assert_eq!(facts.fields[0].size, None);
    }

    #[test]
    fn target_pointer_width_changes_physical_facts() {
        let definition = structure(StructLayout::Columnar, vec![field("label", Type::String)]);
        let engine = TargetLayoutEngine::new(
            std::iter::empty::<&Item>(),
            TargetLayout::from_triple("wasm32-unknown-unknown"),
        );
        let facts = engine.struct_facts(&definition);
        assert_eq!(
            facts.bytes,
            Some(ByteLayout {
                size: 12,
                alignment: 4,
                stride: 12,
            })
        );
        assert_eq!(facts.fields[0].size, Some(12));
    }
}
