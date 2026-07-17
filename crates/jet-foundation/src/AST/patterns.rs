use super::{CallArg, CtValue, Expr, MetaAttr, Type};
use crate::Diagnostics::Span;

/// D-PATW / D-PATR (ratified 2026-06-19): a single payload slot inside a variant pattern.
/// `Active(_)` — wildcard (D-PATW); `Closing(500..599)` — range (D-PATR).
#[derive(Debug, Clone)]
pub enum PatSlot {
    /// D-PATW: `_` in payload position — ignore this field, bind nothing.
    Wildcard,
    /// Regular name binding: `Active(id)`.
    Bind(String),
    /// D-PATR: `lo..hi` range in payload slot (inclusive). Field type must be Int or Char.
    Range { lo: i64, hi: i64 },
}

impl PatSlot {
    /// Returns the binding name if this is a `Bind` slot, else `None`.
    pub fn as_bind(&self) -> Option<&str> {
        if let PatSlot::Bind(s) = self {
            Some(s)
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
        span: Span,
    },
    Present {
        binding: String,
        span: Span,
    },
    Absent(Span),
    /// S34: `Ok(binding)` pattern on `T ? E`.
    Ok {
        binding: String,
        span: Span,
    },
    /// S34: `Err(binding)` pattern on `T ? E`.
    Err {
        binding: String,
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
    /// D-BINPAT1 (ratified 2026-07-12, card #506): the byte-mode sibling of
    /// `StrMatch`. A `b"…"` literal in pattern position matches a `[U8]`
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
    /// field is renamed. `None` means bind under the field's own name
    /// (`self.name`). Always `None` for `List`/`Tuple` patterns.
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
/// `[ elems ]` for lists, `( a, b )` for named tuples (S73/S74).
#[derive(Debug, Clone)]
pub enum BindPattern {
    /// `Point.{ x, y } :: p;` — binds a subset of the struct's fields.
    /// D-DESTRUCT1: `rest` is `Some(span)` of a trailing `..` — MANDATORY
    /// whenever `fields` doesn't name every field of the struct (E0326); a
    /// `..` on a pattern that already names every field is E0327.
    Struct {
        type_name: String,
        type_span: Span,
        fields: Vec<BindName>,
        rest: Option<Span>,
        span: Span,
    },
    /// `[a, b] :: xs` — binds list elements by position.
    List { elems: Vec<BindName>, span: Span },
    /// `(x, y) :: p` — binds named tuple fields in canonical (sorted) order.
    Tuple { elems: Vec<BindName>, span: Span },
}

impl BindPattern {
    pub fn span(&self) -> Span {
        match self {
            BindPattern::Struct { span, .. }
            | BindPattern::List { span, .. }
            | BindPattern::Tuple { span, .. } => *span,
        }
    }

    /// Every name this pattern brings into scope, in source order.
    pub fn names(&self) -> &[BindName] {
        match self {
            BindPattern::Struct { fields, .. } => fields,
            BindPattern::List { elems, .. } => elems,
            BindPattern::Tuple { elems, .. } => elems,
        }
    }
}

/// S35/D-ORRETURN-ERG1: right-hand side of `expr ?? …`.
#[derive(Debug, Clone)]
pub enum OrFallback {
    Value(Box<Expr>),
    Return(Option<Box<Expr>>, Span),
    Panic {
        name_span: Span,
        args: Vec<CallArg>,
    },
    /// D-ORRETURN-ERG1=B: `expr ?? break` — loop-only, sema-gated.
    Break(Span),
    /// D-ORRETURN-ERG1=B: `expr ?? continue` — loop-only, sema-gated.
    Continue(Span),
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
    /// D-CANVASMETA1=B: `@Meta(...)` facts for Canvas/tooling. Checked by sema;
    /// ignored by codegen.
    pub meta: Option<MetaAttr>,
    pub attrs: Vec<ConstAttr>,
    pub rust_kind: RustConstKind,
    /// S57 (M9.5): `comptime NAME = expr;` — evaluated at compile time.
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
    /// D-PERSIST1: `@Persist` was present before this module-level binding —
    /// its value survives a `jet dev` hot reload instead of resetting
    /// (identity = module path + binding name). Inert in release builds.
    pub is_persist: bool,
    pub persist_span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustConstKind {
    Const,
    Static,
}
