use super::{CallArg, CtValue, Expr, MetaAttr, Stmt, Type};
use crate::Diagnostics::Span;

/// D-PATW / D-PATR (ratified 2026-06-19): a single payload slot inside a variant pattern.
/// `Active(_)` — wildcard (D-PATW); `Closing(500..599)` — range (D-PATR).
#[derive(Debug, Clone)]
pub enum PatSlot {
    /// D-PATW: `_` in payload position — ignore this field, bind nothing.
    Wildcard,
    /// Regular name binding: `Active(id)`.
    Bind { name: String, span: Span },
    /// D-PATR: `lo..hi` range in payload slot (inclusive). Field type must be Int or Char.
    Range { lo: i64, hi: i64 },
    /// S31: a nested pattern in payload position (`.Err(.Low)`,
    /// `.Val(.Rect(w, h))`). Patterns nest to any depth.
    Nested(Box<Pattern>),
    /// D-PAT-NAMED-NEST1=A: one `field: slot` entry of the named payload form
    /// `.Case{field: pat, …}`; a bare `field` is `field: field`. Sema maps
    /// each entry onto the positional slot the field names, so every later
    /// pass sees only positional slots.
    Named {
        field: String,
        field_span: Span,
        slot: Box<PatSlot>,
    },
    /// D-PAT-NAMED-NEST1=A: the trailing `..` of `.Case{field: pat, ..}`;
    /// sema fills every unnamed field with a wildcard.
    Rest(Span),
}

impl PatSlot {
    /// Returns the binding name if this is a `Bind` slot, else `None`.
    pub fn as_bind(&self) -> Option<&str> {
        if let PatSlot::Bind { name, .. } = self {
            Some(name)
        } else {
            None
        }
    }

    /// Returns the exact source span of a binding name, if present.
    pub fn binding_span(&self) -> Option<Span> {
        if let PatSlot::Bind { span, .. } = self {
            Some(*span)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Variant {
        variant: String,
        /// D-PATW/D-PATR: slots can be wildcards or ranges, not just names.
        bindings: Vec<PatSlot>,
        /// D-ENUMDOT1: preserve whether source used the leading-dot spelling
        /// until sema applies contextual Optional/Result normalization.
        leading_dot: bool,
        span: Span,
    },
    /// S31: `.Val(binding)` on `T?`. `inner` holds a nested payload pattern
    /// (`.Val(.Rect(w, h))`); the binding is then `_`.
    Present {
        binding: String,
        binding_span: Span,
        inner: Option<Box<Pattern>>,
        span: Span,
    },
    Absent(Span),
    /// S34: `Ok(binding)` pattern on `T !E`; `inner` as for `Present`.
    Ok {
        binding: String,
        binding_span: Span,
        inner: Option<Box<Pattern>>,
        span: Span,
    },
    /// S34: `Err(binding)` pattern on `T !E`; `inner` as for `Present`
    /// (`.Err(.Low)`).
    Err {
        binding: String,
        binding_span: Span,
        inner: Option<Box<Pattern>>,
        span: Span,
    },
    /// D-PATR (ratified 2026-06-19): range pattern at arm-head level (`0..59 -> "F"`).
    /// Subject must be Int or Char. Open types always still require `else`.
    Range {
        lo: i64,
        hi: i64,
        span: Span,
    },
    /// D-PATO (ratified 2026-06-19): structural or-pattern `A(x) | B(x)`.
    /// All alternatives must bind the same names at the same types (E0317).
    Or(Vec<Pattern>, Span),
    /// D-DESTRUCT1: a struct-shaped dispatch arm head:
    /// `.{ kind: "page", title, .. } -> ...`.
    Struct {
        fields: Vec<StructPatField>,
        rest: Option<Span>,
        span: Span,
    },
    /// D-PARSESTR1: the same interpolation literal that formats a string can
    /// sit in pattern position — matches the fixed text and binds each
    /// `{hole}` to a name (untyped binds `String`; `{hole:Type}` binds `Type`
    /// and is a fallible parse). Always refutable (D-PARSESTR2 amendment):
    /// the literal text might not match, and a typed hole's parse can fail.
    StrMatch {
        parts: Vec<StrMatchPart>,
        span: Span,
    },
    /// D-BINPAT1 / D-UNIFYLIT1=A: the byte-mode sibling of
    /// `StrMatch`. An `[U8].{"…"}` literal in pattern position matches a `[U8]`
    /// subject bit-by-bit — each `{name:U4}` reads a fixed-width bit field,
    /// `be`/`le` picks endianness on a multi-byte read, and a final
    /// `{name:...}` captures the remaining bytes as `[U8]`. Always refutable
    /// (the fixed bytes might not match, and the subject might be too short),
    /// so an `if == {}` table needs an `else` (E0148).
    BinMatch {
        parts: Vec<BinMatchPart>,
        span: Span,
    },
}

/// D-BINPAT1: one piece of a binary pattern literal — fixed bytes to match, or
/// a bit-typed hole to bind.
#[derive(Debug, Clone)]
pub enum BinMatchPart {
    /// Fixed literal bytes that must appear verbatim (byte-aligned).
    Lit(Vec<u8>),
    Hole {
        name: String,
        spec: BinSpec,
        span: Span,
    },
}

/// D-BINPAT1: the shape a binary hole reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinSpec {
    /// `U4`, `U16be`, `U16le` — a fixed-width unsigned bit field. `width` is
    /// in bits, 1..=64. `endian` is `Big`/`Little` for a multi-byte read
    /// (width > 8) and `None` for a single-byte-or-smaller read (width <= 8),
    /// where byte order is irrelevant.
    Bits { width: u8, endian: BinEndian },
    /// `...` — the trailing rest capture, binding the remaining bytes as
    /// `[U8]`. Must be the final part of the pattern (E0968).
    Rest,
}

/// D-BINPAT1: byte order of a multi-byte bit read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinEndian {
    /// No suffix — only valid for a single-byte-or-smaller read (width <= 8).
    None,
    Big,
    Little,
}

