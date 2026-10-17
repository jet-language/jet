use super::super::{Diagnostic, Func, Item, MetaAttr, Parser, Span, Syntax, TokKind};
use crate::Diagnostics::TextEdit;

const VISIBILITY_ITEM_FORMS: &str =
    "`fn`, `struct`, `enum`, `trait`, `tag`, `module`, `protocol`, `alias`, `distinct`, `#UnitFamily`, or `NAME :: value`";

impl<'a> Parser<'a> {
    /// D-VISDEFAULT2=A: parse one top-level item after a visibility qualifier.
    pub(in crate::Parser) fn item_after_visibility(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<Item, Diagnostic> {
        match self.peek().kind.clone() {
            TokKind::KwStruct => self
                .struct_def_after_pub_pkg(is_pub, is_package_pub)
                .map(Item::Struct),
            TokKind::KwEnum => self
                .enum_def_after_pub(is_pub, is_package_pub)
                .map(Item::Enum),
            TokKind::KwTrait => self.trait_def(false).map(|mut td| {
                td.is_pub = is_pub;
                td.is_package_pub = is_package_pub;
                Item::Trait(td)
            }),
            TokKind::KwTag => self.tag_def(false).map(|mut td| {
                td.is_pub = is_pub;
                td.is_package_pub = is_package_pub;
                Item::Tag(td)
            }),
            TokKind::KwFn => self
                .bump_then_func_after_fn(
                    is_pub,
                    is_package_pub,
                    false,
                    false,
                    false,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    None,
                    None,
                )
                .map(Item::Func),
            TokKind::KwModule if self.is_code_module_at(1) => {
                self.code_module_with_pkg(is_pub, is_package_pub)
            }
            TokKind::Ident(ref n) if n.as_str() == Syntax::KW_STATE_DECL => {
                self.reject_top_level_state_decl(is_pub, is_package_pub)
            }
            TokKind::Ident(ref n) if n.as_str() == Syntax::KW_PROTOCOL => self
                .protocol_decl_with_pkg(is_pub, is_package_pub)
                .map(Item::ProtocolDecl),
            TokKind::Ident(ref n) if n.as_str() == Syntax::KW_ALIAS => self
                .type_alias_def(is_pub, is_package_pub)
                .map(Item::TypeAlias),
            TokKind::Ident(ref n) if n.as_str() == Syntax::KW_MARKER && self.at_marker_decl() => {
                let mut declaration = self.marker_decl()?;
                declaration.is_pub = is_pub;
                declaration.is_package_pub = is_package_pub;
                Ok(Item::MarkerDecl(declaration))
            }
            TokKind::Ident(_) if self.at_distinct_def() => self
                .distinct_def(is_pub, is_package_pub)
                .map(Item::Distinct),
            // 3A: `pub NAME :: value` exports an immutable module binding.
            TokKind::Ident(_) if matches!(self.peek2().kind, TokKind::ColonColon) => self
                .module_binding_def()
                .map(|mut constant| {
                    constant.is_pub = is_pub;
                    constant.is_package_pub = is_package_pub;
                    Item::Const(constant)
                }),
            TokKind::Hash if self.unqualified_marker_name_at(self.pos) == Some(Syntax::MARKER_UNIT_FAMILY) => {
                self.unit_family_def(is_pub, is_package_pub)
                    .map(Item::UnitFamily)
            }
            _ => {
                let d = Diagnostic::error(
                    "E0003",
                    format!(
                        "expected one of {VISIBILITY_ITEM_FORMS} after a visibility qualifier"
                    ),
                    "all visibility qualifiers share one dispatcher for the canonical top-level declaration forms"
                        .to_string(),
                    format!(
                        "write a visibility qualifier before one of {VISIBILITY_ITEM_FORMS}"
                    ),
                    Some(self.peek().span),
                );
                Err(d)
            }
        }
    }

    /// D-VIS-MODULE1 (card #4723): parse the visibility head of a top-level or
    /// module-body declaration. Declarations are public by default; the only
    /// visibility a declaration may carry is `priv` / `priv(package)` before
    /// `module`. Every retired form (`pub`, `pub(…)`, `private`, declaration
    /// `priv`, section labels) is reported with an edit that deletes it, and the
    /// declaration then parses as if it were absent. Returns the declaration's
    /// own `(is_pub, is_package_pub)`; [`Parser::apply_module_visibility`]
    /// narrows members of private modules after the file is parsed.
    pub(in crate::Parser) fn parse_item_visibility(&mut self) -> (bool, bool) {
        loop {
            let is_label_word = match &self.peek().kind {
                TokKind::KwPub | TokKind::KwPriv => true,
                TokKind::Ident(n) => n == Syntax::KW_PUB || n == Syntax::KW_PRIV || n == Syntax::FOREIGN_PRIVATE,
                _ => false,
            };
            if is_label_word && matches!(self.peek2().kind, TokKind::Colon) {
                self.reject_section_label();
                continue;
            }
            if matches!(self.peek().kind, TokKind::KwPub) {
                self.reject_retired_pub();
                continue;
            }
            if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::FOREIGN_PRIVATE) {
                let word = self.bump().span;
                self.diags.push(
                    Diagnostic::error(
                        "E0412",
                        format!("Jet has no `{}`", Syntax::FOREIGN_PRIVATE),
                        "declarations are public by default; privacy belongs to a `priv module` or a `priv` field".to_string(),
                        format!(
                            "delete `{}`; move the declaration into a `{} module` if it must stay internal",
                            Syntax::FOREIGN_PRIVATE,
                            Syntax::KW_PRIV
                        ),
                        Some(word),
                    )
                    .with_edit(self.deletion_edit(word)),
                );
                continue;
            }
            if matches!(self.peek().kind, TokKind::KwPriv) {
                let (span, package) = self.parse_priv_qualifier();
                if matches!(self.peek().kind, TokKind::KwModule) {
                    return (false, package);
                }
                self.diags.push(
                    Diagnostic::error(
                        "E0413",
                        format!("`{}` marks only modules and fields", Syntax::KW_PRIV),
                        "declarations are public by default; privacy belongs to a `priv module` or a `priv` field".to_string(),
                        format!(
                            "delete `{}`; move the declaration into a `{} module` if it must stay internal",
                            Syntax::KW_PRIV,
                            Syntax::KW_PRIV
                        ),
                        Some(span),
                    )
                    .with_edit(self.deletion_edit(span)),
                );
                continue;
            }
            return (true, false);
        }
    }

