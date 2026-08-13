use super::{
    AccessConvention, BinMatchPart, CallablePolicyChain, CtValue, EnumLitArg, Func, IndexKind, OrFallback, Param,
    Pattern, Stmt, StrMatchPart, TryConvert, Type,
};
use crate::{Diagnostics::Span, Syntax};

#[derive(Debug, Clone)]
pub struct Call {
    pub name: String,
    pub name_span: Span,
    /// D-GENERIC-CALL1=A: optional explicit type arguments on every generic
    /// free call (\`identity<Int>(value)\`). Empty means infer as usual.
    pub type_args: Vec<Type>,
    pub args: Vec<CallArg>,
    /// D-ZIPPAD1: a built-in free zip call carries its resolved result so the
    /// code generator can declare the concrete named row type before lowering.
    /// Ordinary calls leave this unset; method calls already carry the same
    /// fact on `Expr::MethodCall`.
    pub resolved_ret: Option<Type>,
    /// D-RANGETYPE1: sema sets this on a range-constrained distinct
    /// constructor when it appears under postfix `?`. Codegen then emits the
    /// checked constructor as a `Result`, while the ordinary constructor form
    /// still stays infallible and is rejected for runtime values.
    pub range_checked: bool,
    /// D-NUMWIDEN-CROSS1=E / card #1662: sema sets this when `approx(value)`
    /// validates as one integer-to-float precision opt-out. Replaces the
    /// retired `\0numeric.approx_widen` fake-call-name marker. Lowering
    /// consumes it: a surrounding numeric widen folds the crossing in;
    /// otherwise the call erases to its one argument.
    pub widen_approx: bool,
}

#[derive(Debug, Default, Clone)]
pub struct CallArgFlags {
    pub implicit_clone: bool,
    pub shared_auto_clone: bool,
    /// Retired D-TRAILBLOCK1 flag (D-TRAILBLOCK2=A): trailing `{ }` sugar no
    /// longer parses, so this stays false. Kept so older AST snapshots and
    /// defensive formatter paths remain stable.
    pub is_trailing_block: bool,
    /// D-CABI-CALLBACK1: sema proved this argument is a stable C callback symbol.
    pub c_callback_symbol: bool,
    /// D-APILABEL1=A: where the caller wrote this argument, when labels put the
    /// list out of declaration order. The binder rewrites `args` into
    /// declaration order, so lowering needs this to keep the ratified rule that
    /// supplied expressions run left to right in source order. `None` means the
    /// call reads in the order it was written and needs no temporaries.
    pub source_index: Option<usize>,
    /// Declaration slot assigned by the shared binder. Present for supplied
    /// and inserted slots so source-order lowering can materialize one value
    /// for defaults that refer to it.
    pub binder_slot: Option<usize>,
    /// Default-expression references rewritten to compiler-private slot names.
    /// The supplied AST is never copied into a default; lowering resolves these
    /// names to the slot temporary instead.
    pub binder_refs: Vec<(String, usize, Type)>,
    /// Stable call-site identity used to name declaration-slot temporaries.
    pub binder_site: Option<u32>,
    /// D-CALLPOLICY1=E: sema attaches the exact replacement chain to the
    /// callable argument of `apply`; lowering unwraps the value through the
    /// shared callable seam and never rebuilds its signature.
    pub callable_policy: Option<CallablePolicyChain>,
}

#[derive(Debug, Clone)]
pub struct CallArg {
    pub convention: AccessConvention,
    pub expr: Expr,
    pub span: Span,
    pub flags: CallArgFlags,
    /// S61: optional `name:` label at the call site. When present, sema checks
    /// that it matches the parameter name at this position.
    pub label: Option<(String, Span)>,
    /// D-VARIADIC1: `f(...xs)` — expand a list into the remaining parameter slots.
    pub spread: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    /// D-FLOORDIV1=A: infix `/%` divides and rounds the answer down, toward
    /// negative infinity, on whole numbers and on floats alike.
    FloorDiv,
    /// D-MODSEM1=A: infix `%` is the floored modulo — its answer takes the
    /// divisor's sign, so `-7 % 2` is 1. It pairs with `/%`.
    Mod,
    /// D-MODSEM1=A: infix `%%` is the truncated remainder — its answer takes
    /// the dividend's sign, so `-7 %% 2` is -1. It pairs with `/`.
    Rem,
    /// D-EXPOP1=A / D-EXPSEM1=A: infix `^` raises the left side to the power of
    /// the right side. Right-associative, binds tighter than unary minus.
    Pow,
    BitAnd,
    BitOr,
    /// D-XORSPELL1=A: bitwise exclusive-or is infix `~|`, because `^` is power.
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    /// D-CMP3WAY1=B: three-way comparison, desugared to `compare` for hooks.
    Compare,
    And,
    Or,
}

