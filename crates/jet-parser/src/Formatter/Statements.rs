use super::*;
use crate::AST::{
    BinOp, BindPattern, Binding, ElseBranch, Expr, ForKind, IfStmt, LValue, Stmt, StrPart,
    SwitchArm,
};

impl<'a> Fmt<'a> {
    fn fmt_statement_switch_attr(&mut self, marker: &str, body: &[Stmt]) {
        if body.len() == 1 {
            self.write(&format!("#{} ", marker));
            self.fmt_stmt(&body[0]);
            return;
        }
        self.write(&format!("#{} {{", marker));
        self.newline();
        self.with_indent(|f| f.fmt_block_stmts(body));
        self.end_block();
    }

    pub(super) fn fmt_block_stmts(&mut self, body: &[Stmt]) {
        for (i, stmt) in body.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            self.emit_leading(stmt_start(stmt));
            self.fmt_stmt(stmt);
            self.emit_trailing(stmt_end(stmt));
        }
    }

    /// D-FMT1: render a single simple statement (no leading indent/newline) for
    /// the inline brace-body path. The caller guarantees `is_simple_stmt`.
    pub(super) fn fmt_stmt_inline(&mut self, stmt: &Stmt) {
        self.fmt_stmt(stmt);
    }

    fn fmt_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(e) => {
                self.fmt_expr(e, Prec::OrFallback);
            }
            Stmt::Val(b) => {
                self.fmt_binding(b);
            }
            Stmt::Assign {
                target, op, value, ..
            } => {
                self.fmt_lvalue(target);
                if let Some(op) = op {
                    self.write(" ");
                    self.write(compound_spell(*op));
                } else {
                    self.write(" =");
                }
                self.write(" ");
                self.fmt_expr(value, Prec::OrFallback);
            }
            Stmt::Return(expr, _) => {
                self.write("return");
                if let Some(e) = expr {
                    self.write(" ");
                    self.fmt_expr(e, Prec::OrFallback);
                }
            }
            Stmt::Yield(e, _) => {
                self.write("yield ");
                self.fmt_expr(e, Prec::OrFallback);
            }
            Stmt::If(i) => self.fmt_if(i),
            Stmt::While {
                cond, body, label, ..
            } => {
                // S19: canonical loop keyword is `loop`. D-LOOPLABEL2=A: `name@ loop`.
                if let Some((_n, _)) = label {
                    self.write(&format!("{}@ ", _n));
                }
                self.write("loop ");
                self.fmt_cond(cond);
                self.write(" {");
                self.fmt_body(body);
            }
            Stmt::For {
                var,
                var2,
                kind,
                body,
                label,
                ..
            } => {
                if let Some((_n, _)) = label {
                    self.write(&format!("{}@ ", _n));
                }
                self.write("loop ");
                self.write(var);
                if let Some((v2, _)) = var2 {
                    self.write(", ");
                    self.write(v2);
                }
                self.write(" in ");
                match kind {
                    ForKind::Range { start, end, step } => {
                        self.fmt_expr(start, Prec::OrFallback);
                        self.write("..");
                        self.fmt_expr(end, Prec::OrFallback);
                        if let Some(step) = step {
                            self.write(&format!(" {} ", Syntax::KW_RANGE_STEP));
                            self.fmt_expr(step, Prec::OrFallback);
                        }
                    }
                    ForKind::In { collection } => {
                        self.fmt_expr(collection, Prec::OrFallback);
                    }
                }
                self.write(" {");
                self.fmt_body(body);
            }
            // D-IF3: multi-arm dispatch renders as `if subject == { head -> body }`
            // (the `Stmt::Switch` IR is shared with the retired `when`). The `==`
            // marker enters dispatch; arm heads carry no repeated `subject ==`.
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                self.write(Syntax::KW_IF);
                self.write(" ");
                self.fmt_dispatch(subject, arms, else_body.as_deref());
            }
            Stmt::Break(_) => self.write("break"),
            Stmt::Continue(_) => self.write("continue"),
            Stmt::BreakLabel(name, _) => self.write(&format!("break {}@", name)),
            Stmt::ContinueLabel(name, _) => self.write(&format!("continue {}@", name)),
            // D-LOOP-SEMICOLON1=A: `loop init; cond; step { body }` — emit verbatim.
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                label,
                ..
            } => {
                if let Some((n, _)) = label {
                    self.write(&format!("{}@ ", n));
                }
                self.write("loop ");
                self.fmt_binding(init);
                self.write("; ");
                self.fmt_cond(cond);
                self.write("; ");
                self.fmt_stmt(step);
                self.write(" {");
                self.fmt_body(body);
            }
            Stmt::Loop {
                body: inner, label, ..
            } => {
                if let Some((_n, _)) = label {
                    self.write(&format!("{}@ ", _n));
                }
                self.write("loop {");
                self.fmt_body(inner);
            }
            Stmt::Unsafe { audit, body, .. } => {
                // D-UNSAFE2: the reason is the argument of `#Unsafe` itself; the
                // separate `#Audit` line is retired.
                match audit {
                    Some(reason) => self.write(&format!(
                        "#{}(\"{}\") {{",
                        Syntax::KW_UNSAFE,
                        escape_str_lit(reason)
                    )),
                    None => self.write(&format!("#{} {{", Syntax::KW_UNSAFE)),
                }
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-CTEFFECT1: `#Impure("reason") { … }` round-trips verbatim.
            Stmt::Impure { reason, body, .. } => {
                match reason {
                    Some(r) => self.write(&format!("#{}(\"{}\") {{", Syntax::KW_IMPURE, r)),
                    None => self.write(&format!("#{} {{", Syntax::KW_IMPURE)),
                }
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-IGNORERET2=A: `#Suppress(MustUse) { … }` round-trips verbatim.
            Stmt::SuppressMustUse { body, .. } => {
                self.write(&format!(
                    "#{}({}) {{",
                    Syntax::ATTR_SUPPRESS,
                    Syntax::SUPPRESS_MUST_USE
                ));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            Stmt::Off { body, .. } => self.fmt_statement_switch_attr(Syntax::ATTR_OFF, body),
            Stmt::DebugOnly { body, .. } => {
                self.fmt_statement_switch_attr(Syntax::ATTR_DEBUG_ONLY, body)
            }
            // D-REACTCORE1: `#Reactive { … }` round-trips verbatim.
            Stmt::Reactive { body, .. } => {
                self.write(&format!("#{} {{", Syntax::KW_REACTIVE));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-SHIELDNAME1=A: `#Shield { … }` round-trips verbatim.
            Stmt::Shield { body, .. } => {
                self.write(&format!("#{} {{", Syntax::KW_SHIELD));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-REGION1 (opt B): `region r { … }`.
            Stmt::Region { name, body, .. } => {
                self.write(&format!("{} {} {{", Syntax::KW_REGION, name));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-TASKSCOPE1=A: `taskgroup g { … }`.
            Stmt::TaskGroup { name, body, .. } => {
                self.write(&format!("{} {} {{", Syntax::KW_TASKGROUP, name));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-LAYOUT1: `layout form { … }`. The parser desugared every
            // `box.anchor` read into a `NAME.h(box, anchor)`/`NAME.v(box,
            // anchor)` method call at parse time (D-LAYOUT1, `Parser/
            // Statements.rs`); re-sugar those calls back to `box.anchor`
            // before formatting so `layout` round-trips byte-for-byte
            // (original spans are reused end-to-end, so leading/trailing
            // trivia lookups on the re-sugared clone still land correctly).
            Stmt::Layout { name, body, .. } => {
                self.write(&format!("{} {} {{", Syntax::KW_LAYOUT, name));
                self.newline();
                let resugared: Vec<Stmt> =
                    body.iter().map(|s| resugar_layout_stmt(name, s)).collect();
                self.with_indent(|f| f.fmt_block_stmts(&resugared));
                self.end_block();
            }
            // D-EFF1 / D-QUAL1: `#Caps(Net, Db) { … }` effect-restriction region.
            Stmt::Caps { caps, body, .. } => {
                let list = caps
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.write(&format!("#{}({}) {{", Syntax::KW_CAPS, list));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-SCAP1: `#Grant(Fs) { caps -> … }` scoped-capability grant region.
            Stmt::Grant {
                caps,
                binding,
                body,
                ..
            } => {
                let list = caps
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.write(&format!(
                    "#{}({}) {{ {} {}",
                    Syntax::KW_GRANT,
                    list,
                    binding,
                    Syntax::GRANT_ARROW
                ));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-CTMARKER1 (ratified 2026-06-25, piece 2): `comptime { … }` block.
            Stmt::ComptimeBlock { body, .. } => {
                self.write(&format!("{} {{", Syntax::KW_COMPTIME));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-WHEN1 (ratified 2026-06-19): format like `if` with `comptime` lead.
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                self.write(&format!("{} {} ", Syntax::KW_COMPTIME, Syntax::KW_IF));
                self.fmt_cond(cond);
                self.write(" {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(then_body));
                self.end_block();
                if let Some(eb) = else_body {
                    self.write(" else {");
                    self.newline();
                    self.with_indent(|f| f.fmt_block_stmts(eb));
                    self.end_block();
                }
            }
            // D-OSTARGET2=B (ratified 2026-07-03): `comptime if build.os == { … }`
            // — the OS-dispatch switch. Formats exactly like a `Stmt::Switch`
            // (D-IF3 arm grammar) with a `comptime` lead.
            Stmt::ComptimeSwitch {
                subject,
                arms,
                else_body,
                ..
            } => {
                self.write(&format!("{} {} ", Syntax::KW_COMPTIME, Syntax::KW_IF));
                self.fmt_dispatch(subject, arms, else_body.as_deref());
            }
            // D-CTX1 (ratified 2026-06-22, G2): `#Context(field: value, …) { … }`.
            Stmt::ContextBlock { fields, body, .. } => {
                self.write(&format!("#{}", Syntax::CTX_BLOCK));
                self.write("(");
                for (i, (name, val, _)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", name));
                    self.fmt_expr(val, Prec::OrFallback);
                }
                self.write(") {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-TERM1 (ratified 2026-06-22): `live { … }` — terminal direct-input block.
            Stmt::Live { body, .. } => {
                self.write(Syntax::KW_LIVE);
                self.write(" {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-DET1: `assume_deterministic { … }` — the expert determinism escape.
            Stmt::AssumeDet { body, .. } => {
                self.write(Syntax::KW_ASSUME_DET);
                self.write(" {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` (the handle
            // is optional — a bare `#Transact { … }` with no hooks stays legal).
            Stmt::Transact { name, body, .. } => {
                match name {
                    Some(name) => self.write(&format!("#{}({}) {{", Syntax::KW_TRANSACT, name)),
                    None => self.write(&format!("#{} {{", Syntax::KW_TRANSACT)),
                }
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-DOTSCOPE1: a scope-member statement `.name { … }` /
            // `.name(args) { … }` inside a marker block.
            Stmt::ScopeMember {
                name, args, body, ..
            } => {
                self.write(&format!(".{}", name));
                if !args.is_empty() {
                    self.write("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.fmt_expr(a, Prec::OrFallback);
                    }
                    self.write(")");
                }
                self.write(" {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
        }
    }

    /// S68 (D-SG2): render an `if`/`while` condition without wrapping the
    /// outermost expression in redundant parens. Precedence-required parens on
    /// nested sub-expressions are preserved by the normal `fmt_expr` rules.
    pub(super) fn fmt_cond(&mut self, cond: &Expr) {
        // A condition is its own statement slot, so it imposes no binding
        // requirement: render at the lowest precedence so the outermost
        // operator never gains redundant parens (precedence-required parens on
        // nested sub-expressions are still added by `fmt_expr`).
        self.fmt_expr(cond, Prec::OrFallback);
    }

    /// S68 (D-SG2): render an `if`-expression branch `{ stmts… value }`.
    /// D-FMT1: when `inline` is set and the branch holds only its value
    /// expression (no leading statements, no inner comment, author wrote it on
    /// one line, fits width 100), keep it as `{ value }`. Otherwise expand.
    pub(super) fn fmt_value_block(&mut self, stmts: &[Stmt], value: &Expr, inline: bool) {
        self.write("{");
        if inline && self.try_inline_value_block(stmts, value) {
            return;
        }
        self.newline();
        self.with_indent(|f| {
            f.fmt_block_stmts(stmts);
            if !stmts.is_empty() {
                f.newline();
            }
            f.fmt_expr(value, Prec::OrFallback);
        });
        self.end_block();
    }

    /// D-FMT1: is this if-expression branch (no statements, just `value`)
    /// eligible to stay on one line? Mirrors `body_inline_eligible`.
    pub(super) fn value_block_inlineable(&self, stmts: &[Stmt], value: &Expr) -> bool {
        if !stmts.is_empty() {
            return false;
        }
        self.value_block_braces(value).is_some_and(|(open, close)| {
            !self.span_has_comment(open, close)
                && self.src.get(open..close).is_some_and(|s| !s.contains('\n'))
        })
    }

    fn fmt_if(&mut self, i: &IfStmt) {
        // D-FMT1: the whole if/else chain shares one line shape. If any branch
        // is multiline (author broke it, or a gate forces expansion), expand the
        // entire chain for readability; only a chain whose every branch is
        // inline-eligible stays inline.
        let inline = self.chain_inlineable(i);
        self.fmt_if_chain(i, inline);
    }

    /// True when every branch of the if/else chain is eligible to render inline
    /// (D-FMT1 gates a–d) AND the author wrote each on a single source line.
    fn chain_inlineable(&self, i: &IfStmt) -> bool {
        let then_ok = i.then_body.len() == 1
            && self
                .single_stmt_braces(&i.then_body[0])
                .is_some_and(|(o, c)| self.body_inline_eligible(&i.then_body, o, c));
        if !then_ok {
            return false;
        }
        match &i.else_branch {
            None => true,
            Some(ElseBranch::ElseIf(inner)) => self.chain_inlineable(inner),
            Some(ElseBranch::Else(body)) => {
                body.len() == 1
                    && self
                        .single_stmt_braces(&body[0])
                        .is_some_and(|(o, c)| self.body_inline_eligible(body, o, c))
            }
        }
    }

    /// Render the chain. `inline` is the shared decision from `chain_inlineable`;
    /// each branch still passes its rendered width through `fmt_body`'s gate (d),
    /// so an over-wide branch falls back to the expanded form for that branch.
    fn fmt_if_chain(&mut self, i: &IfStmt, inline: bool) {
        self.write("if ");
        // S68 (D-SG2): conditions use the no-paren house style — the outer
        // redundant parens of `if (cond)` are stripped.
        self.fmt_cond(&i.cond);
        self.write(" {");
        if inline {
            self.fmt_body(&i.then_body);
        } else {
            self.fmt_body_expanded(&i.then_body);
        }
        if let Some(else_b) = &i.else_branch {
            self.write(" else ");
            match else_b {
                ElseBranch::ElseIf(inner) => self.fmt_if_chain(inner, inline),
                ElseBranch::Else(body) => {
                    self.write("{");
                    if inline {
                        self.fmt_body(body);
                    } else {
                        self.fmt_body_expanded(body);
                    }
                }
            }
        }
    }

    /// D-IF1: render one arm as `head -> { body }`. A bare-value arm
    /// (`subject == value`) prints just the value; a full condition prints as
    /// written. A single-statement body could be braceless, but fmt always uses
    /// a block for a stable, idempotent shape.
    /// D-IF3 / D-OSTARGET2=B: render a dispatch body `== { arm -> … [else -> …] }`
    /// (the caller has already written the `if` / `comptime if` lead and subject
    /// keyword). Shared verbatim by `Stmt::Switch` and `Stmt::ComptimeSwitch` so
    /// both render byte-for-byte identically.
    fn fmt_dispatch(&mut self, subject: &Expr, arms: &[SwitchArm], else_body: Option<&[Stmt]>) {
        self.fmt_expr(subject, Prec::OrFallback);
        self.write(" == {");
        self.newline();
        self.with_indent(|f| {
            for arm in arms {
                f.fmt_switch_arm(subject, arm);
                f.newline();
            }
            if let Some(else_b) = else_body {
                f.write(Syntax::KW_ELSE);
                f.write(" ");
                f.write(Syntax::OP_ARM_ARROW);
                f.write(" {");
                f.newline();
                f.with_indent(|f| f.fmt_block_stmts(else_b));
                f.end_block();
            }
        });
        self.end_block();
    }

    fn fmt_switch_arm(&mut self, subject: &Expr, arm: &SwitchArm) {
        self.fmt_switch_cond(subject, &arm.cond, Prec::OrFallback);
        self.write(" ");
        self.write(Syntax::OP_ARM_ARROW);
        self.write(" {");
        self.fmt_body(&arm.body);
    }

    fn fmt_switch_cond(&mut self, subject: &Expr, cond: &Expr, prec: Prec) {
        // D-MATCHARM1: if the whole expression is an Or of subject-equalities,
        // emit it with `|` alternation syntax instead of `||`.
        if self.is_all_subject_alts(subject, cond) {
            // D-MATCHARM2=B: parens required when inside && or || context.
            let needs_paren = prec > Prec::OrFallback;
            if needs_paren {
                self.write("(");
            }
            self.fmt_arm_alternates(subject, cond);
            if needs_paren {
                self.write(")");
            }
            return;
        }
        match cond {
            Expr::Binary(op @ (BinOp::And | BinOp::Or), lhs, rhs, _) => {
                let my_prec = Prec::of_bin(*op);
                let needs_paren = prec > my_prec;
                if needs_paren {
                    self.write("(");
                }
                self.fmt_switch_cond(subject, lhs, my_prec);
                self.write(" ");
                self.write(op.spell());
                self.write(" ");
                self.fmt_switch_cond(subject, rhs, my_prec.add_rhs());
                if needs_paren {
                    self.write(")");
                }
            }
            Expr::Binary(BinOp::Eq, lhs, rhs, _) if self.same_subject(lhs, subject) => {
                self.fmt_expr(rhs, Prec::Cmp);
            }
            Expr::PatternTest {
                subject: lhs,
                pattern,
                ..
            } => {
                // D-IF3: a pattern arm head is bare — the `==` marker on the `if`
                // already binds it to the subject, so the head prints just the
                // pattern (`Active(id)`, `ok(n)`, `null`), no repeated `subject ==`.
                let _ = lhs;
                self.fmt_pattern(pattern);
            }
            _ => self.fmt_expr(cond, prec),
        }
    }

    /// True when `e` is an Or tree whose every leaf is Eq(subject, value).
    /// A bare single Eq is NOT matched here — the existing `fmt_switch_cond`
    /// match arm already strips the `subject ==` prefix for that case.
    fn is_all_subject_alts(&self, subject: &Expr, e: &Expr) -> bool {
        match e {
            Expr::Binary(BinOp::Or, lhs, rhs, _) => {
                self.is_subject_alt_leaf(subject, lhs) && self.is_subject_alt_leaf(subject, rhs)
            }
            _ => false,
        }
    }

    fn is_subject_alt_leaf(&self, subject: &Expr, e: &Expr) -> bool {
        match e {
            Expr::Binary(BinOp::Eq, lhs, _, _) => self.same_subject(lhs, subject),
            Expr::Binary(BinOp::Or, lhs, rhs, _) => {
                self.is_subject_alt_leaf(subject, lhs) && self.is_subject_alt_leaf(subject, rhs)
            }
            _ => false,
        }
    }

    /// Emit alternates with `|` separators (caller adds outer parens if needed).
    fn fmt_arm_alternates(&mut self, subject: &Expr, e: &Expr) {
        match e {
            Expr::Binary(BinOp::Or, lhs, rhs, _) => {
                self.fmt_arm_alternates(subject, lhs);
                self.write(" | ");
                self.fmt_arm_alternates(subject, rhs);
            }
            Expr::Binary(BinOp::Eq, _, rhs, _) => {
                self.fmt_expr(rhs, Prec::Cmp);
            }
            _ => unreachable!("is_all_subject_alts should guard this path"),
        }
    }

    /// True when `a` denotes the `when` subject: either the `it` placeholder or
    /// an expression with byte-for-byte the same source text as `subject`.
    fn same_subject(&self, a: &Expr, subject: &Expr) -> bool {
        if let Expr::Ident(name, _) = a {
            if name == Syntax::KW_IT {
                return true;
            }
        }
        let a_src = self.src.get(a.span().start..a.span().end);
        let subj_src = self.src.get(subject.span().start..subject.span().end);
        matches!((a_src, subj_src), (Some(x), Some(y)) if x == y)
    }

    fn fmt_binding(&mut self, b: &Binding) {
        if let Some(meta) = &b.meta {
            self.fmt_meta_attr(meta);
            self.write(" ");
        }
        if b.track {
            self.write(&format!("#{} ", Syntax::ATTR_TRACK));
        }
        // S57: comptime stays keyword-led (`comptime NAME = …`). D-BIND4: ordinary
        // bindings are sigil-led (`name :: …` / `name := …`), no leading keyword.
        if b.is_comptime {
            self.write(Syntax::KW_COMPTIME);
            self.write(" ");
            self.write(&b.name);
            self.write(" = ");
            self.fmt_expr(&b.init, Prec::OrFallback);
            return;
        }
        if let Some(pat) = &b.pattern {
            // S74: a destructuring target stands in for the name.
            self.fmt_bind_pattern(pat);
            self.write(" ");
            self.write(if b.mutable {
                Syntax::SIGIL_BIND_MUT
            } else {
                Syntax::SIGIL_BIND_IMMUT
            });
        } else if let Some(ty) = &b.ty {
            // D-BIND4: explicit-type form.
            // Immutable: `name: Type :: expr`. Mutable: `name: Type := expr`.
            self.write(&b.name);
            self.write(": ");
            self.fmt_type(ty);
            self.write(" ");
            self.write(if b.mutable {
                Syntax::SIGIL_BIND_MUT
            } else {
                Syntax::SIGIL_BIND_IMMUT
            });
        } else {
            self.write(&b.name);
            self.write(" ");
            self.write(if b.mutable {
                Syntax::SIGIL_BIND_MUT
            } else {
                Syntax::SIGIL_BIND_IMMUT
            });
        }
        self.write(" ");
        // D-UNINIT-SENTINEL1: `b.init` is a harmless never-evaluated placeholder
        // for a `:= uninit` binding — print the `uninit` keyword literally
        // instead of formatting the placeholder expression.
        if b.uninit {
            self.write(Syntax::KW_UNINIT);
        } else {
            self.fmt_expr(&b.init, Prec::OrFallback);
        }
    }

    fn fmt_bind_pattern(&mut self, pat: &BindPattern) {
        match pat {
            BindPattern::Struct {
                type_name,
                fields,
                rest,
                ..
            } => {
                // D-DOTCTOR1: emit `Type.{x, y}` (auto-fixes E0320 recovery).
                // D-DESTRUCT1: preserve a `field: rename` and a trailing `..`.
                self.write(type_name);
                self.write(".{");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&f.name);
                    if let Some((rn, _)) = &f.rename {
                        self.write(": ");
                        self.write(rn);
                    }
                }
                if rest.is_some() {
                    if !fields.is_empty() {
                        self.write(", ");
                    }
                    self.write("..");
                }
                self.write("}");
            }
            BindPattern::List { elems, .. } => {
                self.write("[");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&e.name);
                }
                self.write("]");
            }
            BindPattern::Tuple { elems, .. } => {
                self.write("(");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&e.name);
                }
                self.write(")");
            }
        }
    }

    fn fmt_lvalue(&mut self, lv: &LValue) {
        match lv {
            LValue::Local { name, .. } => self.write(name),
            LValue::Index { base, index, .. } => {
                self.fmt_expr(base, Prec::Postfix);
                self.write("[");
                self.fmt_expr(index, Prec::OrFallback);
                self.write("]");
            }
            // D-MUTSELF1: `place.field` field-assignment target.
            LValue::Field { base, field, .. } => {
                self.fmt_expr(base, Prec::Postfix);
                self.write(".");
                self.write(field);
            }
        }
    }
}

/// D-LAYOUT1: the inverse of `Parser::desugar_layout_anchors` — turns a
/// `NAME.h(box, anchor)` / `NAME.v(box, anchor)` call back into the
/// `box.anchor` source spelling before formatting. Only fires on the EXACT
/// shape the parser's desugar produces (receiver is a bare `Ident` equal to
/// `layout_name`, method is `h`/`v`, exactly two single-literal `Str` args);
/// anything else (a real user call, e.g. `form.h(a, b)` written by hand
/// against an unrelated `form`, or any other method) is left as-is, so this
/// never corrupts a program that merely happens to call `.h`/`.v`.
fn single_str_lit(e: &Expr) -> Option<&str> {
    if let Expr::Str(parts, _) = e {
        if let [StrPart::Lit(s)] = parts.as_slice() {
            return Some(s.as_str());
        }
    }
    None
}

fn resugar_layout_stmt(layout_name: &str, stmt: &Stmt) -> Stmt {
    match stmt {
        Stmt::Expr(e) => Stmt::Expr(resugar_layout_expr(layout_name, e)),
        Stmt::Val(b) => {
            let mut b2 = b.clone();
            b2.init = resugar_layout_expr(layout_name, &b.init);
            Stmt::Val(b2)
        }
        other => other.clone(),
    }
}

fn resugar_layout_expr(layout_name: &str, e: &Expr) -> Expr {
    if let Expr::MethodCall {
        receiver,
        method,
        method_span,
        args,
        ..
    } = e
    {
        if (method == "h" || method == "v") && args.len() == 2 {
            if let Expr::Ident(recv_name, recv_span) = receiver.as_ref() {
                if recv_name == layout_name {
                    if let (Some(box_name), Some(anchor)) =
                        (single_str_lit(&args[0].expr), single_str_lit(&args[1].expr))
                    {
                        return Expr::Field(
                            Box::new(Expr::Ident(box_name.to_string(), *recv_span)),
                            anchor.to_string(),
                            *method_span,
                        );
                    }
                }
            }
        }
    }
    match e {
        Expr::Binary(op, l, r, span) => Expr::Binary(
            *op,
            Box::new(resugar_layout_expr(layout_name, l)),
            Box::new(resugar_layout_expr(layout_name, r)),
            *span,
        ),
        Expr::Unary(op, x, span) => {
            Expr::Unary(*op, Box::new(resugar_layout_expr(layout_name, x)), *span)
        }
        Expr::Field(base, field, span) => Expr::Field(
            Box::new(resugar_layout_expr(layout_name, base)),
            field.clone(),
            *span,
        ),
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            type_args,
            args,
            recv_type,
            resolved_ret,
        } => Expr::MethodCall {
            receiver: Box::new(resugar_layout_expr(layout_name, receiver)),
            method: method.clone(),
            method_span: *method_span,
            type_args: type_args.clone(),
            args: args
                .iter()
                .map(|a| {
                    let mut a2 = a.clone();
                    a2.expr = resugar_layout_expr(layout_name, &a.expr);
                    a2
                })
                .collect(),
            recv_type: recv_type.clone(),
            resolved_ret: resolved_ret.clone(),
        },
        Expr::Call(call) => {
            let mut call2 = call.clone();
            for a in &mut call2.args {
                a.expr = resugar_layout_expr(layout_name, &a.expr);
            }
            Expr::Call(call2)
        }
        _ => e.clone(),
    }
}
