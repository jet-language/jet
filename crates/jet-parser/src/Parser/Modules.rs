use super::*;

impl<'a> Parser<'a> {
    /// U3 / D-SHAPE-MODULEINTERNAL1=A: `module name { … }`. Many modules may
    /// share a file; a leading-`_` name opts one out of automatic discovery.
    ///
    /// D-JPK-MODBODY1=A (ratified 2026-07-02): the canonical role-module form
    /// puts the namespace in the declaration name — `module env.dev { fields }`,
    /// `module system.laptop { … }` — with the role's fields bare in the body.
    /// The old contribution form (`module dev { env.dev: Env.{ … } }`) still
    /// parses for recovery but teaches E1229; it is not a legal stable spelling.
    pub(super) fn module_decl(&mut self) -> Result<ModuleDecl, Diagnostic> {
        let start = self.bump().span; // `module`
                                      // S84: module names may be kebab-case (a module is the package the
                                      // payload manifest discovers by name).
        let (name, name_span) = self.expect_dashed_name("for the module name")?;
        let auto_discovered = !name.starts_with(Syntax::MODULE_INTERNAL_PREFIX);
        // D-JPK-MODBODY1=A: a dot after the name means a role declaration —
        // the name is a reserved namespace, the segment after the dot the role.
        if matches!(self.peek().kind, TokKind::Dot) {
            return self.role_module_decl(start, &name, name_span, auto_discovered);
        }
        self.expect(TokKind::LBrace, "to open the module body")?;
        let mut sources = Vec::new();
        let mut imports = Vec::new();
        let mut members = Vec::new();
        let mut contributions = Vec::new();
        // A module body holds four kinds of entry (U3/U8/D-WORKSPACE1):
        // `sources:`, `imports:`, and `members:` fields, and typed
        // `namespace.path: Value` contributions. The first three are
        // distinguished by their reserved name followed by `:`; contributions
        // begin with a namespace name followed by `.`.
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            match &self.peek().kind {
                TokKind::Ident(n)
                    if n == Syntax::MODULE_FIELD_SOURCES
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    sources.extend(self.module_sources()?);
                }
                TokKind::Ident(n)
                    if n == Syntax::MODULE_FIELD_IMPORTS
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    imports.push(self.module_import()?);
                }
                TokKind::Ident(n)
                    if n == Syntax::MODULE_FIELD_MEMBERS
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    members.push(self.module_members()?);
                }
                _ => contributions.push(self.contribution()?),
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close the module body")?;
        Ok(ModuleDecl {
            name,
            name_span,
            auto_discovered,
            sources,
            imports,
            members,
            contributions,
            span: Span::new(start.start, end),
        })
    }

    /// D-JPK-MODBODY1=A: the canonical role-module declaration —
    /// `module env.dev { fields }` / `module system.laptop { … }` /
    /// `module image.installer-iso { … }`. The namespace lives in the
    /// declaration name; the body carries the role's fields bare (plus the U8
    /// reserved `sources:`/`imports:` siblings). Desugars to the same
    /// `Contribution` IR the merge engine already evaluates, so one module
    /// declares exactly one role.
    fn role_module_decl(
        &mut self,
        start: Span,
        ns_name: &str,
        ns_span: Span,
        auto_discovered: bool,
    ) -> Result<ModuleDecl, Diagnostic> {
        // A leading `_` opts the module out of automatic discovery — strip it
        // for the namespace word: `module _env.dev { … }`.
        let ns_word = ns_name.trim_start_matches(Syntax::MODULE_INTERNAL_PREFIX);
        let namespace = match ns_word {
            _ if ns_word == Syntax::NS_ENV => Namespace::Env,
            _ if ns_word == Syntax::NS_SYSTEM => Namespace::System,
            _ if ns_word == Syntax::NS_IMAGE => Namespace::Image,
            _ if ns_word == Syntax::NS_FLEET => Namespace::Fleet,
            _ if ns_word == Syntax::NS_VMTEST => Namespace::VmTest,
            _ if ns_word == Syntax::NS_PERF => Namespace::Perf,
            _ => {
                return Err(Diagnostic::error(
                    "E0960",
                    format!("`{}` is not a module namespace", ns_word),
                    format!(
                        "a role module declares one of the reserved namespaces in its name: `{}` (a dev environment), `{}` (a whole machine), `{}` (a disk image), `{}` (a host fleet), `{}` (a VM test), or `{}` (performance policy)",
                        Syntax::NS_ENV, Syntax::NS_SYSTEM, Syntax::NS_IMAGE, Syntax::NS_FLEET, Syntax::NS_VMTEST, Syntax::NS_PERF
                    ),
                    format!(
                        "write `module {}.<name> {{ … }}`, `module {}.<name> {{ … }}`, `module {}.<name> {{ … }}`, `module {}.<name> {{ … }}`, `module {}.<name> {{ … }}`, or `module {}.<name> {{ … }}`",
                        Syntax::NS_ENV, Syntax::NS_SYSTEM, Syntax::NS_IMAGE, Syntax::NS_FLEET, Syntax::NS_VMTEST, Syntax::NS_PERF
                    ),
                    Some(ns_span),
                ));
            }
        };
        self.bump(); // `.`
                     // S84: role names may be kebab-case (`system.my-host`).
        let (path, path_span) = self.expect_dashed_name("for the role name")?;
        self.expect(TokKind::LBrace, "to open the module body")?;

        let mut sources = Vec::new();
        let mut imports = Vec::new();
        // Env bodies collect bare `field: expr` pairs into an inferred record
        // (U18); system/image bodies reuse their dedicated field parsers.
        let mut env_fields: Vec<(String, Span, Expr)> = Vec::new();
        // U12: a dev `services: { name: { … }, … }` map, reusing the exact
        // `services_map()` grammar `system.<name>.services` already parses —
        // captured separately from `env_fields` because it isn't a bare
        // `field: expr` pair (see `EnvLit`).
        let mut env_services: Vec<crate::AST::ServiceEntry> = Vec::new();
        let mut system_fields: Vec<crate::AST::SystemField> = Vec::new();
        let mut image_fields: Vec<crate::AST::ImageField> = Vec::new();
        let mut fleet_fields: Vec<crate::AST::FleetField> = Vec::new();
        let mut vmtest_fields: Vec<crate::AST::VmTestField> = Vec::new();
        let mut perf_budgets: Option<(Span, Expr)> = None;
        let body_start = self.peek().span.start;

        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            match &self.peek().kind {
                TokKind::Ident(n)
                    if n == Syntax::MODULE_FIELD_SOURCES
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    sources.extend(self.module_sources()?);
                }
                TokKind::Ident(n)
                    if n == Syntax::MODULE_FIELD_IMPORTS
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    imports.push(self.module_import()?);
                }
                _ => match namespace {
                    Namespace::Env => {
                        let (field, field_span) = self.expect_ident("for a field name")?;
                        self.expect(TokKind::Colon, "after a field name")?;
                        // U12: `services:` is a dev-supervised map, not a bare
                        // scalar/list field — parse it with the same
                        // dedicated grammar `System.services` uses.
                        if field == Syntax::SYSTEM_FIELD_SERVICES {
                            env_services.extend(self.services_map()?);
                        } else {
                            let value = self.expr()?;
                            env_fields.push((field, field_span, value));
                        }
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                        }
                    }
                    Namespace::System => {
                        system_fields.push(self.system_field()?);
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                        }
                    }
                    Namespace::Image => {
                        image_fields.push(self.image_field()?);
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                        }
                    }
                    Namespace::Fleet => {
                        fleet_fields.push(self.fleet_field()?);
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                        }
                    }
                    Namespace::VmTest => {
                        vmtest_fields.push(self.vmtest_field()?);
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                        }
                    }
                    Namespace::Perf => {
                        let (field, field_span) = self.expect_ident("for a performance policy field")?;
                        self.expect(TokKind::Colon, "after a performance policy field")?;
                        if field != Syntax::PERF_FIELD_BUDGETS || perf_budgets.is_some() {
                            return Err(Diagnostic::error(
                                "E2903",
                                format!("performance budget role `{path}` is not valid"),
                                "a performance role contains exactly one `budgets` list".to_string(),
                                "write `budgets: [Budget.{ ... }]` once".to_string(),
                                Some(field_span),
                            ));
                        }
                        perf_budgets = Some((field_span, self.expr()?));
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                        }
                    }
                },
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close the module body")?;
        let body_span = Span::new(body_start, end);

        let value = match namespace {
            Namespace::Env => crate::AST::ContribValue::Env(crate::AST::EnvLit {
                fields: env_fields,
                services: env_services,
                span: body_span,
            }),
            Namespace::System => crate::AST::ContribValue::System(crate::AST::SystemLit {
                explicit_type: None,
                fields: system_fields,
                span: body_span,
            }),
            Namespace::Image => crate::AST::ContribValue::Image(crate::AST::ImageLit {
                explicit_type: None,
                fields: image_fields,
                span: body_span,
            }),
            Namespace::Fleet => crate::AST::ContribValue::Fleet(crate::AST::FleetLit {
                explicit_type: None,
                fields: fleet_fields,
                span: body_span,
            }),
            Namespace::VmTest => crate::AST::ContribValue::VmTest(crate::AST::VmTestLit {
                explicit_type: None,
                fields: vmtest_fields,
                span: body_span,
            }),
            Namespace::Perf => {
                let Some((budgets_span, budgets)) = perf_budgets else {
                    return Err(Diagnostic::error(
                        "E2903",
                        format!("performance budget role `{path}` is not valid"),
                        "a performance role requires one `budgets` list".to_string(),
                        "add `budgets: [Budget.{ ... }]`".to_string(),
                        Some(path_span),
                    ));
                };
                let list_span = budgets.span();
                // D-PERFBUDGET-SURFACE1: `budgets: [Budget.{ … }]`.
                // D-DOTCTOR3: also `budgets: [Budget].{ .{ … }, … }` (typed-list head).
                let typed_budgets = Self::perf_budget_decls(budgets, &path)?;
                crate::AST::ContribValue::Perf(crate::AST::PerfLit {
                    budgets: typed_budgets,
                    budgets_span,
                    list_span,
                    span: body_span,
                })
            }
        };
        let contribution = Contribution {
            namespace,
            path: path.clone(),
            path_span,
            value,
            span: Span::new(ns_span.start, end),
        };
        Ok(ModuleDecl {
            // The declaration name is the full dotted role — `env.dev`.
            name: format!("{ns_name}.{path}"),
            name_span: Span::new(ns_span.start, path_span.end),
            auto_discovered,
            sources,
            imports,
            members: Vec::new(),
            contributions: vec![contribution],
            span: Span::new(start.start, end),
        })
    }

    /// Collect `Budget` decls from a `budgets:` value.
    /// Accepts plain `[Budget.{ … }, …]` and D-DOTCTOR3 `[Budget].{ .{ … }, … }`.
    fn perf_budget_decls(
        budgets: Expr,
        path: &str,
    ) -> Result<Vec<crate::AST::BudgetDecl>, Diagnostic> {
        let (entries, allow_inferred) = match budgets {
            Expr::ListLit(entries, _) => (entries, false),
            Expr::TypedLit {
                head: Some(Type::List(inner)),
                body,
                span,
            } => {
                let ok = matches!(inner.as_ref(), Type::Named(n) if n == Syntax::TYPE_BUDGET);
                if !ok {
                    return Err(Diagnostic::error(
                        "E2903",
                        format!("performance budget role `{path}` is not valid"),
                        format!(
                            "`budgets` must be a list of `{0}` values, not `{1}`",
                            Syntax::TYPE_BUDGET,
                            Type::List(inner).name()
                        ),
                        format!("write `budgets: [{0}].{{ … }}` or `budgets: [{0}.{{ … }}]`", Syntax::TYPE_BUDGET),
                        Some(span),
                    ));
                }
                match body {
                    TypedLitBody::Elements(entries) => (entries, true),
                    TypedLitBody::Empty => (Vec::new(), true),
                    _ => {
                        return Err(Diagnostic::error(
                            "E2903",
                            format!("performance budget role `{path}` is not valid"),
                            "`budgets` must be a list of typed `Budget` values".to_string(),
                            format!(
                                "write `budgets: [{0}].{{ .{{ … }}, … }}` or `budgets: [{0}.{{ … }}]`",
                                Syntax::TYPE_BUDGET
                            ),
                            Some(span),
                        ));
                    }
                }
            }
            other => {
                return Err(Diagnostic::error(
                    "E2903",
                    format!("performance budget role `{path}` is not valid"),
                    "`budgets` must be a list of typed `Budget` values".to_string(),
                    format!(
                        "write `budgets: [{0}].{{ … }}` or `budgets: [{0}.{{ … }}]`",
                        Syntax::TYPE_BUDGET
                    ),
                    Some(other.span()),
                ));
            }
        };
        let mut typed_budgets = Vec::with_capacity(entries.len());
        for entry in entries {
            typed_budgets.push(Self::perf_budget_entry(entry, allow_inferred)?);
        }
        Ok(typed_budgets)
    }

    fn perf_budget_entry(
        entry: Expr,
        allow_inferred: bool,
    ) -> Result<crate::AST::BudgetDecl, Diagnostic> {
        let entry_span = entry.span();
        match entry {
            Expr::StructLit {
                type_name,
                fields,
                span,
                inferred,
                ..
            } => {
                let named_budget = type_name == Syntax::TYPE_BUDGET;
                let inferred_budget = allow_inferred && inferred && type_name.is_empty();
                if !named_budget && !inferred_budget {
                    let why = if type_name.is_empty() {
                        "every `budgets` item must be one typed `Budget` literal".to_string()
                    } else {
                        format!("this list item has type `{type_name}`, not `Budget`")
                    };
                    return Err(Diagnostic::error(
                        "E2903",
                        "performance budget entry is not valid".to_string(),
                        why,
                        "write `Budget.{ name: ..., metric: ..., limit: ... }`".to_string(),
                        Some(span),
                    ));
                }
                Ok(crate::AST::BudgetDecl {
                    fields: fields
                        .into_iter()
                        .map(|(name, name_span, value)| {
                            let value_span = value.span();
                            crate::AST::BudgetField {
                                name,
                                name_span,
                                value,
                                span: Span::new(name_span.start, value_span.end),
                            }
                        })
                        .collect(),
                    span,
                })
            }
            Expr::TypedLit {
                head: Some(Type::Named(name)),
                body: TypedLitBody::Fields(fields),
                span,
            } if name == Syntax::TYPE_BUDGET => Ok(crate::AST::BudgetDecl {
                fields: fields
                    .into_iter()
                    .map(|(name, name_span, value)| {
                        let value_span = value.span();
                        crate::AST::BudgetField {
                            name,
                            name_span,
                            value,
                            span: Span::new(name_span.start, value_span.end),
                        }
                    })
                    .collect(),
                span,
            }),
            _ => Err(Diagnostic::error(
                "E2903",
                "performance budget entry is not valid".to_string(),
                "every `budgets` item must be one typed `Budget` literal".to_string(),
                "write `Budget.{ name: ..., metric: ..., limit: ... }`".to_string(),
                Some(entry_span),
            )),
        }
    }

    /// D-MOD1/2: decide whether `module name` at position `offset` (1 = current pos
    /// already on `module`, 2 = one past `pub`) starts a code module vs a JetOS module.
    ///
    /// Rules (from the plan):
    /// - `module name ;` → code module (file declaration), always.
    /// - `module name {` and first token inside is `fn`, `struct`, `enum`, `impl`,
    ///   `pub`, `const`, `use`, `module`, `}` → code module (inline body).
    /// - `module name {` and first token inside is `sources`, `imports`, or an
    ///   ident followed by `.` → JetOS module.
    pub(super) fn is_code_module_at(&self, offset: usize) -> bool {
        // After `module` is at pos+offset, name is at pos+offset+1.
        let kw_pos = self.pos + offset - 1; // position of `module` token
                                            // If the token after the name is `-`, the name is dashed (my-host) —
                                            // that's always a JetOS module; code module names are plain identifiers.
        let after_name = kw_pos + 2;
        if matches!(
            self.toks.get(after_name).map(|t| &t.kind),
            Some(TokKind::Minus)
        ) {
            return false;
        }
        // Find the `{` or `;` that follows the (plain) name.
        let scan = after_name;
        let next = &self.toks[scan.min(self.toks.len() - 1)];
        match &next.kind {
            TokKind::Semi => true, // `module name;` — always a code module
            // D-GENMOD2=A: `module name<params> { … }` — generic module template.
            TokKind::Lt => true,
            // D-GENMOD2=A: `module alias = module_name<args>` — module alias.
            TokKind::Eq => true,
            TokKind::LBrace => {
                // Peek inside the `{` at scan+1
                let inside = &self.toks[(scan + 1).min(self.toks.len() - 1)];
                match &inside.kind {
                    // Code module body starters
                    TokKind::KwFn
                    | TokKind::KwStruct
                    | TokKind::KwEnum
                    | TokKind::KwImpl
                    | TokKind::KwPub
                    | TokKind::KwConst
                    | TokKind::KwUse
                    | TokKind::KwModule
                    | TokKind::KwTrait
                    | TokKind::KwTag
                    | TokKind::KwComptime
                    // D-CASING1 follow-on: `#Test`/`#Pure`/`#Unsafe` markers start
                    // with `#` inside a code-module body.
                    | TokKind::Hash
                    | TokKind::RBrace => true,
                    // JetOS body starters: `sources:`, `imports:`, `members:`, or `ident .`
                    TokKind::Ident(n)
                        if n == Syntax::MODULE_FIELD_SOURCES
                            || n == Syntax::MODULE_FIELD_IMPORTS
                            || n == Syntax::MODULE_FIELD_MEMBERS =>
                    {
                        false
                    }
                    TokKind::Ident(_) => {
                        // `ident .` → JetOS contribution; anything else → code module
                        let after_inside =
                            &self.toks[(scan + 2).min(self.toks.len() - 1)];
                        !matches!(after_inside.kind, TokKind::Dot)
                    }
                    _ => false, // unknown → treat as JetOS to be safe
                }
            }
            _ => false,
        }
    }

    /// D-MOD1/2: parse `[pub] module name ;` or `[pub] module name { items }`,
    /// or D-GENMOD2=A generic/alias forms. Returns an `Item` (CodeModule,
    /// GenericModule, or ModuleAlias) so all callers share one return type.
    pub(super) fn code_module(&mut self, is_pub: bool) -> Result<Item, Diagnostic> {
        self.code_module_with_pkg(is_pub, false)
    }

    pub(super) fn code_module_with_pkg(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<Item, Diagnostic> {
        let web_target = if self.at_web_target() {
            match self.parse_web_target_marker()? {
                super::Items::TargetMarker::Bucket(b) => Some(b),
                super::Items::TargetMarker::DefaultWeb => {
                    let span = self.peek().span;
                    return Err(Diagnostic::error(
                        "E0003",
                        "`#Target(Web)` isn't valid on a module".to_string(),
                        "`Web` is a file-level default-backend marker, not a partition ceiling"
                            .to_string(),
                        "move `#Target(Web)` to the top of the file, outside any module; use `#Target(Wasm)` or `#Target(JS)` on a module".to_string(),
                        Some(span),
                    ));
                }
                // D-OSTARGET1=A: `OS.*` gates an `impl` block, never a module.
                super::Items::TargetMarker::OS(os) => {
                    let span = self.peek().span;
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("`#Target(OS.{})` isn't valid on a module", os.name()),
                        "`OS.Linux`/`OS.MacOS`/`OS.Windows` gates a single `impl` block, not a module".to_string(),
                        format!("move `#Target(OS.{})` to the `impl` block itself", os.name()),
                        Some(span),
                    ));
                }
            }
        } else {
            None
        };
        self.code_module_with_pkg_and_target(is_pub, is_package_pub, web_target)
    }

    pub(super) fn code_module_with_pkg_and_target(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
        web_target: Option<crate::Syntax::WebBucket>,
    ) -> Result<Item, Diagnostic> {
        if is_pub {
            self.bump(); // consume `pub`
        }
        let start = self.bump().span; // consume `module`
        let (name, name_span) = self.expect_ident("for the code module name")?;
        match &self.peek().kind {
            // D-GENMOD2=A: `module name<params> { body }` — generic module template
            TokKind::Lt => {
                self.finish_generic_module(name, name_span, is_pub, is_package_pub, start)
            }
            // D-GENMOD2=A: `module alias = target<args>` — module alias
            TokKind::Eq => self.finish_module_alias(name, name_span, is_pub, is_package_pub, start),
            TokKind::Semi => {
                let end = self.bump().span.end; // consume `;`
                Ok(Item::CodeModule(CodeModule {
                    name,
                    name_span,
                    is_pub,
                    is_package_pub,
                    body: None,
                    web_target,
                    instance_identity: None,
                    span: Span::new(start.start, end),
                }))
            }
            TokKind::LBrace => {
                self.bump(); // consume `{`
                let mut items = Vec::new();
                while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                    if matches!(self.peek().kind, TokKind::Semi) {
                        self.bump();
                        continue;
                    }
                    match self.top_level_item_in_code_module() {
                        Ok(item) => items.push(item),
                        Err(d) => {
                            self.diags.push(d);
                            self.sync_top();
                        }
                    }
                }
                let end = self.peek().span.end;
                self.expect(TokKind::RBrace, "to close the module body")?;
                Ok(Item::CodeModule(CodeModule {
                    name,
                    name_span,
                    is_pub,
                    is_package_pub,
                    body: Some(items),
                    web_target,
                    instance_identity: None,
                    span: Span::new(start.start, end),
                }))
            }
            other => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected `{{` or `;` after a module name, found {}",
                    describe(other)
                ),
                "write `module name;` to load a file, or `module name { … }` for an inline module"
                    .to_string(),
                "example: `module math;` or `module math { pub fn double(n: Int) => Int { … } }`"
                    .to_string(),
                Some(self.peek().span),
            )),
        }
    }

    /// D-GENMOD2=A: finish parsing `module name<params> { body }` after the name.
    fn finish_generic_module(
        &mut self,
        name: String,
        name_span: Span,
        is_pub: bool,
        is_package_pub: bool,
        start: Span,
    ) -> Result<Item, Diagnostic> {
        self.bump(); // consume `<`
        let mut params = Vec::new();
        while !matches!(self.peek().kind, TokKind::Gt | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            let (pname, pname_span) = self.expect_ident("for a generic module parameter name")?;
            if matches!(self.peek().kind, TokKind::Colon) {
                self.bump();
                let (annotation, _) = self.type_()?;
                params.push(GenericModuleParam::Annotated { name:pname, name_span:pname_span, annotation });
            } else {
                params.push(GenericModuleParam::Bare { name:pname, name_span:pname_span });
            }
        }
        self.expect(TokKind::Gt, "to close the generic module parameter list")?;
        self.expect(TokKind::LBrace, "to open the generic module body")?;
        let mut body = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            match self.top_level_item_in_code_module() {
                Ok(item) => body.push(item),
                Err(d) => {
                    self.diags.push(d);
                    self.sync_top();
                }
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close the generic module body")?;
        Ok(Item::GenericModule(GenericModuleDef {
            name,
            name_span,
            is_pub,
            is_package_pub,
            params,
            body,
            span: Span::new(start.start, end),
        }))
    }

    /// D-GENMOD2=A: finish parsing `module alias = target<args>` after the name.
    fn finish_module_alias(
        &mut self,
        name: String,
        name_span: Span,
        is_pub: bool,
        is_package_pub: bool,
        start: Span,
    ) -> Result<Item, Diagnostic> {
        self.bump(); // consume `=`
        let (target, target_span) = self.expect_ident("for the target module name")?;
        let mut args = Vec::new();
        if matches!(self.peek().kind, TokKind::Lt) {
            self.bump(); // consume `<`
            while !matches!(self.peek().kind, TokKind::Gt | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    continue;
                }
                let arg_start = self.peek().span;
                // Preserve unresolved syntax. Literal-led and enum-case
                // expressions are values; an identifier can still be
                // contextualized as either a type or an earlier constant by sema.
                match &self.peek().kind {
                    TokKind::Int(_, _) | TokKind::KwTrue | TokKind::KwFalse | TokKind::Char(_) | TokKind::Str(_)
                    | TokKind::LParen | TokKind::Minus | TokKind::Bang => {
                        let expr = self.module_arg_expr()?;
                        args.push(ModuleArg::Value(expr, arg_start));
                    }
                    TokKind::Ident(_)
                        if matches!(
                            self.peek2().kind,
                            TokKind::Dot
                                | TokKind::LParen
                                | TokKind::Plus
                                | TokKind::Minus
                                | TokKind::Star
                                | TokKind::Slash
                                | TokKind::Percent
                                | TokKind::Amp
                                | TokKind::Pipe
                                | TokKind::Caret
                                | TokKind::Shl
                                | TokKind::Shr
                                | TokKind::AndAnd
                                | TokKind::OrOr
                                | TokKind::EqEq
                                | TokKind::NotEq
                                | TokKind::Le
                                | TokKind::Ge
                        ) =>
                    {
                        let expr = self.module_arg_expr()?;
                        args.push(ModuleArg::Value(expr, arg_start));
                    }
                    _ => {
                        let (ty, _) = self.type_()?;
                        args.push(ModuleArg::Type(ty, arg_start));
                    }
                }
            }
            self.expect(TokKind::Gt, "to close the module alias argument list")?;
        }
        let end = self.peek().span.end;
        // Consume optional trailing `;`
        if matches!(self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        Ok(Item::ModuleAlias(ModuleAliasDef {
            name,
            name_span,
            is_pub,
            is_package_pub,
            target,
            target_span,
            args,
            span: Span::new(start.start, end),
        }))
    }

    /// Parse one top-level item inside an inline `module name { … }` body.
    fn top_level_item_in_code_module(&mut self) -> Result<Item, Diagnostic> {
        match &self.peek().kind {
            TokKind::KwFn => self.func().map(Item::Func),
            TokKind::Hash if self.marker_sequence_leads_to_function() => {
                self.func_with_marker_list().map(Item::Func)
            }
            TokKind::Hash if self.at_meta_attr() => {
                if matches!(self.meta_attr_next_kind(), Some(TokKind::KwConst)) {
                    self.retired_const_def().map(Item::Const)
                } else if matches!(self.meta_attr_next_kind(), Some(TokKind::KwComptime))
                    || self.at_comptime_marker_after_meta()
                {
                    self.comptime_def().map(Item::Const)
                } else if self.at_persist_after_meta() {
                    self.persist_def().map(Item::Const)
                } else {
                    self.func().map(Item::Func)
                }
            }
            // S60 (D-CASING1 follow-on) / D-MARKERMOVE2: `#Pure fn` inside a module
            // body (old `#Pure fn` spelling is E0062, taught inside `func()`).
            TokKind::Hash if self.at_pure_fn() => self.func().map(Item::Func),
            // D-TAINT1: `#Sanitizer fn` inside a module body.
            TokKind::Hash if self.at_sanitizer_fn() => self.func().map(Item::Func),
            // D-REPLAY1: `#Replayable fn` inside a module body.
            TokKind::Hash if self.at_replayable_fn() => self.func().map(Item::Func),
            // D-SCHEDULE1 (card #505): `#Job fn` / `#Every(…) fn` inside a module body.
            TokKind::Hash if self.at_task_fn() || self.at_every_fn() => self.func().map(Item::Func),
            // D-MUSTUSE1 / D-MARKERMOVE1: `#MustUse fn` inside a module body (old
            // `#MustUse fn` spelling is E0062, taught inside `func()`).
            TokKind::Hash if self.at_must_use_fn() => self.func().map(Item::Func),
            // D-STATE1: `#State(S) fn` / `#Transition(From, To) fn` in a module.
            TokKind::Hash if self.at_state_fn() || self.at_transition_fn() => {
                self.func().map(Item::Func)
            }
            // D-REACTCORE1: `#Reactive fn` inside a module body.
            TokKind::Hash if self.at_reactive_fn() => self.reactive_fn().map(Item::Func),
            // D-WASM1=A: `#Wasm` / `#JS` / `#WasmExport fn` inside a module body.
            TokKind::Hash if self.at_web_partition_fn() => self.func().map(Item::Func),
            // D-SHAPE2: `#[RenameAll(camel)]` / `#[Codable]` / `#Codable`
            // type rules inside a module body.
            TokKind::Hash if self.at_marker_list() || self.at_single_type_marker() =>
            {
                self.type_def_with_any_markers()
            }
            TokKind::KwPriv => match self.peek2().kind {
                TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
                TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
                TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
                TokKind::KwTag => self.tag_def(false).map(Item::Tag),
                _ => self.func().map(Item::Func),
            },
            TokKind::KwPub => match self.peek2().kind {
                TokKind::KwModule => self.code_module(true),
                TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
                TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
                TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
                TokKind::KwTag => self.tag_def(false).map(Item::Tag),
                TokKind::KwUse => {
                    let span = self.peek().span;
                    self.sync_stmt();
                    Err(Diagnostic::error(
                        "E0003",
                        "`pub use` inside an inline code module is not yet supported".to_string(),
                        "unqualified imports inside modules need D-MOD4 ratification".to_string(),
                        "call items by their qualified path for now".to_string(),
                        Some(span),
                    ))
                }
                _ => self.func().map(Item::Func),
            },
            TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
            TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
            TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
            TokKind::KwTag => self.tag_def(false).map(Item::Tag),
            TokKind::KwModule => self.code_module(false),
            // D-OSTARGET1=A: `#Target(OS.X) impl …` inside a module body.
            TokKind::Hash if self.at_web_target() => match self.parse_web_target_marker()? {
                super::Items::TargetMarker::OS(os) => self.os_gated_impl(os),
                super::Items::TargetMarker::DefaultWeb => {
                    let span = self.peek().span;
                    Err(Diagnostic::error(
                        "E0003",
                        "`#Target(Web)` isn't valid on a module".to_string(),
                        "`Web` is a file-level default-backend marker, not a partition ceiling"
                            .to_string(),
                        "move `#Target(Web)` to the top of the file, outside any module"
                            .to_string(),
                        Some(span),
                    ))
                }
                super::Items::TargetMarker::Bucket(_) => {
                    let span = self.peek().span;
                    Err(Diagnostic::error(
                        "E0003",
                        "`#Target(Wasm)`/`#Target(JS)` isn't valid on an item inside a module body".to_string(),
                        "the web bucket ceiling is file- or module-level (`#Target(Wasm) module name { … }`), not per-item".to_string(),
                        "move the marker to the `module` declaration itself".to_string(),
                        Some(span),
                    ))
                }
            },
            TokKind::KwImpl => self.impl_or_error_conv(),
            TokKind::KwConst => self.retired_const_def().map(Item::Const),
            TokKind::KwComptime => self.comptime_def().map(Item::Const),
            TokKind::Hash if self.at_test_def() => self.test_def().map(Item::Test),
            // D-BENCH1/D-BENCH-MARKER1=A: `#Bench("name") { … }`.
            TokKind::Hash if self.at_bench_def() => self.bench_def().map(Item::Bench),
            TokKind::Hash if self.at_persist_binding() => self.persist_def().map(Item::Const),
            TokKind::Hash if self.at_comptime_marker() => self.comptime_def().map(Item::Const),
            TokKind::KwUse => {
                let span = self.peek().span;
                self.sync_stmt();
                Err(Diagnostic::error(
                    "E0003",
                    "`use` inside an inline code module is not yet supported".to_string(),
                    "unqualified imports inside modules need D-MOD4 ratification".to_string(),
                    "call items by their qualified path for now".to_string(),
                    Some(span),
                ))
            }
            TokKind::At => {
                let span = self.bump().span;
                Err(Diagnostic::error(
                    "E0063",
                    "applied rules use `#`, not `@`".to_string(),
                    "`#` marks attributes, instructions, and properties; `@` marks locations, addresses, and sources (D-VERDICT-732-1)".to_string(),
                    "replace the leading `@` with `#`".to_string(),
                    Some(span),
                ))
            }
            other => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected a function, struct, enum, or other item inside a module, found {}",
                    describe(other)
                ),
                "an inline code module body may only contain top-level items".to_string(),
                "example: `pub fn double(n: Int) => Int { return n * 2; }`".to_string(),
                Some(self.peek().span),
            )),
        }
    }

    /// U8/D-JPK-REF1: a module's `sources: { name: target@provider, … }` block.
    /// Each ref is not a single token, so we record its source span and leave
    /// validation to modeval (`classify_provider_ref`).
    fn module_sources(&mut self) -> Result<Vec<crate::AST::SourceDecl>, Diagnostic> {
        self.bump(); // `sources`
        self.expect(TokKind::Colon, "after `sources`")?;
        self.expect(TokKind::LBrace, "to open the sources block")?;
        let mut out = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            let (name, name_span) = self.expect_ident("for a source name")?;
            self.expect(TokKind::Colon, "after a source name")?;
            // Consume the `target@provider` ref tokens up to the next `,`/`}`;
            // the recovered span slices back to the exact written text.
            let ref_start = self.peek().span;
            let mut ref_end = ref_start.end;
            if matches!(
                self.peek().kind,
                TokKind::Comma | TokKind::RBrace | TokKind::Eof
            ) {
                return Err(Diagnostic::error(
                    "E0003",
                    "a source needs a `target@provider` ref or bare path".to_string(),
                    "every named source resolves to an upstream, e.g. `default: owner/repo/rev@github`"
                        .to_string(),
                    "write the ref after the `:`".to_string(),
                    Some(ref_start),
                ));
            }
            while !matches!(
                self.peek().kind,
                TokKind::Comma | TokKind::RBrace | TokKind::Eof
            ) {
                ref_end = self.peek().span.end;
                self.bump();
            }
            out.push(crate::AST::SourceDecl {
                name,
                name_span,
                ref_span: Span::new(ref_start.start, ref_end),
                span: Span::new(name_span.start, ref_end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBrace, "to close the sources block")?;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(out)
    }

    /// U8: a module's `imports: find("./modules")` directive. The value is an
    /// ordinary call expression; the `find` walk itself lands with U4 discovery.
    fn module_import(&mut self) -> Result<Expr, Diagnostic> {
        self.bump(); // `imports`
        self.expect(TokKind::Colon, "after `imports`")?;
        let value = self.expr()?;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(value)
    }

    /// D-WORKSPACE1=B: `members: <expr>` in `module workspace { … }`. The value
    /// is any expression — most commonly `find("./packages")` or a list literal.
    fn module_members(&mut self) -> Result<Expr, Diagnostic> {
        self.bump(); // `members`
        self.expect(TokKind::Colon, "after `members`")?;
        let value = self.expr()?;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(value)
    }

    /// U3 (unified-ecosystem §5): one typed namespace contribution,
    /// `namespace.path: Value`, e.g. `env.dev: Env { … }`. The value reuses the
    /// ordinary expression parser (struct literals, lists, strings).
    fn contribution(&mut self) -> Result<Contribution, Diagnostic> {
        let (ns_name, ns_span) =
            self.expect_ident("for a namespace (`env`, `system`, or `image`)")?;
        let namespace = match ns_name.as_str() {
            Syntax::NS_ENV => Namespace::Env,
            Syntax::NS_SYSTEM => Namespace::System,
            Syntax::NS_IMAGE => Namespace::Image,
            Syntax::NS_FLEET => Namespace::Fleet,
            Syntax::NS_VMTEST => Namespace::VmTest,
            Syntax::NS_PERF => Namespace::Perf,
            _ => {
                return Err(Diagnostic::error(
                    "E0960",
                    format!("`{}` is not a module namespace", ns_name),
                    format!(
                        "a module contributes to the reserved namespaces `{}` (a dev environment), `{}` (a whole machine), `{}` (a disk image), `{}` (a host fleet), and `{}` (a VM test)",
                        Syntax::NS_ENV, Syntax::NS_SYSTEM, Syntax::NS_IMAGE, Syntax::NS_FLEET, Syntax::NS_VMTEST
                    ),
                    format!(
                        "begin the contribution with `{}`, `{}`, `{}`, `{}`, or `{}`",
                        Syntax::NS_ENV, Syntax::NS_SYSTEM, Syntax::NS_IMAGE, Syntax::NS_FLEET, Syntax::NS_VMTEST
                    ),
                    Some(ns_span),
                ));
            }
        };
        self.expect(TokKind::Dot, "after the namespace name")?;
        // S84: contribution names (`system.<name>`, `image.<name>`, `env.<name>`)
        // may be kebab-case, e.g. `image.halcyon-iso`.
        let (path, path_span) = self.expect_dashed_name("for the contribution name")?;
        self.expect(TokKind::Colon, "after the contribution name")?;
        // U11/U14/U18: `system.<name>:` and `image.<name>:` parse into dedicated
        // typed literals (the U13 `options` list, the typed `target` value, the
        // U12 `Service` map, and U18 bare `{ … }` don't fit the ordinary
        // expression grammar). `env.<name>:` keeps the ordinary expression parser.
        let value = match namespace {
            Namespace::Env => crate::AST::ContribValue::Expr(self.expr()?),
            Namespace::System => crate::AST::ContribValue::System(self.system_lit()?),
            Namespace::Image => crate::AST::ContribValue::Image(self.image_lit()?),
            Namespace::Fleet => crate::AST::ContribValue::Fleet(self.fleet_lit()?),
            Namespace::VmTest => crate::AST::ContribValue::VmTest(self.vmtest_lit()?),
            Namespace::Perf => {
                return Err(Diagnostic::error(
                    "E2903",
                    format!("performance budget role `{path}` is not valid"),
                    "performance budgets use the dedicated `module perf.<role>` declaration"
                        .to_string(),
                    format!("write `module {}.{path} {{ budgets: [...] }}`", Syntax::NS_PERF),
                    Some(ns_span),
                ));
            }
        };
        let end = value.span().end;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(Contribution {
            namespace,
            path,
            path_span,
            value,
            span: Span::new(ns_span.start, end),
        })
    }

    /// U11/U18: parse a `System { … }` or bare `{ … }` record. The type name
    /// `System` is optional (U18 inferred constructor); when present it is
    /// recorded so modeval can keep allowing the explicit form (S29).
    fn system_lit(&mut self) -> Result<crate::AST::SystemLit, Diagnostic> {
        let start = self.peek().span.start;
        let explicit_type = self.opt_record_type(Syntax::TYPE_SYSTEM)?;
        self.expect(TokKind::LBrace, "to open a `System` record")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            fields.push(self.system_field()?);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close a `System` record")?;
        Ok(crate::AST::SystemLit {
            explicit_type,
            fields,
            span: Span::new(start, end),
        })
    }

    /// U18: an optional record type name before `{`. Returns its span when the
    /// author wrote it (`System.{ … }` / `Image.{ … }` / `Service.{ … }`), `None`
    /// for a bare `{ … }`. D-DOTCTOR1: also accepts the old dotless `System { … }`
    /// form (E0320 recovery — `jet fmt` auto-fixes).
    fn opt_record_type(&mut self, expected: &str) -> Result<Option<Span>, Diagnostic> {
        // D-DOTCTOR1: new form `TypeName.{` — consume Ident and Dot.
        if self.peek_is_ident(expected)
            && matches!(self.peek2().kind, TokKind::Dot)
            && matches!(self.peek3().kind, TokKind::LBrace)
        {
            let span = self.bump().span; // consume type name
            self.bump(); // consume dot
            return Ok(Some(span));
        }
        // D-DOTCTOR2 recovery: old dotless `TypeName {` — emit E0320, consume Ident.
        if self.peek_is_ident(expected) && matches!(self.peek2().kind, TokKind::LBrace) {
            let span = self.bump().span;
            let brace_span = self.peek().span;
            self.diags.push(Diagnostic::error(
                "E0320",
                format!(
                    "struct construction uses `{}.{{…}}`, not `{} {{…}}`",
                    expected, expected
                ),
                "named construction has a dot before the brace (D-DOTCTOR1)".to_string(),
                format!("write `{}.{{…}}` instead", expected),
                Some(brace_span),
            ));
            return Ok(Some(span));
        }
        Ok(None)
    }

    /// U11/U12/U13: one field inside a `System { … }` record.
    fn system_field(&mut self) -> Result<crate::AST::SystemField, Diagnostic> {
        let (name, name_span) = self.expect_ident("for a `System` field name")?;
        self.expect(TokKind::Colon, "after a `System` field name")?;
        let value = match name.as_str() {
            Syntax::SYSTEM_FIELD_TARGET => {
                let (os, arch, span) = self.platform_value()?;
                crate::AST::SystemFieldValue::Platform { os, arch, span }
            }
            Syntax::SYSTEM_FIELD_PACKAGES => crate::AST::SystemFieldValue::Packages(self.expr()?),
            Syntax::SYSTEM_FIELD_SERVICES => {
                crate::AST::SystemFieldValue::Services(self.services_map()?)
            }
            Syntax::SYSTEM_FIELD_OPTIONS => {
                crate::AST::SystemFieldValue::Options(self.options_list()?)
            }
            _ => crate::AST::SystemFieldValue::Other(self.expr()?),
        };
        let end = value_end_system(&value);
        Ok(crate::AST::SystemField {
            name,
            name_span,
            value,
            span: Span::new(name_span.start, end),
        })
    }

    /// U13: a dotted typed platform value — `linux.x64`. Two name segments joined
    /// by `.`; modeval checks they name a known platform.
    fn platform_value(&mut self) -> Result<(String, String, Span), Diagnostic> {
        let (os, os_span) = self.expect_ident("for a platform, e.g. `linux`")?;
        self.expect(TokKind::Dot, "between the platform and its architecture")?;
        let (arch, arch_span) = self.expect_ident("for an architecture, e.g. `x64`")?;
        Ok((os, arch, Span::new(os_span.start, arch_span.end)))
    }

    /// U12/U18: a `services: { name: { … }, … }` map — each entry is a service
    /// name and an inferred (or explicit) `Service` record.
    fn services_map(&mut self) -> Result<Vec<crate::AST::ServiceEntry>, Diagnostic> {
        self.expect(TokKind::LBrace, "to open the `services` map")?;
        let mut out = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            let (name, name_span) = self.expect_ident("for a service name")?;
            self.expect(TokKind::Colon, "after a service name")?;
            let explicit_type = self.opt_record_type(Syntax::TYPE_SERVICE)?;
            self.expect(TokKind::LBrace, "to open a `Service` record")?;
            let mut fields = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                let (field, field_span) = self.expect_ident("for a `Service` field name")?;
                self.expect(TokKind::Colon, "after a `Service` field name")?;
                let value = self.expr()?;
                fields.push((field, field_span, value));
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                }
            }
            let rec_end = self.peek().span.end;
            self.expect(TokKind::RBrace, "to close a `Service` record")?;
            out.push(crate::AST::ServiceEntry {
                name,
                name_span,
                explicit_type,
                fields,
                span: Span::new(name_span.start, rec_end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBrace, "to close the `services` map")?;
        Ok(out)
    }

    /// U13: an `options: [ dotted.key: value, … ]` ordered list. Each entry is a
    /// dotted key path and a value expression (bare identifier, dotted typed
    /// value, list, or quoted free-form string).
    fn options_list(&mut self) -> Result<Vec<crate::AST::OptionEntry>, Diagnostic> {
        self.expect(TokKind::LBracket, "to open the `options` list")?;
        let mut out = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBracket | TokKind::Eof) {
            let (mut key, key_start) =
                self.expect_ident("for an option key, e.g. `network.hostName`")?;
            let mut key_end = key_start.end;
            while matches!(self.peek().kind, TokKind::Dot) {
                self.bump();
                let (seg, seg_span) = self.expect_ident("for the next part of the option key")?;
                key.push('.');
                key.push_str(&seg);
                key_end = seg_span.end;
            }
            let key_span = Span::new(key_start.start, key_end);
            self.expect(TokKind::Colon, "after an option key")?;
            let value_start = self.peek().span.start;
            let value = self.expr()?;
            // Record the value's full written span from the first token of the
            // value to the last token consumed (the token before the cursor) —
            // robust for dotted typed values like `default.fish` whose `Expr`
            // span covers only the final member.
            let value_end = self.prev_end();
            out.push(crate::AST::OptionEntry {
                key,
                key_span,
                value,
                value_span: Span::new(value_start, value_end),
                span: Span::new(key_start.start, value_end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBracket, "to close the `options` list")?;
        Ok(out)
    }

    /// U14/U18: parse an `Image { … }` or bare `{ … }` record.
    fn image_lit(&mut self) -> Result<crate::AST::ImageLit, Diagnostic> {
        let start = self.peek().span.start;
        let explicit_type = self.opt_record_type(Syntax::TYPE_IMAGE)?;
        self.expect(TokKind::LBrace, "to open an `Image` record")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            fields.push(self.image_field()?);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close an `Image` record")?;
        Ok(crate::AST::ImageLit {
            explicit_type,
            fields,
            span: Span::new(start, end),
        })
    }

    /// U14: one field inside an `Image { … }` record.
    fn image_field(&mut self) -> Result<crate::AST::ImageField, Diagnostic> {
        let (name, name_span) = self.expect_ident("for an `Image` field name")?;
        self.expect(TokKind::Colon, "after an `Image` field name")?;
        let value = match name.as_str() {
            Syntax::IMAGE_FIELD_FROM => {
                // D-JPK-IMAGE1: `from: system.<name>` (the `.Iso` tier) or
                // `from: packages.<name>` (the `.Oci` tier) — the `system`/
                // `packages` keyword then the name.
                let (kw, kw_span) = self.expect_ident(
                    "for `system` or `packages`, e.g. `system.halcyon` or `packages.cli`",
                )?;
                if kw != Syntax::NS_SYSTEM && kw != Syntax::IMAGE_FROM_PACKAGES {
                    return Err(image_from_not_system(kw_span));
                }
                self.expect(TokKind::Dot, "after `system`/`packages`")?;
                // S84: `from: system.<name>` may reference a kebab-case System
                // (or package) name; must read the same way the definition does
                // so the E0978/E1267 cross-checks still string-match.
                let (name, name_span) = self.expect_dashed_name("for the name")?;
                let source = if kw == Syntax::NS_SYSTEM {
                    crate::AST::ImageFromRef::System(name)
                } else {
                    crate::AST::ImageFromRef::Package(name)
                };
                crate::AST::ImageFieldValue::From {
                    source,
                    span: Span::new(kw_span.start, name_span.end),
                }
            }
            Syntax::IMAGE_FIELD_FORMAT => {
                let (word, span) = self.expect_ident("for a format, e.g. `iso`")?;
                crate::AST::ImageFieldValue::Format { word, span }
            }
            Syntax::SYSTEM_FIELD_TARGET => {
                let (os, arch, span) = self.platform_value()?;
                crate::AST::ImageFieldValue::Platform { os, arch, span }
            }
            _ => crate::AST::ImageFieldValue::Other(self.expr()?),
        };
        let end = value_end_image(&value);
        Ok(crate::AST::ImageField {
            name,
            name_span,
            value,
            span: Span::new(name_span.start, end),
        })
    }

    /// U15: parse a `Fleet { … }` or bare `{ … }` record.
    fn fleet_lit(&mut self) -> Result<crate::AST::FleetLit, Diagnostic> {
        let start = self.peek().span.start;
        let explicit_type = self.opt_record_type(Syntax::TYPE_FLEET)?;
        self.expect(TokKind::LBrace, "to open a `Fleet` record")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            fields.push(self.fleet_field()?);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close a `Fleet` record")?;
        Ok(crate::AST::FleetLit {
            explicit_type,
            fields,
            span: Span::new(start, end),
        })
    }

    /// U15: one field inside a `Fleet { … }` record. The known field is `hosts:`;
    /// anything else is captured as an expression so modeval reports it.
    fn fleet_field(&mut self) -> Result<crate::AST::FleetField, Diagnostic> {
        let (name, name_span) = self.expect_ident("for a `Fleet` field name")?;
        self.expect(TokKind::Colon, "after a `Fleet` field name")?;
        let value = if name == Syntax::FLEET_FIELD_HOSTS {
            crate::AST::FleetFieldValue::Hosts(self.fleet_hosts()?)
        } else {
            crate::AST::FleetFieldValue::Other(self.expr()?)
        };
        let end = match &value {
            crate::AST::FleetFieldValue::Hosts(hs) => {
                hs.last().map(|h| h.span.end).unwrap_or(name_span.end)
            }
            crate::AST::FleetFieldValue::Other(e) => e.span().end,
        };
        Ok(crate::AST::FleetField {
            name,
            name_span,
            value,
            span: Span::new(name_span.start, end),
        })
    }

    /// U15: `hosts: { <host>: system.<name>[.{ overrides }], … }`.
    fn fleet_hosts(&mut self) -> Result<Vec<crate::AST::HostEntry>, Diagnostic> {
        self.expect(TokKind::LBrace, "to open the `hosts` map")?;
        let mut out = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            // S84: host names may be kebab-case.
            let (host, host_span) = self.expect_dashed_name("for a host name")?;
            self.expect(TokKind::Colon, "after a host name")?;
            // `system.<name>` — the `system` keyword then the referenced name.
            let (kw, kw_span) = self.expect_ident("for `system`, e.g. `system.web`")?;
            if kw != Syntax::NS_SYSTEM {
                return Err(fleet_host_not_system(kw_span));
            }
            self.expect(TokKind::Dot, "after `system`")?;
            let (sys, sys_span) = self.expect_dashed_name("for the system name")?;
            // Optional `.{ overrides }` copy-with-update tail — captured as a span.
            let mut overrides = None;
            let mut end = sys_span.end;
            if matches!(self.peek().kind, TokKind::Dot)
                && matches!(self.peek2().kind, TokKind::LBrace)
            {
                self.bump(); // consume `.`
                let ov = self.skip_balanced_brace_span()?;
                end = ov.end;
                overrides = Some(ov);
            }
            out.push(crate::AST::HostEntry {
                name: host,
                name_span: host_span,
                system: sys,
                system_span: Span::new(kw_span.start, sys_span.end),
                overrides,
                span: Span::new(host_span.start, end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBrace, "to close the `hosts` map")?;
        Ok(out)
    }

    /// D-JOS-VMTEST1: parse a `VmTest { hosts, run }` or bare `{ … }` record.
    fn vmtest_lit(&mut self) -> Result<crate::AST::VmTestLit, Diagnostic> {
        let start = self.peek().span.start;
        let explicit_type = self.opt_record_type(Syntax::TYPE_VMTEST)?;
        self.expect(TokKind::LBrace, "to open a `VmTest` record")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            fields.push(self.vmtest_field()?);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close a `VmTest` record")?;
        Ok(crate::AST::VmTestLit {
            explicit_type,
            fields,
            span: Span::new(start, end),
        })
    }

    fn vmtest_field(&mut self) -> Result<crate::AST::VmTestField, Diagnostic> {
        let (name, name_span) = self.expect_ident("for a `VmTest` field name")?;
        self.expect(TokKind::Colon, "after a `VmTest` field name")?;
        let value = if name == Syntax::VMTEST_FIELD_HOSTS {
            crate::AST::VmTestFieldValue::Hosts(self.fleet_hosts()?)
        } else if name == Syntax::VMTEST_FIELD_RUN {
            crate::AST::VmTestFieldValue::Run {
                span: self.test_block_span()?,
            }
        } else {
            crate::AST::VmTestFieldValue::Other(self.expr()?)
        };
        let end = match &value {
            crate::AST::VmTestFieldValue::Hosts(hs) => {
                hs.last().map(|h| h.span.end).unwrap_or(name_span.end)
            }
            crate::AST::VmTestFieldValue::Run { span } => span.end,
            crate::AST::VmTestFieldValue::Other(e) => e.span().end,
        };
        Ok(crate::AST::VmTestField {
            name,
            name_span,
            value,
            span: Span::new(name_span.start, end),
        })
    }

    fn test_block_span(&mut self) -> Result<Span, Diagnostic> {
        let (word, word_span) = self.expect_ident("for `test` before a VM test body")?;
        if word != "test" {
            return Err(Diagnostic::error(
                "E0960",
                "vmtest run body starts with `test`".to_string(),
                "D-JOS-VMTEST1=A: the run field is a checked VM test body.".to_string(),
                "write `run: test { host.wait_for_boot() }`.".to_string(),
                Some(word_span),
            ));
        }
        let block = self.skip_balanced_brace_span()?;
        Ok(Span::new(word_span.start, block.end))
    }

    /// Consume a balanced `{ … }` block (the parser is positioned on the opening
    /// `{`) and return its full source span. Used to capture a host's `.{ … }`
    /// override text without semantically parsing it (U15 capture-only).
    fn skip_balanced_brace_span(&mut self) -> Result<Span, Diagnostic> {
        let open_span = self.peek().span;
        let start = open_span.start;
        self.expect(TokKind::LBrace, "to open the override record")?;
        let mut end = open_span.end;
        let mut depth = 1u32;
        while depth > 0 {
            match &self.peek().kind {
                TokKind::Eof => {
                    return Err(fleet_unterminated_override(Span::new(start, end)));
                }
                TokKind::LBrace => {
                    depth += 1;
                    end = self.bump().span.end;
                }
                TokKind::RBrace => {
                    depth -= 1;
                    end = self.bump().span.end;
                }
                _ => {
                    end = self.bump().span.end;
                }
            }
        }
        Ok(Span::new(start, end))
    }
}