impl BinOp {
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
        )
    }

    /// The user-typed spelling (for diagnostics and codegen).
    pub fn spell(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::FloorDiv => "/%",
            BinOp::Mod => "%",
            BinOp::Rem => "%%",
            BinOp::Pow => "^",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "~|",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::Compare => "<=>",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }

    /// The Rust operator that carries this operation, for generated code.
    /// It matches `spell` everywhere the two languages agree; `~|`
    /// (D-XORSPELL1) is Rust's `^`, and Jet's `%%` is Rust's `%` — both
    /// truncate.
    ///
    /// `None` means Rust has no operator for it and codegen must call the
    /// Prelude instead: `^` (D-EXPSEM1), `/%` (D-FLOORDIV1), and the floored
    /// `%` (D-MODSEM1) are all shaped that way. Returning `None` rather than
    /// panicking keeps the answer a value the caller has to handle.
    pub fn rust_spell(self) -> Option<&'static str> {
        Some(match self {
            BinOp::BitXor => "^",
            BinOp::Rem => "%",
            BinOp::Pow | BinOp::FloorDiv | BinOp::Mod | BinOp::Compare => return None,
            other => other.spell(),
        })
    }

    /// S17 compound-assignment spelling for this binary op, when one exists.
    pub fn compound_spell(self) -> Option<&'static str> {
        match self {
            BinOp::Add => Some("+="),
            BinOp::Sub => Some("-="),
            BinOp::Mul => Some("*="),
            BinOp::Div => Some("/="),
            BinOp::FloorDiv => Some("/%="),
            BinOp::Mod => Some("%="),
            BinOp::Rem => Some("%%="),
            BinOp::Pow => Some("^="),
            BinOp::BitAnd => Some("&="),
            BinOp::BitOr => Some("|="),
            BinOp::BitXor => Some("~|="),
            BinOp::Shl => Some("<<="),
            BinOp::Shr => Some(">>="),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

/// D-INCR1: increment (`Inc`) or decrement (`Dec`) on a mutable integer lvalue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncDecOp {
    Inc,
    Dec,
}

/// D-QUANTITY-PRINT1: explicit unit formatting styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitFormat {
    /// Declared unit symbol, such as `meter` or `px`.
    Symbol,
    /// Generated unit type name, such as `Meter` or `Px`.
    Name,
    /// Numeric magnitude without a unit.
    Bare,
}

/// D-DISPLAYDBG2/D-FMT-INTERP1/D-QUANTITY-PRINT1: how an interpolated value is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrFormat {
    /// Bare `{value}` — calls `Display` (D-DISPLAY-SHAPE).
    #[default]
    Display,
    /// `{value:Debug}` — calls auto-derived or explicit `Debug`.
    Debug,
    /// `{value:Fixed(n)}` — uses `core.fmt.decimal(value, n)`.
    Fixed(i64),
    /// `{value:Unit(name)}` / `{value:Unit(bare)}`.
    Unit(UnitFormat),
}

/// One piece of a string literal (S8): literal text or an interpolated
/// expression.
#[derive(Debug, Clone)]
pub enum StrPart {
    Lit(String),
    Interp(Box<Expr>, StrFormat),
}

/// S46 (M8): one parameter in `(x: Int) => …`.
#[derive(Debug, Clone)]
pub struct LambdaParam {
    pub name: String,
    pub name_span: Span,
    pub ty: Option<Type>,
    pub ty_span: Option<Span>,
}

/// S46: expression or block body after `=>`.
#[derive(Debug, Clone)]
pub enum LambdaBody {
    Expr(Box<Expr>),
    Block(Vec<Stmt>),
}

/// S47/D-ARROW-CONTROL1: capture and escape lowering hints filled by sema.
#[derive(Debug, Clone, Default)]
pub struct LambdaMeta {
    pub escapes: bool,
    pub needs_fn_mut: bool,
    pub mut_captures: Vec<String>,
    pub cloned_captures: Vec<String>,
    pub moved_captures: Vec<String>,
    /// D-LOOPEVAL1: compiler-private zero-parameter closure used to carry one
    /// finite yielding loop through existing expression, comptime, JIT, and
    /// AOT paths. The formatter restores the `loop … -> …` source surface.
    pub collecting_loop: bool,
    /// Item type inferred by sema for the compiler-private collecting closure.
    pub collect_item_type: Option<Type>,
    /// D-LOOPSTATE1: compiler-private carrier for a bare loop expression whose
    /// final value comes from `break value`.
    pub result_loop: bool,
    /// D-CHOOSE-FIND1=A: a finite value loop must be paired with a written
    /// exhaustion route (`?? ...`) before sema can accept the expression.
    pub requires_exhaustion_route: bool,
    /// Closing-brace span used by the exhaustion-route diagnostic.
    pub exhaustion_span: Option<Span>,
    /// Sema has attached and checked the written exhaustion route.
    pub exhaustion_route_attached: bool,
    pub loop_result_type: Option<Type>,
    pub loop_label: Option<(String, Span)>,
    /// D-SHAREDGUARD1=A: sema-validated direct field path for a guard
    /// `map`/`split` projection. Lowering consumes this fact without
    /// re-validating the source lambda.
    pub guard_projection: Option<Vec<String>>,
    /// Sema-inferred relation between callback inputs and any returned views.
    /// TIR and codegen consume this fact without widening the owner set.
    pub return_view_provenance: Option<super::ViewProvenanceMap>,
    /// D-LOCALCELL1=A: sema-proved path for a `guard.map` / `guard.split`
    /// projector. Later tiers consume this fact instead of reinterpreting the
    /// lambda body.
    pub cell_projection_path: Option<Vec<String>>,
}

/// S46/S47 (M8): `(params) => body`; captures are inferred.
#[derive(Debug, Clone)]
pub struct Lambda {
    /// Retired `take(...)` names kept only for one-pass migration diagnostics.
    pub take_names: Vec<(String, Span)>,
    pub params: Vec<LambdaParam>,
    pub body: LambdaBody,
    pub span: Span,
    pub meta: LambdaMeta,
}

/// D-SHAPE-PLACE1=A: checked local access to a maximal place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceAccess {
    Read,
    Write,
}

