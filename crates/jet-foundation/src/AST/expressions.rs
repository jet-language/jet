use super::{
    AccessConvention, BinMatchPart, CtValue, EnumLitArg, Func, IndexKind, OrFallback, Param,
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
    /// D-RANGETYPE1: sema sets this on a range-constrained distinct
    /// constructor when it appears under postfix `?`. Codegen then emits the
    /// checked constructor as a `Result`, while the ordinary constructor form
    /// still stays infallible and is rejected for runtime values.
    pub range_checked: bool,
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
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
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
            BinOp::Rem => "%",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }

    /// S17 compound-assignment spelling for this binary op, when one exists.
    pub fn compound_spell(self) -> Option<&'static str> {
        match self {
            BinOp::Add => Some("+="),
            BinOp::Sub => Some("-="),
            BinOp::Mul => Some("*="),
            BinOp::Div => Some("/="),
            BinOp::Rem => Some("%="),
            BinOp::BitAnd => Some("&="),
            BinOp::BitOr => Some("|="),
            BinOp::BitXor => Some("^="),
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
    /// `{value#Debug}` — calls auto-derived or explicit `Debug`.
    Debug,
    /// `{value#Fixed(n)}` — uses `core.fmt.decimal(value, n)`.
    Fixed(i64),
    /// `{value#Unit(name)}` / `{value#Unit(bare)}`.
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
    /// D-TASKBORROW1=A: this lambda is a `taskgroup` child that captures one or
    /// more borrowed places sema proved disjoint. Every tier spawns it through
    /// the scoped path whose loan the group closes at join.
    pub scoped_task_borrow: bool,
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
    /// Lowers to Rust `x.clone()` (E0211 if the type isn't cloneable).
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
        /// for the polymorphic core specials (`math.abs/min/max/clamp`,
        /// `random.pick/shuffle`, `io.eprint`) whose return type is arg-type
        /// dependent and not in `core_fixed_sig`. Total fact read by TIR
        /// lowering so codegen never re-infers it (I3). `None` for every other
        /// call shape (their type comes from a `cx` table or is unused).
        resolved_ret: Option<Type>,
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
    /// S7/S80/D-ERR-CONV: `expr?` — propagates failure.
    /// `TryConvert` records how (if at all) the error type is converted.
    Try(Box<Expr>, Span, TryConvert),
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
    /// D-CTMARKER1=C: `$name` — comptime splice expression. In a comptime
    /// context (derive body, `#Known {}` block, comptime binding RHS), looks
    /// up `name` in the comptime scope. Outside comptime context: E2712.
    /// Inside `emit("… $name …")` strings, `$name` is handled by
    /// `apply_dollar_splices` (string interpolation, not this AST node).
    ComptimeSplice {
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
            | Expr::ReduceMarker(_, s)
            | Expr::Ok(_, s)
            | Expr::Err(_, s)
            | Expr::Try(_, s, _)
            | Expr::OrFallback { span: s, .. }
            | Expr::PatternTest { span: s, .. }
            | Expr::If { span: s, .. }
            | Expr::CallValue { span: s, .. }
            | Expr::PtrFromAddr { span: s, .. }
            | Expr::ComptimeSplice { span: s, .. }
            | Expr::CompareChain { span: s, .. }
            | Expr::UnitLit { span: s, .. }
            | Expr::IncDec { span: s, .. } => *s,
            Expr::Paren(_, s) => *s,
            Expr::Lambda(l) => l.span,
            Expr::Call(c) => c.name_span,
            Expr::MethodCall { method_span, .. } => *method_span,
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
