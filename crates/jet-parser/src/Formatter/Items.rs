use super::*;
use crate::AST::{
    AccessConvention, CModuleKind, CodeModule, ConstAttr, ConstDef, EnumDef, EnumGroup, ExternFn,
    DeriveBodyItem, ExternRustBlock, Field, Func, GenericModuleDef, GenericModuleParam, ImplDef,
    ImportDecl, ImportKind, Item, Marker, MetaAttr, MetaField, Param, Pattern, StructDef,
    TraitImplBlock, Type, TypeParam, Variant, VariantPayload,
};

enum EnumFmtEntry<'b> {
    Leaf(&'b Variant),
    Group(&'b EnumGroup),
}

/// One member of a struct or enum body, tagged with where the author wrote it.
///
/// The parser splits a type body into parallel vectors — `fields`,
/// `cli_bindings`, `trait_impls`, `methods`, `validate_block` — which erases the
/// authored interleaving. Printing those vectors back to back rewrites every
/// program that did not happen to use that order: a `fn` written before an
/// `impl Trait { … }` block comes back out after it. `emit_leading` consumes
/// comments in ascending-offset order, so the same reordering additionally hands
/// each comment to whichever member happens to be printing at the time. Rethread
/// the vectors into one source-ordered list before printing.
#[derive(Clone, Copy)]
enum TypeBodyMember<'b> {
    Field(&'b Field),
    CLIBinding(&'b crate::AST::CLICommandBinding),
    TraitImpl(&'b TraitImplBlock),
    Method(&'b Func),
    /// D-VALIDATE1 (card #506): `validate { … }`, keyed by its `validate` keyword.
    Validate {
        start: usize,
        body: &'b [crate::AST::Stmt],
    },
}

impl TypeBodyMember<'_> {
    /// The member's leftmost authored offset. Every arm names a span *inside*
    /// the member, so the keys rise exactly as the source does — a field's or
    /// binding's markers, a method's `fn`, and a block's `impl` keyword all sit
    /// between the previous member and the span used here, and no two members
    /// can share a start.
    fn start(&self) -> usize {
        match self {
            Self::Field(field) => field.name_span.start,
            Self::CLIBinding(binding) => binding.name_span.start,
            Self::TraitImpl(block) => block.trait_span.start,
            Self::Method(method) => method.span.start,
            Self::Validate { start, .. } => *start,
        }
    }

    /// Data declarations stack on adjacent lines; anything carrying a body is
    /// set off from its neighbour by a blank line.
    fn is_compact(&self) -> bool {
        matches!(self, Self::Field(_))
    }
}

/// One member of an `impl` body, tagged with where the author wrote it. Same
/// split-vector hazard as `TypeBodyMember`: D-LIB2 `type Name = Concrete` rows
/// live in `assoc_type_impls`, methods in `methods`.
#[derive(Clone, Copy)]
enum ImplBodyMember<'b> {
    AssocType(&'b (String, Span, Type)),
    Method(&'b Func),
}

impl ImplBodyMember<'_> {
    fn start(&self) -> usize {
        match self {
            Self::AssocType((_, span, _)) => span.start,
            Self::Method(method) => method.span.start,
        }
    }

    fn is_compact(&self) -> bool {
        matches!(self, Self::AssocType(_))
    }
}

/// One member of a `trait` body, tagged with where the author wrote it. Same
/// split-vector hazard as `TypeBodyMember`: D-LIB2 `type Name` rows live in
/// `assoc_types`, signatures in `methods`.
#[derive(Clone, Copy)]
enum TraitBodyMember<'b> {
    AssocType(&'b (String, Span)),
    Method(&'b crate::AST::TraitMethodSig),
}

impl TraitBodyMember<'_> {
    fn start(&self) -> usize {
        match self {
            Self::AssocType((_, span)) => span.start,
            Self::Method(method) => method.span.start,
        }
    }
}

/// One member of an inline or generic module body. Module imports and items
/// live in separate AST vectors, so retain authored order while formatting.
enum ModuleBodyEntry<'b> {
    Import(&'b ImportDecl),
    Item(&'b Item),
}

impl ModuleBodyEntry<'_> {
    fn start(&self, src: &str) -> usize {
        match self {
            Self::Import(import) => import.span.start,
            Self::Item(item) => item_span_start(item, src),
        }
    }

    fn end(&self) -> usize {
        match self {
            Self::Import(import) => import.span.end,
            Self::Item(item) => item_span_end(item),
        }
    }
}

/// D-FMT-SIMPLIFY1=A: may this rendered expression be a `->` one-line function
/// body? The parser reads that body with `expr`, which accepts a headless
/// record literal after `->` — a `Type{ … }` construction or a
/// block lambda (`Parser/Items/functions_params.rs`,
/// `Parser/Expressions/primary.rs`). Braces inside `(`/`[` reopen the ordinary
/// expression grammar, and braces inside text are the lexer's business.
fn reads_back_as_one_line_body(rendered: &str) -> bool {
    let mut depth = 0usize;
    let mut chars = rendered.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' | '\'' => {
                while let Some(inner) = chars.next() {
                    if inner == '\\' {
                        chars.next();
                    } else if inner == ch {
                        break;
                    }
                }
            }
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '{' if depth == 0 => return false,
            _ => {}
        }
    }
    true
}

fn has_ambiguous_decode_union(items: &[Item], ty: &Type) -> bool {
    if let Type::Union(members) = ty {
        let mut seen = Vec::new();
        for member in members {
            if let Some(shapes) = crate::AST::resolved_decode_wire_shapes(items, member) {
                for shape in shapes {
                    if seen.contains(&shape) {
                        return true;
                    }
                    seen.push(shape);
                }
            }
        }
    }
    match ty {
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::Tagged { inner, .. }
        | Type::FixedList { elem: inner, .. } => has_ambiguous_decode_union(items, inner),
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            has_ambiguous_decode_union(items, key)
                || has_ambiguous_decode_union(items, value)
        }
        Type::Fn { params, ret, .. } => {
            params
                .iter()
                .any(|param| has_ambiguous_decode_union(items, param))
                || ret
                    .as_deref()
                    .is_some_and(|ret| has_ambiguous_decode_union(items, ret))
        }
        Type::Apply { args, .. } | Type::Union(args) => args
            .iter()
            .any(|arg| has_ambiguous_decode_union(items, arg)),
        Type::Tuple(fields) => fields
            .iter()
            .any(|(_, field)| has_ambiguous_decode_union(items, field)),
        _ => false,
    }
}