/// D-DOTCTOR3=A: body of a universal `Type.{ … }` / inferred `.{ … }` literal.
#[derive(Debug, Clone)]
pub enum TypedLitBody {
    /// Record fields: `Point.{ x: 1, y: 2 }` / punning `.{ x, y }`.
    Fields(Vec<(String, Span, Expr)>),
    /// List / fixed-array elements: `[U8].{ 1, 2 }` / `.{ 1, 2 }`.
    Elements(Vec<Expr>),
    /// Map entries: `[String: Int].{ "a": 1 }`.
    Entries(Vec<(Expr, Expr)>),
    /// One expression: scalar `U8.{ 250 }` or assertion `Int.{ fetch_rows() }`.
    Value(Box<Expr>),
    /// Explicit empty: `[T].{}` / `[K: V].{}` / `.{}`.
    Empty,
}

impl TypedLitBody {
    pub fn for_each_expr<'a>(&'a self, mut f: impl FnMut(&'a Expr)) {
        match self {
            TypedLitBody::Fields(fields) => {
                for (_, _, e) in fields {
                    f(e);
                }
            }
            TypedLitBody::Elements(elems) => {
                for e in elems {
                    f(e);
                }
            }
            TypedLitBody::Entries(entries) => {
                for (k, v) in entries {
                    f(k);
                    f(v);
                }
            }
            TypedLitBody::Value(e) => f(e),
            TypedLitBody::Empty => {}
        }
    }