    /// D-VIS-MODULE1: the visibility head of a struct field. `priv` keeps the
    /// field to its type's module; returns the field's `is_pub`.
    pub(in crate::Parser) fn parse_field_visibility(&mut self) -> bool {
        let mut is_pub = true;
        loop {
            if matches!(self.peek().kind, TokKind::KwPub) {
                self.reject_retired_pub();
                continue;
            }
            if matches!(self.peek().kind, TokKind::KwPriv) {
                let (span, package) = self.parse_priv_qualifier();
                if package {
                    let qualifier = Span::new(span.start + Syntax::KW_PRIV.len(), span.end);
                    self.diags.push(
                        Diagnostic::error(
                            "E0417",
                            format!("a field takes plain `{}`, not `{}(package)`", Syntax::KW_PRIV, Syntax::KW_PRIV),
                            "a `priv` field is usable only by its type's module; package privacy belongs to modules".to_string(),
                            format!("write `{}`", Syntax::KW_PRIV),
                            Some(span),
                        )
                        .with_edit(TextEdit { span: qualifier, new_text: String::new() }),
                    );
                }
                is_pub = false;
                continue;
            }
            return is_pub;
        }
    }

    /// D-MOD4: `pub use` re-export. Consumes `pub`; any `pub(…)` qualifier is
    /// retired (E0411) and deleted. Returns `true` for the re-export flag.
    pub(in crate::Parser) fn parse_reexport_qualifier(&mut self) -> bool {
        if !matches!(self.peek().kind, TokKind::KwPub) {
            return false;
        }
        self.bump();
        if matches!(self.peek().kind, TokKind::LParen) {
            let start = self.peek().span.start;
            while !matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
                self.bump();
            }
            let end = self.bump().span.end;
            let span = Span::new(start, end);
            self.diags.push(
                Diagnostic::error(
                    "E0411",
                    "a re-export is written `pub use`; `pub(…)` qualifiers were removed".to_string(),
                    "declarations are public by default and package privacy belongs to `priv(package) module`".to_string(),
                    "delete the qualifier".to_string(),
                    Some(span),
                )
                .with_edit(TextEdit { span, new_text: String::new() }),
            );
        }
        true
    }

    /// D-VIS-MODULE1: consume a retired `pub` / `pub(…)` before a declaration,
    /// a method, or a field, reporting E0411 with an edit that deletes it.
    pub(in crate::Parser) fn reject_retired_pub(&mut self) {
        let start = self.bump().span;
        let mut span = start;
        if matches!(self.peek().kind, TokKind::LParen) {
            while !matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
                self.bump();
            }
            span = Span::new(start.start, self.bump().span.end);
        }
        self.diags.push(
            Diagnostic::error(
                "E0411",
                format!("`{}` was removed: everything is public by default", Syntax::KW_PUB),
                format!(
                    "privacy belongs to a `{} module` (or `{}(package) module`) and to `{}` fields",
                    Syntax::KW_PRIV,
                    Syntax::KW_PRIV,
                    Syntax::KW_PRIV
                ),
                format!("delete `{}`", self.source_text(span)),
                Some(span),
            )
            .with_edit(self.deletion_edit(span)),
        );
    }

    /// D-VIS-MODULE1: `pub:` / `priv:` section labels (E0415), deleted.
    pub(in crate::Parser) fn reject_section_label(&mut self) {
        let span = Span::new(self.peek().span.start, self.peek2().span.end);
        self.bump();
        self.bump();
        self.diags.push(
            Diagnostic::error(
                "E0415",
                "section visibility labels like `pub:` / `priv:` are not supported".to_string(),
                "moving an item above or below a label would silently change its visibility".to_string(),
                format!(
                    "delete the label; group internal declarations in a `{} module`",
                    Syntax::KW_PRIV
                ),
                Some(span),
            )
            .with_edit(self.deletion_edit(span)),
        );
    }

    /// D-VIS-MODULE1: the retired `#PubFile` / `#PublicFile` file marker
    /// (E0418), deleted.
    pub(in crate::Parser) fn reject_retired_file_marker(&mut self, span: Span) {
        self.diags.push(
            Diagnostic::error(
                "E0418",
                format!("`#{}` was removed: every file is public by default", Syntax::MARKER_PUB_FILE),
                "declarations are public by default, so the file marker has nothing left to flip".to_string(),
                "delete the marker".to_string(),
                Some(span),
            )
            .with_edit(self.deletion_edit(span)),
        );
    }

    /// Parse `priv` or `priv(package)`; an unknown `priv(…)` qualifier is
    /// E0414 and deleted. Returns the whole marker span and whether it names
    /// the package.
    fn parse_priv_qualifier(&mut self) -> (Span, bool) {
        let start = self.bump().span;
        if !matches!(self.peek().kind, TokKind::LParen) {
            return (start, false);
        }
        let open = self.bump().span;
        let package = matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::PRIV_PACKAGE_QUALIFIER)
            && matches!(self.peek2().kind, TokKind::RParen);
        while !matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
            self.bump();
        }
        let end = self.bump().span.end;
        if !package {
            let qualifier = Span::new(open.start, end);
            self.diags.push(
                Diagnostic::error(
                    "E0414",
                    format!(
                        "unknown `{}(…)` qualifier: only `{}({})` exists",
                        Syntax::KW_PRIV,
                        Syntax::KW_PRIV,
                        Syntax::PRIV_PACKAGE_QUALIFIER
                    ),
                    "`priv(package) module` is the only qualified module privacy".to_string(),
                    format!("delete the qualifier, or write `{}({})`", Syntax::KW_PRIV, Syntax::PRIV_PACKAGE_QUALIFIER),
                    Some(qualifier),
                )
                .with_edit(TextEdit { span: qualifier, new_text: String::new() }),
            );
        }
        (Span::new(start.start, end), package)
    }

    /// An edit deleting `span` and the horizontal whitespace after it.
    pub(in crate::Parser) fn deletion_edit(&self, span: Span) -> TextEdit {
        let bytes = self.source.as_deref().unwrap_or_default().as_bytes();
        let mut end = span.end;
        while end < bytes.len() && matches!(bytes[end], b' ' | b'\t') {
            end += 1;
        }
        TextEdit { span: Span::new(span.start, end), new_text: String::new() }
    }

    fn source_text(&self, span: Span) -> &str {
        self.source
            .as_deref()
            .and_then(|source| source.get(span.start..span.end))
            .unwrap_or(Syntax::KW_PUB)
    }

    /// D-VIS-MODULE1: after a file is parsed, give every declaration inside a
    /// `priv module` the module's privacy. A `priv module` is usable only by
    /// its enclosing module (Private: the enclosing package namespace), a
    /// `priv(package) module` by its package (Package); every other
    /// declaration is public. Fields keep their own `priv`.
    pub(in crate::Parser) fn apply_module_visibility(items: &mut [Item], inherited: (bool, bool)) {
        fn narrow(outer: (bool, bool), own: (bool, bool)) -> (bool, bool) {
            let private = |v: (bool, bool)| !v.0 && !v.1;
            if private(outer) || private(own) {
                (false, false)
            } else if outer.1 || own.1 {
                (false, true)
            } else {
                (true, false)
            }
        }
        fn set_methods(methods: &mut [Func], vis: (bool, bool)) {
            for method in methods {
                method.is_pub = vis.0;
                method.is_package_pub = vis.1;
            }
        }
        for item in items {
            match item {
                Item::CodeModule(module) => {
                    let effective = narrow(inherited, (module.is_pub, module.is_package_pub));
                    if let Some(body) = module.body.as_mut() {
                        Self::apply_module_visibility(body, effective);
                    }
                }
                Item::GenericModule(module) => {
                    let effective = narrow(inherited, (module.is_pub, module.is_package_pub));
                    Self::apply_module_visibility(&mut module.body, effective);
                }
                Item::ModuleAlias(alias) => {
                    let effective = narrow(inherited, (alias.is_pub, alias.is_package_pub));
                    (alias.is_pub, alias.is_package_pub) = effective;
                }
                Item::Func(f) => (f.is_pub, f.is_package_pub) = inherited,
                Item::Struct(def) => {
                    (def.is_pub, def.is_package_pub) = inherited;
                    set_methods(&mut def.methods, inherited);
                }
                Item::Enum(def) => {
                    (def.is_pub, def.is_package_pub) = inherited;
                    set_methods(&mut def.methods, inherited);
                }
                Item::Impl(def) => set_methods(&mut def.methods, inherited),
                Item::Distinct(def) => (def.is_pub, def.is_package_pub) = inherited,
                Item::TypeAlias(def) => (def.is_pub, def.is_package_pub) = inherited,
                Item::UnitFamily(def) => (def.is_pub, def.is_package_pub) = inherited,
                Item::Trait(def) => (def.is_pub, def.is_package_pub) = inherited,
                Item::Tag(def) => (def.is_pub, def.is_package_pub) = inherited,
                Item::Const(def) => (def.is_pub, def.is_package_pub) = inherited,
                Item::ProtocolDecl(def) => (def.is_pub, def.is_package_pub) = inherited,
                _ => {}
            }
        }
    }

    /// Parse a function whose `#Pure`/`#Sanitizer` modifiers are already known.
    pub(super) fn func_with_modifiers(
        &mut self,
        is_pure: bool,
        is_sanitizer: bool,
    ) -> Result<Func, Diagnostic> {
        self.func_with_modifiers_full(
            is_pure,
            is_sanitizer,
            None,
            None,
            None,
            None,
            false,
            None,
            None,
            None,
            false,
            false,
            None,
            false,
            None,
        )
    }

    /// Parse a function whose `#Pure`/`#Sanitizer` and D-STATE1 typestate markers
    /// are already known.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn func_with_modifiers_full(
        &mut self,
        is_pure: bool,
        is_sanitizer: bool,
        meta: Option<MetaAttr>,
        state_requires: Option<(String, Span)>,
        state_transition: Option<crate::AST::StateTransition>,
        web_marker: Option<crate::Syntax::WebPartitionMarker>,
        is_must_use: bool,
        must_use_span: Option<Span>,
        maturity: Option<crate::AST::MaturityTag>,
        maturity_span: Option<Span>,
        is_inline: bool,
        is_inline_always: bool,
        inline_span: Option<Span>,
        is_replayable: bool,
        replayable_span: Option<Span>,
    ) -> Result<Func, Diagnostic> {
        let (is_pub, is_package_pub) = self.parse_item_visibility();
        self.expect_kw(TokKind::KwFn, "to start a function definition")?;
        self.func_after_fn(
            is_pub,
            is_package_pub,
            false,
            None,
            None,
            is_pure,
            is_sanitizer,
            meta,
            state_requires,
            state_transition,
            false,
            web_marker,
            is_must_use,
            must_use_span,
            maturity,
            maturity_span,
            is_inline,
            is_inline_always,
            inline_span,
            is_replayable,
            replayable_span,
            false,
        )
    }
}
