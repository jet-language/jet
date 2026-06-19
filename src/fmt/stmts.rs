use super::*;
use crate::ast::{
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
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
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
                            self.write(&format!(" {} ", syntax::KW_RANGE_STEP));
                            self.fmt_expr(step, Prec::OrFallback);
                        }
                    }
                    ForKind::In { collection } => {
                        self.fmt_expr(collection, Prec::OrFallback);
                    }
                }
                self.write(" {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-IF1: multi-arm dispatch renders as `if subject { head -> body }`
            // (the `Stmt::Switch` IR is shared with the retired `when`).
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                self.write(syntax::KW_IF);
                self.write(" ");
                self.fmt_expr(subject, Prec::OrFallback);
                self.write(" {");
                self.newline();
                self.with_indent(|f| {
                    for arm in arms {
                        f.fmt_switch_arm(subject, arm);
                        f.newline();
                    }
                    if let Some(else_b) = else_body {
                        f.write(syntax::KW_ELSE);
                        f.write(" ");
                        f.write(syntax::OP_ARM_ARROW);
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
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(inner));
                self.end_block();
            }
            Stmt::Unsafe { audit, body, .. } => {
                if let Some(reason) = audit {
                    self.write(&format!("@{}(\"{}\")", syntax::ATTR_AUDIT, reason));
                    self.newline();
                }
                self.write(&format!("@{} {{", syntax::KW_UNSAFE));
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
    pub(super) fn fmt_value_block(&mut self, stmts: &[Stmt], value: &Expr) {
        self.write("{");
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

    fn fmt_if(&mut self, i: &IfStmt) {
        self.write("if ");
        // S68 (D-SG2): conditions use the no-paren house style — the outer
        // redundant parens of `if (cond)` are stripped.
        self.fmt_cond(&i.cond);
        self.write(" {");
        self.newline();
        self.with_indent(|f| f.fmt_block_stmts(&i.then_body));
        self.end_block();
        if let Some(else_b) = &i.else_branch {
            self.write(" else ");
            match else_b {
                ElseBranch::ElseIf(inner) => self.fmt_if(inner),
                ElseBranch::Else(body) => {
                    self.write("{");
                    self.newline();
                    self.with_indent(|f| f.fmt_block_stmts(body));
                    self.end_block();
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
        self.write(syntax::OP_ARM_ARROW);
        self.write(" {");
        self.newline();
        self.with_indent(|f| f.fmt_block_stmts(&arm.body));
        self.end_block();
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
                // D-IF1: a pattern arm keeps its `subject == pattern` shape (a
                // bare pattern would re-parse as a value comparison and drop the
                // binding names, e.g. `ok(n)` wouldn't bind `n`). Emit the real
                // subject expression as written. `it` is preserved only when the
                // source already used it (a complex subject sema declares as
                // `it`); a plain subject identifier prints itself — no collapse.
                self.fmt_expr(lhs, Prec::Cmp);
                self.write(" == ");
                self.fmt_pattern(pattern);
            }
            _ => self.fmt_expr(cond, prec),
        }
    }

    /// True when `a` denotes the `when` subject: either the `it` placeholder or
    /// an expression with byte-for-byte the same source text as `subject`.
    fn same_subject(&self, a: &Expr, subject: &Expr) -> bool {
        if let Expr::Ident(name, _) = a {
            if name == syntax::KW_IT {
                return true;
            }
        }
        let a_src = self.src.get(a.span().start..a.span().end);
        let subj_src = self.src.get(subject.span().start..subject.span().end);
        matches!((a_src, subj_src), (Some(x), Some(y)) if x == y)
    }

    fn fmt_binding(&mut self, b: &Binding) {
        // S57: comptime stays keyword-led (`comptime NAME = …`). D-BIND1: ordinary
        // bindings are sigil-led (`name :: …` / `name := …`), no leading keyword.
        if b.is_comptime {
            self.write(syntax::KW_COMPTIME);
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
            syntax::SIGIL_BIND_MUT
        } else {
            syntax::SIGIL_BIND_IMMUT
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
        }
    }
}