    pub fn for_each_expr_mut(&mut self, mut f: impl FnMut(&mut Expr)) {
        match self {
            TypedLitBody::Fields(fields) => {
                for (_, _, e) in fields {
                    f(e);
                }
            }
            TypedLitBody::Elements(elems) => {
                for e in elems {
                    f(e);
                }
            }
            TypedLitBody::Entries(entries) => {
                for (k, v) in entries {
                    f(k);
                    f(v);
                }
            }
            TypedLitBody::Value(e) => f(e),
            TypedLitBody::Empty => {}
        }
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    /// String literal, possibly with interpolation parts.
    Str(Vec<StrPart>, Span),
    /// D-SHIFT1 (c7shift): a pattern-literal call argument — the sole legal
    /// shape of `cursor.take_pattern("…")`'s argument. Same source syntax as
    /// `Str` (a string literal with `{hole}`/`{hole:Type}` interpolation
    /// holes) but parsed via the D-PARSESTR1 pattern engine (`StrMatchPart`)
    /// instead of ordinary `Expr::Str`, because a typed hole `{id:Int}` is
    /// not a legal interpolation value expression. Legal ONLY as a
    /// `take_pattern` call argument; sema rejects it anywhere else.
    StrMatchLit(Vec<StrMatchPart>, Span),
    /// D-BINPAT1 / D-UNIFYLIT1=A: the byte-mode sibling of
    /// `StrMatchLit` — the sole legal shape of `reader.take_pattern([U8].{"…"})`'s
    /// argument. Same source syntax as a bin-match `Pattern::BinMatch` (an
    /// `[U8].{"…"}` literal with `{name:U<width>}`/`{name:...}` holes), parsed via
    /// the same D-BINPAT1 hole engine (`BinMatchPart`). Legal ONLY as a
    /// `take_pattern` call argument; sema rejects it anywhere else.
    BinMatchLit(Vec<BinMatchPart>, Span),
    /// Integer literal. The third field is the D-SG9 elaborated fixed width
    /// `(signed, bits)`, filled by sema when the literal sits in a sized-integer
    /// context; `None` means the default `Int` (i64). Codegen reads it to pick
    /// the Rust literal suffix.
    /// The fourth field preserves exact lexer spelling for source-authored
    /// literals. Synthesized nodes carry `None`.
    Int(i64, Span, Option<(bool, u8)>, Option<String>),
    /// D-FLOATW1: the bool is `true` when the literal is resolved as F32 in a
    /// typed context (e.g. `x: F32 = 1.5`). `false` = default F64/Float.
    Float(f64, Span, bool),
    Bool(bool, Span),
    /// S41: single-quoted `'a'`.
    Char(char, Span),
    /// S37: `[a, b, c]` or `[]`.
    ListLit(Vec<Expr>, Span),
    /// D-SPREAD1=A: `prefix.[a, b, c]` — member spread. Sema desugars to
    /// `[prefix.a, prefix.b, prefix.c]` (spliced when nested in a list).
    MemberSpread {
        base: Box<Expr>,
        members: Vec<(String, Span)>,
        span: Span,
    },
    /// D-VARIADIC1: `...expr` inside a list literal — flatten the list's elements in place.
    Spread(Box<Expr>, Span),
    /// S38: `["k": v]` or `[:]`.
    MapLit(Vec<(Expr, Expr)>, Span),
    /// S39: `xs[i]` or `m[k]`.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
        /// Filled by sema so codegen picks the right runtime helper.
        kind: IndexKind,
    },
    /// S40/D-SHAPE-PLACE1: range projection `xs[a..b]` or `xs[range]`.
    Slice {
        base: Box<Expr>,
        start: Box<Expr>,
        end: Box<Expr>,
        /// D-RANGE-VALUE1=A: `Some` carries one Range expression. Legacy
        /// literal slices keep direct bounds in `start`/`end`.
        range: Option<Box<Expr>>,
        span: Span,
    },
    /// D-RANGE-VALUE1=A: one nominal `Range` value over `Int`.
    /// `..` is inclusive; `..<` carries a half-open end in the same value.
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        exclusive: bool,
        span: Span,
    },
    Ident(String, Span),
    Call(Call),
    Unary(UnOp, Box<Expr>, Span),
    Binary(BinOp, Box<Expr>, Box<Expr>, Span),
    /// D-CHAINCMP1: a same-direction relational chain `0 <= sev < 10`, any
    /// length ≥ 2 pairs (`operands.len() == ops.len() + 1`). Only `<`/`<=`/
    /// `>`/`>=` chain; `==`/`!=` never appear here (they stay plain `Binary`,
    /// non-chainable). Each shared middle operand is evaluated exactly once —
    /// a lowering fact resolved by TIR (R1), not a parser/sema concern. A
    /// single relational pair stays plain `Binary` (this node only appears for
    /// chains of length ≥ 2 ops).
    CompareChain {
        operands: Vec<Expr>,
        ops: Vec<BinOp>,
        /// D-OPDEF1: pairwise comparisons that dispatch through `Comparable.compare`.
        /// Sema fills this parallel to `ops`; parser-created entries are false.
        hooks: Vec<bool>,
        span: Span,
    },
    /// D-UNITLIT1: a numeric literal with a unit suffix — `500ms`, `12.50usd`.
    /// The lexer only carries the raw value + suffix text (imports aren't
    /// known to it); sema resolves `suffix` against an in-scope `#UnitFamily`
    /// member (PascalCased to its minted distinct-type name) and REWRITES
    /// this node in place to an ordinary distinct-type constructor call
    /// (`Ms(500.0)`) — sugar over the existing distinct-type path, not a new
    /// type or a new TIR/codegen shape (E0134 if the suffix isn't a member in
    /// scope).
    UnitLit {
        /// Exact source digits. Configuration semantics must not recover
        /// quantities from the convenience numeric fields below.
        raw: String,
        int: Option<i64>,
        float: Option<f64>,
        suffix: String,
        suffix_span: Span,
        span: Span,
    },
    /// D-CAP9: postfix `p.*` — dereference a raw pointer. Lowers to Rust `*p`;
    /// gated to `#Unsafe` (E0208). Composes with `.field` as `p.*.field`.
    Deref(Box<Expr>, Span),
    /// D-CAP9: prefix `*x` — take a raw pointer to `x` (raw-pointer-of). Legal
    /// only inside an `#Unsafe` region/fn (E0208). Lowers to `&x as *const _`
    /// inside the gated region.
    RawOf(Box<Expr>, Span),
    /// D-CAP2 (D-MEM1/S4): `copy x` — the one copy verb. Produces a fresh,
    /// independent value from `x` (a temporary; never needs `^`, never trips
    /// E0209). Legal on any expression, most useful on a named binding.
    /// Lowers to the canonical Prelude copy operation for Tensor values and
    /// to ordinary clone/materialization for other cloneable values (E0211 if
    /// the type is not cloneable).
    Copy(Box<Expr>, Span),
    /// Bare place acquisition is elaborated to `Read` by sema; written
    /// `&place` parses as `Write`. This never carries call-argument meaning.
    Place(Box<Expr>, PlaceAccess, Span),
    /// Field access: `v.field`.
    Field(Box<Expr>, String, Span),
    /// S71 (D-SG6): `base?.field` optional chaining. Yields a `T?` and
    /// short-circuits to absent when `base` is absent.
    OptField {
        base: Box<Expr>,
        member: String,
        member_span: Span,
        /// Filled by sema: true when the field type is itself optional, so
        /// codegen flattens (`and_then`) instead of wrapping (`map`).
        flatten: bool,
        span: Span,
    },
    /// Method call: `v.method(args)`.
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        method_span: Span,
        /// Generic arguments on a type receiver, such as `Pool<Int>.new()`.
        /// These belong to the receiver type, not to the method call.
        owner_type_args: Vec<Type>,
        /// D-GENERIC-CALL1=A: call-site type arguments — `decode<Order>(text)` or
        /// any other generic method. Empty for an ordinary call. Jet uses adjacent
        /// angle brackets, not Rust's `::<T>` separator.
        type_args: Vec<Type>,
        args: Vec<CallArg>,
        /// Filled by sema when the method resolves to a user-defined type,
        /// so codegen can apply the parameter conventions (`&`/`&mut`).
        recv_type: Option<String>,
        /// Filled by sema (c109 Phase 20) with the call's resolved return type
        /// for polymorphic core specials and arg-dependent handle methods such
        /// as generic `Rng` draws. Total fact read by TIR lowering so codegen
        /// never re-infers it (I3). `None` when a fixed codegen table owns the
        /// return type or the method is void.
        resolved_ret: Option<Type>,
        /// D-NUMWIDEN-CROSS1=E / card #1662: sema sets this when it
        /// synthesizes this method-call shape as an implicit checked
        /// integer-to-float conversion. Replaces the retired
        /// `\0numeric.checked_widen` fake-`recv_type` marker (`recv_type`
        /// stays `None` on this shape now, so it keeps its one honest
        /// meaning of "resolved user-defined receiver type"). Lowering
        /// consumes it without reconstructing the language rule.
        checked_widen: bool,
    },
    /// D-DOTCTOR1 (ratified 2026-06-25): `Type.{ field: expr, ... }` (named) or
    /// `.{ field: expr, ... }` (inferred — type from context). Replaces the old
    /// dotless `Type { … }` form (E0320). Also: `Type<Args>.{ … }` and
    /// `alias.Type.{ … }` for generic / namespaced structs.
    StructLit {
        type_name: String,
        /// S45: generic args in `Pair<Int>.{ … }`.
        type_args: Vec<Type>,
        /// When set, the struct type lives in the imported module `alias`.
        import_ns: Option<String>,
        /// S48: box as `Box<dyn Trait>` when coerced into a trait-object list.
        as_trait: Option<String>,
        fields: Vec<(String, Span, Expr)>,
        /// `true` for the `.{ … }` inferred form (type resolved by sema from
        /// the expected-type context). `false` for the `Type.{ … }` named form.
        inferred: bool,
        span: Span,
    },
    /// D-DOTCTOR3=A: universal typed-literal head `Type.{ body }` (and inferred
    /// `.{ body }` when the body is not a record field list). Sema elaborates
    /// the body against `head` like an expected-type position and rewrites this
    /// node to the ordinary ListLit / MapLit / StructLit / value shape.
    TypedLit {
        /// `None` for inferred `.{ body }`; `Some` for an explicit head.
        head: Option<Type>,
        body: TypedLitBody,
        span: Span,
    },
    /// S30: `Type.Variant(args)`.
    EnumLit {
        type_name: String,
        variant: String,
        args: Vec<EnumLitArg>,
        /// D-ENUMDOT2: whether source used the contextual leading-dot form.
        /// Canonical `Val`/`None` literals use the same generic node with this
        /// unset, then sema normalizes both spellings to the dedicated nodes.
        leading_dot: bool,
        span: Span,
    },
    /// D-TAG-SURFACE1=A: `#Tag value` attaches a declared value fact. It rides
    /// the value, spreads to derived values, and is checked against the tag's
    /// denied destinations. `#Scrub(Tag)` removes exactly that fact. The tag is
    /// static and **erased in codegen** (I3) — lowering
    /// emits the inner expression unchanged, like `Expr::Present` but unwrapped.
    ///
    /// The direct tag name. `None` exists only while recovering old syntax.
    Tainted(Box<Expr>, Option<String>, Span),
    /// S32: `value(expr)` — present optional.
    Present(Box<Expr>, Span),
    /// S32: bare `null` — absent optional.
    Absent(Span),
    /// D-TOOL2 (E2-M11; D-CASING1): `#Todo` typed hole. Compiles anywhere; panics at
    /// runtime with file, line, and the expected type (filled in by sema).
    Todo {
        span: Span,
        /// The expected type, as a display string — filled by sema.
        expected_type: Option<String>,
    },
    /// Card #1440: the synthesized final arm of an else-less all-pattern value
    /// dispatch (`if subject == { .A -> x  .B -> y }`). Never user-spellable —
    /// only `parse_dispatch_expr` builds it. Sema proves the pattern arms cover
    /// the subject's whole type (E0307 otherwise); codegen emits a diverging
    /// unreachable, exactly like the statement form's dead match arm.
    NoElse(Span),
    /// Internal teaching node for a retired `#Add`/`#Mul`/`#Min`/`#Max`
    /// reduce selector. Canonical calls retain a typed `ReduceOp` enum literal.
    ReduceMarker(String, Span),
    /// S31: `subject == pattern` (stored as dedicated node for sema/codegen).
    PatternTest {
        subject: Box<Expr>,
        pattern: Pattern,
        span: Span,
    },
    /// S34: `Ok(expr)` — success value for `T ? E`.
    Ok(Box<Expr>, Span),
    /// S34: `Err(expr)` — failure value for `T ? E`.
    Err(Box<Expr>, Span),
    /// S7: postfix `?` — propagate a fallible value.
    /// S7/D-FAIL-CTX1: `expr?` — propagates failure and may carry a lazy
    /// string note written immediately after the operator.
    /// `TryConvert` records how (if at all) the error type is converted.
    Try(Box<Expr>, Span, TryConvert, Option<Box<Expr>>),
    /// S35: `value or fallback`.
    OrFallback {
        value: Box<Expr>,
        fallback: OrFallback,
        /// Set during typechecking: `true` when the left side is `T?`.
        is_option: bool,
        span: Span,
    },
    /// S68 (D-SG2): `if` in expression position. Each branch is a block whose
    /// trailing expression (no `;`) is its value; the `else` is required and
    /// both branches share a type. `else if` nests as the else value.
    If {
        cond: Box<Expr>,
        then_body: Vec<Stmt>,
        then_value: Box<Expr>,
        else_body: Vec<Stmt>,
        else_value: Box<Expr>,
        span: Span,
    },
    /// S73 (D-SG7): `(x: 1, y: 2)` — named members only; source order preserved for fmt.
    /// `ty` is filled by sema for codegen (canonical sorted shape).
    TupleLit(Vec<(String, Expr)>, Span, Option<Type>),
    /// S46 (M8): `(params) => expr` or block body.
    Lambda(Lambda),
    /// S47: call any function-valued expression: `f(args)`.
    CallValue {
        callee: Box<Expr>,
        args: Vec<CallArg>,
        span: Span,
    },
    /// S58 (E2-M13): `mem.Ptr<T>.from_addr(addr)` — build a typed pointer from
    /// an integer address. The element type `elem` is the `<T>` argument; the
    /// result type is `Ptr<elem>`. Only legal inside an `#Unsafe` region in a
    /// module that did `use core.mem` (else E3101/E3102).
    PtrFromAddr {
        /// The module alias the call came through (`mem` in the example).
        alias: String,
        alias_span: Span,
        elem: Type,
        addr: Box<Expr>,
        span: Span,
    },
    /// D-META-STAGE1=B (formerly D-CTMARKER1's splice-expression spelling):
    /// a compile-time name, `@limit`. The mark is part of the
    /// name and is written at every mention, so this is an ordinary identifier
    /// read that happens to name a value the compiler already computed. `name`
    /// holds the written text, mark included, and never denotes the same
    /// binding as the unmarked spelling. There is no scope to cross and nothing
    /// to carry: sema folds the value into `value` before codegen, and codegen
    /// emits it as a literal.
    ComptimeName {
        name: String,
        span: Span,
        value: Option<CtValue>,
    },
    /// D-FMTPARENS1=A: explicit author grouping parentheses `(expr)`.
    /// Transparent to type-checking and codegen; formatter always emits the parens.
    Paren(Box<Expr>, Span),
    /// D-INCR1: `++x`/`--x` (prefix) or `x++`/`x--` (postfix). Prefix returns the
    /// updated value; postfix returns the value before the update. Operand must be
    /// a mutable integer lvalue (same LHS policy as S17 compound assignment).
    IncDec {
        op: IncDecOp,
        operand: Box<Expr>,
        postfix: bool,
        span: Span,
    },
}

