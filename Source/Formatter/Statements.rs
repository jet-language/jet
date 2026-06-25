use super::*;
use crate::AST::{
    BinOp, BindPattern, Binding, ElseBranch, Expr, ForKind, IfStmt, LValue, Stmt, SwitchArm,
};

impl<'a> Fmt<'a> {
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
            Stmt::If(i) => self.fmt_if(i),
            Stmt::While { cond, body, label, .. } => {
                // S19: the canonical loop keyword is `loop` (a parsed `loop cond`
                // becomes a `While` node). D-LABEL1: print the `@name` label.
                if let Some((_n, _)) = label {
                    self.write(&format!("@{} ", _n));
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
                    self.write(&format!("@{} ", _n));
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
            Stmt::Break(_) => self.write("break"),
            Stmt::Continue(_) => self.write("continue"),
            Stmt::BreakLabel(name, _) => self.write(&format!("break @{}", name)),
            Stmt::ContinueLabel(name, _) => self.write(&format!("continue @{}", name)),
            Stmt::Loop { body: inner, label, .. } => {
                if let Some((_n, _)) = label {
                    self.write(&format!("@{} ", _n));
                }
                self.write("loop {");
                self.fmt_body(inner);
            }
            Stmt::Unsafe { audit, body, .. } => {
                // D-UNSAFE2: the reason is the argument of `#Unsafe` itself; the
                // separate `#Audit` line is retired.
                match audit {
                    Some(reason) => {
                        self.write(&format!("#{}(\"{}\") {{", Syntax::KW_UNSAFE, reason))
                    }
                    None => self.write(&format!("#{} {{", Syntax::KW_UNSAFE)),
                }
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
            // D-SCAP1: `#grant(Fs) { caps -> … }` scoped-capability grant region.
            Stmt::Grant { caps, binding, body, .. } => {
                let list = caps
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.write(&format!(
                    "#{}({}) {{ {} {}",
                    Syntax::KW_GRANT, list, binding, Syntax::GRANT_ARROW
                ));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-WHEN1 (ratified 2026-06-19): format like `if` with `comptime` lead.
            Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
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
        }
    }

    /// S68 (D-SG2): render an `if`/`while` condition without wrapping the
    /// outermost expression in redundant parens. Precedence-required parens on
    /// nested sub-expressions are preserved by the normal `fmt_expr` rules.
    pub(super) fn fmt_cond(&mut self, cond: &Expr) {
        self.fmt_expr(cond, Prec::Primary);
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
        self.value_block_braces(value)
            .is_some_and(|(open, close)| {
                !self.span_has_comment(open, close)
                    && self
                        .src
                        .get(open..close)
                        .is_some_and(|s| !s.contains('\n'))
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
    fn fmt_switch_arm(&mut self, subject: &Expr, arm: &SwitchArm) {
        self.fmt_switch_cond(subject, &arm.cond, Prec::OrFallback);
        self.write(" ");
        self.write(Syntax::OP_ARM_ARROW);
        self.write(" {");
        self.fmt_body(&arm.body);
    }

    fn fmt_switch_cond(&mut self, subject: &Expr, cond: &Expr, prec: Prec) {
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
        // S57: comptime stays keyword-led (`comptime NAME = …`). D-BIND2: ordinary
        // bindings are sigil-led (`name @= …` / `name := …`), no leading keyword.
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
        } else {
            self.write(&b.name);
            if let Some(ty) = &b.ty {
                self.write(": ");
                self.fmt_type(ty);
            }
        }
        self.write(" ");
        self.write(if b.mutable {
            Syntax::SIGIL_BIND_MUT
        } else {
            Syntax::SIGIL_BIND_IMMUT
        });
        self.write(" ");
        self.fmt_expr(&b.init, Prec::OrFallback);
    }

    fn fmt_bind_pattern(&mut self, pat: &BindPattern) {
        match pat {
            BindPattern::Struct {
                type_name, fields, ..
            } => {
                self.write(type_name);
                self.write("{");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&f.name);
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