/// D-PARSESTR1: one piece of a string-interpolation-literal used as a
/// pattern — fixed text to match, or a hole to bind (optionally typed).
#[derive(Debug, Clone)]
pub enum StrMatchPart {
    Lit(String),
    Hole {
        name: String,
        /// `None` binds `String`; `Some(t)` binds `t` via a fallible parse
        /// from the matched substring (E0148 if unhandled by an `else`).
        ty: Option<Type>,
        span: Span,
    },
}

/// D-DESTRUCT1: one field inside a struct pattern arm head.
#[derive(Debug, Clone)]
pub enum StructPatField {
    /// `field` or `field: local` — bind the field value into the arm body.
    Bind {
        field: String,
        field_span: Span,
        local: String,
        local_span: Span,
    },
    /// `field: value` — require the field to equal this value.
    Value {
        field: String,
        field_span: Span,
        value: Box<Expr>,
    },
}

impl StructPatField {
    pub fn field_name(&self) -> &str {
        match self {
            StructPatField::Bind { field, .. } | StructPatField::Value { field, .. } => field,
        }
    }

    pub fn field_span(&self) -> Span {
        match self {
            StructPatField::Bind { field_span, .. } | StructPatField::Value { field_span, .. } => {
                *field_span
            }
        }
    }
}

/// S74: a single name bound by a destructuring target.
#[derive(Debug, Clone)]
pub struct BindName {
    pub name: String,
    pub span: Span,
    /// D-DESTRUCT1: `severity: sev` — the local binding name when the struct
    /// field (or tuple member) is renamed. `None` means bind under the field's
    /// own name (`self.name`). Always `None` for `List` patterns; a `Tuple`
    /// pattern carries renames only after sema normalizes an inferred record
    /// pattern `{ member: local } :: tuple` into it.
    pub rename: Option<(String, Span)>,
}

impl BindName {
    /// The name actually bound in scope: the rename if present, else the
    /// field/element name itself.
    pub fn local_name(&self) -> &str {
        self.rename
            .as_ref()
            .map(|(n, _)| n.as_str())
            .unwrap_or(&self.name)
    }
}

