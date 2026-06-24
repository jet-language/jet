use super::*;
use crate::AST::{
    AccessConvention, ConstAttr, ConstDef, EnumDef, ExternFn, ExternRustBlock, Field, Func,
    ImplDef, ImportDecl, ImportKind, Item, Param, StructDef, TraitImplBlock, TypeParam, Variant,
    VariantPayload,
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
            }
            // Stage 1a: modules are emitted verbatim (non-destructive). A
            // canonical module formatter lands with the eval pipeline.
            Item::Module(m) => {
                let text = self.src[m.span.start..m.span.end].to_string();
                self.write(&text);
            }
            // S59: C FFI modules are emitted verbatim (non-destructive). A
            // canonical formatter can land alongside the bind backend.
            Item::CModule(cm) => {
                let text = self.src[cm.span.start..cm.span.end].to_string();
                self.write(&text);
            }
            // Code modules are emitted verbatim pending a dedicated formatter.
            Item::CodeModule(cm) => {
                let text = self.src[cm.span.start..cm.span.end].to_string();
                self.write(&text);
            }
            // D-DIST1: distinct type declarations are emitted verbatim.
            Item::Distinct(d) => {
                let text = self.src[d.span.start..d.span.end].to_string();
                self.write(&text);
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
            }
            // D-MIGRATE1: migration blocks are emitted verbatim (non-destructive).
            Item::Migration(m) => {
                let text = self.src[m.span.start..m.span.end].to_string();
                self.write(&text);
                self.newline();
            }
        }
    }

    fn fmt_trait(&mut self, t: &crate::AST::TraitDef) {
        if t.is_pub {
            self.write("pub ");
        }
        self.write("trait ");
        self.write(&t.name);
        self.write(" ");
        self.write(Syntax::BLOCK_OPEN);
        self.newline();
        self.with_indent(|f| {
            for m in &t.methods {
                // D-EFF3: `#Pure` prefix bound on a trait method.
                if m.is_pure {
                    f.write(&format!("#{} ", Syntax::KW_PURE));
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
                    let list = effects.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ");
                    f.write(&format!(" #({})", list));
                }
                if let Some(ret) = &m.return_type {
                    f.write(" -> ");
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
                self.write("view ");
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
        self.write(" ");
        self.write("\"");
        self.write(&t.name.replace('\\', "\\\\").replace('"', "\\\""));
        self.write("\"");
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

    fn fmt_pub(&mut self, is_pub: bool) {
        if is_pub {
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
        // S60 (D-CASING1 follow-on): `#Pure` marker precedes `pub`/`fn`.
        if f.is_pure {
            self.write(&format!("#{} ", Syntax::KW_PURE));
        }
        if top_level {
            self.fmt_pub(f.is_pub);
        } else if f.is_pub {
            self.write("pub ");
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
        // list and the return arrow. A `#Pure fn` carries no `#(…)` list.
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
        if let Some(ret) = &f.return_type {
            self.write(" -> ");
            if f.is_view_return {
                self.write("view ");
            }
            self.fmt_return_type(ret);
        }
        self.write(" {");
        self.newline();
        let body = &f.body;
        self.with_indent(|f| f.fmt_block_stmts(body));
        self.end_block();
    }

    fn fmt_param(&mut self, p: &Param) {
        match p.convention {
            // D-CAP8/9: Infer is unmarked; Share/Raw not produced yet (D-CAP7 migration).
            AccessConvention::Read
            | AccessConvention::Infer
            | AccessConvention::Share
            | AccessConvention::Raw => {}
            AccessConvention::Write => self.write("mut "),
            AccessConvention::Move => self.write("take "),
        }
        self.write(&p.name);
        if p.name != Syntax::KW_SELF || !p.ty.name().is_empty() {
            self.write(": ");
            self.fmt_type(&p.ty);
        }
        // S61: a trailing parameter may carry a `= default` value.
        if let Some(default) = &p.default {
            self.write(" = ");
            self.fmt_expr(default, Prec::OrFallback);
        }
    }

    fn fmt_struct(&mut self, s: &StructDef, top_level: bool) {
        if top_level {
            self.fmt_pub(s.is_pub);
        }
        self.write("struct ");
        self.write(&s.name);
        self.fmt_type_params(&s.type_params);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (i, field) in s.fields.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                f.fmt_field(field);
            }
            for (i, (trait_name, _)) in s.derives.iter().enumerate() {
                if i > 0 || !s.fields.is_empty() {
                    f.newline();
                }
                f.fmt_derive_line(trait_name);
            }
            for (i, block) in s.trait_impls.iter().enumerate() {
                if i > 0 || !s.fields.is_empty() || !s.derives.is_empty() {
                    f.newline();
                    f.newline();
                }
                f.fmt_trait_impl_block(block);
            }
            for (i, m) in s.methods.iter().enumerate() {
                if i > 0
                    || !s.fields.is_empty()
                    || !s.derives.is_empty()
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
        if top_level {
            self.fmt_pub(e.is_pub);
        }
        self.write("enum ");
        self.write(&e.name);
        self.fmt_type_params(&e.type_params);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (i, v) in e.variants.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                f.fmt_variant(v);
            }
            for (i, (trait_name, _)) in e.derives.iter().enumerate() {
                if i > 0 || !e.variants.is_empty() {
                    f.newline();
                }
                f.fmt_derive_line(trait_name);
            }
            for (i, block) in e.trait_impls.iter().enumerate() {
                if i > 0 || !e.variants.is_empty() || !e.derives.is_empty() {
                    f.newline();
                    f.newline();
                }
                f.fmt_trait_impl_block(block);
            }
            for (i, m) in e.methods.iter().enumerate() {
                if i > 0
                    || !e.variants.is_empty()
                    || !e.derives.is_empty()
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
        self.write("impl ");
        self.write(&i.type_name);
        if let Some(tr) = &i.trait_name {
            self.write(": ");
            self.write(tr);
        }
        // S62: `impl Type: Trait using field` — delegation, no method body.
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
        if imp.is_pub {
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
            ImportKind::Unqualified { module_alias, items, .. } => {
                if items.len() == 1 {
                    self.write(module_alias);
                    self.write(".");
                    self.write(&items[0]);
                } else {
                    self.write(module_alias);
                    self.write(".{");
                    self.write(&items.join(", "));
                    self.write("}");
                }
            }
        }
    }

    fn fmt_field(&mut self, field: &Field) {
        if field.is_pub {
            self.write("pub ");
        }
        if field.is_stored_ref {
            self.write("ref");
            if let Some(label) = &field.stored_ref_label {
                self.write("[");
                self.write(label);
                self.write("]");
            }
            self.write(" ");
        }
        self.write(&field.name);
        self.write(": ");
        self.fmt_type(&field.ty);
    }
}