impl<'a> Fmt<'a> {
    /// Rewrite retired surface tokens inside a migration while preserving its
    /// source layout. Migration AST nodes keep operation spans for sema, not
    /// the authored punctuation, so token edits are the narrow formatter seam.
    fn canonical_migration_source(&self, span: Span) -> String {
        let tokens: Vec<&Token> = self
            .source_toks
            .iter()
            .filter(|token| token.span.start >= span.start && token.span.end <= span.end)
            .collect();
        let mut edits: Vec<(usize, usize, &str)> = Vec::new();
        let mut effect_arrows = Vec::new();

        for index in 0..tokens.len() {
            let effect_open = matches!(
                tokens[index].kind,
                TokKind::Eq | TokKind::Colon | TokKind::MinusMinus
            ) && matches!(
                tokens.get(index + 1).map(|token| &token.kind),
                Some(TokKind::LBracket)
            );
            if !effect_open {
                continue;
            }
            let Some(close) = (index + 2..tokens.len())
                .find(|&candidate| matches!(tokens[candidate].kind, TokKind::RBracket))
            else {
                continue;
            };
            if matches!(
                tokens.get(close + 1).map(|token| &token.kind),
                Some(TokKind::Gt | TokKind::UnifiedArrow | TokKind::Arrow | TokKind::LambdaArrow)
            ) {
                edits.push((tokens[index].span.start, tokens[index].span.end, "-"));
                effect_arrows.push(close + 1);
            }
        }

        for (index, token) in tokens.iter().enumerate() {
            let replacement = match token.kind {
                TokKind::Arrow if effect_arrows.contains(&index) => Some(">"),
                TokKind::Arrow => Some(Syntax::OP_UNIFIED_ARROW),
                TokKind::UnifiedArrow if effect_arrows.contains(&index) => Some(">"),
                TokKind::LambdaArrow if effect_arrows.contains(&index) => Some(">"),
                TokKind::LambdaArrow => Some(Syntax::OP_UNIFIED_ARROW),
                TokKind::Dot
                    if matches!(tokens.get(index + 1).map(|next| &next.kind), Some(TokKind::LBrace)) =>
                {
                    Some("")
                }
                _ => None,
            };
            if let Some(replacement) = replacement {
                edits.push((token.span.start, token.span.end, replacement));
            }
        }

        edits.sort_unstable_by_key(|(start, _, _)| *start);
        let mut out = String::with_capacity(span.end - span.start);
        let mut cursor = span.start;
        for (start, end, replacement) in edits {
            if start < cursor {
                continue;
            }
            out.push_str(&self.src[cursor..start]);
            out.push_str(replacement);
            cursor = end;
        }
        out.push_str(&self.src[cursor..span.end]);
        out
    }

    fn fmt_decode_type(&mut self, ty: &Type, span: Span, derives_decode: bool) {
        let source_ty = (derives_decode && has_ambiguous_decode_union(self.items, ty))
            .then(|| self.source_type_spelling(span.start))
            .flatten()
            .map(str::to_owned);
        if let Some(source_ty) = source_ty {
            self.write(&source_ty);
            self.skip_verbatim_comments(span.start + source_ty.len());
        } else {
            self.fmt_type(ty);
        }
    }

    pub(super) fn fmt_meta_attr(&mut self, meta: &MetaAttr) {
        self.write("#");
        self.fmt_meta_rule(meta);
    }

