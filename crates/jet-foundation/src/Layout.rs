//! Canonical target layout facts for compiler-owned physical layouts.
//!
//! This module deliberately does not ask rustc for answers.  A layout fact is
//! concrete only when Jet owns the representation (`#Layout(c)` or
//! `#Layout(columnar)`); ordinary Rust-layout types remain unknown.  The
//! comptime reflection layer is a formatter for this model, not another
//! layout implementation.

use crate::AST::{
    numeric_type_from_name, CEnumTag, EnumDef, Item, StructDef, StructLayout, Type, VariantPayload,
};

/// The target properties needed by the Jet layout ABI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetLayout {
    /// Canonical target triple shown in reflection and tooling.
    pub triple: String,
    /// Width of a target pointer in bytes.
    pub pointer_size: u64,
    /// ABI alignment of a target pointer in bytes.
    pub pointer_alignment: u64,
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
        {
            4
        } else {
            8
        };
        Self {
            triple,
            pointer_size,
            pointer_alignment: pointer_size,
        }
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
        if !matches!(
            definition.layout,
            Some(StructLayout::C | StructLayout::Columnar)
        ) {
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
        facts
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

fn item_name(item: &Item) -> &str {
    match item {
        Item::Struct(definition) => &definition.name,
        Item::Enum(definition) => &definition.name,
        Item::Distinct(definition) => &definition.name,
        Item::TypeAlias(definition) => &definition.name,
        _ => "",
    }
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
    use super::{ByteLayout, TargetLayout, TargetLayoutEngine};
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