/// S74: the destructuring target on the left of a `val`/`var` binding.
/// Reuses the existing bracket conventions — `Type { fields }` for structs,
/// `{ fields }` for the inferred record form, `[ elems ]` for lists, and
/// `( a, b )` for named tuples (S73/S74).
#[derive(Debug, Clone)]
pub enum BindPattern {
    /// `Point{ x, y } :: p` — binds a subset of the struct's fields.
    /// D-DESTRUCT1: `rest` is `Some(span)` of a trailing `..` — MANDATORY
    /// whenever `fields` doesn't name every field of the struct (E0326); a
    /// `..` on a pattern that already names every field is E0327.
    /// An empty `type_name` is the inferred record form `{ x, y: local } :: v`:
    /// sema reads the fields from `v`'s checked type, and rewrites the pattern
    /// into `Tuple` when `v` is a tuple (#4605).
    Struct {
        type_name: String,
        type_span: Span,
        fields: Vec<BindName>,
        rest: Option<Span>,
        span: Span,
    },
    /// `[a, b] :: xs` — binds list elements by position.
    List { elems: Vec<BindName>, span: Span },
    /// `(x, y) :: p` — binds named tuple members BY NAME (#4605): every
    /// element names a member of the tuple type, in any order. A positional
    /// destructure whose names are not members is E0314.
    Tuple { elems: Vec<BindName>, span: Span },
    /// D-CHOOSE-TEST1=A: `subject == pattern ?? route` binds the captures from
    /// the successful pattern match after the route has been checked to
    /// diverge. The subject is stored on `Binding::init`; this node stores the
    /// refutable test and its route as one statement-level binding operation.
    Refutable {
        pattern: Pattern,
        fallback: OrFallback,
        names: Vec<BindName>,
        span: Span,
        /// Compiler-generated: sema's D-FLOWTYPE1 rewrite of an exiting
        /// `x == None` guard into `x == .Val(x) ?? { … }`. Only this form
        /// refines its Optional subject in place; a written binding that
        /// reuses the subject's name still shadows it (E0118).
        synthesized: bool,
    },
}

impl BindPattern {
    pub fn span(&self) -> Span {
        match self {
            BindPattern::Struct { span, .. }
            | BindPattern::List { span, .. }
            | BindPattern::Tuple { span, .. }
            | BindPattern::Refutable { span, .. } => *span,
        }
    }

    /// Every name this pattern brings into scope, in source order.
    pub fn names(&self) -> &[BindName] {
        match self {
            BindPattern::Struct { fields, .. } => fields,
            BindPattern::List { elems, .. } => elems,
            BindPattern::Tuple { elems, .. } => elems,
            BindPattern::Refutable { names, .. } => names,
        }
    }
}