    fn fmt_meta_rule(&mut self, meta: &MetaAttr) {
        self.write(&format!("{}(", Syntax::MARKER_META));
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
                // External-method sugar (`fn Type.method(…) :[]>`) is
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
                    // The AST does not retain the marker ordering for this
                    // sugar, but the formatter still owns the canonical arrow.
                    // Restrict the rewrite to the declaration head; the path
                    // string and any quoted text remain verbatim.
                    let (head, tail) = text.split_once(" = \"").unwrap_or((text, ""));
                    let head = head
                        .replace("=[", "-[")
                        .replace(":[", "-[")
                        .replace("]=>", "]>")
                        .replace(":>", "->")
                        .replace("=>", "->");
                    self.write(&head);
                    if !tail.is_empty() {
                        self.write(" = \"");
                        self.write(tail);
                    }
                    self.newline();
                    self.skip_verbatim_comments(i.span.end);
                } else {
                    self.fmt_impl(i);
                }
            }
            Item::Const(c) => self.fmt_const(c),
            Item::Test(t) => self.fmt_test(t),
            Item::ExternRust(b) => self.fmt_extern_rust(b),
            Item::Trait(t) => self.fmt_trait(t),
            // D-QUAL2: tag declarations are emitted verbatim (non-destructive).
            Item::Tag(t) => {
                // `TagDef::span` starts at `tag`; visibility is parsed before
                // that span and must be reconstructed from the AST fact.
                self.fmt_pub_qualifier(t.is_pub, t.is_package_pub);
                let text = self.src[t.span.start..t.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(t.span.end);
            }
            Item::EffectDecl(declaration) => {
                let text = self.src[declaration.span.start..declaration.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(declaration.span.end);
            }
            // D-META-NAME1/FORM1: declarations with typed bodies use the same
            // item-template formatter as derives. Text-head contracts still use
            // their source form because their body is a separate DSL.
            Item::MarkerDecl(declaration) => {
                self.fmt_marker_decl(declaration);
            }
            // D-FACTDECL1=A: fact declarations keep their source spelling;
            // they erase before TIR like marker declarations.
            Item::FactDecl(declaration) => {
                let text = self.src[declaration.span.start..declaration.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(declaration.span.end);
            }
            // Stage 1a: JetOS contribution modules retain their source-level
            // contribution shape; inline code modules use the typed formatter
            // below. Their bodies are not interchangeable ASTs.
            Item::Module(m) => {
                if !self.fmt_perf_module(m) {
                    let text = self.src[m.span.start..m.span.end].to_string();
                    self.write(&text);
                    self.skip_verbatim_comments(m.span.end);
                }
            }
            // S59: C FFI modules contain typed foreign declarations. Format
            // their Jet signatures while leaving the foreign symbol strings
            // untouched.
            Item::CModule(cm) => self.fmt_c_module(cm),
            // D-MOD2: inline code-module bodies are ordinary typed items and
            // must receive the same expression rewrites as file-level items.
            Item::CodeModule(cm) => self.fmt_code_module(cm),
            Item::Distinct(d) => self.fmt_distinct(d),
            // D-TYPEALIAS1 / D-ALIAS-OP1=B: aliases use the binding sigil.
            Item::TypeAlias(a) => self.fmt_type_alias(a),
            // D-QUAL3: unit-family declarations are emitted verbatim (the sugar
            // surface is preserved; it is not expanded into per-member distincts).
            Item::UnitFamily(uf) => {
                // The marker span begins at `#UnitFamily`, after any `pub`
                // qualifier consumed by the visibility parser.
                self.fmt_pub_qualifier(uf.is_pub, uf.is_package_pub);
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
                self.write(" ");
                self.write(Syntax::OP_UNIFIED_ARROW);
                self.write(" ");
                self.write(&ec.to_ty);
                self.write(" ");
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(ec.body_span.end);
            }
            // D-MIGRATE1: preserve migration layout while emitting the
            // canonical arrow and literal spellings inside the block.
            Item::Migration(m) => {
                let text = self.canonical_migration_source(m.span);
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(m.span.end);
            }
            // D-STATE-DECL: state-set declarations are emitted verbatim (non-destructive).
            Item::StateDecl(s) => {
                // `StateDecl::span` starts at `state`, not at the visibility
                // token that the top-level parser consumed first.
                self.fmt_pub_qualifier(s.is_pub, s.is_package_pub);
                let text = self.src[s.span.start..s.span.end].to_string();
                self.write(&text);
                self.newline();
                self.skip_verbatim_comments(s.span.end);
            }
            // D-PROTO1/D-PROTO2: protocol declarations have typed message
            // fields, so format those fields instead of copying the block.
            Item::ProtocolDecl(p) => self.fmt_protocol_decl(p),
            // D-META-CODE1=A: derive bodies are typed item templates.
            Item::UserDerive(d) => self.fmt_derive_decl(d),
            // D-STRUCT-ONCE1=A: root declaration loops use the same typed
            // body formatter as derive and marker templates.
            Item::TemplateLoop(loop_item) => self.fmt_item_template_loop(loop_item),
            // D-CONF-GENSPELL1=A: generic module templates are typed item
            // templates, not opaque source strings.
            Item::GenericModule(gm) => self.fmt_generic_module(gm),
            // D-CONF-GENSPELL1=A: module alias declarations emitted verbatim (non-destructive)
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

    fn fmt_marker_decl(&mut self, declaration: &crate::AST::MarkerDecl) {
        if declaration.text.is_some() {
            let text = self.src[declaration.span.start..declaration.span.end].to_string();
            self.write(&text);
            self.newline();
            self.skip_verbatim_comments(declaration.span.end);
            return;
        }
        self.write("marker ");
        self.write(&declaration.name);
        self.write("(");
        for (index, param) in declaration.params.iter().enumerate() {
            if index > 0 {
                self.write(", ");
            }
            self.write(&param.name);
            self.write(": ");
            if let Some(ty) = &param.ty {
                if param.variadic {
                    self.write("...");
                }
                self.fmt_type(ty);
                if let Some(default) = &param.value {
                    self.write(" = ");
                    self.fmt_expr(default, Prec::OrFallback);
                }
            } else if let Some(value) = &param.value {
                self.fmt_expr(value, Prec::OrFallback);
            }
        }
        self.write(")");
        if let Some(body) = &declaration.body {
            self.write(" {");
            self.newline();
            self.with_indent(|f| f.fmt_derive_body_items(body));
            self.emit_leading(declaration.span.end);
            self.end_template_block();
        }
        self.newline();
    }

    fn fmt_protocol_decl(&mut self, protocol: &crate::AST::ProtocolDecl) {
        self.fmt_pub_qualifier(protocol.is_pub, protocol.is_package_pub);
        self.write("protocol ");
        self.write(&protocol.name);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (index, message) in protocol.messages.iter().enumerate() {
                if index > 0 {
                    f.newline();
                }
                f.emit_leading(message.span.start);
                let sender = match message.direction {
                    crate::AST::ProtocolDirection::ClientToServer => Syntax::PROTO_CLIENT,
                    crate::AST::ProtocolDirection::ServerToClient => Syntax::PROTO_SERVER,
                };
                f.write(sender);
                f.write(": ");
                f.write(&message.name);
                f.write("(");
                for (field_index, (name, ty)) in message.fields.iter().enumerate() {
                    if field_index > 0 {
                        f.write(", ");
                    }
                    f.write(name);
                    f.write(": ");
                    f.fmt_type(ty);
                }
                f.write(")");
                f.emit_trailing(message.span.end);
            }
        });
        self.emit_leading(protocol.span.end);
        self.end_block();
    }

    /// D-STRUCT-POLICY1=A: policy wrappers are carried beside ordinary items,
    /// so preserve their checked source boundary in the formatter's first
    /// implementation slice. `span` starts at the `policy` keyword, so the
    /// visibility prefix comes from `is_pub` — echoing the slice alone drops
    /// `pub` and the formatter's own output then fails E0003.
    pub(super) fn fmt_user_policy_decl(&mut self, declaration: &crate::AST::UserPolicyDecl) {
        if declaration.is_pub {
            self.write("pub ");
        }
        let text = &self.src[declaration.span.start..declaration.span.end];
        self.write(text);
        self.newline();
        self.skip_verbatim_comments(declaration.span.end);
    }

    fn fmt_derive_decl(&mut self, derive: &crate::AST::DeriveDef) {
        self.write("derive ");
        self.write(&derive.type_param);
        self.write(".");
        self.write(&derive.trait_name);
        self.write(" {");
        self.newline();
        self.with_indent(|f| f.fmt_derive_body_items(&derive.body));
        self.emit_leading(derive.span.end);
        self.end_template_block();
    }

    fn fmt_item_template_loop(&mut self, loop_item: &crate::AST::ItemTemplateLoop) {
        self.write("@loop ");
        self.write(&loop_item.var);
        self.write(", ");
        self.fmt_expr(&loop_item.source, Prec::OrFallback);
        self.write(" {");
        self.newline();
        self.with_indent(|f| f.fmt_derive_body_items(&loop_item.body));
        self.emit_leading(loop_item.span.end);
        self.end_template_block();
    }

    /// Format typed derive, marker, and generated-source bodies through their
    /// AST. These bodies share one representation, so one walk closes all
    /// three formatter gaps.
    pub(super) fn fmt_derive_body_items(&mut self, body: &[DeriveBodyItem]) {
        for (index, body_item) in body.iter().enumerate() {
            if index > 0 && !self.at_line_start {
                self.newline();
            }
            let start = self.derive_body_item_start(body_item);
            self.emit_leading(start);
            match body_item {
                DeriveBodyItem::Item(item) => self.fmt_item(item),
                DeriveBodyItem::Stmt(stmt) => self.fmt_stmt(stmt),
                DeriveBodyItem::Loop {
                    var, source, body, ..
                } => {
                    self.write("@loop ");
                    self.write(var);
                    self.write(", ");
                    self.fmt_expr(source, Prec::OrFallback);
                    self.write(" {");
                    self.newline();
                    self.with_indent(|f| f.fmt_derive_body_items(body));
                    self.end_template_block();
                }
            }
            self.emit_trailing(self.derive_body_item_end(body_item));
            if !self.at_line_start {
                self.newline();
            }
        }
    }

    fn end_template_block(&mut self) {
        if !self.at_line_start {
            self.newline();
        }
        self.write("}");
    }

    fn derive_body_item_start(&self, body_item: &DeriveBodyItem) -> usize {
        match body_item {
            DeriveBodyItem::Item(item) => item_span_start(item, self.src),
            DeriveBodyItem::Stmt(stmt) => stmt.span().start,
            DeriveBodyItem::Loop { span, .. } => span.start,
        }
    }

    fn derive_body_item_end(&self, body_item: &DeriveBodyItem) -> usize {
        match body_item {
            DeriveBodyItem::Item(item) => item_span_end(item),
            DeriveBodyItem::Stmt(stmt) => self.statement_source_end(stmt),
            DeriveBodyItem::Loop { span, .. } => span.end,
        }
    }

    fn fmt_module_body(&mut self, imports: &[ImportDecl], items: &[Item], span_end: usize) {
        let mut entries = Vec::with_capacity(imports.len() + items.len());
        entries.extend(imports.iter().map(ModuleBodyEntry::Import));
        entries.extend(items.iter().map(ModuleBodyEntry::Item));
        entries.sort_by_key(|entry| entry.start(self.src));

        for (index, entry) in entries.iter().enumerate() {
            if index > 0 {
                self.blank_separator_before_item();
            }
            self.emit_leading(entry.start(self.src));
            match entry {
                ModuleBodyEntry::Import(import) => self.fmt_import(import),
                ModuleBodyEntry::Item(item) => self.fmt_item(item),
            }
            self.emit_trailing(entry.end());
        }
        // Comments between the last body member and the closing brace have no
        // following item to claim them. Consume only comments inside this
        // module, leaving outer comments for the enclosing formatter.
        self.emit_leading(span_end);
    }

    fn end_module_block(&mut self) {
        if !self.at_line_start {
            self.newline();
        }
        self.write("}");
    }

    fn fmt_code_module(&mut self, module: &CodeModule) {
        self.fmt_pub_qualifier(module.is_pub, module.is_package_pub);
        self.write("module ");
        self.write(&module.name);
        if let Some(target) = module.web_target {
            self.write(" ");
            self.write(target.name());
        }
        match &module.body {
            None => self.write(";"),
            Some(items) => {
                self.write(" {");
                self.newline();
                self.with_indent(|f| f.fmt_module_body(&module.imports, items, module.span.end));
                self.end_module_block();
            }
        }
    }

    fn fmt_generic_module(&mut self, module: &GenericModuleDef) {
        self.fmt_pub_qualifier(module.is_pub, module.is_package_pub);
        self.write("module ");
        self.write(&module.name);

        let type_params = module
            .params
            .iter()
            .filter_map(|param| match param {
                GenericModuleParam::Type { .. } => Some(param),
                GenericModuleParam::Value { .. } => None,
            })
            .collect::<Vec<_>>();
        if !type_params.is_empty() {
            self.write("<");
            for (index, param) in type_params.iter().enumerate() {
                if index > 0 {
                    self.write(", ");
                }
                let GenericModuleParam::Type { name, bound, .. } = param else {
                    unreachable!("type parameter filter returned a value parameter");
                };
                self.write(name);
                if let Some(bound) = bound {
                    self.write(": ");
                    self.fmt_type(bound);
                }
            }
            self.write(">");
        }

        let value_params = module
            .params
            .iter()
            .filter_map(|param| match param {
                GenericModuleParam::Value { .. } => Some(param),
                GenericModuleParam::Type { .. } => None,
            })
            .collect::<Vec<_>>();
        if !value_params.is_empty() {
            self.write("(");
            for (index, param) in value_params.iter().enumerate() {
                if index > 0 {
                    self.write(", ");
                }
                let GenericModuleParam::Value { name, ty, .. } = param else {
                    unreachable!("value parameter filter returned a type parameter");
                };
                self.write(name);
                self.write(": ");
                self.fmt_type(ty);
            }
            self.write(")");
        }

        self.write(" {");
        self.newline();
        self.with_indent(|f| f.fmt_module_body(&module.imports, &module.body, module.span.end));
        self.end_module_block();
    }

    fn fmt_c_module(&mut self, module: &crate::AST::CModule) {
        self.write(match module.kind {
            CModuleKind::Extern => "#Extern module c.",
            CModuleKind::Bindgen => "#Bindgen module c.",
        });
        self.write(&module.lib);
        if matches!(module.kind, CModuleKind::Bindgen) {
            self.write(".__bindgen__");
        }
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (index, function) in module.functions.iter().enumerate() {
                if index > 0 && !f.at_line_start {
                    f.newline();
                }
                f.emit_leading(function.span.start);
                f.fmt_extern_fn(function);
                f.emit_trailing(function.span.end);
            }
            f.emit_leading(module.span.end);
        });
        self.end_module_block();
    }

    fn fmt_perf_module(&mut self, module: &crate::AST::ModuleDecl) -> bool {
        if !module.sources.is_empty()
            || !module.imports.is_empty()
            || !module.members.is_empty()
            || module.contributions.len() != 1
            || self.comments.iter().any(|comment| {
                comment.span.start >= module.span.start && comment.span.start < module.span.end
            })
        {
            return false;
        }
        let crate::AST::Contribution {
            namespace: crate::AST::Namespace::Perf,
            value: crate::AST::ContribValue::Perf(perf),
            ..
        } = &module.contributions[0]
        else {
            return false;
        };
        let typed_budget_list = self
            .src
            .get(perf.list_span.start..perf.list_span.end)
            .is_some_and(|source| source.contains("]{") || source.contains("].{"));

        self.write("module ");
        self.write(&module.name);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            if !perf.compile_workloads.is_empty() {
                f.write("compile_workloads: {");
                for (index, workload) in perf.compile_workloads.iter().enumerate() {
                    if index == 0 {
                        f.write(" ");
                    } else {
                        f.write(", ");
                    }
                    f.write(&workload.name);
                    f.write(": CompilerWorkload.Edit{target: ");
                    f.fmt_expr(&workload.target, Prec::OrFallback);
                    f.write(", patch: ");
                    f.fmt_expr(&workload.patch, Prec::OrFallback);
                    f.write("}");
                }
                f.write(" }");
                f.newline();
            }
            if typed_budget_list {
                f.write("budgets: [Budget]{");
                for (index, budget) in perf.budgets.iter().enumerate() {
                    if index > 0 {
                        f.write(", ");
                    }
                    f.write("{");
                    f.fmt_perf_budget_fields(&budget.fields);
                    f.write("}");
                }
                f.write("}");
            } else {
                f.write("budgets: [");
                for (index, budget) in perf.budgets.iter().enumerate() {
                    if index > 0 {
                        f.write(", ");
                    }
                    f.write("Budget{");
                    f.fmt_perf_budget_fields(&budget.fields);
                    f.write("}");
                }
                f.write("]");
            }
        });
        self.end_block();
        true
    }

    fn fmt_perf_budget_fields(&mut self, fields: &[crate::AST::BudgetField]) {
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                self.write(", ");
            }
            self.write(&field.name);
            self.write(": ");
            self.fmt_expr(&field.value, Prec::OrFallback);
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
            // D-LIB2: `type Name` associated-type declarations live in
            // `assoc_types` and signatures in `methods`, so printing one vector
            // and then the other moves a signature the author wrote before a
            // row. The rows were dropped outright until they were printed here
            // at all — a real token-dropping bug, caught reformatting
            // examples/features/types/associated_types.jet.
            let mut members: Vec<TraitBodyMember<'_>> =
                Vec::with_capacity(t.assoc_types.len() + t.methods.len());
            members.extend(t.assoc_types.iter().map(TraitBodyMember::AssocType));
            members.extend(t.methods.iter().map(TraitBodyMember::Method));
            members.sort_by_key(|member| member.start());
            for member in members {
                let m = match member {
                    TraitBodyMember::AssocType((name, _)) => {
                        f.write("type ");
                        f.write(name);
                        f.newline();
                        continue;
                    }
                    TraitBodyMember::Method(m) => m,
                };
                f.write("fn ");
                f.write(&m.name);
                f.write("(");
                f.fmt_param_list(&m.params);
                f.write(")");
                if let Some(ret) = &m.return_type {
                    if Self::is_unit_fallible_type(ret) {
                        f.fmt_unit_fallible_return(ret);
                    } else {
                        f.write(" ");
                        f.fmt_return_type(ret);
                    }
                }
                if let Some(map) = &m.declared_return_view_provenance {
                    f.fmt_declared_return_view_from(map, &m.params);
                }
                // D-SIG-SHAPE1=B / D-EFF3: result first, effect ceiling second.
                if let Some(effects) = &m.declared_effects {
                    let list = effects
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    f.write(" ");
                    f.write(Syntax::EFFECT_ARROW_OPEN);
                    f.write(&list);
                    f.write(Syntax::EFFECT_ARROW_CLOSE);
                } else if m.is_pure {
                    f.write(" ");
                    f.write(Syntax::EFFECT_ARROW_OPEN);
                    f.write(Syntax::EFFECT_ARROW_CLOSE);
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
        if let Some((inverse, _)) = &ef.undo {
            self.write(&format!("#{}({}) ", Syntax::MARKER_UNDO, inverse));
        }
        if let Some((abi, _)) = &ef.abi {
            self.write(&format!("#{}({}) ", Syntax::MARKER_ABI, abi));
        }
        self.write("fn ");
        self.write(&ef.name);
        self.write("(");
        self.fmt_param_list(&ef.params);
        self.write(")");
        if let Some(ret) = &ef.return_type {
            if Self::is_unit_fallible_type(ret) {
                self.fmt_unit_fallible_return(ret);
            } else {
                self.write(" ");
                self.fmt_return_type(ret);
            }
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
            if let Some(faults) = &t.faults_expr {
                self.write(", ");
                self.write(Syntax::TEST_FAULTS_PARAM);
                self.write(": ");
                self.fmt_expr(faults, Prec::OrFallback);
            }
            if let Some(expected_fail) = &t.expected_fail_expr {
                self.write(", ");
                self.write(Syntax::TEST_EXPECTED_FAIL_PARAM);
                self.write(": ");
                self.fmt_expr(expected_fail, Prec::OrFallback);
            }
            self.write(")");
        } else {
            if t.faults_expr.is_some() || t.expected_fail_expr.is_some() {
                self.write("(");
                if let Some(faults) = &t.faults_expr {
                    self.write(Syntax::TEST_FAULTS_PARAM);
                    self.write(": ");
                    self.fmt_expr(faults, Prec::OrFallback);
                }
                if let Some(expected_fail) = &t.expected_fail_expr {
                    if t.faults_expr.is_some() {
                        self.write(", ");
                    }
                    self.write(Syntax::TEST_EXPECTED_FAIL_PARAM);
                    self.write(": ");
                    self.fmt_expr(expected_fail, Prec::OrFallback);
                }
                self.write(")");
            }
            self.write(" fn ");
            self.write(t.name.as_deref().expect("property tests have a parsed name"));
            self.write("(");
            self.fmt_param_list(&t.params);
            self.write(")");
        }
        self.write(" ");
        self.write(Syntax::BLOCK_OPEN);
        self.newline();
        self.with_indent(|f| f.fmt_block_stmts(&t.body));
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
        if let Some((low, high, _)) = d.range {
            self.write("(");
            self.write(&low.to_string());
            self.write("..");
            self.write(&high.to_string());
            self.write(")");
        }
    }

    fn fmt_type_alias(&mut self, a: &crate::AST::TypeAliasDef) {
        self.fmt_pub_qualifier(a.is_pub, a.is_package_pub);
        self.write("alias ");
        self.write(&a.name);
        self.fmt_type_params(&a.type_params);
        self.write(" :: ");
        self.fmt_type(&a.target);
    }

    fn fmt_type_params(&mut self, params: &[TypeParam]) {
        self.write(&crate::Generics::format_type_params(params));
    }

    /// D-SHAPE2 / D-SERDE2–8: render one applied rule.
    pub(super) fn fmt_marker(&mut self, m: &Marker) {
        if m.negated {
            self.write("!");
        }
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
        if markers.len() == 1 && !lone_hash_ok {
            self.write(Syntax::RULE_PREFIX);
            self.fmt_marker(&markers[0]);
            self.write(" ");
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

    fn fmt_trait_impl_block(&mut self, block: &TraitImplBlock) {
        // The block is a body member like any other, so its own leading
        // comments belong before `impl`, not inside the braces where the first
        // method's `emit_leading` would otherwise flush them.
        self.emit_leading(block.trait_span.start);
        self.write("impl ");
        self.write(&block.trait_name);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            f.fmt_impl_body(&block.assoc_type_impls, &block.methods);
        });
        self.end_block();
    }

    /// Emit an `impl` body in authored order.
    ///
    /// D-LIB2 `type Name = Concrete` rows and methods arrive in two vectors, so
    /// printing one vector and then the other moves a row the author wrote after
    /// a method. An in-type `impl Trait { … }` block additionally never printed
    /// `assoc_type_impls` at all, which deleted the row from the program.
    fn fmt_impl_body(&mut self, assoc_types: &[(String, Span, Type)], methods: &[Func]) {
        let mut members: Vec<ImplBodyMember<'_>> =
            Vec::with_capacity(assoc_types.len() + methods.len());
        members.extend(assoc_types.iter().map(ImplBodyMember::AssocType));
        members.extend(methods.iter().map(ImplBodyMember::Method));
        members.sort_by_key(|member| member.start());
        let mut previous: Option<ImplBodyMember<'_>> = None;
        for member in members {
            if previous.is_some() {
                self.newline();
                if !previous.is_some_and(|prior| prior.is_compact() && member.is_compact()) {
                    self.newline();
                }
            }
            match member {
                ImplBodyMember::AssocType((name, span, ty)) => {
                    self.emit_leading(span.start);
                    self.write("type ");
                    self.write(name);
                    self.write(" = ");
                    self.fmt_type(ty);
                }
                ImplBodyMember::Method(method) => {
                    self.emit_leading(method.name_span.start);
                    self.fmt_func(method, false);
                }
            }
            previous = Some(member);
        }
    }

    fn fmt_func(&mut self, f: &Func, top_level: bool) {
        let ordered_rules = f.markers.clone();
        if !ordered_rules.is_empty() {
            let rules = ordered_rules.iter().collect::<Vec<_>>();
            if rules.len() == 1 {
                self.write(Syntax::RULE_PREFIX);
                self.fmt_marker(rules[0]);
                if jet_foundation::Registry::callable_marker_needs_line_break(&rules[0].name) {
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
        if let Some(Pattern::Variant {
            variant, bindings, ..
        }) = &f.head_pattern
        {
            self.write(variant);
            if !bindings.is_empty() {
                self.write("(");
                for (index, param) in f.params.iter().enumerate() {
                    if index > 0 {
                        self.write(", ");
                    }
                    self.fmt_param(param);
                }
                self.write(")");
            }
        } else {
            self.fmt_param_list(&f.params);
        }
        self.write(")");
        let unit_fallible = f
            .return_type
            .as_ref()
            .is_some_and(|ty| Self::is_unit_fallible_type(ty));
        if let Some(ret) = &f.return_type {
            if unit_fallible {
                self.fmt_unit_fallible_return(ret);
            } else {
                self.write(" ");
                self.fmt_return_type(ret);
            }
        }
        if let Some(map) = &f.declared_return_view_provenance {
            self.fmt_declared_return_view_from(map, &f.params);
        }
        // D-SIG-SHAPE1=B / D-ARROW-CONTROL1: the result is bare, then the
        // effect ceiling owns the body-arrow position.
        if let Some(effects) = &f.declared_effects {
            self.write(" ");
            self.write(Syntax::EFFECT_ARROW_OPEN);
            for (i, (name, _)) in effects.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(name);
            }
            self.write(Syntax::EFFECT_ARROW_CLOSE);
        } else if f.is_pure {
            self.write(" ");
            self.write(Syntax::EFFECT_ARROW_OPEN);
            self.write(Syntax::EFFECT_ARROW_CLOSE);
        }
        if let Some((param, _)) = &f.effect_via {
            self.write(" ");
            self.write(Syntax::EFFECT_ARROW_OPEN);
            self.write(Syntax::KW_VIA);
            self.write(" ");
            self.write(param);
            self.write(Syntax::EFFECT_ARROW_CLOSE);
        }
        let saved_return_type =
            std::mem::replace(&mut self.expected_return_type, f.return_type.clone());
        // D-SIG-SHAPE1=B: preserve the canonical concise callable body.
        // The parser desugars the marker plus expression to `return expr`; its
        // synthetic return span starts on the author-written marker. Retired
        // `::`/`=`/arrow input is recovered for the teaching diagnostic and
        // rewritten.
        if let [crate::AST::Stmt::Return(Some(expr), span)] = f.body.as_slice() {
            let marker = self.src.get(span.start..span.start.saturating_add(2));
            let retired_marker = self
                .src
                .get(span.start..span.start.saturating_add(1))
                .is_some_and(|source| source == "=");
            let effect_body = f.declared_effects.is_some()
                || f.effect_via.is_some()
                || f.is_pure;
            let effect_marker = effect_body
                && self
                    .src
                    .get(span.start..span.start.saturating_add(2))
                    .is_some_and(|source| source == "-[");
            if marker == Some("::")
                || marker == Some("->")
                || marker == Some(":>")
                || marker == Some("=>")
                || retired_marker
                || effect_marker
            {
                if effect_body {
                    self.write(" ");
                } else {
                    self.write(" -> ");
                }
                self.fmt_expr(expr, Prec::OrFallback);
                self.expected_return_type = saved_return_type;
                return;
            }
        }
        // D-FMT-SIMPLIFY1=A / card #1514 criterion 3: a braced body with one
        // return uses the canonical `->` one-line body when it fits. Keep
        // comments and wide output in the explicit block form.
        if self.simplify {
            if let [crate::AST::Stmt::Return(Some(expr), span)] = f.body.as_slice() {
                // A word test, not a prefix test: the marker forms above have
                // already returned, so the only construct D-FMT-SIMPLIFY1=A
                // ratified here is a body whose single statement the author
                // wrote as the `return` keyword.
                let after_keyword = span.start.saturating_add("return".len());
                let is_authored_return = self.src.get(span.start..after_keyword)
                    == Some("return")
                    && self
                        .src
                        .get(after_keyword..)
                        .and_then(|rest| rest.chars().next())
                        .map_or(true, |ch| !ch.is_alphanumeric() && ch != '_');
                let comment_free = self
                    .single_stmt_braces(&f.body[0])
                    .is_some_and(|(open, close)| !self.span_has_comment(open, close));
                if is_authored_return && comment_free {
                    let saved_out = self.out.len();
                    let saved_col = self.col;
                    let saved_line_start = self.at_line_start;
                    let saved_pending_blank = self.pending_blank;
                    let saved_comment_i = self.comment_i;
                    if f.declared_effects.is_some() || f.effect_via.is_some() || f.is_pure {
                        self.write(" ");
                    } else {
                        self.write(" -> ");
                    }
                    self.fmt_expr(expr, Prec::OrFallback);
                    if self.col <= MAX_WIDTH
                        && !self.out[saved_out..].contains('\n')
                        && reads_back_as_one_line_body(&self.out[saved_out..])
                    {
                        self.expected_return_type = saved_return_type;
                        return;
                    }
                    self.out.truncate(saved_out);
                    self.col = saved_col;
                    self.at_line_start = saved_line_start;
                    self.pending_blank = saved_pending_blank;
                    self.comment_i = saved_comment_i;
                }
            }
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
            self.expected_return_type = saved_return_type;
            return;
        }
        self.write(" {");
        if f.body.is_empty()
            && self
                .src
                .get(f.span.start..f.span.end)
                .and_then(|source| source.rsplit_once('{'))
                .and_then(|(_, body)| body.rsplit_once('}'))
                .is_some_and(|(body, _)| !body.contains('\n') && !body.contains("//") && !body.contains("/*"))
        {
            self.write("}");
            self.expected_return_type = saved_return_type;
            return;
        }
        // D-FMT1: a one-line `fn` body the author wrote inline survives.
        self.fmt_body(&f.body);
        self.expected_return_type = saved_return_type;
    }

    pub(super) fn fmt_policy_declarations(&mut self, declarations: &[crate::Policy::PolicyDeclaration]) {
        self.write("#");
        self.fmt_policy_rule(declarations);
    }

    fn fmt_policy_rule(&mut self, declarations: &[crate::Policy::PolicyDeclaration]) {
        self.write(&format!("{}(", Syntax::MARKER_POLICY));
        for (i, declaration) in declarations.iter().enumerate() {
            if i > 0 { self.write(", "); }
            self.write(declaration.key.name());
            match declaration.value {
                crate::Policy::PolicyValue::Limit(limit) => self.write(&format!("({limit})")),
                crate::Policy::PolicyValue::On
                | crate::Policy::PolicyValue::Off
                | crate::Policy::PolicyValue::Explicit => {
                    self.write(": ");
                    self.write(&declaration.value.display());
                }
                _ => {}
            }
        }
        self.write(")");
    }

    /// D-APILABEL1=A: reprint a parameter list with its zone separators.
    /// `/` goes after the last positional-only parameter, `*` before the first
    /// label-only one, so the printed form re-parses to the same zones.
    fn fmt_param_list(&mut self, params: &[Param]) {
        use crate::AST::ParamZone;
        let mut written = 0usize;
        let mut star_done = false;
        for (i, p) in params.iter().enumerate() {
            if p.zone == ParamZone::LabelOnly && !star_done {
                star_done = true;
                if written > 0 {
                    self.write(", ");
                }
                self.write(Syntax::PARAM_ZONE_LABEL_ONLY);
                written += 1;
            }
            if written > 0 {
                self.write(", ");
            }
            self.fmt_param(p);
            written += 1;
            let last_positional_only = p.zone == ParamZone::PositionalOnly
                && params
                    .get(i + 1)
                    .is_none_or(|next| next.zone != ParamZone::PositionalOnly);
            if last_positional_only {
                self.write(", ");
                self.write(Syntax::PARAM_ZONE_POSITIONAL_ONLY);
                written += 1;
            }
        }
    }

    fn fmt_param(&mut self, p: &Param) {
        if p.root {
            self.write("#");
            self.write(Syntax::MARKER_ROOT);
            self.write(" ");
        }
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
            // D-APILABEL1=A: `timeout seconds: Int` — public label, then local name.
            if let Some((label, _)) = &p.public_label {
                self.write(label);
                self.write(" ");
            }
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
            if let Some(names) = &p.declared_view_from_names {
                if !names.is_empty() {
                    self.write(" ");
                    self.write(Syntax::VIEW_FROM);
                    self.write(" ");
                    self.write(&names.join(" | "));
                }
            }
        }
        // S61: a trailing parameter may carry a `= default` value.
        if let Some(default) = &p.default {
            self.write(" = ");
            self.fmt_expr(default, Prec::OrFallback);
        }
    }

    /// D-MEMPROVENANCE3=A: reprint a function return `from` clause.
    fn fmt_declared_return_view_from(
        &mut self,
        map: &crate::AST::ViewProvenanceMap,
        params: &[crate::AST::Param],
    ) {
        if map.is_empty() {
            return;
        }
        self.write(" ");
        self.write(Syntax::VIEW_FROM);
        self.write(" ");
        if map.len() == 1 {
            if let Some((path, provenance)) = map.iter().next() {
                if path.is_empty() {
                    self.fmt_view_source_union(provenance, params);
                    return;
                }
            }
        }
        self.write("(");
        for (i, (path, provenance)) in map.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&path.join("."));
            self.write(": ");
            self.fmt_view_source_union(provenance, params);
        }
        self.write(")");
    }

    fn fmt_view_source_union(
        &mut self,
        provenance: &crate::AST::ViewProvenance,
        params: &[crate::AST::Param],
    ) {
        for (i, path) in provenance.sources.iter().enumerate() {
            if i > 0 {
                self.write(" | ");
            }
            match &path.source {
                crate::AST::ViewSource::Receiver => self.write(Syntax::KW_SELF),
                crate::AST::ViewSource::Parameter(index) => {
                    let ordinary: Vec<&crate::AST::Param> = params
                        .iter()
                        .filter(|param| param.name != Syntax::KW_SELF)
                        .collect();
                    if let Some(param) = ordinary.get(*index) {
                        self.write(&param.name);
                    } else {
                        self.write(&format!("param{index}"));
                    }
                }
                crate::AST::ViewSource::Static { module_path, name } => {
                    self.write(Syntax::VIEW_FROM_STATIC);
                    self.write(".");
                    if !module_path.is_empty() {
                        self.write(module_path);
                        self.write(".");
                    }
                    self.write(name);
                }
            }
            for projection in &path.projections {
                match projection {
                    crate::AST::ViewSourceProjection::Field(field) => {
                        self.write(".");
                        self.write(field);
                    }
                    crate::AST::ViewSourceProjection::Index | crate::AST::ViewSourceProjection::Range => {}
                }
            }
        }
    }

    /// Print one type-body member. `derives_decode` is the type-level `Decode`
    /// fact a field's type spelling depends on.
    fn fmt_type_body_member(&mut self, member: TypeBodyMember<'_>, derives_decode: bool) {
        match member {
            TypeBodyMember::Field(field) => {
                self.emit_leading(field.name_span.start);
                let decodes_field = derives_decode
                    && !field.serde_markers.iter().any(|marker| {
                        matches!(
                            marker.name.as_str(),
                            Syntax::MARKER_SKIP | Syntax::MARKER_FLATTEN
                        )
                    });
                self.fmt_field(field, decodes_field);
            }
            TypeBodyMember::CLIBinding(binding) => {
                self.emit_leading(binding.name_span.start);
                self.fmt_cli_binding(binding);
            }
            TypeBodyMember::TraitImpl(block) => self.fmt_trait_impl_block(block),
            TypeBodyMember::Method(method) => {
                self.emit_leading(method.name_span.start);
                self.fmt_func(method, false);
            }
            TypeBodyMember::Validate { start, body } => {
                self.emit_leading(start);
                self.write(Syntax::KW_VALIDATE_BLOCK);
                self.write(" {");
                if body.is_empty() {
                    // An authored `validate { }` is still a block the program
                    // contains; `fmt_body` would render it as a blank line
                    // between the braces.
                    self.end_block();
                } else {
                    self.fmt_body(body);
                }
            }
        }
    }

    /// Emit a source-ordered type-body member list. `preceded` says whether the
    /// body already printed something this list must be separated from (an
    /// enum's variant list).
    fn fmt_type_body_members(
        &mut self,
        members: &[TypeBodyMember<'_>],
        derives_decode: bool,
        preceded: bool,
    ) {
        let mut previous: Option<TypeBodyMember<'_>> = None;
        for member in members.iter().copied() {
            if previous.is_some() || preceded {
                self.newline();
                if !previous.is_some_and(|prior| prior.is_compact() && member.is_compact()) {
                    self.newline();
                }
            }
            self.fmt_type_body_member(member, derives_decode);
            previous = Some(member);
        }
    }

    fn fmt_struct(&mut self, s: &StructDef, top_level: bool) {
        // D-VERDICT-1455-1: the retained registry marker nodes are the sole
        // formatter input. Typed flags remain for sema/codegen only.
        let lone_hash_ok =
            s.layout.is_none() && !s.is_published_schema && !s.is_single_use && !s.is_must_use;
        self.fmt_type_markers(&s.type_markers, lone_hash_ok);
        if top_level {
            self.fmt_pub_qualifier(s.is_pub, s.is_package_pub);
        }
        self.write("struct ");
        self.write(&s.name);
        self.fmt_type_params(&s.type_params);
        self.write(" {");
        self.newline();
        let derives_decode = s
            .derives
            .iter()
            .any(|(name, _)| name == crate::Generics::DECODE);
        self.with_indent(|f| {
            let mut members: Vec<TypeBodyMember<'_>> = Vec::with_capacity(
                s.fields.len() + s.cli_bindings.len() + s.trait_impls.len() + s.methods.len() + 1,
            );
            members.extend(s.fields.iter().map(TypeBodyMember::Field));
            members.extend(s.cli_bindings.iter().map(TypeBodyMember::CLIBinding));
            members.extend(s.trait_impls.iter().map(TypeBodyMember::TraitImpl));
            members.extend(s.methods.iter().map(TypeBodyMember::Method));
            // D-VALIDATE1 (card #506): `validate { … }` is one more body member,
            // printed where it was authored like every other one.
            if let Some(span) = s.validate_span {
                members.push(TypeBodyMember::Validate {
                    start: span.start,
                    body: s.validate_block.as_slice(),
                });
            }
            members.sort_by_key(|member| member.start());
            f.fmt_type_body_members(&members, derives_decode, false);
        });
        self.end_block();
    }

    fn fmt_enum(&mut self, e: &EnumDef, top_level: bool) {
        // D-VERDICT-1455-1: type markers are retained nodes, not flag
        // reconstruction. The flags below remain semantic data only.
        let lone_hash_ok = !e.is_single_use && !e.is_must_use;
        self.fmt_type_markers(&e.type_markers, lone_hash_ok);
        if top_level {
            self.fmt_pub_qualifier(e.is_pub, e.is_package_pub);
        }
        self.write("enum ");
        self.write(&e.name);
        self.fmt_type_params(&e.type_params);
        self.write(" {");
        self.newline();
        let derives_decode = e
            .derives
            .iter()
            .any(|(name, _)| name == crate::Generics::DECODE);
        self.with_indent(|f| {
            if e.groups.is_empty() {
                for (i, v) in e.variants.iter().enumerate() {
                    if i > 0 {
                        f.newline();
                    }
                    f.fmt_variant(v, derives_decode);
                }
            } else {
                f.fmt_enum_grouped(e, derives_decode);
            }
            let mut members: Vec<TypeBodyMember<'_>> =
                Vec::with_capacity(e.trait_impls.len() + e.methods.len());
            members.extend(e.trait_impls.iter().map(TypeBodyMember::TraitImpl));
            members.extend(e.methods.iter().map(TypeBodyMember::Method));
            members.sort_by_key(|member| member.start());
            f.fmt_type_body_members(&members, derives_decode, !e.variants.is_empty());
        });
        self.end_block();
    }

    fn fmt_variant(&mut self, v: &Variant, derives_decode: bool) {
        self.fmt_variant_name_and_payload(v, &v.name, derives_decode);
    }

    /// D-TAG1: emit grouped enum bodies from flat leaves + `groups` metadata.
    fn fmt_enum_grouped(&mut self, e: &EnumDef, derives_decode: bool) {
        let entries = Self::enum_entries_at_prefix(e, "");
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            match entry {
                EnumFmtEntry::Leaf(v) => self.fmt_variant(v, derives_decode),
                EnumFmtEntry::Group(g) => self.fmt_enum_group(e, g, derives_decode),
            }
        }
    }

    fn fmt_enum_group(&mut self, e: &EnumDef, g: &EnumGroup, derives_decode: bool) {
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
                        f.fmt_variant_name_and_payload(v, leaf, derives_decode);
                    }
                    EnumFmtEntry::Group(sub) => f.fmt_enum_group(e, sub, derives_decode),
                }
            }
        });
        self.end_block();
    }

    fn fmt_variant_name_and_payload(
        &mut self,
        v: &Variant,
        name: &str,
        derives_decode: bool,
    ) {
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
            VariantPayload::Single(ty, span) => {
                self.write("(");
                self.fmt_decode_type(ty, *span, derives_decode);
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
                    self.fmt_decode_type(&fld.ty, fld.ty_span, derives_decode);
                }
                self.write(")");
            }
        }
        if let Some(expr) = &v.discriminant_expr {
            self.write(" = ");
            self.fmt_expr(expr, Prec::OrFallback);
        } else if let Some(value) = v.discriminant {
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
        // D-OSTARGET1=A (ratified 2026-07-01, c134): `#Target(OS.Linux|OS.MacOS|OS.Windows)`
        // precedes the `impl` block it gates, on its own line.
        if let Some(os) = i.os_target {
            self.write(&format!(
                "#{}({}.{})",
                Syntax::MARKER_TARGET,
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
            f.fmt_impl_body(&i.assoc_type_impls, &i.methods);
        });
        self.end_block();
    }

    fn fmt_const(&mut self, c: &ConstDef) {
        if let Some(meta) = &c.meta {
            self.fmt_meta_attr(meta);
            self.newline();
        }
        // D-PERSIST1: `#Persist` on a bare binding (not on `comptime`).
        // D-BIND-BARE1: preserve `::` vs `:=`.
        if c.is_persist {
            self.write(&format!("#{} ", Syntax::MARKER_PERSIST));
            self.write(&c.name);
            self.write(" ");
            self.write(if c.mutable {
                Syntax::SIGIL_BIND_MUT
            } else {
                Syntax::SIGIL_BIND_IMMUT
            });
            self.write(" ");
            self.fmt_expr(&c.value, Prec::OrFallback);
            return;
        }
        if c.is_comptime {
            // D-CONSTMARK1: `#Static` / `#Inline` precede the marked name.
            for attr in &c.attrs {
                match attr {
                    ConstAttr::ForceStatic => self.write("#Static "),
                    ConstAttr::ForceInline => self.write("#Inline "),
                }
            }
            self.write(&c.name);
            self.write(" :: ");
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
        // Fallback: treat as an explicit known value.
        self.write(&c.name);
        self.write(" :: ");
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
            ImportKind::Unqualified { .. } => {
                let bindings = imp.walk_bindings();
                let Some(first) = bindings.first() else {
                    return;
                };
                // The dotless single-item spelling (`use math.clamp`) only means
                // the same thing when nothing else in the path carries a dot or
                // an alias: `use core.math.[abs]` and `use math.[abs as a]`
                // would both re-read as a module import.
                let collapses = bindings.len() == 1
                    && first.alias.is_none()
                    && first.original.is_some_and(|original| !original.contains('.'))
                    && !first.module_alias.contains('.');
                if collapses {
                    self.write(&first.path());
                } else {
                    self.write(first.module_alias);
                    self.write(".[");
                    let rendered: Vec<String> = bindings
                        .iter()
                        .map(|binding| {
                            let original = binding
                                .original
                                .expect("member walker returned a binding without a member");
                            binding
                                .alias
                                .map(|alias| format!("{original} as {alias}"))
                                .unwrap_or_else(|| original.to_string())
                        })
                        .collect();
                    self.write(&rendered.join(", "));
                    self.write("]");
                }
            }
        }
    }

    fn fmt_field(&mut self, field: &Field, derives_decode: bool) {
        // D-SHAPE2: field rules share one inline `#[…]` group. Redact has a
        // dedicated semantic bit, so fold it back into the same group here.
        if field.redact || !field.serde_markers.is_empty() {
            self.write(Syntax::RULE_PREFIX);
            let count = usize::from(field.redact) + field.serde_markers.len();
            if count > 1 {
                self.write("[");
            }
            if field.redact {
                self.write(Syntax::MARKER_REDACT);
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
        self.fmt_decode_type(&field.ty, field.ty_span, derives_decode);
        // D-FIELDPOL1: `name: T -> expr` — a computed field.
        if let Some(expr) = &field.computed {
            self.write(" ");
            self.write(Syntax::OP_UNIFIED_ARROW);
            self.write(" ");
            self.fmt_expr(expr, Prec::OrFallback.add_rhs());
        }
        // D-FIELDDEF1=C: `name: T = expr` — absence / construction default.
        if let Some(expr) = &field.default {
            self.write(" = ");
            self.fmt_expr(expr, Prec::OrFallback.add_rhs());
        }
        let end = field
            .default
            .as_deref()
            .or(field.computed.as_deref())
            .map(|expr| expr.span().end)
            .unwrap_or(field.ty_span.end);
        self.emit_trailing(end);
    }

    /// D-CLI-GLOBAL1=E: format a callable program-struct member without
    /// lowering it into a field or a method.
    fn fmt_cli_binding(&mut self, binding: &crate::AST::CLICommandBinding) {
        if !binding.markers.is_empty() {
            self.write(Syntax::RULE_PREFIX);
            if binding.markers.len() > 1 {
                self.write("[");
            }
            for (index, marker) in binding.markers.iter().enumerate() {
                if index > 0 {
                    self.write(", ");
                }
                self.fmt_marker(marker);
            }
            if binding.markers.len() > 1 {
                self.write("]");
            }
            self.write(" ");
        }
        self.write(&binding.name);
        self.write(" = ");
        self.fmt_expr(&binding.target, Prec::OrFallback);
        self.emit_trailing(binding.target.span().end);
    }
}