impl Expr {
    /// D-FMTPARENS1=A: author grouping is transparent to semantic shape.
    /// Keep the unwrapping in the AST so every consumer shares one helper.
    pub fn without_parens(&self) -> &Expr {
        let mut expr = self;
        while let Expr::Paren(inner, _) = expr {
            expr = inner.as_ref();
        }
        expr
    }

    pub fn span(&self) -> Span {
        match self {
            Expr::Str(_, s)
            | Expr::StrMatchLit(_, s)
            | Expr::BinMatchLit(_, s)
            | Expr::Int(_, s, _, _)
            | Expr::Float(_, s, _)
            | Expr::Bool(_, s)
            | Expr::Char(_, s)
            | Expr::ListLit(_, s)
            | Expr::MemberSpread { span: s, .. }
            | Expr::Spread(_, s)
            | Expr::TupleLit(_, s, _)
            | Expr::MapLit(_, s)
            | Expr::Index { span: s, .. }
            | Expr::Slice { span: s, .. }
            | Expr::Range { span: s, .. }
            | Expr::Ident(_, s)
            | Expr::Unary(_, _, s)
            | Expr::Binary(_, _, _, s)
            | Expr::Deref(_, s)
            | Expr::RawOf(_, s)
            | Expr::Copy(_, s)
            | Expr::Place(_, _, s)
            | Expr::Field(_, _, s)
            | Expr::OptField { span: s, .. }
            | Expr::StructLit { span: s, .. }
            | Expr::TypedLit { span: s, .. }
            | Expr::EnumLit { span: s, .. }
            | Expr::Tainted(_, _, s)
            | Expr::Present(_, s)
            | Expr::Absent(s)
            | Expr::Todo { span: s, .. }
            | Expr::NoElse(s)
            | Expr::ReduceMarker(_, s)
            | Expr::Ok(_, s)
            | Expr::Err(_, s)
            | Expr::Try(_, s, _, _)
            | Expr::OrFallback { span: s, .. }
            | Expr::PatternTest { span: s, .. }
            | Expr::If { span: s, .. }
            | Expr::CallValue { span: s, .. }
            | Expr::PtrFromAddr { span: s, .. }
            | Expr::ComptimeName { span: s, .. }
            | Expr::CompareChain { span: s, .. }
            | Expr::UnitLit { span: s, .. }
            | Expr::IncDec { span: s, .. } => *s,
            Expr::Paren(_, s) => *s,
            Expr::Lambda(l) => l.span,
            Expr::Call(c) => c.name_span,
            Expr::MethodCall { method_span, .. } => *method_span,
        }
    }

