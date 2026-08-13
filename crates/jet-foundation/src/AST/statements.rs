use super::{BinOp, Binding, Expr, ForKind, LValue, Marker};
use crate::Diagnostics::Span;

/// D-CANVASSTATE1=D: which statement switch a `Stmt::Switched` carries. `#Off`
/// bodies never reach runtime; `#DebugOnly` bodies do, in debug and dev builds
/// only. Every stage asks the retained marker instead of keeping its own flag.
pub fn switched_off(marker: &Marker) -> bool {
    marker.name == crate::Syntax::MARKER_OFF
}

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

/// D-IFGUARD1=A: ordered arm tables without a named subject reuse
/// `Stmt::Switch` with a compiler-private `true` subject at the `if` span.
pub fn is_subjectless_guard(subject: &Expr, span: Span) -> bool {
    matches!(subject, Expr::Bool(true, subject_span) if *subject_span == span)
}

/// Card #1440: does this `if` expression chain end in the parser-synthesized
/// `Expr::NoElse` marker (an else-less all-pattern value dispatch)?
pub fn noelse_terminated(e: &Expr) -> bool {
    let mut cur = e;
    while let Expr::If { else_value, .. } = cur {
        if matches!(else_value.as_ref(), Expr::NoElse(_)) {
            return true;
        }
        cur = else_value;
    }
    false
}

