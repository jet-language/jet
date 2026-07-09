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
    /// D-OOBPROOF1 / D-REFINE1: fixed-size list index proven in-bounds by a
    /// range-refined distinct `Int`. Codegen may emit direct indexing because
    /// sema carried the proof here.
    FixedListProof,
    Map,
    /// D-SIMD2: `v[i]` lane access on a SIMD lane type (`F32x4`/`F64x2`). Lowers to a
    /// bounds-checked lane read `{root}jet_math_<T>_lane(&v, i, file, line)`.
    Lane(String),
    /// D-INDEX-HOOK: `mytype[k]` when the type implements `Index` (+ optional `IndexMut`).
    User(String),
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
    Unknown { name: String, span: Span },
}

/// D-CANVASMETA1=B: tooling metadata attached to a binding, const, or function.
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
                MetaField::Unknown { .. } => {}
            }
        }
        out
    }
}

/// `for i in 1..10` vs `for x in xs` (M5).
#[derive(Debug, Clone)]
pub enum ForKind {
    /// S22 (D-SG8): `start..end` inclusive, with an optional `step n` stride.
    Range {
        start: Expr,
        end: Expr,
        step: Option<Expr>,
    },
    In {
        collection: Expr,
    },
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub mutable: bool,
    /// Binding-level `#Track` marker. Parser/formatter preserve it; sema assigns
    /// meaning in the later tracking slice.
    pub track: bool,
    pub track_span: Option<Span>,
    /// D-CANVASMETA1=B: `#Meta(category: "…", tunable)` for Canvas/tooling.
    pub meta: Option<MetaAttr>,
    pub name: String,
    pub name_span: Span,
    /// S74: when present, this binding destructures `init` instead of binding
    /// the single `name`. `name` is empty and `name_span` covers the pattern.
    pub pattern: Option<BindPattern>,
    pub ty: Option<Type>,
    pub ty_span: Option<Span>,
    pub init: Expr,
    /// S57 (M9.5): local `comptime NAME = expr;` — immutable, evaluated
    /// after ordinary type checking and emitted as literal data.
    pub is_comptime: bool,
    pub ct: Option<CtValue>,
    /// D-UNINIT-SENTINEL1 (ratified 2026-07-02, opt D; supersedes D-UNINIT1's
    /// `#Uninit name: Type` marker spelling): `name: Type := uninit` — an
    /// uninitialized binding, gated by `use core.mem`. `init` is a harmless
    /// placeholder (the `uninit` token's own span, never evaluated); sema
    /// proves write-before-read (E0420) and codegen lowers to `MaybeUninit`.
    /// `false` for every ordinary binding.
    pub uninit: bool,
    /// D-ALLOC2 (ratified 2026-06-21): set by sema when `init` is an
    /// `arena.alloc(value)` call, so this binding holds a scope-bound *view*
    /// into the arena's storage (Rust `&mut T`), not an owned `T`. Codegen
    /// binds it as a reference and dereferences reads; sema (E0631/E0632)
    /// forbids it escaping its arena's scope or outliving a `reset`/`free`.
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
}
