use super::*;
use crate::AST::{
    BinOp, BindPattern, Binding, Expr, ForKind, LValue, Stmt, StrPart, SwitchArm,
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
        let mut index = 0usize;
        let mut previous_end = None;
        while index < body.len() {
            let stmt = &body[index];
            let fence = self.fenced_statement_for(stmt);
            let source_start = fence
                .as_ref()
                .map_or_else(|| stmt_start(stmt), |fact| fact.span.start);
            if let Some(end) = previous_end {
                self.newline();
                self.emit_leading_statement_gap(end, source_start);
            } else {
                self.emit_leading(source_start);
            }
            if let Some(fact) = fence {
                self.fmt_fenced_statement(&fact);
                self.emit_trailing(fact.span.end);
                previous_end = Some(fact.span.end);
                index += fact.copies;
            } else {
                self.fmt_stmt(stmt);
                let end = self.statement_source_end(stmt);
                self.emit_trailing(end);
                previous_end = Some(end);
                index += 1;
            }
        }
    }

    fn fenced_statement_for(&self, stmt: &Stmt) -> Option<crate::AST::FencedStatement> {
        let start = stmt_start(stmt);
        self.fenced_statements
            .iter()
            .find(|fact| fact.span.start <= start && start <= fact.span.end)
            .cloned()
    }

    fn fmt_fenced_statement(&mut self, fact: &crate::AST::FencedStatement) {
        let preserve_layout = self
            .comments
            .iter()
            .any(|comment| fact.span.start <= comment.span.start && comment.span.start < fact.span.end);
        let mut cursor = fact.span.start;
        for fence in &fact.fences {
            self.write_fence_fragment(cursor, fence.span.start, preserve_layout);
            self.fmt_fenced_names(fence);
            cursor = fence.span.end;
        }
        self.write_fence_fragment(cursor, fact.span.end, preserve_layout);
        if preserve_layout {
            self.skip_verbatim_comments(fact.span.end);
        }
    }

    fn fmt_fenced_names(&mut self, fence: &crate::AST::FencedNames) {
        if let Some((start, end)) = &fence.range {
            self.write(&format!(
                "{} {}..{} {}",
                Syntax::SIGIL_FENCE_OPEN,
                start,
                end,
                Syntax::SIGIL_FENCE_CLOSE
            ));
            return;
        }

        // Expression entries (D-FENCE-GLYPH1=A) store an empty display name;
        // emit their authored source slice instead.
        let names = fence
            .names
            .iter()
            .map(|(name, span)| {
                if name.is_empty() {
                    self.src.get(span.start..span.end).unwrap_or("").to_string()
                } else {
                    name.clone()
                }
            })
            .collect::<Vec<_>>();
        let inline = format!(
            "{} {} {}",
            Syntax::SIGIL_FENCE_OPEN,
            names.join(", "),
            Syntax::SIGIL_FENCE_CLOSE
        );
        if self.col + inline.chars().count() <= MAX_WIDTH {
            self.write(&inline);
            return;
        }

        self.write(Syntax::SIGIL_FENCE_OPEN);
        self.newline();
        self.with_indent(|formatter| {
            for (index, name) in names.iter().enumerate() {
                formatter.write(name);
                if index + 1 != names.len() {
                    formatter.write(",");
                }
                formatter.newline();
            }
        });
        self.write(Syntax::SIGIL_FENCE_CLOSE);
    }

    fn write_fence_fragment(&mut self, start: usize, end: usize, preserve_layout: bool) {
        if preserve_layout {
            if let Some(fragment) = self.src.get(start..end) {
                self.write(fragment);
            }
            return;
        }
        let literal_spans = self
            .source_toks
            .iter()
            .filter(|token| {
                token.span.start >= start
                    && token.span.end <= end
                    && matches!(token.kind, TokKind::Str(_) | TokKind::Char(_))
            })
            .map(|token| token.span)
            .collect::<Vec<_>>();
        let mut cursor = start;
        for span in literal_spans {
            if let Some(fragment) = self.src.get(cursor..span.start) {
                self.write_normalized_source_fragment(fragment);
            }
            if let Some(literal) = self.src.get(span.start..span.end) {
                self.write(literal);
            }
            cursor = span.end;
        }
        if let Some(fragment) = self.src.get(cursor..end) {
            self.write_normalized_source_fragment(fragment);
        }
    }

    fn write_normalized_source_fragment(&mut self, fragment: &str) {
        let starts_with_space = fragment.chars().next().is_some_and(char::is_whitespace);
        let ends_with_space = fragment.chars().next_back().is_some_and(char::is_whitespace);
        let words = fragment.split_whitespace().collect::<Vec<_>>();
        if words.is_empty() {
            if starts_with_space {
                self.write(" ");
            }
            return;
        }
        let mut normalized = words.join(" ");
        if starts_with_space {
            normalized.insert(0, ' ');
        }
        if ends_with_space {
            normalized.push(' ');
        }
        self.write(&normalized);
    }

    /// Emit leading comments and preserve section breaks on either side of,
    /// and between, comment groups. A single boolean for the whole gap can move
    /// a comment across a section boundary and attach it to the wrong phase.
    fn emit_leading_statement_gap(&mut self, start: usize, end: usize) {
        let mut cursor = start;
        while self.comment_i < self.comments.len() {
            let (text, span) = {
                let comment = &self.comments[self.comment_i];
                (comment.text.clone(), comment.span)
            };
            if span.start >= end {
                break;
            }
            if span.start < start {
                self.emit_comment_line(&text);
                self.comment_i += 1;
                self.pending_blank = false;
                cursor = cursor.max(span.end);
                continue;
            }
            if self
                .src
                .get(cursor..span.start)
                .is_some_and(Self::source_has_blank_line)
            {
                self.newline();
            }
            self.emit_comment_line(&text);
            self.comment_i += 1;
            self.pending_blank = false;
            cursor = span.end;
        }
        if self
            .src
            .get(cursor..end)
            .is_some_and(Self::source_has_blank_line)
        {
            self.newline();
        }
    }

    fn source_has_blank_line(gap: &str) -> bool {
        let mut saw_newline = false;
        let mut only_space_since_newline = true;
        for ch in gap.chars() {
            if ch == '\n' {
                if saw_newline && only_space_since_newline {
                    return true;
                }
                saw_newline = true;
                only_space_since_newline = true;
            } else if !ch.is_whitespace() {
                only_space_since_newline = false;
            }
        }
        false
    }

    /// D-FMT1: render a single simple statement (no leading indent/newline) for
    /// the inline brace-body path. The caller guarantees `is_simple_stmt`.
    pub(super) fn fmt_stmt_inline(&mut self, stmt: &Stmt) {
        self.fmt_stmt(stmt);
    }

    fn fmt_block_marker(&mut self, marker: &jet_foundation::Registry::MarkerRowAndArgs) {
        self.write("#");
        if marker.negated {
            self.write("!");
        }
        self.write(marker.row.name);
        if !marker.args.is_empty() {
            self.write("(");
            let mut first = true;
            for argument in &marker.args {
                match argument {
                    jet_foundation::Registry::MarkerArgument::Expr { label, value } => {
                        if !first {
                            self.write(", ");
                        }
                        if let Some(label) = label {
                            self.write(label);
                            self.write(": ");
                        }
                        self.fmt_expr(value, Prec::OrFallback);
                        first = false;
                    }
                    jet_foundation::Registry::MarkerArgument::Ident(value) => {
                        if !first {
                            self.write(", ");
                        }
                        self.write(value);
                        first = false;
                    }
                    jet_foundation::Registry::MarkerArgument::Idents { label, values } => {
                        if !first {
                            self.write(", ");
                        }
                        if let Some(label) = label {
                            self.write(label);
                            self.write(": ");
                        }
                        self.write(&values.join(", "));
                        first = false;
                    }
                    jet_foundation::Registry::MarkerArgument::Text(value) => {
                        if !first {
                            self.write(", ");
                        }
                        self.write("\"");
                        self.write(&escape_str_lit(value));
                        self.write("\"");
                        first = false;
                    }
                    jet_foundation::Registry::MarkerArgument::Policy(declarations) => {
                        for declaration in declarations {
                            if !first {
                                self.write(", ");
                            }
                            self.write(declaration.key.name());
                            if let crate::Policy::PolicyValue::Limit(limit) = declaration.value {
                                self.write(&format!("({limit})"));
                            }
                            first = false;
                        }
                    }
                }
            }
            self.write(")");
        }
        self.write(" {");
        self.newline();
    }

    pub(super) fn fmt_stmt(&mut self, stmt: &Stmt) {
        if !matches!(stmt, Stmt::Switched { .. }) {
            if let Some(marker) =
                jet_foundation::Registry::block_marker(stmt, self.policy_declarations)
            {
                self.fmt_block_marker(&marker);
            }
        }
        match stmt {
            Stmt::Expr(Expr::Call(call)) if call.name == Syntax::INTERNAL_DEFER_CLOSE => {
                self.write("defer ");
                if let Some(arg) = call.args.first() {
                    self.fmt_expr(&arg.expr, Prec::OrFallback);
                }
            }
            Stmt::Expr(Expr::Call(call)) if call.name == Syntax::INTERNAL_UNSAFE_ASSERT => {
                self.write(Syntax::KW_ASSERT);
                for (index, arg) in call.args.iter().enumerate() {
                    self.write(if index == 0 { " " } else { ", " });
                    if let Expr::Ident(name, _) = &arg.expr { self.write(name); }
                }
            }
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
            Stmt::While {
                cond, body, label, ..
            } => {
                // D-LOOPLABEL3=A: loop labels use declaration spelling.
                if let Some((_n, _)) = label {
                    self.write(&format!("{} :: ", _n));
                }
                self.write("loop ");
                self.fmt_cond(cond);
                self.fmt_effect_loop_body(body, cond.span().end);
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
                    self.write(&format!("{} :: ", _n));
                }
                self.write("loop ");
                if var2.is_some() {
                    self.write("(");
                }
                self.write(var);
                if let Some((v2, _)) = var2 {
                    self.write(", ");
                    self.write(v2);
                    self.write(")");
                }
                let clause_width = match kind {
                    ForKind::Range { start, end, step, exclusive } => {
                        start.span().end - start.span().start
                            + end.span().end - end.span().start
                            + step.as_ref().map_or(0, |step| step.span().end - step.span().start + 2)
                            + if *exclusive { 6 } else { 5 }
                    }
                    ForKind::In { collection, step } => {
                        collection.span().end - collection.span().start
                            + step.as_ref().map_or(0, |step| step.span().end - step.span().start + 2)
                            + 3
                    }
                };
                let wrap = self.col + clause_width > MAX_WIDTH;
                let first_clause_start = match kind {
                    ForKind::Range { start, .. } => start.span().start,
                    ForKind::In { collection, .. } => collection.span().start,
                };
                self.loop_clause_separator(first_clause_start, wrap);
                match kind {
                    ForKind::Range { start, end, step, exclusive } => {
                        self.fmt_expr(start, Prec::OrFallback);
                        self.write(if *exclusive { "..<" } else { ".." });
                        self.fmt_expr(end, Prec::OrFallback);
                        if let Some(step) = step {
                            self.loop_clause_separator(step.span().start, wrap);
                            self.fmt_expr(step, Prec::OrFallback);
                        }
                    }
                    ForKind::In { collection, step } => {
                        self.fmt_expr(collection, Prec::OrFallback);
                        if let Some(step) = step {
                            self.loop_clause_separator(step.span().start, wrap);
                            self.fmt_expr(step, Prec::OrFallback);
                        }
                    }
                }
                let header_end = match kind {
                    ForKind::Range { end, step, .. } => {
                        step.as_ref().map_or(end.span().end, |value| value.span().end)
                    }
                    ForKind::In { collection, step } => step
                        .as_ref()
                        .map_or(collection.span().end, |value| value.span().end),
                };
                self.fmt_effect_loop_body(body, header_end);
            }
            // D-IF3 / D-IFDIST1: multi-arm dispatch renders as
            // `if subject OP { head -> body }` (the `Stmt::Switch` IR is shared
            // with the retired `when`). The comparison marker enters dispatch;
            // arm heads carry no repeated `subject OP`.
            Stmt::Switch {
                subject,
                arms,
                else_body,
                span,
            } => {
                self.write(Syntax::KW_IF);
                self.write(" ");
                if crate::AST::is_subjectless_guard(subject, *span) {
                    if self.switch_was_classic_if(arms, *span) {
                        self.fmt_classic_switch(arms, else_body.as_deref());
                    } else {
                        self.fmt_guard_dispatch(arms, else_body.as_deref());
                    }
                } else {
                    self.fmt_dispatch(subject, arms, else_body.as_deref());
                }
            }
            Stmt::Break(_) => self.write("break"),
            Stmt::BreakValue(value, _) => {
                self.write("break ");
                self.fmt_expr(value, Prec::OrFallback);
            }
            Stmt::Continue(_) => self.write(Syntax::KW_NEXT),
            Stmt::BreakLabel(name, _)
                if super::is_generated_label(name) =>
            {
                self.write("break")
            }
            Stmt::BreakLabel(name, _) => self.write(&format!("break({})", name)),
            Stmt::BreakLabelValue(name, _, value, _)
                if super::is_generated_label(name) =>
            {
                self.write("break ");
                self.fmt_expr(value, Prec::OrFallback);
            }
            Stmt::BreakLabelValue(name, _, value, _) => {
                self.write(&format!("break({}, ", name));
                self.fmt_expr(value, Prec::OrFallback);
                self.write(")");
            }
            Stmt::ContinueLabel(name, _)
                if super::is_generated_label(name) =>
            {
                self.write(Syntax::KW_NEXT)
            }
            Stmt::ContinueLabel(name, _) => self.write(&format!("next({})", name)),
            // D-LOOP-COMMA1=A: `loop init, cond, step { body }`.
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                label,
                ..
            } => {
                if let Some((n, _)) = label {
                    self.write(&format!("{} :: ", n));
                }
                self.write("loop ");
                let header_width = init.init.span().end.saturating_sub(init.name_span.start)
                    + cond.span().end.saturating_sub(cond.span().start)
                    + step.as_ref().map_or(0, |step| stmt_end(step).saturating_sub(stmt_start(step)) + 2)
                    + 5;
                let wrap = self.col + header_width > MAX_WIDTH;
                self.fmt_binding(init);
                self.loop_clause_separator(cond.span().start, wrap);
                self.fmt_cond(cond);
                if let Some(step) = step {
                    self.loop_clause_separator(step.span().start, wrap);
                    self.fmt_stmt(step);
                }
                let header_end = step
                    .as_ref()
                    .map_or(cond.span().end, |statement| statement.span().end);
                self.fmt_effect_loop_body(body, header_end);
            }
            Stmt::Loop {
                body: inner, label, ..
            } => {
                if let Some((_n, _)) = label {
                    self.write(&format!("{} :: ", _n));
                }
                self.write("loop {");
                self.fmt_body(inner);
            }
            Stmt::Unsafe { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-CTEFFECT1: `#Impure("reason") { … }` round-trips verbatim.
            Stmt::Impure { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            Stmt::Switched { marker, body, .. } => {
                self.fmt_statement_switch_attr(&marker.name, body)
            }
            // D-REACTCORE1: `#Reactive { … }` round-trips verbatim.
            Stmt::Reactive { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-SHIELDNAME1=A: `#Shield { … }` round-trips verbatim.
            Stmt::Shield { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-BLOCKPLANE1=A: `#Region(r) { … }`.
            Stmt::Region { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            Stmt::Policy { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-CONC-SPAWN1=D: `task.group g(limit: n) { … }`.
            Stmt::TaskGroup { name, limit, body, .. } => {
                self.write(&format!("{}.group {}", Syntax::KW_CONC_TASK, name));
                if let Some(limit) = limit {
                    self.write("(limit: ");
                    self.fmt_expr(limit, Prec::OrFallback);
                    self.write(")");
                }
                self.write(" {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-LAYOUT-CTOR1: `name :: Layout.{ … }` — typed-literal element
            // body (comma-separated Constraints). Re-sugar `h`/`v` calls back
            // to `box.anchor` / `self.anchor`.
            Stmt::Layout { name, body, .. } => {
                self.write(&format!(
                    "{} {} {}.{{",
                    name,
                    Syntax::SIGIL_BIND_IMMUT,
                    Syntax::LAYOUT_TYPE
                ));
                self.newline();
                let resugared: Vec<Stmt> =
                    body.iter().map(|s| resugar_layout_stmt(name, s)).collect();
                self.with_indent(|f| {
                    for (i, stmt) in resugared.iter().enumerate() {
                        if i > 0 {
                            f.newline();
                        }
                        f.fmt_stmt(stmt);
                        if i + 1 < resugared.len() {
                            f.write(",");
                        }
                    }
                });
                self.end_block();
            }
            // D-EFF1 / D-QUAL1: `#Caps(Net, DB) { … }` effect-restriction region.
            Stmt::Caps { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-SCAP1 / D-ARROW-CONTROL1:
            // `#Grant(caps: FS, Net) { … }` scoped-capability grant region.
            Stmt::Grant { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-VERDICT-1308-1: `@ { … }` demand block.
            Stmt::ComptimeBlock { body, .. } => {
                self.write(&format!("{} {{", Syntax::COMPTIME_MARK));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-VERDICT-1308-2: format like `if` with an `@` lead.
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                self.write(&format!("{}{} ", Syntax::COMPTIME_MARK, Syntax::KW_IF));
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
            // D-OSTARGET2=B (ratified 2026-07-03): `@if build.os == { … }`
            // — the OS-dispatch switch. Formats exactly like a `Stmt::Switch`
            // (D-IF3 arm grammar) with an `@if` lead.
            Stmt::ComptimeSwitch {
                subject,
                arms,
                else_body,
                ..
            } => {
                self.write(&format!("{}{} ", Syntax::COMPTIME_MARK, Syntax::KW_IF));
                self.fmt_dispatch(subject, arms, else_body.as_deref());
            }
            // D-CTX1 (ratified 2026-06-22, G2): `#Context(field: value, …) { … }`.
            Stmt::ContextBlock { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-BLOCKPLANE1=A: `#Live { … }`.
            Stmt::Live { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-BLOCKPLANE1=A: `#Nondeterministic("reason") { … }`.
            Stmt::AssumeDet { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` (the handle
            // is optional — a bare `#Transact { … }` with no hooks stays legal).
            Stmt::Transact { body, .. } => {
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            // D-DOTSCOPE1 / D-META-DSL1: a scope-member statement `.name { … }` /
            // `.name(args) { … }`, or a declared `#Name { … }` block.
            Stmt::ScopeMember {
                name, args, body, dsl, ..
            } => {
                if *dsl {
                    self.write(&format!("#{}", name));
                } else {
                    self.write(&format!(".{}", name));
                }
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

    fn fmt_effect_loop_body(&mut self, body: &[Stmt], header_end: usize) {
        let _ = header_end;
        self.write(" {");
        self.fmt_control_body(body);
    }

    /// D-IF1/D-FMT1: render one dispatch arm. A bare-value arm
    /// (`subject OP value`) prints just the value; a full condition prints as
    /// written. Preserve an author-written braceless simple body when it fits.
    /// D-IF3 / D-OSTARGET2=B / D-IFDIST1: render a dispatch body
    /// `OP { arm -> … [else -> …] }` (the caller has already written the `if` /
    /// `@if` lead). Shared by `Stmt::Switch` and `Stmt::ComptimeSwitch`.
    fn fmt_dispatch(&mut self, subject: &Expr, arms: &[SwitchArm], else_body: Option<&[Stmt]>) {
        let table_op = self
            .dispatch_op_from_source(subject)
            .unwrap_or_else(|| self.dispatch_op(subject, arms));
        self.fmt_expr(subject, Prec::OrFallback);
        self.write(" ");
        self.write(table_op.spell());
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (index, arm) in arms.iter().enumerate() {
                if index > 0 {
                    f.newline();
                }
                let next_starts_with_dot = arms
                    .get(index + 1)
                    .is_some_and(|next| Self::arm_head_starts_with_dot(&next.cond));
                f.fmt_switch_arm(subject, table_op, arm, next_starts_with_dot);
            }
            if let Some(else_b) = else_body {
                if !arms.is_empty() {
                    f.newline();
                }
                f.write(Syntax::KW_ELSE);
                f.write(" ");
                f.write(Syntax::OP_ARM_ARROW);
                f.fmt_arm_body(else_b, false);
            }
        });
        self.end_block();
    }

    fn fmt_guard_dispatch(&mut self, arms: &[SwitchArm], else_body: Option<&[Stmt]>) {
        self.write("{");
        self.newline();
        self.with_indent(|f| {
            for (index, arm) in arms.iter().enumerate() {
                if index > 0 {
                    f.newline();
                }
                f.fmt_expr(&arm.cond, Prec::OrFallback);
                f.write(" ");
                f.write(Syntax::OP_ARM_ARROW);
                f.fmt_arm_body(&arm.body, false);
            }
            if let Some(body) = else_body {
                if !arms.is_empty() {
                    f.newline();
                }
                f.write(Syntax::KW_ELSE);
                f.write(" ");
                f.write(Syntax::OP_ARM_ARROW);
                f.fmt_arm_body(body, false);
            }
        });
        self.end_block();
    }

    fn switch_was_classic_if(&self, arms: &[SwitchArm], span: Span) -> bool {
        let Some(first) = arms.first() else {
            return false;
        };
        crate::AST::uses_classic_if_spelling(self.src, span, first.cond.span())
    }

    fn fmt_classic_switch(&mut self, arms: &[SwitchArm], else_body: Option<&[Stmt]>) {
        let Some(arm) = arms.first() else {
            return;
        };
        self.emit_classic_if_condition_trivia(arm.cond.span().start);
        self.fmt_cond(&arm.cond);
        self.write(" {");
        self.fmt_control_body(&arm.body);
        match else_body {
            Some([
                Stmt::Switch {
                    subject,
                    arms,
                    else_body,
                    span,
                },
            ]) if crate::AST::is_subjectless_guard(subject, *span)
                && self.switch_was_classic_if(arms, *span) =>
            {
                self.write(" else if ");
                self.fmt_classic_switch(arms, else_body.as_deref());
            }
            Some(body) => {
                self.write(" else {");
                self.fmt_control_body(body);
            }
            None => {}
        }
    }

    fn emit_classic_if_condition_trivia(&mut self, condition_start: usize) {
        while self.comment_i < self.comments.len()
            && self.comments[self.comment_i].span.start < condition_start
        {
            let text = self.comments[self.comment_i].text.clone();
            self.write(&text);
            self.comment_i += 1;
            if text.starts_with("//") {
                self.newline();
            } else {
                self.write(" ");
            }
        }
    }

    fn fmt_switch_arm(
        &mut self,
        subject: &Expr,
        table_op: BinOp,
        arm: &SwitchArm,
        force_braces: bool,
    ) {
        self.fmt_switch_cond(subject, table_op, &arm.cond, Prec::OrFallback);
        self.write(" ");
        self.write(Syntax::OP_ARM_ARROW);
        self.fmt_arm_body(&arm.body, force_braces);
    }

    /// Preserve `head -> statement` when the author chose that shape and the
    /// rendered arm still fits the width limit. Braced and multiline bodies
    /// keep their explicit scope. Add concise braces when the next leading-dot
    /// arm would otherwise parse as a chain on this body's final expression.
    fn fmt_arm_body(&mut self, body: &[Stmt], force_braces: bool) {
        if body.is_empty() {
            self.write(" {}");
            return;
        }
        let was_braceless = self.arm_body_was_braceless(body);
        if was_braceless && force_braces {
            let saved_out = self.out.len();
            let saved_col = self.col;
            let saved_line_start = self.at_line_start;
            let saved_pending_blank = self.pending_blank;
            let saved_comment_i = self.comment_i;
            self.write(" { ");
            self.fmt_stmt_inline(&body[0]);
            self.write(" }");
            if self.col <= MAX_WIDTH {
                return;
            }
            self.out.truncate(saved_out);
            self.col = saved_col;
            self.at_line_start = saved_line_start;
            self.pending_blank = saved_pending_blank;
            self.comment_i = saved_comment_i;
            self.write(" {");
            self.fmt_body_expanded(body);
            return;
        }
        if was_braceless {
            let saved_out = self.out.len();
            let saved_col = self.col;
            let saved_line_start = self.at_line_start;
            let saved_pending_blank = self.pending_blank;
            let saved_comment_i = self.comment_i;
            self.write(" ");
            self.fmt_stmt_inline(&body[0]);
            if self.col <= MAX_WIDTH {
                return;
            }
            self.out.truncate(saved_out);
            self.col = saved_col;
            self.at_line_start = saved_line_start;
            self.pending_blank = saved_pending_blank;
            self.comment_i = saved_comment_i;
            self.write(" {");
            self.fmt_body_expanded(body);
            return;
        }
        self.write(" {");
        self.fmt_body(body);
    }

    fn arm_head_starts_with_dot(cond: &Expr) -> bool {
        match cond {
            Expr::PatternTest {
                pattern: crate::AST::Pattern::Variant { .. },
                ..
            } => true,
            Expr::Binary(_, left, _, _) => Self::arm_head_starts_with_dot(left),
            _ => false,
        }
    }

    /// The nearest source `->` belongs to this arm. No opening brace or line
    /// break between it and the lone simple statement means the body was the
    /// concise braceless form.
    fn arm_body_was_braceless(&self, body: &[Stmt]) -> bool {
        if body.len() != 1 || !is_simple_stmt(&body[0]) {
            return false;
        }
        let start = stmt_start(&body[0]);
        let Some(prefix) = self.src.get(..start) else {
            return false;
        };
        let Some(arrow) = prefix.rfind(Syntax::OP_ARM_ARROW) else {
            return false;
        };
        let between = &prefix[arrow + Syntax::OP_ARM_ARROW.len()..];
        let source_end = self.statement_source_end(&body[0]);
        let line_end = self.src[start..]
            .find('\n')
            .map_or(self.src.len(), |offset| start + offset);
        !between.contains('{')
            && !between.contains('\n')
            && self
                .src
                .get(start..source_end)
                .is_some_and(|source| !source.contains('\n'))
            && !self.span_has_comment(arrow + Syntax::OP_ARM_ARROW.len(), line_end)
    }

    fn fmt_switch_cond(&mut self, subject: &Expr, table_op: BinOp, cond: &Expr, prec: Prec) {
        // D-MATCHARM1 / D-IFDIST1: if the whole expression is an Or of
        // subject-table_op atoms, emit it with `|` alternation syntax instead of `||`.
        if self.is_all_subject_alts(subject, table_op, cond) {
            // Parens required when inside && or || context.
            let needs_paren = prec > Prec::OrFallback;
            if needs_paren {
                self.write("(");
            }
            self.fmt_arm_alternates(subject, table_op, cond);
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
                self.fmt_switch_cond(subject, table_op, lhs, my_prec);
                self.write(" ");
                self.write(op.spell());
                self.write(" ");
                self.fmt_switch_cond(subject, table_op, rhs, my_prec.add_rhs());
                if needs_paren {
                    self.write(")");
                }
            }
            // Only strip the table's distributed marker — predicate arms like
            // `code >= 500` under `if code == { … }` must print in full.
            Expr::Binary(op, lhs, rhs, _)
                if *op == table_op && self.same_subject(lhs, subject) =>
            {
                self.fmt_expr(rhs, Prec::Cmp);
            }
            Expr::PatternTest {
                subject: lhs,
                pattern,
                ..
            } => {
                // D-IF3: a pattern arm head is bare — the comparison marker on
                // the `if` already binds it to the subject.
                let _ = lhs;
                self.fmt_pattern(pattern);
            }
            _ => self.fmt_expr(cond, prec),
        }
    }

    /// Read the comparison marker from source between the subject and `{`.
    fn dispatch_op_from_source(&self, subject: &Expr) -> Option<BinOp> {
        let rest = self.src.get(subject.span().end..)?;
        let trimmed = rest.trim_start();
        if trimmed.starts_with("==") {
            Some(BinOp::Eq)
        } else if trimmed.starts_with("!=") {
            Some(BinOp::Ne)
        } else if trimmed.starts_with("<=") {
            Some(BinOp::Le)
        } else if trimmed.starts_with(">=") {
            Some(BinOp::Ge)
        } else if trimmed.starts_with('<') {
            Some(BinOp::Lt)
        } else if trimmed.starts_with('>') {
            Some(BinOp::Gt)
        } else {
            None
        }
    }

    /// Infer the table's distributed comparison from subject-`table_op` leaves.
    /// Prefers a consistent OP across pure distributed leaves; defaults to `==`.
    fn dispatch_op(&self, subject: &Expr, arms: &[SwitchArm]) -> BinOp {
        let mut found: Option<BinOp> = None;
        for arm in arms {
            self.collect_dispatch_ops(subject, &arm.cond, &mut found);
        }
        found.unwrap_or(BinOp::Eq)
    }

    fn collect_dispatch_ops(&self, subject: &Expr, e: &Expr, found: &mut Option<BinOp>) {
        match e {
            Expr::Binary(op, lhs, _, _)
                if op.is_comparison() && self.same_subject(lhs, subject) =>
            {
                match *found {
                    None => *found = Some(*op),
                    Some(prev) if prev != *op => {
                        // Mixed predicate + distributed leaves: keep the first
                        // distributed OP only when source was unavailable.
                    }
                    _ => {}
                }
            }
            Expr::Binary(BinOp::And | BinOp::Or, lhs, rhs, _) => {
                self.collect_dispatch_ops(subject, lhs, found);
                self.collect_dispatch_ops(subject, rhs, found);
            }
            _ => {}
        }
    }

    /// True when `e` is an Or tree whose every leaf is `subject table_op value`.
    fn is_all_subject_alts(&self, subject: &Expr, table_op: BinOp, e: &Expr) -> bool {
        match e {
            Expr::Binary(BinOp::Or, lhs, rhs, _) => {
                self.is_subject_alt_leaf(subject, table_op, lhs)
                    && self.is_subject_alt_leaf(subject, table_op, rhs)
            }
            _ => false,
        }
    }

    fn is_subject_alt_leaf(&self, subject: &Expr, table_op: BinOp, e: &Expr) -> bool {
        match e {
            Expr::Binary(op, lhs, _, _) if *op == table_op => self.same_subject(lhs, subject),
            Expr::Binary(BinOp::Or, lhs, rhs, _) => {
                self.is_subject_alt_leaf(subject, table_op, lhs)
                    && self.is_subject_alt_leaf(subject, table_op, rhs)
            }
            _ => false,
        }
    }

    /// Emit alternates with `|` separators (caller adds outer parens if needed).
    fn fmt_arm_alternates(&mut self, subject: &Expr, table_op: BinOp, e: &Expr) {
        match e {
            Expr::Binary(BinOp::Or, lhs, rhs, _) => {
                self.fmt_arm_alternates(subject, table_op, lhs);
                self.write(" | ");
                self.fmt_arm_alternates(subject, table_op, rhs);
            }
            Expr::Binary(op, _, rhs, _) if *op == table_op => {
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

    pub(super) fn fmt_binding(&mut self, b: &Binding) {
        if let Some(meta) = &b.meta {
            self.fmt_meta_attr(meta);
            self.write(" ");
        }
        for marker in &b.markers {
            self.write(&format!("#{} ", marker.name));
        }
        // D-VERDICT-1308-1: explicit compile-time demand is marker-led.
        if b.is_comptime {
            self.write(&b.name);
            self.write(" :: ");
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
        } else {
            // D-BIND-BARE1: bindings are always bare (`name :: …` / `name := …`).
            // Types ride the value (`Type.{ … }`), never the binding name.
            self.write(&b.name);
            self.write(" ");
            self.write(if b.mutable {
                Syntax::SIGIL_BIND_MUT
            } else {
                Syntax::SIGIL_BIND_IMMUT
            });
        }
        self.write(" ");
        // D-UNINIT-SENTINEL2: print `Type.{ uninit }` from the binding's type.
        if b.uninit {
            if let Some(ty) = &b.ty {
                self.fmt_type(ty);
                self.write(".{ ");
                self.write(Syntax::KW_UNINIT);
                self.write(" }");
            } else {
                self.write(Syntax::KW_UNINIT);
            }
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

/// D-LAYOUT1 / D-LAYOUT-CTOR1: the inverse of `Parser::desugar_layout_anchors`
/// — turns a `name.h(box, anchor)` / `name.v(box, anchor)` call back into the
/// `box.anchor` / `self.anchor` source spelling before formatting. Only fires
/// on the EXACT shape the parser's desugar produces (receiver is a bare
/// `Ident` equal to `layout_name`, method is `h`/`v`, exactly two
/// single-literal `Str` args); anything else is left as-is.
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
                        let shown = if box_name == layout_name {
                            Syntax::KW_SELF.to_string()
                        } else {
                            box_name.to_string()
                        };
                        return Expr::Field(
                            Box::new(Expr::Ident(shown, *recv_span)),
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
            owner_type_args,
            type_args,
            args,
            recv_type,
            resolved_ret,
            checked_widen,
        } => Expr::MethodCall {
            receiver: Box::new(resugar_layout_expr(layout_name, receiver)),
            method: method.clone(),
            method_span: *method_span,
            owner_type_args: owner_type_args.clone(),
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
            checked_widen: *checked_widen,
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
