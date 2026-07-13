//! D-METHODMACRO1=A: checked `@Inline` / `@InlineAlways` contracts.
//!
//! `@Inline` is a soft hint — never rejected here, never even inspected by
//! this module. `@InlineAlways` is a checked promise: if the compiler can
//! prove it genuinely cannot inline the call, that's a compile error naming
//! why (never a silent miss). Three ways a function fails the promise:
//!
//!   - **E0917** self-recursive — inlining a call to itself has no fixed
//!     expansion (would loop forever at compile time or need an artificial
//!     depth cutoff). Checked here: a direct call/method-call to the
//!     function's own name anywhere in its straight-line/control-flow body.
//!     Coverage note: only DIRECT self-recursion is checked (a function
//!     calling itself by name). Mutual recursion between two `@InlineAlways`
//!     functions is NOT checked — that needs a whole-program call graph,
//!     which nothing in sema builds *before* per-function checking runs (the
//!     effect-summary call graph in `Effects.rs` is only complete *after*
//!     every function has been checked once). Out of scope for this card;
//!     flagged here rather than silently narrowed.
//!   - **E0918** address-taken — the function's bare name is used as a VALUE
//!     anywhere in the program (stored in a binding, passed as a callback,
//!     returned), not just called directly. Checked via `inline_addr_taken`,
//!     a whole-program accumulator threaded through `check_func_body`/
//!     `check_func_body_bundle` (see `Checker::inline_addr_taken`) and
//!     populated for free (`CheckerInfer/expr.rs`'s `Expr::Ident` arm already
//!     visits every expression in every function during ordinary type
//!     inference; the one line that resolves a bare name to a global
//!     function's signature is exactly "this name was read as a value").
//!     Methods can't appear in this set — Jet's grammar has no way to take a
//!     method's address as a bare value — so E0918 only ever fires for
//!     top-level functions.
//!   - **E0919** too large — the body exceeds `INLINE_ALWAYS_MAX_STMTS`
//!     statements (counted transitively through nested blocks — `if`/`loop`/
//!     `switch`/etc — but NOT through a nested `Lambda` body, which compiles
//!     to its own separate closure, not inline text of this function).
//!
//! I3: this module only decides; `crates/jet-codegen` emits `#[inline]` /
//! `#[inline(always)]` purely off `Func::is_inline`/`is_inline_always` once
//! this check (and the rest of sema) has passed.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{Binding, ElseBranch, Expr, Func, LValue, Stmt};

/// D-METHODMACRO1=A: the size ceiling named in E0919's fix text. A statement
/// count, not a byte/token count — cheap to compute, easy to explain, and
/// generous enough that no shipped example needs to cross it. Chosen as a
/// round number comfortably above every hand-written helper in the stdlib;
/// revisit with real inlining-cost data if it ever cuts off a legitimate use.
pub const INLINE_ALWAYS_MAX_STMTS: usize = 40;

/// E0917: `@InlineAlways fn {name}` calls itself.
fn e0917_self_recursive(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0917",
        format!("`{name}` calls itself, so `@InlineAlways` cannot expand it"),
        "inlining a recursive call would either loop forever at compile time or require an \
         artificial depth cutoff — neither is a real inline."
            .to_string(),
        "drop `@InlineAlways` (use `@Inline` as a hint), or restructure the function to be \
         non-recursive."
            .to_string(),
        Some(span),
    )
}

/// E0918: `@InlineAlways fn {name}` had its address taken (used as a value)
/// somewhere in the program instead of being called directly.
pub(crate) fn e0918_address_taken(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0918",
        format!("`{name}` cannot be inlined: its address is taken"),
        format!(
            "`@InlineAlways` promises every call to `{name}` expands in place — but `{name}` is \
             also used as a plain value somewhere (stored, returned, or passed as a callback), \
             and a value needs a real function to point at."
        ),
        format!("drop `@InlineAlways`, or call `{name}` directly instead of through a value."),
        Some(span),
    )
}

/// E0919: `@InlineAlways fn {name}` is too large to inline.
fn e0919_too_large(name: &str, stmt_count: usize, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0919",
        format!("`{name}` is too large for `@InlineAlways`"),
        format!(
            "its body has {stmt_count} statements — over the {INLINE_ALWAYS_MAX_STMTS}-statement \
             ceiling `@InlineAlways` enforces so a promised inline doesn't quietly bloat every \
             call site."
        ),
        "drop `@InlineAlways` (use `@Inline` as a hint the compiler is free to ignore), or split \
         the function so the hot part is small enough to inline."
            .to_string(),
        Some(span),
    )
}