/// S35/D-ORRETURN-ERG1: right-hand side of `expr ?? …`.
#[derive(Debug, Clone)]
pub enum OrFallback {
    Value(Box<Expr>),
    /// D-FAIL-BIND1=A: `expr ?? { statements; value-or-exit }`. `None` marks
    /// a body whose final statement diverges; sema provides the fallback-only
    /// ambient `err` binding.
    Block {
        body: Vec<Stmt>,
        value: Option<Box<Expr>>,
        span: Span,
    },
    Return(Option<Box<Expr>>, Span),
    Panic {
        name_span: Span,
        args: Vec<CallArg>,
    },
    /// D-ORRETURN-ERG1=B: `expr ?? break` — loop-only, sema-gated.
    Break(Span),
    /// D-ORRETURN-ERG1=B: `expr ?? next` — loop-only, sema-gated.
    Continue(Span),
    /// D-LOOPLABEL3=A + D-ARROW-CONTROL1: `expr ?? break(label)`.
    BreakLabel(String, Span),
    /// D-LOOPLABEL3=A + D-ARROW-CONTROL1: `expr ?? next(label)`.
    ContinueLabel(String, Span),
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Variant { span, .. }
            | Pattern::Present { span, .. }
            | Pattern::Ok { span, .. }
            | Pattern::Err { span, .. }
            | Pattern::Range { span, .. } => *span,
            Pattern::Absent(span) => *span,
            Pattern::Or(_, span) => *span,
            Pattern::Struct { span, .. } => *span,
            Pattern::StrMatch { span, .. } => *span,
            Pattern::BinMatch { span, .. } => *span,
        }
    }

    /// S31: whether a payload is tested by a nested pattern (`.Err(.Low)`),
    /// so this pattern covers only part of its head variant.
    pub fn has_nested_pattern(&self) -> bool {
        fn slot_nests(slot: &PatSlot) -> bool {
            match slot {
                PatSlot::Nested(_) => true,
                PatSlot::Named { slot, .. } => slot_nests(slot),
                _ => false,
            }
        }
        match self {
            Pattern::Variant { bindings, .. } => bindings.iter().any(slot_nests),
            Pattern::Present { inner, .. }
            | Pattern::Ok { inner, .. }
            | Pattern::Err { inner, .. } => inner.is_some(),
            Pattern::Or(alternatives, _) => alternatives.iter().any(Pattern::has_nested_pattern),
            _ => false,
        }
    }

    /// Capture names introduced when this pattern succeeds. The parser uses
    /// this to turn a pattern test into a binding; sema remains authoritative
    /// for each capture's type and the codegen consumes the normalized pattern.
    pub fn binding_names(&self) -> Vec<BindName> {
        fn slot_names(slot: &PatSlot) -> Vec<BindName> {
            match slot {
                PatSlot::Bind { name, span } => vec![BindName {
                    name: name.clone(),
                    span: *span,
                    rename: None,
                }],
                PatSlot::Nested(inner) => inner.binding_names(),
                PatSlot::Named { slot, .. } => slot_names(slot),
                PatSlot::Wildcard | PatSlot::Range { .. } | PatSlot::Rest(_) => Vec::new(),
            }
        }
        match self {
            Pattern::Variant { bindings, .. } => bindings.iter().flat_map(slot_names).collect(),
            Pattern::Present {
                inner: Some(inner), ..
            }
            | Pattern::Ok {
                inner: Some(inner), ..
            }
            | Pattern::Err {
                inner: Some(inner), ..
            } => inner.binding_names(),
            Pattern::Present {
                binding,
                binding_span,
                ..
            }
            | Pattern::Ok {
                binding,
                binding_span,
                ..
            }
            | Pattern::Err {
                binding,
                binding_span,
                ..
            } => vec![BindName {
                name: binding.clone(),
                span: *binding_span,
                rename: None,
            }],
            Pattern::Or(alternatives, _) => alternatives
                .first()
                .map(Pattern::binding_names)
                .unwrap_or_default(),
            Pattern::Struct { fields, .. } => fields
                .iter()
                .filter_map(|field| match field {
                    StructPatField::Bind {
                        field,
                        field_span,
                        local,
                        local_span,
                    } => Some(BindName {
                        name: field.clone(),
                        span: *field_span,
                        rename: Some((local.clone(), *local_span)),
                    }),
                    StructPatField::Value { .. } => None,
                })
                .collect(),
            Pattern::StrMatch { parts, .. } => parts
                .iter()
                .filter_map(|part| match part {
                    StrMatchPart::Hole { name, span, .. } => Some(BindName {
                        name: name.clone(),
                        span: *span,
                        rename: None,
                    }),
                    StrMatchPart::Lit(_) => None,
                })
                .collect(),
            Pattern::BinMatch { parts, .. } => parts
                .iter()
                .filter_map(|part| match part {
                    BinMatchPart::Hole {
                        name,
                        spec: BinSpec::Bits { .. },
                        span,
                    } => Some(BindName {
                        name: name.clone(),
                        span: *span,
                        rename: None,
                    }),
                    BinMatchPart::Lit(_)
                    | BinMatchPart::Hole {
                        spec: BinSpec::Rest,
                        ..
                    } => None,
                })
                .collect(),
            Pattern::Absent(_) | Pattern::Range { .. } => Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum EnumLitArg {
    Positional(Expr),
    Named { label: String, expr: Expr },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstAttr {
    ForceStatic,
    ForceInline,
}

#[derive(Debug, Clone)]
pub struct ConstDef {
    pub span: Span,
    pub name: String,
    pub name_span: Span,
    pub value: Expr,
    /// 3A / D-PUBPKG1=A: `pub NAME :: v` exports the binding like any other
    /// top-level item; `pub(package)` keeps it inside the package.
    pub is_pub: bool,
    pub is_package_pub: bool,
    /// D-CANVASMETA1=B: `#Meta(...)` facts for Canvas/tooling. Checked by sema;
    /// ignored by codegen.
    pub meta: Option<MetaAttr>,
    pub attrs: Vec<ConstAttr>,
    pub rust_kind: RustConstKind,
    /// S57 (M9.5): `@name :: expr;` — evaluated at compile time.
    pub is_comptime: bool,
    /// Filled by sema for comptime bindings: the evaluated constant value,
    /// serialized to a Rust literal at use sites by codegen.
    pub ct: Option<CtValue>,
    /// Filled by sema for comptime bindings alongside `ct`: the binding's Jet
    /// type. Normally redundant with `ct.jet_type()`, but for a comptime
    /// builtin with a fixed, non-polymorphic return type (e.g. `find(glob)`
    /// always returns `[String]`), this carries that static type even when
    /// the runtime value is an empty collection and `CtValue::jet_type()`
    /// alone can't recover the element type from zero elements. Codegen reads
    /// this to render a correctly-typed empty Rust collection (`Vec::<T>::new()`)
    /// instead of a bare `vec![]`, which rustc rejects as E0282 (I2).
    pub ty: Option<Type>,
    /// D-PERSIST1: `#Persist` was present before this module-level binding —
    /// its value survives a `jet dev` hot reload instead of resetting
    /// (identity = module path + binding name). Inert in release builds.
    pub is_persist: bool,
    pub persist_span: Option<Span>,
    /// D-BIND-BARE1 / D-PERSIST1: `true` for `#Persist name := …`, `false` for
    /// `#Persist name :: …`. Irrelevant for `comptime` / Output consts.
    pub mutable: bool,
    /// D-SHAPE-OUTPUT-CALLABLE1: sema-owned checked output link. `None` for
    /// ordinary constants and for an Output rejected before resolution.
    pub resolved_output: Option<ResolvedOutput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Library,
    Executable,
    Service,
    Check,
    Environment,
    Image,
    Bundle,
    System,
    Fleet,
}

impl OutputKind {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "Library" => Self::Library,
            "Executable" => Self::Executable,
            "Service" => Self::Service,
            "Check" => Self::Check,
            "Environment" => Self::Environment,
            "Image" => Self::Image,
            "Bundle" => Self::Bundle,
            "System" => Self::System,
            "Fleet" => Self::Fleet,
            _ => return None,
        })
    }

    pub fn is_runnable(self) -> bool {
        matches!(self, Self::Executable | Self::Service | Self::Check)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::Executable => "Executable",
            Self::Service => "Service",
            Self::Check => "Check",
            Self::Environment => "Environment",
            Self::Image => "Image",
            Self::Bundle => "Bundle",
            Self::System => "System",
            Self::Fleet => "Fleet",
        }
    }
}

