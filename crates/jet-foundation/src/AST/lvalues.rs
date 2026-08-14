use super::{BindPattern, CtValue, Expr, Marker, StrPart, Type};
use crate::Diagnostics::Span;

/// Assignment target: local name or indexed collection slot (M5).
#[derive(Debug, Clone)]
pub enum LValue {
    Local {
        name: String,
        name_span: Span,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
        /// Filled by sema (like `Expr::Index`) so codegen picks the right
        /// runtime helper for `xs[i] = v` vs `m[k] = v`.
        kind: IndexKind,
    },
    /// D-MUTSELF1: a field-assignment target `place.field = v`. The headline use is
    /// `self.field = v` inside a `mut self` method (lowers to `(*self).field = v` on
    /// the `&mut Self` receiver). `base` is the receiver expression (an `Expr`, not a
    /// nested `LValue`), `field` the member name. Sema gates the root: a field-assign
    /// rooted at a non-`mut` `self` (or any non-changeable place) is E0205.
    Field {
        base: Box<Expr>,
        field: String,
        span: Span,
    },
}

impl LValue {
    /// The source span of an assignment target (for the D-DBG3 debugger line map).
    pub fn span(&self) -> Span {
        match self {
            LValue::Local { name_span, .. } => *name_span,
            LValue::Index { span, .. } | LValue::Field { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum IndexKind {
    #[default]
    Unknown,
    List,
    /// D-RANGE-VALUE1=A: `xs[range]` is a copy slice using one Range value.
    Range,
    /// D-OOBPROOF1 / D-REFINE1: fixed-size list index proven in-bounds by a
    /// range-refined distinct `Int`. Codegen lowers this fact to the shared
    /// checked fixed-list operation; sema still owns the proof that the path is
    /// valid.
    FixedListProof,
    Map,
    /// D-SIMD2: `v[i]` lane access on a SIMD lane type (`F32x4`/`F64x2`). Lowers to a
    /// bounds-checked lane read `{root}jet_math_<T>_lane(&v, i, file, line)`.
    Lane(String),
    /// D-INDEX-HOOK: `mytype[k]` when the type implements `Index` (+ optional `IndexMut`).
    User(String),
    /// D-LAYOUT-FACTS1=B: sema-approved typed selection from `LayoutInfo`.
    /// The selector name is carried so evaluator/codegen do not reinterpret
    /// an unresolved `IndexKind::Unknown` as an ordinary list index.
    LayoutField(String),
    /// D-MEM1 S6: `pool[id]` on a `Pool<T>` — generation-checked slot access, panics
    /// on a stale `Id<T>` (removed/reused slot). Read returns a clone of `T`; write
    /// (plain overwrite or a nested `pool[id].field = v`) goes through a genuine
    /// mutable place (`jet_pool_get_mut`), not a value round-trip.
    Pool,
}

/// D-CANVASMETA1=B: one raw field inside `#Meta(...)`.
#[derive(Debug, Clone)]
pub enum MetaField {
    Category { value: Expr, span: Span },
    Tunable { span: Span },
    Maturity { value: Expr, span: Span },
    Unknown {
        name: String,
        value: Option<Expr>,
        span: Span,
    },
}

/// D-CANVASMETA1=B: tooling metadata attached to a binding or function.
/// Sema validates the raw fields; codegen ignores them.
#[derive(Debug, Clone)]
pub struct MetaAttr {
    pub fields: Vec<MetaField>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaFacts {
    pub category: Option<String>,
    pub tunable: bool,
}

impl MetaAttr {
    pub fn facts(&self) -> MetaFacts {
        let mut out = MetaFacts {
            category: None,
            tunable: false,
        };
        for field in &self.fields {
            match field {
                MetaField::Category { value, .. } => {
                    if let Expr::Str(parts, _) = value {
                        if let [StrPart::Lit(s)] = parts.as_slice() {
                            if !s.is_empty() {
                                out.category = Some(s.clone());
                            }
                        }
                    }
                }
                MetaField::Tunable { .. } => out.tunable = true,
                MetaField::Maturity { .. } => {}
                MetaField::Unknown { .. } => {}
            }
        }
        out
    }
}

impl MetaAttr {
    /// D-MARK-META1=B: extract a valid closed maturity value for `Func` docs.
    pub fn maturity(&self) -> Option<(crate::AST::MaturityTag, Span)> {
        self.fields.iter().find_map(|field| {
            let MetaField::Maturity { value, span } = field else { return None };
            let Expr::EnumLit { type_name, variant, args, .. } = value else { return None };
            if !type_name.is_empty() || !args.is_empty() { return None; }
            let tag = match variant.as_str() {
                crate::Syntax::MARKER_EXPERIMENTAL => crate::AST::MaturityTag::Experimental,
                crate::Syntax::MARKER_TESTED => crate::AST::MaturityTag::Tested,
                crate::Syntax::MARKER_HARDENED => crate::AST::MaturityTag::Hardened,
                _ => return None,
            };
            Some((tag, *span))
        })
    }
}

/// `for i in 1..10` vs `for x in xs` (M5).
#[derive(Debug, Clone)]
pub enum ForKind {
    /// S22 (D-SG8): `start..end` inclusive by default.
    /// D-RANGE-EXCL1=C: `start..<end` is half-open (`exclusive: true`).
    /// Optional third-clause stride is unchanged.
    Range {
        start: Expr,
        end: Expr,
        step: Option<Expr>,
        exclusive: bool,
    },
    In {
        collection: Expr,
        /// D-LOOP-ADVANCE2=A: positive source stride, evaluated once.
        step: Option<Expr>,
    },
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub mutable: bool,
    /// D-VERDICT-1455-1: every marker written on this binding, retained as the
    /// shared reader read it. The parser derives nothing from them; the
    /// accessors below and sema both answer from these nodes.
    pub markers: Vec<Marker>,
    /// D-DATARACE1=C: sema proved this reactive binding crosses a concurrency
    /// boundary (or `#Shared` pinned it). Codegen/report list the upgrade.
    pub reactive_upgrade: bool,
    /// D-CANVASMETA1=B: `#Meta(category: "…", tunable)` for Canvas/tooling.
    pub meta: Option<MetaAttr>,
    pub name: String,
    pub name_span: Span,
    /// Source span of the binding mutability sigil (`::` or `:=`). Sema uses
    /// this exact token span for a typed E0111 replacement.
    pub sigil_span: Option<Span>,
    /// S74: when present, this binding destructures `init` instead of binding
    /// the single `name`. `name` is empty and `name_span` covers the pattern.
    pub pattern: Option<BindPattern>,
    pub ty: Option<Type>,
    pub ty_span: Option<Span>,
    pub init: Expr,
    /// S57 (M9.5): local `@name :: expr;` — immutable, evaluated
    /// after ordinary type checking and emitted as literal data.
    pub is_comptime: bool,
    pub ct: Option<CtValue>,
    /// D-UNINIT-SENTINEL2: `name := Type.{ uninit }`. When set, sema proves
    /// write-before-read (E0420) and codegen lowers to `MaybeUninit`.
    pub uninit: bool,
    /// D-ALLOC2 (ratified 2026-06-21): set by sema when `init` is an
    /// `arena.alloc(value)` call, so this binding holds a scope-bound *view*
    /// into the arena's storage (Rust `&mut T`), not an owned `T`. Codegen
    /// binds it as a reference and dereferences reads; sema (E0631/E0632)
    /// forbids it escaping its arena's scope or outliving a reset/close.
    pub arena_view: bool,
    /// D-MEM1 stage S5 (2026-07-04): set by sema when `init` is a zero-copy
    /// string slicing call (`s.trim()` / `s.after(sep)` / `s.before(sep)`) on
    /// a plain local/param `String` owner, so this binding holds a scope-bound
    /// *view* (`&str`) into the owner's buffer instead of an owned `String` —
    /// the same D-DYNARRAY1 reasoning as `View<T>`, applied to strings since
    /// they have no distinct Jet-level view type (`String` stays one type).
    /// Codegen emits `&str` for the binding and calls the `_view` prelude
    /// helper; sema (E2307) forbids it escaping the owner's scope.
    pub string_view: bool,
    /// D-OPTGC1=A: sema-authored proof that this heap-owning binding crosses
    /// an ownership boundary inside an effective scoped-GC policy. Codegen
    /// consumes this fact verbatim; it never re-runs escape analysis.
    pub gc_promotion: Option<GcPromotion>,
    /// The value already arrives as a compiler-private GC root transferred
    /// from another opted function; no second allocation or trace site.
    pub gc_transferred: bool,
}

impl Binding {
    /// D-VERDICT-1455-1: read a written marker back off the binding. Every
    /// question about a binding-level marker is answered here, from the marker
    /// node itself, so no stage has to keep a parallel flag.
    pub fn marker(&self, name: &str) -> Option<&Marker> {
        self.markers.iter().find(|marker| marker.name == name)
    }

    /// `#Track`: the parser preserves it, sema assigns meaning.
    pub fn track(&self) -> bool {
        self.marker(crate::Syntax::MARKER_TRACK).is_some()
    }

    /// D-DATARACE1=C: `#Local` pin — a reactive box must not cross a task,
    /// channel, or parallel boundary (E1102).
    pub fn reactive_local(&self) -> bool {
        self.marker(crate::Syntax::MARKER_LOCAL).is_some()
    }

    /// D-DATARACE1=C: `#Shared` pin — a reactive box is explicitly synchronized.
    pub fn reactive_shared(&self) -> bool {
        self.marker(crate::Syntax::MARKER_SHARED).is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcPromotion {
    pub span: Span,
    pub scope: String,
    pub policy_provenance: String,
    pub reason: String,
    /// Other promoted bindings directly stored in this payload at creation,
    /// with the payload slot whose later mutation replaces that relation.
    pub edges: Vec<GcPromotionEdge>,
    pub collection_len: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcPromotionEdge {
    pub binding: String,
    pub slot: String,
    pub group: usize,
}
