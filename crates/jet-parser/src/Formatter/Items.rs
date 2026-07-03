use super::*;
use crate::AST::{
    AccessConvention, ConstAttr, ConstDef, EnumDef, ExternFn, ExternRustBlock, Field, Func,
    ImplDef, ImportDecl, ImportKind, Item, Marker, Param, StructDef, StructLayout, TraitImplBlock,
    TypeParam, Variant, VariantPayload,
};

impl<'a> Fmt<'a> {
    pub(super) fn fmt_item(&mut self, item: &Item) {
        match item {
            Item::Func(f) => self.fmt_func(f, true),
            Item::Struct(s) => self.fmt_struct(s, true),
            Item::Enum(e) => self.fmt_enum(e, true),
            Item::Impl(i) => self.fmt_impl(i),
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
            // D-DIST1: distinct type declarations are emitted verbatim.
            Item::Distinct(d) => {
                let text = self.src[d.span.start..d.span.end].to_string();
                self.write(&text);
                self.skip_verbatim_comments(d.span.end);
            }
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
            // D-GENMOD2=A: generic module templates emitted verbatim (non-destructive).
            Item::GenericModule(gm) => {
                let text = self.src[gm.span.start..gm.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(gm.span.end);
            }
            // D-GENMOD2=A: module alias declarations emitted verbatim (non-destructive).
            Item::ModuleAlias(ma) => {
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
                // D-EFF3: `@Pure` prefix bound on a trait method.
                if m.is_pure {
                    f.write(&format!("@{} ", Syntax::KW_PURE));
                }
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
                // D-EFF3: `#(Gpu)` effect bound between params and the arrow.
                if let Some(effects) = &m.declared_effects {
                    let list = effects
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    f.write(&format!(" #({})", list));
                }
                if let Some(ret) = &m.return_type {
                    f.write(" -> ");
                    // D-CAP7: a borrowed trait-method return renders the `&` sigil.
                    if m.is_view_return {
                        f.write(Syntax::SIGIL_VIEW);
                    }
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
            if ef.is_view_return {
                self.write(Syntax::SIGIL_VIEW);
            }
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
            self.write("(\"");
            self.write(&t.name.replace('\\', "\\\\").replace('"', "\\\""));
            self.write("\")");
        } else {
            self.write(" fn ");
            self.write(&t.name);
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
        self.write(" ");
        self.write("\"");
        self.write(&b.name.replace('\\', "\\\\").replace('"', "\\\""));
        self.write("\"");
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

    fn fmt_type_params(&mut self, params: &[TypeParam]) {
        self.write(&crate::Generics::format_type_params(params));
    }

    fn fmt_derive_line(&mut self, trait_name: &str) {
        self.write("derive ");
        self.write(trait_name);
    }

    /// D-ATTR2 / D-SERDE2–8: render one `#[…]` marker (name + optional `(args)`).
    fn fmt_marker(&mut self, m: &Marker) {
        self.write(&m.name);
        if !m.args.is_empty() {
            self.write("(");
            for (i, a) in m.args.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.fmt_expr(a, Prec::OrFallback);
            }
            self.write(")");
        }
    }

    /// D-MARKER-FAMILY1/G2: render leading type-marker prefixes, split into the
    /// contract plane (`@`) and directive/serde plane (`#`) — one marker on a
    /// plane is `@A`/`#A` when it sits immediately before `struct`/`enum`;
    /// otherwise `@[A, B]`/`#[A, B]` (D-ATTR1/D-ATTR2). Both groups may stack
    /// on one declaration (`@[Codable] #[RenameAll(camel)] struct …`).
    fn fmt_type_markers(&mut self, markers: &[Marker], lone_hash_ok: bool) {
        if markers.is_empty() {
            return;
        }
        let contract: Vec<&Marker> = markers
            .iter()
            .filter(|m| Syntax::is_contract_marker(&m.name))
            .collect();
        let directive: Vec<&Marker> = markers
            .iter()
            .filter(|m| !Syntax::is_contract_marker(&m.name))
            .collect();
        self.fmt_marker_group(&contract, Syntax::CONTRACT_PREFIX, lone_hash_ok);
        self.fmt_marker_group(&directive, Syntax::ATTR_PREFIX, lone_hash_ok);
    }

    /// One marker-list group on a single plane. `sigil` is `"@"` or `"#"`.
    fn fmt_marker_group(&mut self, markers: &[&Marker], sigil: &str, lone_ok: bool) {
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
    /// `#[…]` markers already account for. `Codable` expands to `Encode`+`Decode`;
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
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_func(&mut self, f: &Func, top_level: bool) {
        // S58 (E2-M13): `#Unsafe` whole-function contract sits on its own line.
        if f.is_unsafe {
            self.write(&format!("#{}", Syntax::KW_UNSAFE));
            self.newline();
        }
        // D-WASM1: `#Wasm` / `#Js` / `#WasmExport` per-function web partition
        // override, on its own line before `fn`/`pub` (the convention every
        // web example uses; the parser skips the lexer-inserted `;` between
        // marker lines). fmt used to drop this marker entirely — every
        // browser-side function silently fell back to the Wasm bucket,
        // breaking the cross-partition checks in tests/web_build.rs.
        if let Some(marker) = f.web_marker {
            self.write(&format!("#{}", marker.name()));
            self.newline();
        }
        // D-REACTCORE1: `#Reactive fn` marker precedes `pub`/`fn`.
        if f.is_reactive {
            self.write(&format!("#{} ", Syntax::KW_REACTIVE));
        }
        // S60 (D-CASING1 follow-on): `@Pure` marker precedes `pub`/`fn`.
        if f.is_pure {
            self.write(&format!("@{} ", Syntax::KW_PURE));
        }
        // D-TAINT1: the `#Sanitizer` taint-strip modifier precedes `pub`/`fn`.
        if f.is_sanitizer {
            self.write(&format!("#{} ", Syntax::KW_SANITIZER));
        }
        // D-MUSTUSE1 (c18iwxqx): `@MustUse fn` / method precedes `pub`/`fn`.
        if f.is_must_use {
            self.write(&format!("@{} ", Syntax::ATTR_MUST_USE));
        }
        // D-METHODMACRO1=A: `@Inline`/`@InlineAlways` precedes `pub`/`fn`, in
        // the same slot the parser checks (after `@MustUse`/`@Pure`/
        // `#Sanitizer`, before the typestate markers).
        if f.is_inline_always {
            self.write(&format!("@{} ", Syntax::CONTRACT_INLINE_ALWAYS));
        } else if f.is_inline {
            self.write(&format!("@{} ", Syntax::CONTRACT_INLINE));
        }
        // D-STATE1: typestate markers precede `pub`/`fn`. `#State(S)` is the
        // require-state guard; `#Transition(From -> To)` is the transition (the
        // from-state is `_` for an entry transition). Round-tripped verbatim so
        // `jet fmt` never drops a typestate contract.
        if let Some((state, _)) = &f.state_requires {
            self.write(&format!("#{}({}) ", Syntax::KW_STATE, state));
        }
        if let Some(tr) = &f.state_transition {
            let from = tr.from.as_deref().unwrap_or(Syntax::STATE_ENTRY);
            self.write(&format!(
                "#{}({} -> {}) ",
                Syntax::KW_TRANSITION,
                from,
                tr.to
            ));
        }
        // D-PREPOST1: `@Pre(cond, "msg")` / `@Post(cond, "msg")` clauses
        // precede `pub`/`fn`, one call-shaped marker per clause, in source
        // order — same inline-marker convention as the typestate markers
        // above (not each on its own line: I8, one marker-placement rule).
        for clause in &f.pre {
            self.write(&format!("@{}(", Syntax::CONTRACT_PRE));
            self.fmt_expr(&clause.cond, Prec::OrFallback);
            self.write(", \"");
            self.write(&clause.message.replace('\\', "\\\\").replace('"', "\\\""));
            self.write("\") ");
        }
        for clause in &f.post {
            self.write(&format!("@{}(", Syntax::CONTRACT_POST));
            self.fmt_expr(&clause.cond, Prec::OrFallback);
            self.write(", \"");
            self.write(&clause.message.replace('\\', "\\\\").replace('"', "\\\""));
            self.write("\") ");
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
        // D-EFF1 / D-QUAL1: the `#(Net, Db)` effect bound, between the parameter
        // list and the return arrow. A `@Pure fn` carries no `#(…)` list.
        if let Some(effects) = &f.declared_effects {
            self.write(" #(");
            for (i, (name, _)) in effects.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(name);
            }
            self.write(")");
        }
        // D-EFF2: the `#(via f)` pass-through annotation occupies the same slot.
        if let Some((param, _)) = &f.effect_via {
            self.write(" #(");
            self.write(Syntax::KW_VIA);
            self.write(" ");
            self.write(param);
            self.write(")");
        }
        if let Some(ret) = &f.return_type {
            self.write(" -> ");
            if f.is_view_return {
                self.write(Syntax::SIGIL_VIEW);
            }
            self.fmt_return_type(ret);
        }
        self.write(" {");
        // D-FMT1: a one-line `fn` body the author wrote inline survives.
        self.fmt_body(&f.body);
    }

    fn fmt_param(&mut self, p: &Param) {
        // D-CAP7: capability is a sigil, never a word. The sigil rides the type
        // (`name: ~Type`), or `self` for a receiver (`~self`). `Read`/`Infer` are
        // unmarked; `Share`/`Raw` aren't produced on parameters yet.
        let sigil = match p.convention {
            AccessConvention::Write => Some(Syntax::SIGIL_MUTATE),
            AccessConvention::Move => Some(Syntax::SIGIL_MOVE),
            AccessConvention::Read
            | AccessConvention::Infer
            | AccessConvention::Share
            | AccessConvention::Raw => None,
        };
        let is_self_receiver = p.name == Syntax::KW_SELF && p.ty.name().is_empty();
        if is_self_receiver {
            // `~self` / `^self`: the sigil attaches to `self`, no type printed.
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
        // D-ATTR2/D-SERDE: the leading `#[…]` bracket-marker list (derives +
        // container serde attrs), verbatim as the user wrote it.
        let lone_hash_ok =
            s.layout.is_none() && !s.is_published_schema && !s.is_single_use && !s.is_must_use;
        self.fmt_type_markers(&s.type_markers, lone_hash_ok);
        // D-LIN1: `#SingleUse` precedes `pub`/`struct`, on the same line.
        if s.is_single_use {
            self.write(&format!("#{} ", Syntax::ATTR_SINGLE_USE));
        }
        // D-MUSTUSE1 (c18iwxqx): `@MustUse` precedes `pub`/`struct`, on the same line.
        if s.is_must_use {
            self.write(&format!("@{} ", Syntax::ATTR_MUST_USE));
        }
        // D-MIGRATE1: `@PublishedSchema` precedes `pub`/`struct`, on the same line.
        if s.is_published_schema {
            self.write(&format!("@{} ", Syntax::ATTR_PUBLISHED_SCHEMA));
        }
        // D-REPRC1/D-SOA1: `#layout(…)` sits on its own line before the struct.
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
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_enum(&mut self, e: &EnumDef, top_level: bool) {
        // D-ATTR2/D-SERDE: leading `#[…]` bracket-marker list, verbatim.
        let lone_hash_ok = !e.is_single_use && !e.is_must_use;
        self.fmt_type_markers(&e.type_markers, lone_hash_ok);
        // D-LIN1: `#SingleUse` precedes `pub`/`enum`, on the same line.
        if e.is_single_use {
            self.write(&format!("#{} ", Syntax::ATTR_SINGLE_USE));
        }
        // D-MUSTUSE1 (c18iwxqx): `@MustUse` precedes `pub`/`enum`, on the same line.
        if e.is_must_use {
            self.write(&format!("@{} ", Syntax::ATTR_MUST_USE));
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
            for (i, v) in e.variants.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                f.fmt_variant(v);
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
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_variant(&mut self, v: &Variant) {
        // D-SERDE5: per-variant `#[Rename("x")]` markers sit inline before the name.
        if !v.serde_markers.is_empty() {
            self.write("#[");
            for (i, m) in v.serde_markers.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.fmt_marker(m);
            }
            self.write("] ");
        }
        self.write(&v.name);
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
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_const(&mut self, c: &ConstDef) {
        if c.is_comptime {
            self.write(Syntax::KW_COMPTIME);
            self.write(" ");
            self.write(&c.name);
            self.write(" = ");
            self.fmt_expr(&c.value, Prec::OrFallback);
            return;
        }
        // D-PERSIST1: `@Persist` precedes the const's other attrs.
        if c.is_persist {
            self.write(&format!("@{} ", Syntax::CONTRACT_PERSIST));
        }
        for attr in &c.attrs {
            match attr {
                ConstAttr::ForceStatic => self.write("#static "),
                ConstAttr::ForceInline => self.write("#inline "),
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

    /// D-CLIFLAG1 / D-MARKER-FAMILY1: one field-marker group on a single
    /// plane — always bracketed (`sigil[A, B]`), inline, no trailing
    /// newline. Sibling of `fmt_marker_group` (struct/enum level), which
    /// additionally unwraps a lone marker to bare `@A`/`#A` — field position
    /// never does that (D-ATTR2's field grammar is bracket-only).
    fn fmt_field_marker_group(&mut self, markers: &[&Marker], sigil: &str) {
        if markers.is_empty() {
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
        self.write("] ");
    }

    fn fmt_field(&mut self, field: &Field) {
        // D-DEBUG-REDACT/D-MARKERMOVE1: `@[Redact]` (contract plane) sits
        // inline before the field, ahead of any `#[…]` serde group (G2
        // stacking). Field markers are bracket-only (no bare `@Redact`).
        if field.redact {
            self.write(Syntax::CONTRACT_PREFIX);
            self.write("[");
            self.write(Syntax::ATTR_REDACT);
            self.write("] ");
        }
        // D-SERDE5 / D-CLIFLAG1 (D-MARKER-FAMILY1/G2): field markers split by
        // plane and sit inline before the field on the same line. A
        // contract-plane marker (`@[Doc(...)]`) gets its own `@[...]` group;
        // directive/serde markers (`#[Rename]`/`#[Skip]`/`#[Default]`/
        // `#[Flatten]`) get their own `#[...]` group, contract first — same
        // stacking order as struct-level `fmt_type_markers`. Both groups are
        // always bracketed at field position (no bare `@Name` unwrap here —
        // fields don't support it, unlike a lone struct/enum-level marker).
        let (contract_markers, directive_markers): (Vec<&Marker>, Vec<&Marker>) = field
            .serde_markers
            .iter()
            .partition(|m| Syntax::is_contract_marker(&m.name));
        self.fmt_field_marker_group(&contract_markers, Syntax::CONTRACT_PREFIX);
        self.fmt_field_marker_group(&directive_markers, Syntax::ATTR_PREFIX);
        self.fmt_pub_qualifier(field.is_pub, field.is_package_pub);
        // D-REF-SHORTHAND1/2: a stored-reference field spells its type `&T`; an
        // explicit `#Ref(label)` prefix survives only when the owner was named
        // to disambiguate (inference leaves `stored_ref_label` `None`).
        if field.is_stored_ref {
            if let Some(label) = &field.stored_ref_label {
                self.write("#");
                self.write(crate::Syntax::ATTR_REF);
                self.write("(");
                self.write(label);
                self.write(") ");
            }
            self.write(&field.name);
            self.write(": &");
            self.fmt_type(&field.ty);
        } else {
            self.write(&field.name);
            self.write(": ");
            self.fmt_type(&field.ty);
        }
    }
}