    /// Move diagnostics for a compiler-generated expression back to the source
    /// construct that requested it. Generated Jet fragments are parsed through
    /// the ordinary parser, so their byte offsets belong to the temporary
    /// fragment rather than the user's file.
    pub fn reanchor(&mut self, span: Span) {
        match self {
            Expr::Str(_, current)
            | Expr::StrMatchLit(_, current)
            | Expr::BinMatchLit(_, current)
            | Expr::Int(_, current, _, _)
            | Expr::Float(_, current, _)
            | Expr::Bool(_, current)
            | Expr::Char(_, current)
            | Expr::ListLit(_, current)
            | Expr::Spread(_, current)
            | Expr::TupleLit(_, current, _)
            | Expr::MapLit(_, current)
            | Expr::Index { span: current, .. }
            | Expr::Slice { span: current, .. }
            | Expr::Range { span: current, .. }
            | Expr::Ident(_, current)
            | Expr::Unary(_, _, current)
            | Expr::Binary(_, _, _, current)
            | Expr::CompareChain { span: current, .. }
            | Expr::Deref(_, current)
            | Expr::RawOf(_, current)
            | Expr::Copy(_, current)
            | Expr::Place(_, _, current)
            | Expr::Field(_, _, current)
            | Expr::EnumLit { span: current, .. }
            | Expr::Tainted(_, _, current)
            | Expr::Present(_, current)
            | Expr::Absent(current)
            | Expr::Todo { span: current, .. }
            | Expr::NoElse(current)
            | Expr::ReduceMarker(_, current)
            | Expr::Ok(_, current)
            | Expr::Err(_, current)
            | Expr::Try(_, current, _, _)
            | Expr::OrFallback { span: current, .. }
            | Expr::PatternTest { span: current, .. }
            | Expr::If { span: current, .. }
            | Expr::CallValue { span: current, .. }
            | Expr::PtrFromAddr { span: current, .. }
            | Expr::ComptimeName { span: current, .. }
            | Expr::UnitLit { span: current, .. }
            | Expr::IncDec { span: current, .. }
            | Expr::Paren(_, current) => *current = span,
            Expr::MemberSpread {
                members,
                span: current,
                ..
            } => {
                *current = span;
                for (_, member_span) in members {
                    *member_span = span;
                }
            }
            Expr::OptField {
                member_span,
                span: current,
                ..
            } => {
                *current = span;
                *member_span = span;
            }
            Expr::StructLit {
                fields,
                span: current,
                ..
            } => {
                *current = span;
                for (_, field_span, _) in fields {
                    *field_span = span;
                }
            }
            Expr::TypedLit { span: current, .. } => *current = span,
            Expr::Call(call) => call.name_span = span,
            Expr::MethodCall { method_span, .. } => *method_span = span,
            Expr::Lambda(lambda) => lambda.span = span,
        }
    }
}