/// Whether a subjectless `Stmt::Switch` came from classic `if condition { ... }`
/// spelling rather than `if { condition -> ... }`.
///
/// The parser intentionally gives both forms one semantic node. Tooling may
/// still need the authored spelling for formatting and edit affordances. Look
/// only at significant source between the `if` token and the first condition;
/// braces inside comments are trivia and must not turn a classic branch into a
/// guard table.
pub fn uses_classic_if_spelling(src: &str, if_span: Span, first_condition: Span) -> bool {
    let Some(gap) = src.get(if_span.end..first_condition.start) else {
        return false;
    };
    let bytes = gap.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b if b.is_ascii_whitespace() => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                let mut depth = 1usize;
                while i < bytes.len() && depth > 0 {
                    if bytes.get(i..i + 2) == Some(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if bytes.get(i..i + 2) == Some(b"*/") {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            b'{' => return false,
            _ => return true,
        }
    }
    true
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
    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: Span,
        /// D-LOOPLABEL3: optional compile-time loop name (`outer :: loop cond { }`).
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
        /// D-LOOPLABEL3: optional compile-time loop name.
        label: Option<(String, Span)>,
    },
    Switch {
        subject: Expr,
        arms: Vec<SwitchArm>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    Break(Span),
    /// D-LOOPSTATE1: `break value` returns one final ordinary-loop value.
    BreakValue(Expr, Span),
    Continue(Span),
    /// D-LOOPLABEL3 + D-ARROW-CONTROL1: `break(name)` / `next(name)`.
    BreakLabel(String, Span),
    /// D-LOOPSTATE1: `break(name, value)` returns from a named ordinary loop.
    BreakLabelValue(String, Span, Expr, Span),
    ContinueLabel(String, Span),
    Loop {
        body: Vec<Stmt>,
        span: Span,
        /// D-LOOPLABEL3: optional compile-time loop name (`outer :: loop { }`).
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
    /// S58 (E2-M13): `#Unsafe("reason") { … }` audited region. `audit` carries
    /// the mandatory-at-source reason; the option remains only for parser
    /// recovery after E3112. `body` is the gated statements.
    Unsafe {
        audit: Option<String>,
        /// Raw source argument retained until sema validates and constant-folds it.
        audit_expr: Option<Expr>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-CTEFFECT1 (ratified 2026-06-25): `#Impure("reason") { … }` — the
    /// audited Tier-2 comptime effect gate. `reason` is the argument of
    /// `#Impure` itself (lint L3102 fires when it is `None`). Both this gate
    /// AND `--gate impure=allow` at build time are required to execute ambient
    /// comptime I/O (FS/Env/Exec/IO). Erases to a plain block at codegen;
    /// the gate is enforced entirely in the comptime interpreter (I3). The
    /// retained reason is the sema recording point for the shared gate ledger
    /// planned by D-FACT-GATE1 / card #1571.
    Impure {
        reason: Option<String>,
        /// Raw source argument retained until sema validates and constant-folds it.
        reason_expr: Option<Expr>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-REACTCORE1 (ratified 2026-06-27, opt D): `#Reactive { … }` in statement
    /// position. Lowers to a reactive effect registration at codegen.
    Reactive {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-SHIELDNAME1=A (ratified 2026-07-11): `#Shield { … }` — a cancellation
    /// shield region. A cancellation or blown deadline pending against the running
    /// task is deferred until the block exits (deadline first, then cancel).
    /// Lowers to `jet_scheduler_shield_enter()` / `_leave()` around `body` with a
    /// RAII guard so `_leave` runs on every exit path including unwind. A no-op
    /// outside a task (SHIELD_DEPTH is thread-local). No arguments.
    Shield {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-CANVASSTATE1=D (ratified 2026-07-09): a statement switch attribute,
    /// `#Off <stmt>` or `#DebugOnly <stmt>`, in either the bare or the block
    /// form. D-VERDICT-1455-1: the written marker is retained, so which switch
    /// this is stays a question about the marker rather than a parse-time
    /// choice of node. `#Off` bodies are parsed and checked but never emitted;
    /// `#DebugOnly` bodies run in debug and dev builds and are stripped from
    /// release output.
    Switched {
        marker: Marker,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-REGION1 / D-BLOCKPLANE1: explicit allocation region
    /// `#Region(r) { … }`. `name` names the region; arena `view`s allocated
    /// inside may not escape it (E0631). A lexical scope like `loop`/`#Unsafe`,
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
    /// D-CONC-SPAWN1=D: `task.group g(limit: n) { … }` — a lexical scope that
    /// owns child tasks. The optional limit bounds concurrently active children.
    /// Emitted as a plain block at codegen; lifetime is enforced in sema (I3).
    TaskGroup {
        name: String,
        name_span: Span,
        limit: Option<Expr>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-LAYOUT1 / D-LAYOUT-GATES1 (ratified 2026-06-28/29) + D-LAYOUT-CTOR1
    /// (D-VERDICT-1306-1): `name :: Layout.{ … }` — a Cassowary-style
    /// constraint typed-literal. Unlike `region`/`task.group`, `name` is declared
    /// in the ENCLOSING scope and outlives the literal (solved values are read
    /// after the layout is defined). The parser desugars each `box.anchor` /
    /// `self.anchor` read inside `body` into a `name.h(box, anchor)` /
    /// `name.v(box, anchor)` method call before sema ever sees it, so every
    /// element is an ordinary `Stmt::Expr`/`Stmt::Val` comparison expression
    /// checked by the general GATE-1/GATE-2 machinery — `body` carries no
    /// layout-specific AST shape of its own. Formatter canonical container
    /// spelling is `self.anchor` when the box id equals the binding name.
    Layout {
        name: String,
        name_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-EFF1 / D-QUAL1: a `#Caps(Net, DB) { … }` effect-restriction region. The
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
    /// `#grant(FS) { caps -> … }`. The listed effects are **authorized** inside
    /// the block via the first-class handle `binding` (here `caps`), and the
    /// capability is **revoked at scope end** by the RAII rule (S63) — the handle
    /// is bound only for the block's extent. The dual of `#Caps`: `#Caps`
    /// *restricts* a region to a set, `#grant` *authorizes* one. An effect used
    /// inside the block that the grant doesn't cover has no capability backing it
    /// (E0712); letting the handle escape (returned, stored, captured) is E0711.
    /// A lexical scope emitted as a plain Rust block — the grant/revoke is a
    /// compile-time capability fact, erased in codegen (I3).
    Grant {
        caps: Vec<(String, Span)>,
        caps_span: Span,
        /// The bound capability handle name (`caps` in `#grant(FS) { caps -> … }`).
        binding: String,
        binding_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-WHEN1/D-WHEN2 (ratified 2026-06-19): `$if <cond> { … } else { … }`.
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
    /// D-OSTARGET2 (=B, ratified 2026-07-03): `$if build.os == { .Linux
    /// -> … .MacOS -> … .Windows -> … [else -> …] }` — the compile-time switch
    /// that lets ungated code reach an OS-gated `impl`. `build.os` is a
    /// compiler-known comptime value; the switch folds to the arm matching the
    /// build's active OS (`ProgramBundle.active_os`), discarding the others
    /// *before* any OS-gating check or full type-check sees them. Sema desugars
    /// this node into a chain of `ComptimeIf` (each arm's condition is the
    /// compile-time constant `build.os == .OS`) as the very first step of
    /// `check_bundle`, so no later sema/codegen pass ever sees this variant —
    /// only the parser, formatter, and semindex do. Arm heads are bare OS
    /// variants (reuses the `Stmt::Switch` arm IR / `SwitchArm`).
    ComptimeSwitch {
        subject: Expr,
        arms: Vec<SwitchArm>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    /// D-META-STAGE1=B (ratified 2026-08-06, card #1537, retires D-CTMARKER1's
    /// splice-only spelling): `$ { … }` — a build-time execution block. Runs
    /// at compile time via the tree-walking comptime interpreter; erases
    /// entirely (no runtime Rust emitted, I3).
    /// Purity-checked (E3401, D-META-EFFECT1 c3) then tree-walked
    /// (E0953/E0956); effect tiers per D-CTEFFECT1. Bindings inside do not
    /// leak to the enclosing scope. `$name` splice (piece 1) deferred to c155.
    ComptimeBlock {
        body: Vec<Stmt>,
        span: Span,
    },

    /// D-CTX1 (ratified 2026-06-22, G2): `#Context(field: value, …) { … }`.
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
    /// `use core.term as term` makes `term.read_key() => Key` available.
    Live {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-DET1 (ratified 2026-06-22): `assume_deterministic { … }` — the expert
    /// determinism-escape block. Inside a `#Pure fn`, the body's determinism
    /// rejections (E3401 impure-call / E3403 non-deterministic Core call) are
    /// **suspended** — the "I know this is deterministic" hatch. A semantic
    /// footgun, v1-legal per the card. A lexical scope emitted as a plain Rust
    /// block; the suppression is a compile-time fact, erased in codegen (I3).
    AssumeDet {
        reason: String,
        /// Raw source argument retained until sema validates and constant-folds it.
        reason_expr: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` — a
    /// transaction block. `name` binds a user-chosen transaction handle (any
    /// lowercase ident, mirroring `region r { … }`) typed `Transaction`.
    /// Inside the block an irreversible effect (Net/FS/Exec) that can't be rolled
    /// back is a compile error (E0746, D-TXN2) — the fix is to move it after the
    /// block or register it via `name.on_commit(() => { … })` (D-TXN3) so it runs
    /// only on a clean commit. `on_commit` lambdas are Drop-backed and run LIFO on
    /// commit, dropped on a `?`-failure/rollback. A lexical scope emitted as a
    /// plain Rust block; effects/transaction state erase (I3).
    Transact {
        /// The user-chosen handle name, or `None` for a bare `#Transact { … }` with
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
    /// (`#Test { … }`). The member `name` resolves against the enclosing
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
    /// Visit every expression in this statement, including expressions in
    /// nested statement bodies. This is used when a parsed compiler-owned
    /// fragment needs its expression diagnostics reanchored to the user item
    /// that caused the fragment to be generated.
    pub fn for_each_expr_mut(&mut self, mut f: impl FnMut(&mut Expr)) {
        fn visit_expr(expr: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
            expr.for_each_expr_mut(f);
        }

        fn visit_body(body: &mut [Stmt], f: &mut impl FnMut(&mut Expr)) {
            for stmt in body {
                visit_stmt(stmt, f);
            }
        }

        fn visit_lvalue(lvalue: &mut LValue, f: &mut impl FnMut(&mut Expr)) {
            match lvalue {
                LValue::Local { .. } => {}
                LValue::Index { base, index, .. } => {
                    visit_expr(base, f);
                    visit_expr(index, f);
                }
                LValue::Field { base, .. } => visit_expr(base, f),
            }
        }

        fn visit_for_kind(kind: &mut ForKind, f: &mut impl FnMut(&mut Expr)) {
            match kind {
                ForKind::Range { start, end, step, .. } => {
                    visit_expr(start, f);
                    visit_expr(end, f);
                    if let Some(step) = step {
                        visit_expr(step, f);
                    }
                }
                ForKind::In { collection, step } => {
                    visit_expr(collection, f);
                    if let Some(step) = step {
                        visit_expr(step, f);
                    }
                }
            }
        }

        fn visit_stmt(stmt: &mut Stmt, f: &mut impl FnMut(&mut Expr)) {
            match stmt {
                Stmt::Expr(expr) => visit_expr(expr, f),
                Stmt::Val(binding) => visit_expr(&mut binding.init, f),
                Stmt::Assign { target, value, .. } => {
                    visit_lvalue(target, f);
                    visit_expr(value, f);
                }
                Stmt::Return(value, _) => {
                    if let Some(value) = value {
                        visit_expr(value, f);
                    }
                }
                Stmt::While { cond, body, .. } => {
                    visit_expr(cond, f);
                    visit_body(body, f);
                }
                Stmt::For { kind, body, .. } => {
                    visit_for_kind(kind, f);
                    visit_body(body, f);
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
                    visit_expr(subject, f);
                    for arm in arms {
                        visit_expr(&mut arm.cond, f);
                        visit_body(&mut arm.body, f);
                    }
                    if let Some(body) = else_body {
                        visit_body(body, f);
                    }
                }
                Stmt::BreakValue(value, _) | Stmt::Yield(value, _) => visit_expr(value, f),
                Stmt::BreakLabelValue(_, _, value, _) => visit_expr(value, f),
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
                | Stmt::Transact { body, .. }
                | Stmt::Layout { body, .. } => visit_body(body, f),
                Stmt::Unsafe {
                    audit_expr, body, ..
                } => {
                    if let Some(expr) = audit_expr {
                        visit_expr(expr, f);
                    }
                    visit_body(body, f);
                }
                Stmt::Impure {
                    reason_expr, body, ..
                } => {
                    if let Some(expr) = reason_expr {
                        visit_expr(expr, f);
                    }
                    visit_body(body, f);
                }
                Stmt::CountedLoop {
                    init,
                    cond,
                    step,
                    body,
                    ..
                } => {
                    visit_expr(&mut init.init, f);
                    visit_expr(cond, f);
                    if let Some(step) = step {
                        visit_stmt(step, f);
                    }
                    visit_body(body, f);
                }
                Stmt::TaskGroup { limit, body, .. } => {
                    if let Some(limit) = limit {
                        visit_expr(limit, f);
                    }
                    visit_body(body, f);
                }
                Stmt::ContextBlock { fields, body, .. } => {
                    for (_, value, _) in fields {
                        visit_expr(value, f);
                    }
                    visit_body(body, f);
                }
                Stmt::AssumeDet {
                    reason_expr, body, ..
                } => {
                    visit_expr(reason_expr, f);
                    visit_body(body, f);
                }
                Stmt::ComptimeIf {
                    cond,
                    then_body,
                    else_body,
                    ..
                } => {
                    visit_expr(cond, f);
                    visit_body(then_body, f);
                    if let Some(body) = else_body {
                        visit_body(body, f);
                    }
                }
                Stmt::ScopeMember { args, body, .. } => {
                    for arg in args {
                        visit_expr(arg, f);
                    }
                    visit_body(body, f);
                }
                Stmt::Break(_)
                | Stmt::Continue(_)
                | Stmt::BreakLabel(..)
                | Stmt::ContinueLabel(..) => {}
            }
        }

        visit_stmt(self, &mut f);
    }

    /// Reanchor a compiler-generated statement fragment to the source item
    /// that requested it. Expression spans are not enough for branch nodes:
    /// subjectless `if` guards use the statement span to distinguish their
    /// compiler-private `true` subject from an authored `if true`.
    pub fn reanchor(&mut self, span: Span) {
        self.for_each_expr_mut(|expr| expr.reanchor(span));

        fn visit(stmt: &mut Stmt, span: Span) {
            match stmt {
                Stmt::Expr(_) | Stmt::Val(_) | Stmt::Assign { .. } => {}
                Stmt::Return(_, current)
                | Stmt::Break(current)
                | Stmt::BreakValue(_, current)
                | Stmt::Continue(current)
                | Stmt::BreakLabel(_, current)
                | Stmt::ContinueLabel(_, current)
                | Stmt::Yield(_, current) => *current = span,
                Stmt::BreakLabelValue(_, _, _, current) => *current = span,
                Stmt::While {
                    body, span: current, ..
                }
                | Stmt::For {
                    body, span: current, ..
                }
                | Stmt::Loop {
                    body, span: current, ..
                }
                | Stmt::Unsafe {
                    body, span: current, ..
                }
                | Stmt::Impure {
                    body, span: current, ..
                }
                | Stmt::Reactive {
                    body, span: current, ..
                }
                | Stmt::Shield {
                    body, span: current, ..
                }
                | Stmt::Switched {
                    body, span: current, ..
                }
                | Stmt::Region {
                    body, span: current, ..
                }
                | Stmt::Policy {
                    body, span: current, ..
                }
                | Stmt::TaskGroup {
                    body, span: current, ..
                }
                | Stmt::Layout {
                    body, span: current, ..
                }
                | Stmt::Caps {
                    body, span: current, ..
                }
                | Stmt::Grant {
                    body, span: current, ..
                }
                | Stmt::ComptimeBlock {
                    body, span: current, ..
                }
                | Stmt::Live {
                    body, span: current, ..
                }
                | Stmt::AssumeDet {
                    body, span: current, ..
                }
                | Stmt::Transact {
                    body, span: current, ..
                } => {
                    *current = span;
                    for child in body {
                        visit(child, span);
                    }
                }
                Stmt::ComptimeIf {
                    then_body,
                    else_body,
                    span: current,
                    ..
                } => {
                    *current = span;
                    for child in then_body {
                        visit(child, span);
                    }
                    if let Some(body) = else_body {
                        for child in body {
                            visit(child, span);
                        }
                    }
                }
                Stmt::Switch {
                    arms,
                    else_body,
                    span: current,
                    ..
                }
                | Stmt::ComptimeSwitch {
                    arms,
                    else_body,
                    span: current,
                    ..
                } => {
                    *current = span;
                    for arm in arms {
                        for child in &mut arm.body {
                            visit(child, span);
                        }
                    }
                    if let Some(body) = else_body {
                        for child in body {
                            visit(child, span);
                        }
                    }
                }
                Stmt::CountedLoop {
                    step,
                    body,
                    span: current,
                    ..
                } => {
                    *current = span;
                    if let Some(step) = step {
                        visit(step, span);
                    }
                    for child in body {
                        visit(child, span);
                    }
                }
                Stmt::ContextBlock {
                    body, span: current, ..
                } => {
                    *current = span;
                    for child in body {
                        visit(child, span);
                    }
                }
                Stmt::ScopeMember {
                    body, span: current, ..
                } => {
                    *current = span;
                    for child in body {
                        visit(child, span);
                    }
                }
            }
        }

        visit(self, span);
    }

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
            | Stmt::BreakValue(_, span)
            | Stmt::Continue(span)
            | Stmt::BreakLabel(_, span)
            | Stmt::BreakLabelValue(_, _, _, span)
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
            | Stmt::Switched { span, .. }
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
        }
    }
}
