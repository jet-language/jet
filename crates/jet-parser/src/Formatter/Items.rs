use super::*;
use crate::AST::{
    AccessConvention, ConstAttr, ConstDef, EnumDef, EnumGroup, ExternFn, ExternRustBlock, Field,
    Func, ImplDef, ImportDecl, ImportKind, Item, Marker, MetaAttr, MetaField, Param, StructDef,
    StructLayout, TraitImplBlock, Type, TypeParam, Variant, VariantPayload,
};

enum EnumFmtEntry<'b> {
    Leaf(&'b Variant),
    Group(&'b EnumGroup),
}

impl<'a> Fmt<'a> {
    pub(super) fn fmt_meta_attr(&mut self, meta: &MetaAttr) {
        self.write("#");
        self.fmt_meta_rule(meta);
    }

    fn fmt_meta_rule(&mut self, meta: &MetaAttr) {
        self.write(&format!("{}(", Syntax::ATTR_META));
        for (idx, field) in meta.fields.iter().enumerate() {
            if idx > 0 {
                self.write(", ");
            }
            match field {
                MetaField::Category { value, .. } => {
                    self.write(Syntax::META_FIELD_CATEGORY);
                    self.write(": ");
                    self.fmt_expr(value, Prec::OrFallback);
                }
                MetaField::Tunable { .. } => self.write(Syntax::META_FIELD_TUNABLE),
                MetaField::Maturity { value, .. } => {
                    self.write(Syntax::META_FIELD_MATURITY);
                    self.write(": ");
                    self.fmt_expr(value, Prec::OrFallback);
                }
                MetaField::Unknown { name, value, .. } => {
                    self.write(name);
                    if let Some(value) = value {
                        self.write(": ");
                        self.fmt_expr(value, Prec::OrFallback);
                    }
                }
            }
        }
        self.write(")");
    }

    pub(super) fn fmt_item(&mut self, item: &Item) {
        match item {
            Item::Func(f) => self.fmt_func(f, true),
            Item::Struct(s) => self.fmt_struct(s, true),
            Item::Enum(e) => self.fmt_enum(e, true),
            Item::Impl(i) => {
                // External-method sugar (`fn Type.method(…) --[]->`) is
                // normalized to an ImplDef by the parser. Its AST deliberately
                // no longer carries enough ordering information to reconstruct
                // markers before `fn`, so preserve that source form verbatim.
                // Written `impl Type { … }` blocks still use canonical fmt.
                let line_start = self.src[..i.span.start]
                    .rfind('\n')
                    .map_or(0, |pos| pos + 1);
                let text = &self.src[line_start..i.span.end];
                let external_head = format!("fn {}.", i.type_name);
                if text.contains(&external_head) {
                    self.write(text);
                    self.newline();
                    self.skip_verbatim_comments(i.span.end);
                } else {
                    self.fmt_impl(i);
                }
            }
            Item::Const(c) => self.fmt_const(c),
            Item::Test(t) => self.fmt_test(t),
            Item::Bench(b) => self.fmt_bench(b),
            Item::ExternRust(b) => self.fmt_extern_rust(b),
            Item::Trait(t) => self.fmt_trait(t),
            // D-QUAL2: tag declarations are emitted verbatim (non-destructive).
            Item::Tag(t) => {
                let text = self.src[t.span.start..t.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(t.span.end);
            }
            // Stage 1a: modules are emitted verbatim (non-destructive). A
            // canonical module formatter lands with the eval pipeline.
            Item::Module(m) => {
                let text = self.src[m.span.start..m.span.end].to_string();
                self.write(&text);
                self.skip_verbatim_comments(m.span.end);
            }
            // S59: C FFI modules are emitted verbatim (non-destructive). A
            // canonical formatter can land alongside the bind backend.
            Item::CModule(cm) => {
                let text = self.src[cm.span.start..cm.span.end].to_string();
                self.write(&text);
                self.skip_verbatim_comments(cm.span.end);
            }
            // Code modules are emitted verbatim pending a dedicated formatter.
            Item::CodeModule(cm) => {
                let text = self.src[cm.span.start..cm.span.end].to_string();
                self.write(&text);
                self.skip_verbatim_comments(cm.span.end);
            }
            Item::Distinct(d) => self.fmt_distinct(d),
            // D-TYPEALIAS1: type alias declarations are emitted verbatim.
            Item::TypeAlias(a) => {
                let text = self.src[a.span.start..a.span.end].to_string();
                self.write(&text);
                self.skip_verbatim_comments(a.span.end);
            }
            // D-QUAL3: unit-family declarations are emitted verbatim (the sugar
            // surface is preserved; it is not expanded into per-member distincts).
            Item::UnitFamily(uf) => {
                let text = self.src[uf.span.start..uf.span.end].to_string();
                self.write(&text);
                self.skip_verbatim_comments(uf.span.end);
            }
            // D-ERR-CONV: error conversion declarations are emitted verbatim.
            Item::ErrorConv(ec) => {
                let text = self.src[ec.body_span.start..ec.body_span.end].to_string();
                // Re-emit as `impl From -> To { body }` verbatim for now.
                self.write("impl ");
                self.write(&ec.from_ty);
                self.write(" -> ");
                self.write(&ec.to_ty);
                self.write(" ");
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(ec.body_span.end);
            }
            // D-MIGRATE1: migration blocks are emitted verbatim (non-destructive).
            Item::Migration(m) => {
                let text = self.src[m.span.start..m.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(m.span.end);
            }
            // D-STATE-DECL: state-set declarations are emitted verbatim (non-destructive).
            Item::StateDecl(s) => {
                let text = self.src[s.span.start..s.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(s.span.end);
            }
            // D-PROTO1/D-PROTO2: protocol blocks are emitted verbatim (non-destructive).
            Item::ProtocolDecl(p) => {
                let text = self.src[p.span.start..p.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(p.span.end);
            }
            // D-METADERIVE1=A: derive blocks are emitted verbatim (non-destructive).
            Item::UserDerive(d) => {
                let text = self.src[d.span.start..d.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(d.span.end);
            }
            // D-GENMOD2=A: generic module templates emitted verbatim (non-destructive)
            // apart from the `pub`/`pub(package)` qualifier, which the span excludes
            // (it starts at the `module` keyword) and must be re-added explicitly.
            Item::GenericModule(gm) => {
                self.fmt_pub_qualifier(gm.is_pub, gm.is_package_pub);
                let text = self.src[gm.span.start..gm.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(gm.span.end);
            }
            // D-GENMOD2=A: module alias declarations emitted verbatim (non-destructive)
            // apart from the `pub`/`pub(package)` qualifier — see Item::GenericModule.
            Item::ModuleAlias(ma) => {
                self.fmt_pub_qualifier(ma.is_pub, ma.is_package_pub);
                let text = self.src[ma.span.start..ma.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(ma.span.end);
            }
        }
    }

    fn fmt_trait(&mut self, t: &crate::AST::TraitDef) {
        self.fmt_pub_qualifier(t.is_pub, t.is_package_pub);
        self.write("trait ");
        self.write(&t.name);
        self.write(" ");
        self.write(Syntax::BLOCK_OPEN);
        self.newline();
        self.with_indent(|f| {
            // D-LIB2: `type Name` associated-type declarations. These were
            // being dropped entirely — `fmt_trait` only ever walked
            // `t.methods`, so a trait's `type Elem` line silently vanished
            // on every fmt pass (a real token-dropping bug, caught
            // reformatting examples/features/types/associated_types.jet).
            for (name, _) in &t.assoc_types {
                f.write("type ");
                f.write(name);
                f.newline();
            }
            for m in &t.methods {
                f.write("fn ");
                f.write(&m.name);
                f.write("(");
                for (i, p) in m.params.iter().enumerate() {
                    if i > 0 {
                        f.write(", ");
                    }
                    f.fmt_param(p);
                }
                f.write(")");
                // D-EFF3 / D-SHAPE8: effect bound inside the arrow.
                if let Some(effects) = &m.declared_effects {
                    let list = effects
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    f.write(&format!(" --[{}]->", list));
                } else if m.is_pure {
                    f.write(" --[]->");
                } else if m.return_type.is_some() {
                    f.write(" ->");
                }
                if let Some(ret) = &m.return_type {
                    f.write(" ");
                    f.fmt_return_type(ret);
                }
                // D-LIB2: a trait method may carry a default body.
                if let Some(body) = &m.default_body {
                    f.write(" {");
                    f.newline();
                    f.with_indent(|f| f.fmt_block_stmts(body));
                    f.end_block();
                }
                f.newline();
            }
        });
        self.write(Syntax::BLOCK_CLOSE);
        self.newline();
        self.newline();
    }

    fn fmt_extern_rust(&mut self, block: &ExternRustBlock) {
        self.write(Syntax::KW_EXTERN);
        self.write(" ");
        self.write(Syntax::KW_RUST);
        self.write(" \"");
        self.write(&block.crate_spec);
        self.write("\" ");
        self.write(Syntax::BLOCK_OPEN);
        self.newline();
        self.with_indent(|f| {
            for ef in &block.functions {
                f.fmt_extern_fn(ef);
            }
        });
        self.end_block();
    }

    fn fmt_extern_fn(&mut self, ef: &ExternFn) {
        if let Some((abi, _)) = &ef.abi {
            self.write(&format!("#{}({}) ", Syntax::ATTR_ABI, abi));
        }
        self.write("fn ");
        self.write(&ef.name);
        self.write("(");
        for (i, p) in ef.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.fmt_param(p);
        }
        self.write(")");
        if let Some(ret) = &ef.return_type {
            self.write(" -> ");
            self.fmt_return_type(ret);
        }
        self.write(" = \"");
        self.write(&ef.rust_path);
        self.write("\"");
        // S6-R: no visible `;` — the synthetic terminator ends the declaration.
        self.newline();
    }

    fn fmt_test(&mut self, t: &crate::AST::TestDef) {
        self.write(&format!("#{}", Syntax::KW_TEST));
        // D-TESTPAREN1=A: unit-test block form is `#Test("name") { … }` — no space before `(`.
        // D-TEST1: property test form is `#Test fn name(params) { … }` — space before `fn`.
        if t.params.is_empty() && t.fn_keyword_span.is_none() {
            self.write("(");
            if let Some(name) = &t.name_expr {
                self.fmt_expr(name, Prec::OrFallback);
            } else {
                self.write("\"");
                self.write(
                    &t.name
                        .as_deref()
                        .expect("synthetic test blocks have a resolved name")
                        .replace('\\', "\\\\")
                        .replace('"', "\\\""),
                );
                self.write("\"");
            }
            self.write(")");
        } else {
            self.write(" fn ");
            self.write(
                t.name
                    .as_deref()
                    .expect("property tests have a parsed name"),
            );
            self.write("(");
            for (i, p) in t.params.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.fmt_param(p);
            }
            self.write(")");
        }
        self.write(" ");
        self.write(Syntax::BLOCK_OPEN);
        self.newline();
        self.with_indent(|f| f.fmt_block_stmts(&t.body));
        self.end_block();
    }

    fn fmt_bench(&mut self, b: &crate::AST::BenchDef) {
        self.write(&format!("#{}", Syntax::KW_BENCH));
        // D-BENCH-MARKER1=A: benchmark blocks use the same parenthesized
        // marker-argument shape as `#Test("name")`.
        self.write("(");
        self.fmt_expr(&b.name_expr, Prec::OrFallback);
        self.write(")");
        self.write(" ");
        self.write(Syntax::BLOCK_OPEN);
        self.newline();
        self.with_indent(|f| f.fmt_block_stmts(&b.body));
        self.end_block();
    }

    fn fmt_pub_qualifier(&mut self, is_pub: bool, is_package_pub: bool) {
        if is_package_pub {
            self.write("pub(package) ");
            return;
        }
        if self.pub_file {
            if !is_pub {
                self.write("priv ");
            }
        } else if is_pub {
            self.write("pub ");
        }
    }

    fn fmt_distinct(&mut self, d: &crate::AST::DistinctDef) {
        if d.type_markers.len() == 1 {
            self.write("#");
            self.fmt_marker(&d.type_markers[0]);
            self.write(" ");
        } else if !d.type_markers.is_empty() {
            self.write("#[");
            for (index, marker) in d.type_markers.iter().enumerate() {
                if index > 0 {
                    self.write(", ");
                }
                self.fmt_marker(marker);
            }
            self.write("] ");
        }
        self.fmt_pub_qualifier(d.is_pub, d.is_package_pub);
        self.write(&d.name);
        self.write(" :: distinct ");
        self.fmt_type(&d.base);
        if d.invariant.is_none() {
            if let Some((low, high, _)) = d.range {
                self.write("(");
                self.write(&low.to_string());
                self.write("..");
                self.write(&high.to_string());
                self.write(")");
            }
        }
        self.write(";");
    }

    fn fmt_type_params(&mut self, params: &[TypeParam]) {
        self.write(&crate::Generics::format_type_params(params));
    }

    fn fmt_derive_line(&mut self, trait_name: &str) {
        self.write("derive ");
        self.write(trait_name);
    }

    /// D-SHAPE2 / D-SERDE2–8: render one applied rule.
    pub(super) fn fmt_marker(&mut self, m: &Marker) {
        self.write(&m.name);
        if !m.args.is_empty() {
            self.write("(");
            for (i, a) in m.args.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                if let Some((label, _)) = m.arg_labels.get(i).and_then(Option::as_ref) {
                    self.write(label);
                    self.write(": ");
                }
                self.fmt_expr(a, Prec::OrFallback);
            }
            self.write(")");
        }
    }

    /// D-SHAPE2: render the declaration's one applied-rule group. A lone rule
    /// may use `#A`; multiple rules use `#[A, B]`.
    fn fmt_type_markers(&mut self, markers: &[Marker], lone_hash_ok: bool) {
        if markers.is_empty() {
            return;
        }
        let rules: Vec<&Marker> = markers.iter().collect();
        self.fmt_marker_group(&rules, Syntax::RULE_PREFIX, lone_hash_ok);
    }

    /// One marker-list group on a single plane. `sigil` is `"@"` or `"#"`.
    pub(super) fn fmt_marker_group(&mut self, markers: &[&Marker], sigil: &str, lone_ok: bool) {
        if markers.is_empty() {
            return;
        }
        if markers.len() == 1 && lone_ok {
            self.write(sigil);
            self.fmt_marker(markers[0]);
            self.newline();
            return;
        }
        self.write(sigil);
        self.write("[");
        for (i, m) in markers.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.fmt_marker(m);
        }
        self.write("]");
        self.newline();
    }

    /// D-REPRC1/D-SOA1: `#Layout(c)` / `#Layout(columnar)` on its own line.
    fn fmt_layout(&mut self, layout: &Option<StructLayout>) {
        if let Some(l) = layout {
            let variant = match l {
                StructLayout::C => Syntax::LAYOUT_C,
                StructLayout::Columnar => Syntax::LAYOUT_COLUMNAR,
            };
            self.write(&format!("#{}({})", Syntax::ATTR_LAYOUT, variant));
            self.newline();
        }
    }

    /// Body `derive Name` lines for a type, minus the derives that the leading
    /// `#[…]` rules already account for. `Codable` expands to `Encode`+`Decode`;
    /// `Encode`/`Decode` and any user-trait marker map to themselves. The remaining
    /// `derives` came from `derive Name` lines in the body and must re-emit there.
    fn body_derive_lines(derives: &[(String, Span)], type_markers: &[Marker]) -> Vec<String> {
        let mut covered: Vec<String> = Vec::new();
        for m in type_markers {
            match m.name.as_str() {
                Syntax::ATTR_CODABLE => {
                    covered.push(Syntax::ATTR_ENCODE.to_string());
                    covered.push(Syntax::ATTR_DECODE.to_string());
                }
                // Serde *attribute* markers never reach `derives`; skip them.
                Syntax::ATTR_RENAME_ALL
                | Syntax::ATTR_DENY_UNKNOWN_FIELDS
                | Syntax::ATTR_TAG
                | Syntax::ATTR_UNTAGGED
                | Syntax::ATTR_RENAME
                | Syntax::ATTR_SKIP
                | Syntax::ATTR_DEFAULT
                | Syntax::ATTR_FLATTEN => {}
                other => covered.push(other.to_string()),
            }
        }
        let mut out = Vec::new();
        for (name, _) in derives {
            if let Some(pos) = covered.iter().position(|c| c == name) {
                covered.remove(pos);
            } else {
                out.push(name.clone());
            }
        }
        out
    }

    fn fmt_trait_impl_block(&mut self, block: &TraitImplBlock) {
        self.write("impl ");
        self.write(&block.trait_name);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (i, m) in block.methods.iter().enumerate() {
                if i > 0 {
                    f.newline();
                    f.newline();
                }
                f.emit_leading(m.name_span.start);
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_func(&mut self, f: &Func, top_level: bool) {
        let ordered_rules = self
            .applied_rules
            .iter()
            .filter(|application| application.target == Some(f.span))
            .map(|application| application.marker.clone())
            .collect::<Vec<_>>();
        if !ordered_rules.is_empty() {
            let rules = ordered_rules.iter().collect::<Vec<_>>();
            if rules.len() == 1 {
                self.write(Syntax::RULE_PREFIX);
                self.fmt_marker(rules[0]);
                if matches!(
                    rules[0].name.as_str(),
                    Syntax::ATTR_META
                        | Syntax::KW_UNSAFE
                        | Syntax::ATTR_FFI
                        | Syntax::ATTR_TARGET
                        | Syntax::ATTR_WASM_EXPORT
                ) {
                    self.newline();
                } else {
                    self.write(" ");
                }
            } else {
                self.write("#[");
                for (index, rule) in rules.iter().enumerate() {
                    if index > 0 {
                        self.write(", ");
                    }
                    self.fmt_marker(rule);
                }
                self.write("]");
                self.write(" ");
            }
        } else {
        let policies = self.policy_declarations.iter().filter(|d| d.scope == crate::Policy::PolicyScope::Function && d.target == Some(f.span) && d.key != crate::Policy::PolicyKey::Unsafe).cloned().collect::<Vec<_>>();
        let unsafe_site_mode = self.policy_declarations.iter().find(|declaration| declaration.key == crate::Policy::PolicyKey::Unsafe && declaration.target == Some(f.span)).map(|declaration| declaration.value);
        let rule_count = usize::from(!policies.is_empty())
            + usize::from(f.meta.is_some())
            + usize::from(f.is_unsafe)
            + usize::from(f.inline_foreign.is_some())
            + usize::from(f.web_marker.is_some())
            + usize::from(f.is_reactive)
            + usize::from(f.is_sanitizer)
            + usize::from(f.is_replayable)
            + usize::from(f.is_task)
            + usize::from(f.every.is_some())
            + usize::from(f.is_must_use)
            + usize::from(f.is_inline || f.is_inline_always)
            + usize::from(f.state_requires.is_some())
            + usize::from(f.state_transition.is_some())
            + f.pre.len()
            + f.post.len();
        let grouped = rule_count > 1;
        let mut rule_index = 0usize;
        if grouped {
            self.write("#[");
        }
        macro_rules! start_rule {
            () => {{
                if grouped {
                    if rule_index > 0 {
                        self.write(", ");
                    }
                } else {
                    self.write("#");
                }
                rule_index += 1;
            }};
        }
        if !policies.is_empty() {
            start_rule!();
            self.fmt_policy_rule(&policies);
        }
        if let Some(meta) = &f.meta {
            start_rule!();
            self.fmt_meta_rule(meta);
        }
        if f.is_unsafe {
            start_rule!();
            match (&f.unsafe_reason, unsafe_site_mode) {
                (Some(reason), Some(mode)) => self.write(&format!("{}(\"{}\", obligations: {})", Syntax::KW_UNSAFE, escape_str_lit(reason), mode.display())),
                (None, Some(mode)) => self.write(&format!("{}(obligations: {})", Syntax::KW_UNSAFE, mode.display())),
                (Some(reason), None) => self.write(&format!("{}(\"{}\")", Syntax::KW_UNSAFE, escape_str_lit(reason))),
                (None, None) => self.write(Syntax::KW_UNSAFE),
            }
        }
        if let Some(inline) = &f.inline_foreign {
            start_rule!();
            self.write(&format!("{}({})", Syntax::ATTR_FFI, inline.lang));
        }
        if let Some(marker) = f.web_marker {
            start_rule!();
            self.write(marker.name());
        }
        for (enabled, name) in [
            (f.is_reactive, Syntax::KW_REACTIVE),
            (f.is_sanitizer, Syntax::KW_SANITIZER),
            (f.is_replayable, Syntax::ATTR_REPLAYABLE),
            (f.is_task, Syntax::KW_TASK),
        ] {
            if enabled {
                start_rule!();
                self.write(name);
            }
        }
        if let Some(every) = &f.every {
            start_rule!();
            self.write(&format!("{}(", Syntax::ATTR_EVERY));
            match &every.arg {
                crate::AST::EveryArg::Duration { int, float, suffix, .. } => {
                    if let Some(n) = int {
                        self.write(&n.to_string());
                    } else if let Some(v) = float {
                        self.write(&fmt_float(*v));
                    }
                    self.write(suffix);
                }
                crate::AST::EveryArg::WallClock { text, .. } => {
                    self.write("\"");
                    self.write(&escape_str_lit(text));
                    self.write("\"");
                }
                crate::AST::EveryArg::Expression(expression) => {
                    self.fmt_expr(expression, super::Prec::OrFallback)
                }
            }
            self.write(")");
        }
        if f.is_must_use {
            start_rule!();
            self.write(Syntax::ATTR_MUST_USE);
        }
        if f.is_inline_always {
            start_rule!();
            self.write(&format!("{}(Always)", Syntax::CONTRACT_INLINE));
        } else if f.is_inline {
            start_rule!();
            self.write(Syntax::CONTRACT_INLINE);
        }
        if let Some((state, _)) = &f.state_requires {
            start_rule!();
            self.write(&format!("{}({})", Syntax::KW_STATE, state));
        }
        if let Some(transition) = &f.state_transition {
            start_rule!();
            self.write(&format!(
                "{}({}, {})",
                Syntax::KW_TRANSITION,
                transition.from.as_deref().unwrap_or(Syntax::STATE_ENTRY),
                transition.to
            ));
        }
        for (name, clause) in f.pre.iter().map(|clause| (Syntax::CONTRACT_PRE, clause))
            .chain(f.post.iter().map(|clause| (Syntax::CONTRACT_POST, clause)))
        {
            start_rule!();
            self.write(&format!("{name}("));
            self.fmt_expr(&clause.cond, Prec::OrFallback);
            self.write(", ");
            self.fmt_expr(&clause.message_expr, Prec::OrFallback);
            self.write(")");
        }
        if grouped {
            self.write("] ");
        } else if rule_index == 1 {
            self.write(" ");
        }
        }
        if top_level {
            self.fmt_pub_qualifier(f.is_pub, f.is_package_pub);
        } else if f.is_pub {
            self.fmt_pub_qualifier(f.is_pub, f.is_package_pub);
        }
        self.write("fn ");
        self.write(&f.name);
        self.fmt_type_params(&f.type_params);
        self.write("(");
        for (i, p) in f.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.fmt_param(p);
        }
        self.write(")");
        // D-SHAPE8: effect row lives inside the return arrow.
        if let Some(effects) = &f.declared_effects {
            self.write(" --[");
            for (i, (name, _)) in effects.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(name);
            }
            self.write("]->");
        } else if f.is_pure {
            self.write(" --[]->");
        }
        // D-EFF2: pass-through occupies the same row.
        if let Some((param, _)) = &f.effect_via {
            self.write(" --[");
            self.write(Syntax::KW_VIA);
            self.write(" ");
            self.write(param);
            self.write("]->");
        }
        if f.declared_effects.is_none() && f.effect_via.is_none() && f.return_type.is_some() {
            self.write(" ->");
        }
        if let Some(ret) = &f.return_type {
            self.write(" ");
            self.fmt_return_type(ret);
        }
        // D-FFI-INLINE1=A (card #501): an inline foreign fn's body is a single
        // foreign-source string. Reconstruct the string expression and reuse the
        // ordinary string formatter so the triple-quoted shape round-trips
        // exactly (via `self.src` + span), then close the block on its own line.
        if let Some(inl) = &f.inline_foreign {
            self.write(" {");
            self.indent += 1;
            self.newline();
            self.write("\"\"\"");
            self.write(&inl.source);
            self.write("\"\"\"");
            self.indent -= 1;
            self.newline();
            self.write("}");
            return;
        }
        self.write(" {");
        // D-FMT1: a one-line `fn` body the author wrote inline survives.
        self.fmt_body(&f.body);
    }

    pub(super) fn fmt_policy_declarations(&mut self, declarations: &[crate::Policy::PolicyDeclaration]) {
        self.write("#");
        self.fmt_policy_rule(declarations);
    }

    fn fmt_policy_rule(&mut self, declarations: &[crate::Policy::PolicyDeclaration]) {
        self.write(&format!("{}(", Syntax::ATTR_POLICY));
        for (i, declaration) in declarations.iter().enumerate() {
            if i > 0 { self.write(", "); }
            self.write(declaration.key.name());
            if let crate::Policy::PolicyValue::Limit(limit) = declaration.value { self.write(&format!("({limit})")); }
        }
        self.write(")");
    }

    fn fmt_param(&mut self, p: &Param) {
        // D-MEM1: capability is a sigil, never a word. The sigil rides the type
        // (`name: &Type`), or `self` for a receiver (`&self`). `Read` is
        // unmarked.
        let sigil = match p.convention {
            AccessConvention::Write => Some(Syntax::SIGIL_WRITE),
            AccessConvention::Move => Some(Syntax::SIGIL_MOVE),
            AccessConvention::Read => None,
        };
        let is_self_receiver = p.name == Syntax::KW_SELF && p.ty.name().is_empty();
        if is_self_receiver {
            // `&self` / `^self`: the sigil attaches to `self`, no type printed.
            if let Some(s) = sigil {
                self.write(s);
            }
            self.write(&p.name);
        } else {
            self.write(&p.name);
            self.write(": ");
            if let Some(s) = sigil {
                self.write(s);
            }
            // D-VARIADIC1: `name: ...T` — the rest-parameter marker.
            // D-ANY-JAI1/D-VARARGBOUND1: `...Trait` (bare — falls through to the
            // ordinary `fmt_type` below, same as a concrete `...T`) or
            // `...[TraitA, TraitB]` (the explicit bound-list form).
            if p.variadic {
                self.write("...");
            }
            match &p.variadic_bound_list {
                Some(bounds) => {
                    self.write("[");
                    self.write(&bounds.join(", "));
                    self.write("]");
                }
                None => self.fmt_type(&p.ty),
            }
        }
        // S61: a trailing parameter may carry a `= default` value.
        if let Some(default) = &p.default {
            self.write(" = ");
            self.fmt_expr(default, Prec::OrFallback);
        }
    }

    fn fmt_struct(&mut self, s: &StructDef, top_level: bool) {
        // D-SHAPE2: the leading `#[…]` applied-rule list, verbatim.
        let lone_hash_ok =
            s.layout.is_none() && !s.is_published_schema && !s.is_single_use && !s.is_must_use;
        self.fmt_type_markers(&s.type_markers, lone_hash_ok);
        // D-LIN1: `#SingleUse` precedes `pub`/`struct`, on the same line.
        if s.is_single_use {
            self.write(&format!("#{} ", Syntax::ATTR_SINGLE_USE));
        }
        // D-MUSTUSE1 (c18iwxqx): `#MustUse` precedes `pub`/`struct`, on the same line.
        if s.is_must_use {
            self.write(&format!("#{} ", Syntax::ATTR_MUST_USE));
        }
        // D-MIGRATE1: `#PublishedSchema` precedes `pub`/`struct`, on the same line.
        // A bracket marker list keeps PublishedSchema in `type_markers` while
        // also setting the semantic flag. Emit the dedicated inline spelling
        // only for the standalone `#PublishedSchema struct` parse path.
        if s.is_published_schema
            && !s
                .type_markers
                .iter()
                .any(|marker| marker.name == Syntax::ATTR_PUBLISHED_SCHEMA)
        {
            self.write(&format!("#{} ", Syntax::ATTR_PUBLISHED_SCHEMA));
        }
        // D-REPRC1/D-SOA1: `#Layout(…)` sits on its own line before the struct.
        self.fmt_layout(&s.layout);
        if top_level {
            self.fmt_pub_qualifier(s.is_pub, s.is_package_pub);
        }
        self.write("struct ");
        self.write(&s.name);
        self.fmt_type_params(&s.type_params);
        self.write(" {");
        self.newline();
        // Only `derive Name` lines the user wrote in the body re-emit here; derives
        // lifted from the `#[…]` list are already rendered above.
        let body_derives = Self::body_derive_lines(&s.derives, &s.type_markers);
        self.with_indent(|f| {
            for (i, field) in s.fields.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                f.emit_leading(field.name_span.start);
                f.fmt_field(field);
            }
            for (i, trait_name) in body_derives.iter().enumerate() {
                if i > 0 || !s.fields.is_empty() {
                    f.newline();
                }
                f.fmt_derive_line(trait_name);
            }
            for (i, block) in s.trait_impls.iter().enumerate() {
                if i > 0 || !s.fields.is_empty() || !body_derives.is_empty() {
                    f.newline();
                    f.newline();
                }
                f.fmt_trait_impl_block(block);
            }
            for (i, m) in s.methods.iter().enumerate() {
                if i > 0
                    || !s.fields.is_empty()
                    || !body_derives.is_empty()
                    || !s.trait_impls.is_empty()
                {
                    f.newline();
                    f.newline();
                }
                f.emit_leading(m.name_span.start);
                f.fmt_func(m, false);
            }
            // D-VALIDATE1 (card #506): `validate { … }` in-body block — last
            // in the struct body, same spacing rule as the sections above.
            if !s.validate_block.is_empty() {
                if !s.fields.is_empty()
                    || !body_derives.is_empty()
                    || !s.trait_impls.is_empty()
                    || !s.methods.is_empty()
                {
                    f.newline();
                    f.newline();
                }
                f.write(Syntax::KW_VALIDATE_BLOCK);
                f.write(" {");
                f.fmt_body(&s.validate_block);
            }
        });
        self.end_block();
    }

    fn fmt_enum(&mut self, e: &EnumDef, top_level: bool) {
        // D-SHAPE2: leading `#[…]` applied-rule list, verbatim.
        let lone_hash_ok = !e.is_single_use && !e.is_must_use;
        self.fmt_type_markers(&e.type_markers, lone_hash_ok);
        // D-LIN1: `#SingleUse` precedes `pub`/`enum`, on the same line.
        if e.is_single_use {
            self.write(&format!("#{} ", Syntax::ATTR_SINGLE_USE));
        }
        // D-MUSTUSE1 (c18iwxqx): `#MustUse` precedes `pub`/`enum`, on the same line.
        if e.is_must_use {
            self.write(&format!("#{} ", Syntax::ATTR_MUST_USE));
        }
        if top_level {
            self.fmt_pub_qualifier(e.is_pub, e.is_package_pub);
        }
        self.write("enum ");
        self.write(&e.name);
        self.fmt_type_params(&e.type_params);
        self.write(" {");
        self.newline();
        let body_derives = Self::body_derive_lines(&e.derives, &e.type_markers);
        self.with_indent(|f| {
            if e.groups.is_empty() {
                for (i, v) in e.variants.iter().enumerate() {
                    if i > 0 {
                        f.newline();
                    }
                    f.fmt_variant(v);
                }
            } else {
                f.fmt_enum_grouped(e);
            }
            for (i, trait_name) in body_derives.iter().enumerate() {
                if i > 0 || !e.variants.is_empty() {
                    f.newline();
                }
                f.fmt_derive_line(trait_name);
            }
            for (i, block) in e.trait_impls.iter().enumerate() {
                if i > 0 || !e.variants.is_empty() || !body_derives.is_empty() {
                    f.newline();
                    f.newline();
                }
                f.fmt_trait_impl_block(block);
            }
            for (i, m) in e.methods.iter().enumerate() {
                if i > 0
                    || !e.variants.is_empty()
                    || !body_derives.is_empty()
                    || !e.trait_impls.is_empty()
                {
                    f.newline();
                    f.newline();
                }
                f.emit_leading(m.name_span.start);
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_variant(&mut self, v: &Variant) {
        self.fmt_variant_name_and_payload(v, &v.name);
    }

    /// D-TAG1: emit grouped enum bodies from flat leaves + `groups` metadata.
    fn fmt_enum_grouped(&mut self, e: &EnumDef) {
        let entries = Self::enum_entries_at_prefix(e, "");
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            match entry {
                EnumFmtEntry::Leaf(v) => self.fmt_variant(v),
                EnumFmtEntry::Group(g) => self.fmt_enum_group(e, g),
            }
        }
    }

    fn fmt_enum_group(&mut self, e: &EnumDef, g: &EnumGroup) {
        let short = g.path.rsplit('.').next().unwrap_or(g.path.as_str());
        self.write(short);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            let prefix = format!("{}.", g.path);
            let entries = Self::enum_entries_at_prefix(e, &prefix);
            for (i, entry) in entries.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                match entry {
                    EnumFmtEntry::Leaf(v) => {
                        let leaf = v.name.strip_prefix(&prefix).unwrap_or(&v.name);
                        f.fmt_variant_name_and_payload(v, leaf);
                    }
                    EnumFmtEntry::Group(sub) => f.fmt_enum_group(e, sub),
                }
            }
        });
        self.end_block();
    }

    fn fmt_variant_name_and_payload(&mut self, v: &Variant, name: &str) {
        // D-SERDE5: per-variant `#[Rename("x")]` markers sit inline before the name.
        if !v.serde_markers.is_empty() {
            if v.serde_markers.len() == 1 {
                self.write("#");
                self.fmt_marker(&v.serde_markers[0]);
                self.write(" ");
            } else {
                self.write("#[");
                for (i, m) in v.serde_markers.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_marker(m);
                }
                self.write("] ");
            }
        }
        self.write(name);
        match &v.payload {
            VariantPayload::Unit => {}
            VariantPayload::Single(ty, _) => {
                self.write("(");
                self.fmt_type(ty);
                self.write(")");
            }
            VariantPayload::Named(fields) => {
                self.write("(");
                for (i, fld) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&fld.name);
                    self.write(": ");
                    self.fmt_type(&fld.ty);
                }
                self.write(")");
            }
        }
        if let Some(value) = v.discriminant {
            self.write(" = ");
            self.write(&value.to_string());
        }
    }

    fn enum_entries_at_prefix<'b>(e: &'b EnumDef, prefix: &str) -> Vec<EnumFmtEntry<'b>> {
        let mut items: Vec<(usize, EnumFmtEntry<'b>)> = Vec::new();
        for g in &e.groups {
            if !Self::is_direct_child_path(&g.path, prefix) {
                continue;
            }
            let idx = e
                .variants
                .iter()
                .position(|v| v.name.starts_with(&format!("{}.", g.path)))
                .unwrap_or(usize::MAX);
            items.push((idx, EnumFmtEntry::Group(g)));
        }
        for (i, v) in e.variants.iter().enumerate() {
            if Self::is_direct_child_path(&v.name, prefix) {
                items.push((i, EnumFmtEntry::Leaf(v)));
            }
        }
        items.sort_by_key(|(idx, _)| *idx);
        items.into_iter().map(|(_, entry)| entry).collect()
    }

    fn is_direct_child_path(path: &str, prefix: &str) -> bool {
        if !path.starts_with(prefix) {
            return false;
        }
        let rest = path.strip_prefix(prefix).unwrap_or(path);
        !rest.contains('.')
    }

    fn fmt_impl(&mut self, i: &ImplDef) {
        // D-OSTARGET1=A (ratified 2026-07-01, c134): `#Target(Os.Linux|Os.Macos|Os.Windows)`
        // precedes the `impl` block it gates, on its own line.
        if let Some(os) = i.os_target {
            self.write(&format!(
                "#{}({}.{})",
                Syntax::ATTR_TARGET,
                Syntax::TARGET_OS_NAMESPACE,
                os.name()
            ));
            self.newline();
        }
        self.write("impl ");
        self.write(&i.type_name);
        if let Some(tr) = &i.trait_name {
            self.write(".");
            self.write(tr);
        }
        // D-IMPLDOT1=A / S62: `impl Type.Trait using field` — delegation, no method body.
        if let Some(field) = &i.delegation_field {
            self.write(" using ");
            self.write(field);
            return;
        }
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            // D-LIB2: `type Name = ConcreteType` associated-type implementations.
            for (idx, (name, _, ty)) in i.assoc_type_impls.iter().enumerate() {
                if idx > 0 {
                    f.newline();
                }
                f.write("type ");
                f.write(name);
                f.write(" = ");
                f.fmt_type(ty);
            }
            let had_assoc = !i.assoc_type_impls.is_empty();
            for (idx, m) in i.methods.iter().enumerate() {
                if idx > 0 || had_assoc {
                    f.newline();
                    f.newline();
                }
                f.emit_leading(m.name_span.start);
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_const(&mut self, c: &ConstDef) {
        if let Some(meta) = &c.meta {
            self.fmt_meta_attr(meta);
            self.newline();
        }
        if c.is_comptime {
            self.write(Syntax::KW_COMPTIME);
            self.write(" ");
            self.write(&c.name);
            self.write(" = ");
            self.fmt_expr(&c.value, Prec::OrFallback);
            return;
        }
        if matches!(&c.ty, Some(Type::Named(name)) if name == Syntax::TYPE_OUTPUT) {
            self.write(&c.name);
            self.write(": ");
            self.write(Syntax::TYPE_OUTPUT);
            self.write(" :: ");
            self.fmt_expr(&c.value, Prec::OrFallback);
            self.write(";");
            return;
        }
        if matches!(&c.ty, Some(Type::Named(name)) if name == Syntax::TYPE_OUTPUT_DEFAULTS) {
            self.write(Syntax::OUTPUT_DEFAULTS);
            self.write(": ");
            self.fmt_expr(&c.value, Prec::OrFallback);
            self.write(";");
            return;
        }
        // D-PERSIST1: `#Persist` precedes the const's other attrs.
        if c.is_persist {
            self.write(&format!("#{} ", Syntax::CONTRACT_PERSIST));
        }
        for attr in &c.attrs {
            match attr {
                ConstAttr::ForceStatic => self.write("#Static "),
                ConstAttr::ForceInline => self.write("#Inline "),
            }
        }
        self.write("const ");
        self.write(&c.name);
        self.write(" = ");
        self.fmt_expr(&c.value, Prec::OrFallback);
    }

    pub(super) fn fmt_import(&mut self, imp: &ImportDecl) {
        // Imports don't take `priv` — the parser only accepts `use …` (not
        // re-exported, the ambient default whether or not the file is
        // `#PubFile`) or `pub use …` (re-exported). Unlike struct/fn/etc.,
        // there's no `#PubFile`-relative "priv" spelling to fall back to, so
        // this can't route through `fmt_pub_qualifier` (that produced
        // unparseable `priv use …` output — a real fmt idempotence bug).
        if imp.is_package_pub {
            self.write("pub(package) ");
        } else if imp.is_pub {
            self.write("pub ");
        }
        self.write(Syntax::KW_USE);
        self.write(" ");
        match &imp.kind {
            ImportKind::File(path, _) => {
                self.write("\"");
                self.write(path);
                self.write("\"");
                let default_alias = path.rsplit('/').next().unwrap_or("module");
                if imp.alias != default_alias {
                    self.write(" ");
                    self.write(Syntax::KW_AS);
                    self.write(" ");
                    self.write(&imp.alias);
                }
            }
            ImportKind::Module(name, _) => {
                self.write(name);
                // U11 (D-JPK-SCRIPTDEP1=A): `use pkg#version;` inline dep.
                if let Some(v) = &imp.inline_version {
                    self.write("#");
                    self.write(&v.text);
                }
                let default_alias = name.rsplit('.').next().unwrap_or(name.as_str());
                // A dotted `use a.b` with no `as` parses as an *unqualified*
                // item import (`b` from `a`), not a module import. So for a
                // dotted module name the `as alias` is load-bearing even when it
                // equals the last segment — keep it. Only a single-segment name
                // (`use foo`) can safely omit a matching alias.
                let dotted = name.contains('.');
                if imp.alias != default_alias || dotted {
                    self.write(" ");
                    self.write(Syntax::KW_AS);
                    self.write(" ");
                    self.write(&imp.alias);
                }
            }
            ImportKind::Unqualified {
                module_alias,
                items,
                ..
            } => {
                let fmt_item = |(orig, alias): &(String, Option<String>)| {
                    if let Some(a) = alias {
                        format!("{orig} as {a}")
                    } else {
                        orig.clone()
                    }
                };
                if items.len() == 1 {
                    self.write(module_alias);
                    self.write(".");
                    self.write(&fmt_item(&items[0]));
                } else {
                    self.write(module_alias);
                    self.write(".{");
                    let rendered: Vec<String> = items.iter().map(fmt_item).collect();
                    self.write(&rendered.join(", "));
                    self.write("}");
                }
            }
        }
    }

    fn fmt_field(&mut self, field: &Field) {
        // D-SHAPE2: field rules share one inline `#[…]` group. Redact has a
        // dedicated semantic bit, so fold it back into the same group here.
        if field.redact || !field.serde_markers.is_empty() {
            self.write(Syntax::RULE_PREFIX);
            let count = usize::from(field.redact) + field.serde_markers.len();
            if count > 1 {
                self.write("[");
            }
            if field.redact {
                self.write(Syntax::ATTR_REDACT);
            }
            for (i, marker) in field.serde_markers.iter().enumerate() {
                if field.redact || i > 0 {
                    self.write(", ");
                }
                self.fmt_marker(marker);
            }
            if count > 1 {
                self.write("]");
            }
            self.write(" ");
        }
        self.fmt_pub_qualifier(field.is_pub, field.is_package_pub);
        self.write(&field.name);
        self.write(": ");
        self.fmt_type(&field.ty);
        // D-FIELDPOL1: `name: T => expr` — a computed field.
        if let Some(expr) = &field.computed {
            self.write(" => ");
            self.fmt_expr(expr, Prec::OrFallback.add_rhs());
        }
        let end = field
            .computed
            .as_deref()
            .map(|expr| expr.span().end)
            .unwrap_or(field.ty_span.end);
        self.emit_trailing(end);
    }
}
