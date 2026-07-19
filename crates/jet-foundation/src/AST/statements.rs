use super::{BinOp, Binding, Expr, ForKind, LValue};
use crate::Diagnostics::Span;

/// One `if`/`else if`/`else` chain.
#[derive(Debug, Clone)]
pub struct IfStmt {
    pub cond: Expr,
    pub then_body: Vec<Stmt>,
    pub else_branch: Option<ElseBranch>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ElseBranch {
    ElseIf(Box<IfStmt>),
    Else(Vec<Stmt>),
}

/// One `switch` arm: a condition and a body (S24).
#[derive(Debug, Clone)]
pub struct SwitchArm {
    pub cond: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// D-IFGUARD1=A: subjectless guard tables reuse `Stmt::Switch` with a
/// compiler-private `true` subject located at the `if` keyword span.
pub fn is_subjectless_guard(subject: &Expr, span: Span) -> bool {
    matches!(subject, Expr::Bool(true, subject_span) if *subject_span == span)
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// A call used for its effect, e.g. `print(x);`.
    Expr(Expr),
    Val(Binding),
    /// `target = e;` (op None) or `target += e;` etc. (op Some, S17).
    Assign {
        target: LValue,
        op: Option<BinOp>,
        op_span: Span,
        value: Expr,
    },
    Return(Option<Expr>, Span),
    If(IfStmt),
    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: Span,
        /// D-LABEL1: optional `@name` loop label (`@outer loop cond { }`).
        label: Option<(String, Span)>,
    },
    /// `for i in a..b` (S22) or `for x in collection` / `for k, v in map` (M5).
    For {
        var: String,
        var_span: Span,
        /// Second binding for `for key, value in map`.
        var2: Option<(String, Span)>,
        kind: ForKind,
        body: Vec<Stmt>,
        span: Span,
        /// D-LABEL1: optional `@name` loop label.
        label: Option<(String, Span)>,
    },
    Switch {
        subject: Expr,
        arms: Vec<SwitchArm>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    /// D-LABEL1: `break @name` / `continue @name` targeting a labeled loop.
    BreakLabel(String, Span),
    ContinueLabel(String, Span),
    Loop {
        body: Vec<Stmt>,
        span: Span,
        /// D-LABEL1: optional `@name` loop label (`@outer loop { }`).
        label: Option<(String, Span)>,
    },
    /// D-LOOP-HEADER2=A: `loop name[: Type] := init; cond [; afterthought] { body }`.
    CountedLoop {
        init: Binding,
        cond: Expr,
        step: Option<Box<Stmt>>,
        body: Vec<Stmt>,
        span: Span,
        label: Option<(String, Span)>,
    },
    /// S58 (E2-M13): `@Unsafe { … }` audited region. `audit` carries the
    /// optional reason argument. D-UNSAFE-REASON1=B: missing reason emits
    /// L3101 but does not block compilation. `body` is the gated statements.
    Unsafe {
        audit: Option<String>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-CTEFFECT1 (ratified 2026-06-25): `@Impure("reason") { … }` — the
    /// audited Tier-2 comptime effect gate. `reason` is the argument of
    /// `@Impure` itself (lint L3102 fires when it is `None`). Both this gate
    /// AND `--allow-impure` at build time are required to execute ambient
    /// comptime I/O (Fs/Env/Exec/Io). Erases to a plain block at codegen;
    /// the gate is enforced entirely in the comptime interpreter (I3).
    Impure {
        reason: Option<String>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-REACTCORE1 (ratified 2026-06-27, opt D): `@Reactive { … }` in statement
    /// position. Lowers to a reactive effect registration at codegen.
    Reactive {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-SHIELDNAME1=A (ratified 2026-07-11): `@Shield { … }` — a cancellation
    /// shield region. A cancellation or blown deadline pending against the running
    /// task is deferred until the block exits (deadline first, then cancel).
    /// Lowers to `jet_scheduler_shield_enter()` / `_leave()` around `body` with a
    /// RAII guard so `_leave` runs on every exit path including unwind. A no-op
    /// outside a task (SHIELD_DEPTH is thread-local). No arguments.
    Shield {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-CANVASSTATE1=D (ratified 2026-07-09): `@Off <stmt>` / `@Off { … }`.
    /// The body is parsed and checked, but never emitted or executed.
    Off {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-CANVASSTATE1=D (ratified 2026-07-09): `@DebugOnly <stmt>` /
    /// `@DebugOnly { … }`. The body runs in debug/dev builds and is stripped
    /// from release output.
    DebugOnly {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-REGION1 / D-BLOCKPLANE1: explicit allocation region
    /// `@Region(r) { … }`. `name` names the region; arena `view`s allocated
    /// inside may not escape it (E0631). A lexical scope like `loop`/`@Unsafe`,
    /// emitted as a plain Rust block — the region bound is enforced entirely in
    /// sema (I3: codegen stays dumb).
    Region {
        name: String,
        name_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-MARK-SCOPE1: lexical scoped policy. Compile-time only; codegen emits its body.
    Policy {
        declarations: Vec<crate::Policy::PolicyDeclaration>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-TASKSCOPE1=A / D-NURSERY1=A: `taskgroup g { … }` — a lexical scope that
    /// owns child tasks. `g.task { … }` spawns; scope exit joins/cancels children.
    /// Emitted as a plain block at codegen; lifetime is enforced in sema (I3).
    TaskGroup {
        name: String,
        name_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-LAYOUT1 / D-LAYOUT-GATES1 (ratified 2026-06-28/29): `layout NAME { … }`
    /// — a Cassowary-style constraint block. Unlike `region`/`taskgroup`, `name`
    /// is declared in the ENCLOSING scope and outlives the block (solved values
    /// are read after the layout is defined). The parser desugars each
    /// `box.anchor` read inside `body` into a `NAME.h(box, anchor)` /
    /// `NAME.v(box, anchor)` method call before sema ever sees it, so every line
    /// is an ordinary `Stmt::Expr`/`Stmt::Bind` comparison expression checked by
    /// the general GATE-1/GATE-2 machinery — `body` carries no layout-specific
    /// AST shape of its own.
    Layout {
        name: String,
        name_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-EFF1 / D-QUAL1: a `@Caps(Net, Db) { … }` effect-restriction region. The
    /// `caps` list is the only effects the body (and everything it transitively
    /// calls) may use; an out-of-set effect is E0741. `caps` names are validated
    /// in sema. A lexical scope emitted as a plain Rust block — the restriction
    /// is enforced entirely in sema (I3: codegen stays dumb, effects erase).
    Caps {
        caps: Vec<(String, Span)>,
        caps_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-SCAP1 (ratified 2026-06-21, opt A): a scoped-capability grant region
    /// `#grant(Fs) { caps -> … }`. The listed effects are **authorized** inside
    /// the block via the first-class handle `binding` (here `caps`), and the
    /// capability is **revoked at scope end** by the RAII rule (S63) — the handle
    /// is bound only for the block's extent. The dual of `@Caps`: `@Caps`
    /// *restricts* a region to a set, `#grant` *authorizes* one. An effect used
    /// inside the block that the grant doesn't cover has no capability backing it
    /// (E0712); letting the handle escape (returned, stored, captured) is E0711.
    /// A lexical scope emitted as a plain Rust block — the grant/revoke is a
    /// compile-time capability fact, erased in codegen (I3).
    Grant {
        caps: Vec<(String, Span)>,
        caps_span: Span,
        /// The bound capability handle name (`caps` in `#grant(Fs) { caps -> … }`).
        binding: String,
        binding_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-WHEN1/D-WHEN2 (ratified 2026-06-19): `comptime if <cond> { … } else { … }`.
    /// The condition is evaluated at compile time; only the selected arm is
    /// type-checked and lowered (D-WHEN2: the dropped arm is name-resolved only).
    /// `else_body` is None when no `else` clause is written (statement position
    /// only; in expression position both arms are required by the caller).
    ComptimeIf {
        cond: Expr,
        cond_span: Span,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
        /// Filled by sema: true if the `then` arm is selected, false if `else`.
        /// None before sema runs.
        selected_then: Option<bool>,
    },
    /// D-OSTARGET2 (=B, ratified 2026-07-03): `comptime if build.os == { .Linux
    /// -> … .Macos -> … .Windows -> … [else -> …] }` — the compile-time switch
    /// that lets ungated code reach an OS-gated `impl`. `build.os` is a
    /// compiler-known comptime value; the switch folds to the arm matching the
    /// build's active OS (`ProgramBundle.active_os`), discarding the others
    /// *before* any OS-gating check or full type-check sees them. Sema desugars
    /// this node into a chain of `ComptimeIf` (each arm's condition is the
    /// compile-time constant `build.os == .Os`) as the very first step of
    /// `check_bundle`, so no later sema/codegen pass ever sees this variant —
    /// only the parser, formatter, and semindex do. Arm heads are bare OS
    /// variants (reuses the `Stmt::Switch` arm IR / `SwitchArm`).
    ComptimeSwitch {
        subject: Expr,
        arms: Vec<SwitchArm>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    /// D-CTMARKER1 (ratified 2026-06-25, piece 2): `comptime { … }` — a
    /// build-time execution block. Runs at compile time via the tree-walking
    /// comptime interpreter; erases entirely (no runtime Rust emitted, I3).
    /// Pure-only in Stage A (D-CTCORE1 whitelist + E0951/E0958/E0953/E0956);
    /// effect tiers (D-CTEFFECT1) wire in c157. Bindings inside do not leak to
    /// the enclosing scope. `$name` splice (piece 1) deferred to c155.
    ComptimeBlock {
        body: Vec<Stmt>,
        span: Span,
    },

    /// D-CTX1 (ratified 2026-06-22, G2): `@Context(field: value, …) { … }`.
    /// Swaps named ambient fields for the block's lexical+dynamic extent, then
    /// restores them on all exit paths (return, break, ?, panic unwind) via
    /// a RAII guard. Expert-tier; never surfaced in beginner diagnostics.
    /// v1 fields: `allocator` (allocator handle), `logger` (logger handle),
    /// `deadline` (absolute epoch-millis Int budget).
    /// Q1 = A2: an explicit allocator arg at a call site overrides the ambient.
    /// Q2 = Cβ: restore is per-block (on guard Drop).
    ContextBlock {
        /// `(field_name, value_expr, field_span)` — one entry per `field: value`.
        fields: Vec<(String, Expr, Span)>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-TERM1 (ratified 2026-06-22): `live { … }` — enter un-buffered/no-echo
    /// terminal input mode for the body, restore on every exit (normal, `return`,
    /// `?`, and panic) via the D-DEFER1 scope-guard mechanism.
    /// `use core.term as term` makes `term.read_key() -> Key` available.
    Live {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-DET1 (ratified 2026-06-22): `assume_deterministic { … }` — the expert
    /// determinism-escape block. Inside a `@Pure fn`, the body's determinism
    /// rejections (E3401 impure-call / E3403 non-deterministic Core call) are
    /// **suspended** — the "I know this is deterministic" hatch. A semantic
    /// footgun, v1-legal per the card. A lexical scope emitted as a plain Rust
    /// block; the suppression is a compile-time fact, erased in codegen (I3).
    AssumeDet {
        reason: String,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-TXN1–D-TXN4 (ratified 2026-06-24): `@Transact(name) { … }` — a
    /// transaction block. `name` binds a user-chosen transaction handle (any
    /// lowercase ident, mirroring `region r { … }`) typed `Transaction`.
    /// Inside the block an irreversible effect (Net/Fs/Exec) that can't be rolled
    /// back is a compile error (E0746, D-TXN2) — the fix is to move it after the
    /// block or register it via `name.on_commit(() => { … })` (D-TXN3) so it runs
    /// only on a clean commit. `on_commit` lambdas are Drop-backed and run LIFO on
    /// commit, dropped on a `?`-failure/rollback. A lexical scope emitted as a
    /// plain Rust block; effects/transaction state erase (I3).
    Transact {
        /// The user-chosen handle name, or `None` for a bare `@Transact { … }` with
        /// no hooks (D-TXN4: a transaction without a handle stays legal). A name is
        /// required only to call `name.on_commit(…)`.
        name: Option<String>,
        name_span: Option<Span>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-STREAMYIELD1: `yield expr` — hand a value to a `Stream<T>` consumer
    /// and suspend until the next pull. Legal only in a function whose return
    /// type is `Stream<T>` (E0805 otherwise); `expr: T` (E0807 otherwise).
    Yield(Expr, Span),
    /// D-DOTSCOPE1: a contextual scope-member statement — `.name { … }` /
    /// `.name(args) { … }` in statement position inside a marker block
    /// (`@Test { … }`). The member `name` resolves against the enclosing
    /// marker's declared vocabulary (`Syntax::scope_members`); using one
    /// outside such a block is E0615, an unknown member E0614. The required
    /// trailing block separates it from a leading-dot enum value (D-ENUMDOT1)
    /// and the ident-after-dot separates it from `.{ }` construction (S74).
    ScopeMember {
        /// The member name after the dot (`setup`, `expect_fail`, `timeout`,
        /// `skip`).
        name: String,
        name_span: Span,
        /// Call-style args, when written (`.timeout(500ms)`, `.skip("why")`).
        args: Vec<Expr>,
        /// The span of the whole `(…)` arg group, for arg-shape diagnostics.
        args_span: Option<Span>,
        /// The required trailing `{ … }` block body.
        body: Vec<Stmt>,
        /// The leading `.` position, anchoring the outside-scope error.
        dot_span: Span,
        span: Span,
    },
}

impl Stmt {
    /// The source span this statement occupies, used by the source-level
    /// debugger (D-DBG3) to resolve a Jet line for a breakpoint or `<- here`
    /// caret. For statements that carry no explicit `span` field, this falls
    /// back to the span of the expression/sub-part that anchors them.
    pub fn span(&self) -> Span {
        match self {
            Stmt::Expr(e) => e.span(),
            Stmt::Val(b) => b.name_span,
            Stmt::Assign { target, .. } => target.span(),
            Stmt::Return(_, span)
            | Stmt::Break(span)
            | Stmt::Continue(span)
            | Stmt::BreakLabel(_, span)
            | Stmt::ContinueLabel(_, span)
            | Stmt::While { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Switch { span, .. }
            | Stmt::Loop { span, .. }
            | Stmt::CountedLoop { span, .. }
            | Stmt::Unsafe { span, .. }
            | Stmt::Impure { span, .. }
            | Stmt::Reactive { span, .. }
            | Stmt::Shield { span, .. }
            | Stmt::Off { span, .. }
            | Stmt::DebugOnly { span, .. }
            | Stmt::Region { span, .. }
            | Stmt::Policy { span, .. }
            | Stmt::TaskGroup { span, .. }
            | Stmt::Layout { span, .. }
            | Stmt::Caps { span, .. }
            | Stmt::Grant { span, .. }
            | Stmt::ComptimeIf { span, .. }
            | Stmt::ComptimeSwitch { span, .. }
            | Stmt::ComptimeBlock { span, .. }
            | Stmt::ContextBlock { span, .. }
            | Stmt::Live { span, .. }
            | Stmt::AssumeDet { span, .. }
            | Stmt::Transact { span, .. }
            | Stmt::ScopeMember { span, .. } => *span,
            Stmt::Yield(_, span) => *span,
            Stmt::If(ifs) => ifs.cond.span(),
        }
    }
}