/// Checked identity carried into codegen, dev, and tooling. Names are display
/// facts only; `module` + `definition` are the semantic link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputCallableAuthority {
    SafeJet,
}

impl OutputCallableAuthority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SafeJet => "safe-jet",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedOutput {
    pub address: String,
    pub kind: OutputKind,
    pub output_name: String,
    pub module: usize,
    pub source_path: String,
    pub source_name: String,
    /// Sema-resolved name used by TIR/dev function tables (`module__name` for
    /// an inline module, otherwise the source name).
    pub semantic_name: String,
    /// Fully rendered Rust callable path. Sema freezes it so Rust emission
    /// never performs a second lookup or guesses from the Output expression.
    pub lowered_name: String,
    pub params: Vec<(super::AccessConvention, Type)>,
    pub return_type: Option<Type>,
    /// The sema-owned effective failure fact for the selected callable.
    /// Consumers must project this fact instead of rebuilding it from the
    /// raw return type.
    pub failure_contract: super::FailureContract,
    pub reference: Span,
    pub definition: Span,
    /// Runnable Outputs never grant FFI or unsafe authority. The exact solved
    /// ordinary Jet effect row remains in `effects`.
    pub authority: OutputCallableAuthority,
    pub effects: Vec<String>,
    pub selected: bool,
    pub selection_reason: String,
}

impl ResolvedOutput {
    /// Project the selected callable through the same effective failure fact
    /// used by ordinary callable tooling. Output selection must not create a
    /// second contract interpretation from the raw return type.
    pub fn failure_contract(&self) -> super::FailureContract {
        self.failure_contract.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustConstKind {
    Const,
    Static,
}