impl Func {
    /// S27: first parameter named `self`.
    pub fn self_param(&self) -> Option<&Param> {
        self.params.first().filter(|p| p.name == Syntax::KW_SELF)
    }

    pub fn is_static_method(&self) -> bool {
        self.self_param().is_none()
    }
}

impl Expr {
    /// Visit this expression and every expression nested in it.  Binder
    /// defaults are ordinary expressions, not a special mini-language: calls,
    /// indexes, collections, literals, lambdas, and typed/enum bodies all use
    /// this one recursive walk.
    pub fn for_each_expr_mut(&mut self, mut f: impl FnMut(&mut Expr)) {
        fn walk(e: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
            f(e);
            match e {
                Expr::Str(parts, _) => {
                    for part in parts {
                        match part {
                            StrPart::Lit(_) => {}
                            StrPart::Interp(inner, _) => walk(inner, f),
                        }
                    }
                }
                Expr::ListLit(items, _) => items.iter_mut().for_each(|item| walk(item, f)),
                Expr::MemberSpread { base, .. }
                | Expr::Spread(base, _)
                | Expr::Deref(base, _)
                | Expr::RawOf(base, _)
                | Expr::Copy(base, _)
                | Expr::Place(base, _, _)
                | Expr::Field(base, _, _)
                | Expr::Present(base, _)
                | Expr::Ok(base, _)
                | Expr::Err(base, _)
                | Expr::Paren(base, _) => walk(base, f),
                Expr::Try(base, _, _, note) => {
                    walk(base, f);
                    if let Some(note) = note {
                        walk(note, f);
                    }
                }
                Expr::MapLit(entries, _) => entries.iter_mut().for_each(|(key, value)| {
                    walk(key, f);
                    walk(value, f);
                }),
                Expr::Index { base, index, .. } => {
                    walk(base, f);
                    walk(index, f);
                }
                Expr::Slice { base, start, end, range, .. } => {
                    walk(base, f);
                    walk(start, f);
                    walk(end, f);
                    if let Some(range) = range {
                        walk(range, f);
                    }
                }
                Expr::Range { start, end, .. } => {
                    walk(start, f);
                    walk(end, f);
                }
                Expr::Call(call) => walk_args(&mut call.args, f),
                Expr::Unary(_, inner, _) => walk(inner, f),
                Expr::Binary(_, lhs, rhs, _) => {
                    walk(lhs, f);
                    walk(rhs, f);
                }
                Expr::CompareChain { operands, .. } => {
                    operands.iter_mut().for_each(|operand| walk(operand, f));
                }
                Expr::OptField { base, .. } => walk(base, f),
                Expr::MethodCall { receiver, args, .. } => {
                    walk(receiver, f);
                    walk_args(args, f);
                }
                Expr::StructLit { fields, .. } => {
                    fields.iter_mut().for_each(|(_, _, value)| walk(value, f));
                }
                Expr::TypedLit { body, .. } => body.for_each_expr_mut(|value| walk(value, f)),
                Expr::EnumLit { args, .. } => {
                    for arg in args {
                        match arg {
                            EnumLitArg::Positional(value)
                            | EnumLitArg::Named { expr: value, .. } => walk(value, f),
                        }
                    }
                }
                Expr::Tainted(inner, _, _) => walk(inner, f),
                Expr::PatternTest { subject, pattern, .. } => {
                    walk(subject, f);
                    walk_pattern(pattern, f);
                }
                Expr::OrFallback { value, fallback, .. } => {
                    walk(value, f);
                    walk_fallback(fallback, f);
                }
                Expr::If {
                    cond,
                    then_body,
                    then_value,
                    else_body,
                    else_value,
                    ..
                } => {
                    walk(cond, f);
                    walk_stmts(then_body, f);
                    walk(then_value, f);
                    walk_stmts(else_body, f);
                    walk(else_value, f);
                }
                Expr::TupleLit(fields, _, _) => {
                    fields.iter_mut().for_each(|(_, value)| walk(value, f));
                }
                Expr::Lambda(lambda) => match &mut lambda.body {
                    LambdaBody::Expr(value) => walk(value, f),
                    LambdaBody::Block(body) => walk_stmts(body, f),
                },
                Expr::CallValue { callee, args, .. } => {
                    walk(callee, f);
                    walk_args(args, f);
                }
                Expr::PtrFromAddr { addr, .. } => walk(addr, f),
                Expr::IncDec { operand, .. } => walk(operand, f),
                Expr::StrMatchLit(..)
                | Expr::BinMatchLit(..)
                | Expr::Int(..)
                | Expr::Float(..)
                | Expr::Bool(..)
                | Expr::Char(..)
                | Expr::Ident(..)
                | Expr::UnitLit { .. }
                | Expr::Absent(..)
                | Expr::Todo { .. }
                | Expr::NoElse(..)
                | Expr::ReduceMarker(..)
                | Expr::ComptimeName { .. } => {}
            }
        }

        fn walk_args(args: &mut [CallArg], f: &mut impl FnMut(&mut Expr)) {
            for arg in args {
                walk(&mut arg.expr, f);
            }
        }

        fn walk_stmts(stmts: &mut [Stmt], f: &mut impl FnMut(&mut Expr)) {
            for stmt in stmts {
                walk_stmt(stmt, f);
            }
        }

        fn walk_stmt(stmt: &mut Stmt, f: &mut impl FnMut(&mut Expr)) {
            match stmt {
                Stmt::Expr(expr) => walk(expr, f),
                Stmt::Val(binding) => walk(&mut binding.init, f),
                Stmt::Assign { target, value, .. } => {
                    walk_lvalue(target, f);
                    walk(value, f);
                }
                Stmt::Return(value, _) => {
                    if let Some(value) = value {
                        walk(value, f);
                    }
                }
                Stmt::While { cond, body, .. } => {
                    walk(cond, f);
                    walk_stmts(body, f);
                }
                Stmt::For { kind, body, .. } => {
                    walk_for_kind(kind, f);
                    walk_stmts(body, f);
                }
                Stmt::Switch {
                    subject,
                    arms,
                    else_body,
                    ..
                }
                | Stmt::ComptimeSwitch {
                    subject,
                    arms,
                    else_body,
                    ..
                } => {
                    walk(subject, f);
                    for arm in arms {
                        walk(&mut arm.cond, f);
                        walk_stmts(&mut arm.body, f);
                    }
                    if let Some(body) = else_body {
                        walk_stmts(body, f);
                    }
                }
                Stmt::BreakValue(value, _) | Stmt::Yield(value, _) => walk(value, f),
                Stmt::BreakLabelValue(_, _, value, _) => walk(value, f),
                Stmt::Loop { body, .. }
                | Stmt::Reactive { body, .. }
                | Stmt::Shield { body, .. }
                | Stmt::Switched { body, .. }
                | Stmt::Region { body, .. }
                | Stmt::Policy { body, .. }
                | Stmt::Caps { body, .. }
                | Stmt::Grant { body, .. }
                | Stmt::ComptimeBlock { body, .. }
                | Stmt::Live { body, .. }
                | Stmt::Transact { body, .. } => walk_stmts(body, f),
                Stmt::Unsafe {
                    audit_expr, body, ..
                } => {
                    if let Some(expr) = audit_expr {
                        walk(expr, f);
                    }
                    walk_stmts(body, f);
                }
                Stmt::Impure {
                    reason_expr, body, ..
                } => {
                    if let Some(expr) = reason_expr {
                        walk(expr, f);
                    }
                    walk_stmts(body, f);
                }
                Stmt::CountedLoop {
                    init,
                    cond,
                    step,
                    body,
                    ..
                } => {
                    walk(&mut init.init, f);
                    walk(cond, f);
                    if let Some(step) = step {
                        walk_stmt(step, f);
                    }
                    walk_stmts(body, f);
                }
                Stmt::TaskGroup { limit, body, .. } => {
                    if let Some(limit) = limit {
                        walk(limit, f);
                    }
                    walk_stmts(body, f);
                }
                Stmt::Layout { body, .. } => walk_stmts(body, f),
                Stmt::ContextBlock { fields, body, .. } => {
                    for (_, value, _) in fields {
                        walk(value, f);
                    }
                    walk_stmts(body, f);
                }
                Stmt::AssumeDet {
                    reason_expr, body, ..
                } => {
                    walk(reason_expr, f);
                    walk_stmts(body, f);
                }
                Stmt::ComptimeIf {
                    cond,
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(cond, f);
                    walk_stmts(then_body, f);
                    if let Some(body) = else_body {
                        walk_stmts(body, f);
                    }
                }
                Stmt::ScopeMember { args, body, .. } => {
                    for arg in args {
                        walk(arg, f);
                    }
                    walk_stmts(body, f);
                }
                Stmt::Break(_)
                | Stmt::Continue(_)
                | Stmt::BreakLabel(..)
                | Stmt::ContinueLabel(..) => {}
            }
        }

        fn walk_lvalue(lvalue: &mut super::LValue, f: &mut impl FnMut(&mut Expr)) {
            match lvalue {
                super::LValue::Local { .. } => {}
                super::LValue::Index { base, index, .. } => {
                    walk(base, f);
                    walk(index, f);
                }
                super::LValue::Field { base, .. } => walk(base, f),
            }
        }

        fn walk_for_kind(kind: &mut super::ForKind, f: &mut impl FnMut(&mut Expr)) {
            match kind {
                super::ForKind::Range {
                    start, end, step, ..
                } => {
                    walk(start, f);
                    walk(end, f);
                    if let Some(step) = step {
                        walk(step, f);
                    }
                }
                super::ForKind::In { collection, step } => {
                    walk(collection, f);
                    if let Some(step) = step {
                        walk(step, f);
                    }
                }
            }
        }

        fn walk_pattern(pattern: &mut Pattern, f: &mut impl FnMut(&mut Expr)) {
            match pattern {
                Pattern::Or(alts, _) => alts.iter_mut().for_each(|alt| walk_pattern(alt, f)),
                Pattern::Struct { fields, .. } => {
                    for field in fields {
                        if let super::StructPatField::Value { value, .. } = field {
                            walk(value, f);
                        }
                    }
                }
                _ => {}
            }
        }

        fn walk_fallback(fallback: &mut OrFallback, f: &mut impl FnMut(&mut Expr)) {
            match fallback {
                OrFallback::Value(value) => walk(value, f),
                OrFallback::Block { body, value, .. } => {
                    walk_stmts(body, f);
                    walk(value, f);
                }
                OrFallback::Return(Some(value), _) => walk(value, f),
                OrFallback::Panic { args, .. } => walk_args(args, f),
                _ => {}
            }
        }

        walk(self, &mut f);
    }
}