/// D-METHODMACRO1=A: the local part of the `@InlineAlways` check — direct
/// self-recursion (E0917) and the size ceiling (E0919). Both are provable
/// from `f` alone; the whole-program address-taken check (E0918) lives in
/// `check_with_mode`/`check_bundle_opts` after every function has run through
/// here once (see module docs).
pub(crate) fn check_inline_always_fn(f: &Func) -> Vec<Diagnostic> {
    let mut scan = InlineAlwaysScan {
        target: &f.name,
        stmt_count: 0,
        self_recursive: false,
    };
    scan.scan_stmts(&f.body);
    let span = f.inline_span.unwrap_or(f.name_span);
    let mut out = Vec::new();
    if scan.self_recursive {
        out.push(e0917_self_recursive(&f.name, span));
    }
    if scan.stmt_count > INLINE_ALWAYS_MAX_STMTS {
        out.push(e0919_too_large(&f.name, scan.stmt_count, span));
    }
    out
}

struct InlineAlwaysScan<'a> {
    target: &'a str,
    stmt_count: usize,
    self_recursive: bool,
}

impl<'a> InlineAlwaysScan<'a> {
    fn scan_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.scan_stmt(s);
        }
    }

    fn scan_binding(&mut self, b: &Binding) {
        self.scan_expr(&b.init);
    }

    fn scan_stmt(&mut self, s: &Stmt) {
        self.stmt_count += 1;
        match s {
            Stmt::Expr(e) => self.scan_expr(e),
            Stmt::Val(b) => self.scan_binding(b),
            Stmt::Assign { target, value, .. } => {
                self.scan_lvalue(target);
                self.scan_expr(value);
            }
            Stmt::Return(e, _) => {
                if let Some(e) = e {
                    self.scan_expr(e);
                }
            }
            Stmt::If(ifs) => {
                self.scan_expr(&ifs.cond);
                self.scan_stmts(&ifs.then_body);
                self.scan_else(ifs.else_branch.as_ref());
            }
            Stmt::While { cond, body, .. } => {
                self.scan_expr(cond);
                self.scan_stmts(body);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    crate::AST::ForKind::Range { start, end, step } => {
                        self.scan_expr(start);
                        self.scan_expr(end);
                        if let Some(step) = step {
                            self.scan_expr(step);
                        }
                    }
                    crate::AST::ForKind::In { collection } => self.scan_expr(collection),
                }
                self.scan_stmts(body);
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
                self.scan_expr(subject);
                for arm in arms {
                    self.scan_expr(&arm.cond);
                    self.scan_stmts(&arm.body);
                }
                if let Some(else_body) = else_body {
                    self.scan_stmts(else_body);
                }
            }
            Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::BreakLabel(_, _)
            | Stmt::ContinueLabel(_, _) => {}
            Stmt::Loop { body, .. } => self.scan_stmts(body),
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                self.scan_binding(init);
                self.scan_expr(cond);
                self.scan_stmt(step);
                self.scan_stmts(body);
            }
            Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Off { body, .. }
            | Stmt::DebugOnly { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::ScopeMember { body, .. }
            | Stmt::Transact { body, .. } => self.scan_stmts(body),
            Stmt::Caps { body, .. } => self.scan_stmts(body),
            Stmt::Grant { body, .. } => self.scan_stmts(body),
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                self.scan_expr(cond);
                self.scan_stmts(then_body);
                if let Some(else_body) = else_body {
                    self.scan_stmts(else_body);
                }
            }
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields {
                    self.scan_expr(e);
                }
                self.scan_stmts(body);
            }
            Stmt::Yield(e, _) => self.scan_expr(e),
        }
    }

    fn scan_else(&mut self, e: Option<&ElseBranch>) {
        match e {
            Some(ElseBranch::ElseIf(ifs)) => {
                self.scan_expr(&ifs.cond);
                self.scan_stmts(&ifs.then_body);
                self.scan_else(ifs.else_branch.as_ref());
            }
            Some(ElseBranch::Else(body)) => self.scan_stmts(body),
            None => {}
        }
    }

    fn scan_lvalue(&mut self, l: &LValue) {
        match l {
            LValue::Local { .. } => {}
            LValue::Index { base, index, .. } => {
                self.scan_expr(base);
                self.scan_expr(index);
            }
            LValue::Field { base, .. } => self.scan_expr(base),
        }
    }

    /// Direct self-recursion only (module docs): a call/method-call reached by
    /// walking this function's own straight-line/control-flow body. Does NOT
    /// descend into a `Lambda` literal's body — that's a separate closure, not
    /// inline text of this function, and a call to `target` from inside one is
    /// no different from any other call site in the program.
    fn scan_expr(&mut self, e: &Expr) {
        match e {
            Expr::Call(c) => {
                if c.name == self.target {
                    self.self_recursive = true;
                }
                for a in &c.args {
                    self.scan_expr(&a.expr);
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                if method == self.target {
                    self.self_recursive = true;
                }
                self.scan_expr(receiver);
                for a in args {
                    self.scan_expr(&a.expr);
                }
            }
            Expr::CallValue { callee, args, .. } => {
                self.scan_expr(callee);
                for a in args {
                    self.scan_expr(&a.expr);
                }
            }
            Expr::Str(parts, _) => {
                for p in parts {
                    if let crate::AST::StrPart::Interp(e, _) = p {
                        self.scan_expr(e.as_ref());
                    }
                }
            }
            Expr::StrMatchLit(_, _) | Expr::BinMatchLit(_, _) => {}
            Expr::Int(_, _, _) | Expr::Float(_, _, _) | Expr::Bool(_, _) | Expr::Char(_, _) => {}
            Expr::ListLit(items, _) => {
                for i in items {
                    self.scan_expr(i);
                }
            }
            Expr::Spread(inner, _) => self.scan_expr(inner),
            Expr::MapLit(entries, _) => {
                for (k, v) in entries {
                    self.scan_expr(k);
                    self.scan_expr(v);
                }
            }
            Expr::Index { base, index, .. } => {
                self.scan_expr(base);
                self.scan_expr(index);
            }
            Expr::Slice {
                base, start, end, ..
            } => {
                self.scan_expr(base);
                self.scan_expr(start);
                self.scan_expr(end);
            }
            Expr::Ident(_, _) => {}
            Expr::Unary(_, inner, _) => self.scan_expr(inner),
            Expr::Binary(_, l, r, _) => {
                self.scan_expr(l);
                self.scan_expr(r);
            }
            Expr::CompareChain { operands, .. } => {
                for o in operands {
                    self.scan_expr(o);
                }
            }
            Expr::UnitLit { .. } => {}
            Expr::Deref(inner, _) | Expr::RawOf(inner, _) | Expr::Copy(inner, _) => {
                self.scan_expr(inner)
            }
            Expr::Field(inner, _, _) => self.scan_expr(inner),
            Expr::OptField { base, .. } => self.scan_expr(base),
            Expr::StructLit { fields, .. } => {
                for (_, _, e) in fields {
                    self.scan_expr(e);
                }
            }
            Expr::EnumLit { args, .. } => {
                for a in args {
                    match a {
                        crate::AST::EnumLitArg::Positional(e) => self.scan_expr(e),
                        crate::AST::EnumLitArg::Named { expr, .. } => self.scan_expr(expr),
                    }
                }
            }
            Expr::Tainted(inner, _) | Expr::Present(inner, _) => self.scan_expr(inner),
            Expr::Absent(_) => {}
            Expr::Todo { .. } => {}
            Expr::ReduceMarker(_, _) => {}
            Expr::PatternTest { subject, .. } => self.scan_expr(subject),
            Expr::Ok(inner, _) | Expr::Err(inner, _) => self.scan_expr(inner),
            Expr::Try(inner, _, _) => self.scan_expr(inner),
            Expr::OrFallback {
                value, fallback, ..
            } => {
                self.scan_expr(value);
                match fallback {
                    crate::AST::OrFallback::Value(e) => self.scan_expr(e),
                    crate::AST::OrFallback::Return(e, _) => {
                        if let Some(e) = e {
                            self.scan_expr(e);
                        }
                    }
                    crate::AST::OrFallback::Panic { args, .. } => {
                        for a in args {
                            self.scan_expr(&a.expr);
                        }
                    }
                    crate::AST::OrFallback::Break(_) | crate::AST::OrFallback::Continue(_) => {}
                }
            }
            Expr::If {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                ..
            } => {
                self.scan_expr(cond);
                self.scan_stmts(then_body);
                self.scan_expr(then_value);
                self.scan_stmts(else_body);
                self.scan_expr(else_value);
            }
            Expr::TupleLit(fields, _, _) => {
                for (_, e) in fields {
                    self.scan_expr(e);
                }
            }
            // A lambda literal compiles to its own separate closure — see the
            // doc comment on `scan_expr`. Not descended into.
            Expr::Lambda(_) => {}
            Expr::PtrFromAddr { addr, .. } => self.scan_expr(addr),
            Expr::FanOut { callee, items, .. } => {
                self.scan_expr(callee);
                for i in items {
                    self.scan_expr(i);
                }
            }
            Expr::ComptimeSplice { .. } => {}
            Expr::Paren(inner, _) => self.scan_expr(inner),
            Expr::IncDec { operand, .. } => self.scan_expr(operand),
        }
    }
}
