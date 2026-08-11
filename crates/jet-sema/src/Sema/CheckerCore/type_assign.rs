use crate::AST::Type;
use crate::Collections::is_map_key_type;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{e0905, e0909, generic_depth_exceeded, substitute_type, COMPARABLE};
use crate::Sema::CheckerCoreLib::{
    core_type_known, data_renamed_to_datatree, layout_handle_renamed_to_layout,
    phantom_fact_menu_diag, retired_acronym_spelling_diag,
};
use crate::Sema::Bundle::fn_types_compatible;
use crate::Sema::Checker;
use crate::Sema::Diagnostics::{
    edit_distance, option_used_where_plain_expected, result_used_where_plain_expected,
    soft_public_use, type_fix_hint, undeclared_value_tag,
};
use crate::Syntax;
use super::helpers::no_any_type;

fn union_member_has_open_shape(ty: &Type) -> bool {
    match ty {
        Type::TraitObject(_) | Type::Fn { .. } => true,
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::Tagged { inner, .. }
        | Type::FixedList { elem: inner, .. } => union_member_has_open_shape(inner),
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            union_member_has_open_shape(key) || union_member_has_open_shape(value)
        }
        Type::Apply { args, .. } | Type::Union(args) => {
            args.iter().any(union_member_has_open_shape)
        }
        Type::Tuple(fields) => fields.iter().any(|(_, field)| union_member_has_open_shape(field)),
        _ => false,
    }
}

fn is_core_view_generic(ty: &Type) -> bool {
    // D-PIN1=A / D-PIN3=A: `Pin<T>` joins the borrowed-window family. It is a
    // core generic like `View`/`ViewMut` — declarable on fields, parameters,
    // and returns, and constructed only by `mem.pin`.
    matches!(
        ty,
        Type::Apply { name, .. }
            if matches!(name.as_str(), "View" | "ViewMut" | Syntax::TYPE_PIN)
    )
}

impl<'a> Checker<'a> {
        pub(crate) fn check_declared_type(&mut self, ty: &Type, span: Span) {
            self.warn_soft_public_declared_type(ty, span);
            self.check_declared_type_rules(ty, span);
            if self.cell_guard_storage_is_unsupported(ty) {
                self.report_cell_guard_storage(
                    format!("a Cell guard cannot be stored in `{}`", ty.show()),
                    span,
                );
            }
        }

        pub(crate) fn warn_soft_public_declared_type(&mut self, ty: &Type, span: Span) {
            self.warn_soft_public_type_tree(ty, span);
        }

        fn warn_soft_public_type_tree(&mut self, ty: &Type, span: Span) {
            match ty {
                Type::Named(name) => self.warn_soft_public_type_name(name, span),
                Type::Apply { name, args } => {
                    self.warn_soft_public_type_name(name, span);
                    for arg in args {
                        self.warn_soft_public_type_tree(arg, span);
                    }
                }
                Type::TraitObject(names) => {
                    for name in names {
                        self.warn_soft_public_type_name(name, span);
                    }
                }
                Type::Option(inner) | Type::List(inner) | Type::Shared(inner) => {
                    self.warn_soft_public_type_tree(inner, span);
                }
                Type::Map { key, value, .. } => {
                    self.warn_soft_public_type_tree(key, span);
                    self.warn_soft_public_type_tree(value, span);
                }
                Type::Result { ok, err } => {
                    self.warn_soft_public_type_tree(ok, span);
                    self.warn_soft_public_type_tree(err, span);
                }
                Type::Union(members) => {
                    for m in members {
                        self.warn_soft_public_type_tree(m, span);
                    }
                }
                Type::Tuple(fields) => {
                    for (_, field) in fields {
                        self.warn_soft_public_type_tree(field, span);
                    }
                }
                Type::FixedList { elem, .. } | Type::Tagged { inner: elem, .. } => {
                    self.warn_soft_public_type_tree(elem, span);
                }
                Type::Fn { params, ret, .. } => {
                    for param in params {
                        self.warn_soft_public_type_tree(param, span);
                    }
                    if let Some(ret) = ret {
                        self.warn_soft_public_type_tree(ret, span);
                    }
                }
                _ => {}
            }
        }

        fn warn_soft_public_type_name(&mut self, name: &str, span: Span) {
            let (owner, public_name) = if let Some((alias, leaf)) = name.rsplit_once('.') {
                (self.imports.get(alias).copied(), leaf)
            } else {
                let locally_owned = self.registry.contains(name) || {
                    let declared = self.name_ledger.declaration(self.module_idx, name).is_some();
                    self.modules
                        .is_some_and(|modules| declared && modules[self.module_idx].trait_reg.is_trait_name(name))
                };
                let owner = if locally_owned {
                    Some(self.module_idx)
                } else {
                    self.modules.and_then(|modules| {
                        self.imports.values().copied().find(|&idx| {
                            modules[idx].registry.contains(name)
                                || modules[idx].trait_reg.is_trait_name(name)
                        })
                    })
                };
                (owner, name)
            };
            if Syntax::classify_identifier(public_name) != Syntax::IdentifierClass::SoftPublic {
                return;
            }
            let Some(owner) = owner else { return };
            if owner == self.module_idx || !self.type_is_pub_in(owner, public_name) {
                return;
            }
            self.diags.push(soft_public_use(public_name, span));
        }

        pub(in crate::Sema) fn check_declared_type_rules(&mut self, ty: &Type, span: Span) {
            if let Some(chain) = generic_depth_exceeded(ty) {
                self.diags.push(e0909(&chain, span));
            }
            match ty {
                Type::Named(n) => {
                    if n == "Any" {
                        self.diags.push(no_any_type(span));
                        return;
                    }
                    // D-SERDE13=B: the retired `Data` spelling points at `DataTree`.
                    if n == "Data" {
                        self.diags.push(data_renamed_to_datatree(span));
                        return;
                    }
                    // D-LAYOUT-CTOR1: the retired `LayoutHandle` spelling points at `Layout`.
                    if n == Syntax::LAYOUT_HANDLE_TYPE_RETIRED {
                        self.diags.push(layout_handle_renamed_to_layout(span));
                        return;
                    }
                    // D-ACRO-CASE1=A / D-ACRO-LEX1=A: retired word-cased acronym spellings.
                    if let Some(canonical) = crate::Syntax::retired_acronym_spelling(n) {
                        self.diags.push(retired_acronym_spelling_diag(n, &canonical, span));
                        return;
                    }
                    if core_type_known(n) {
                        return;
                    }
                    if self.type_param_scope.iter().any(|p| p.name == *n) {
                        return;
                    }
                    if self.trait_reg.is_trait_name(n) {
                        return;
                    }
                    if self.registry.is_type_alias(n) {
                        self.diags.push(Diagnostic::error(
                            "E0119",
                            format!("`{}` is a type alias and needs type arguments", n),
                            format!(
                                "write `{}`<{}>",
                                n,
                                self.registry
                                    .type_alias(n)
                                    .map(|(params, _)| {
                                        params
                                            .iter()
                                            .map(|p| p.name.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    })
                                    .unwrap_or_default()
                            ),
                            format!("instantiate the alias, like `{}`<T>", n),
                            Some(span),
                        ));
                        return;
                    }
                    if self.registry.contains(n) {
                        return;
                    }
                    // Canonical imported nominals are already resolved. Keep the
                    // fully-qualified identity through validation; only the leaf
                    // is projected for the owning module's registry lookup.
                    if let Some((namespace, leaf)) = n.rsplit_once("::") {
                        if let Some(owner) = self.struct_owner_module(leaf, Some(namespace)) {
                            if owner == self.module_idx || self.type_is_pub_in(owner, leaf) {
                                return;
                            }
                        }
                    }
                    if let Some((module, leaf)) = n.split_once('.') {
                        if let (Some(modules), Some(&index)) =
                            (self.modules, self.imports.get(module))
                        {
                            if modules[index].registry.contains(leaf)
                                && self.type_is_pub_in(index, leaf)
                            {
                                return;
                            }
                        }
                    }
                    // Check imported file-module registries for pub types.
                    if let Some(mods) = self.modules {
                        let found = self
                            .imports
                            .values()
                            .copied()
                            .filter(|&idx| {
                                mods[idx].registry.contains(n) && self.type_is_pub_in(idx, n)
                            })
                            .collect::<std::collections::HashSet<_>>();
                        // A bare imported leaf is a source lookup convenience only when it
                        // has one visible owner. Keeping it unresolved when two modules export
                        // the same leaf would create a second, collision-prone nominal identity.
                        if found.len() == 1 && self.struct_owner_module(n, None).is_some() {
                            return;
                        }
                    }
                    // D-FACT-HOME1=A: a phantom fact-menu name (`Capability`,
                    // `InlineMode`, ...) is refused with a fix naming the real
                    // path, not the generic "no type called" message.
                    if let Some(diag) = phantom_fact_menu_diag(n, span) {
                        self.diags.push(diag);
                        return;
                    }
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!("there's no type called `{}`", n),
                        format!(
                            "the types are `{}`, `{}`, `{}`, and `{}` (plus types you define)",
                            Syntax::TYPE_INT,
                            Syntax::TYPE_FLOAT,
                            Syntax::TYPE_BOOL,
                            Syntax::TYPE_STRING
                        ),
                        "check the spelling, or define the struct or enum first".to_string(),
                        Some(span),
                    ));
                }
                Type::Apply { name, args } => {
                    let (lookup_name, canonical_owner) = name
                        .rsplit_once("::")
                        .map_or((name.as_str(), None), |(_namespace, leaf)| {
                            (leaf, self.name_ledger.nominal_module(name))
                        });
                    if lookup_name == "Any" {
                        self.diags.push(no_any_type(span));
                        for arg in args {
                            self.check_declared_type_rules(arg, span);
                        }
                        return;
                    }
                    let local_alias = canonical_owner.is_none()
                        && !name.contains("::")
                        && !name.contains('.');
                    if local_alias {
                        if let Some((params, target)) = self.registry.type_alias(lookup_name) {
                        if params.len() != args.len() {
                            self.diags.push(Diagnostic::error(
                                "E0119",
                                format!(
                                    "`{}` expects {} type argument{}, got {}",
                                    name,
                                    params.len(),
                                    if params.len() == 1 { "" } else { "s" },
                                    args.len()
                                ),
                                "every generic parameter needs a matching type argument".to_string(),
                                format!(
                                    "write `{}`<{}>",
                                    name,
                                    params
                                        .iter()
                                        .map(|p| p.name.as_str())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                                Some(span),
                            ));
                        }
                        for arg in args {
                            self.check_declared_type_rules(arg, span);
                        }
                        let subst: std::collections::HashMap<String, Type> = params
                            .iter()
                            .zip(args.iter())
                            .map(|(p, a)| (p.name.clone(), a.clone()))
                            .collect();
                        self.check_declared_type_rules(&substitute_type(target, &subst), span);
                        return;
                        }
                    }
                    let unqualified_core = canonical_owner.is_none()
                        && !name.contains("::")
                        && !name.contains('.');
                    let is_core_generic = unqualified_core
                        && matches!(
                            lookup_name,
                            "Task" | "Channel" | "Sender" | "Ptr" | "Tensor" | "Vec" | "Matrix"
                            // D-COLLBREADTH1=A: Set<T> and Deque<T>.
                            | "Set" | "Bag" | "Deque"
                            // D-ITERTOOLS1=A: expanded generic collection handles.
                            | "SortedSet" | "PriorityQueue" | "Cache"
                            | "BigInt" | "Decimal"
                            // D-REACT1=B: reactive handle types.
                            | "Signal" | "Derived" | "Computed"
                            // D-EVENT1=D: first-party typed event/hook handles.
                            | "Event" | "Hook" | "DecisionHook" | "HookDecision" | "HookOutcome"
                            | "DispatchReport"
                            // D-STREAMYIELD1: generator return type.
                            | "Stream"
                            // D-MIGRATE3=A: `decode_traced<T>`'s return-shape wrapper.
                            | "DecodeResult"
                            // D-DATAFRAME1=A: reserved core.data generic value types.
                            | "Table" | "Series" | "LazyFrame" | "DataJoin"
                            // D-MEM1 S6 (D-POOLID-API1=A): generational-arena handle pair.
                            | "Pool" | "Id"
                            // D-LOCALCELL1=A: one-thread cell and projected guard types.
                            | "Cell" | "CellReadGuard" | "CellEditGuard"
                            // D-TTLVAL1=A / D-TTL-ZEROIZE1=A: one closed
                            // secret-lifetime wrapper.
                            | "ExpiringSecret" | Syntax::TYPE_SHARED_GUARD
                            | Syntax::TYPE_SHARED_WEAK
                            | "KeyRef" | "MutationPlan" | "VaultWrite" | "Rotation" | "WrappedImportPlan"
                        )
                        || (unqualified_core && is_core_view_generic(ty));
                    if is_core_generic && matches!(lookup_name, "Vec" | "Matrix") {
                        let expected = if lookup_name == "Vec" { 1 } else { 2 };
                        if args.len() != expected
                            || args
                                .iter()
                                .any(|arg| arg.compute_dimension_value().is_none())
                        {
                            self.diags.push(Diagnostic::error(
                                "E0119",
                                format!(
                                    "`{name}` needs {expected} literal shape dimension{}",
                                    if expected == 1 { "" } else { "s" }
                                ),
                                "compute aliases carry fixed dimensions that sema checks before codegen".to_string(),
                                if lookup_name == "Vec" {
                                    "write `Vec<N>` with one non-negative integer".to_string()
                                } else {
                                    "write `Matrix<M, N>` with two non-negative integers".to_string()
                                },
                                Some(span),
                            ));
                        }
                        return;
                    }
                    if is_core_generic
                        && lookup_name == "ExpiringSecret"
                        && (args.len() != 1
                            || !args.first().is_some_and(
                                crate::Sema::Diagnostics::is_expiring_secret_member_type,
                            ))
                    {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            "`ExpiringSecret<T>` requires a secret type".to_string(),
                            "only Secret, SigningKey, and X25519SecretKey have the audited move-only zeroizing contract required by this wrapper".to_string(),
                            "use `ExpiringSecret<crypto.Secret>`, `ExpiringSecret<crypto.SigningKey>`, or `ExpiringSecret<crypto.X25519SecretKey>`".to_string(),
                            Some(span),
                        ));
                    }
                    let explicit_import_owner = name
                        .rsplit_once('.')
                        .and_then(|(namespace, leaf)| {
                            self.struct_owner_module(leaf, Some(namespace))
                        });
                    let mut imported_owners = std::collections::HashSet::new();
                    if let Some(owner) = canonical_owner {
                        if owner != self.module_idx
                            && self
                                .modules
                                .is_some_and(|modules| modules[owner].registry.contains(lookup_name))
                            && self.type_is_pub_in(owner, lookup_name)
                        {
                            imported_owners.insert(owner);
                        }
                    } else if let Some(owner) = explicit_import_owner {
                        if owner != self.module_idx && self.type_is_pub_in(owner, lookup_name) {
                            imported_owners.insert(owner);
                        }
                    } else if let Some(modules) = self.modules {
                        for &idx in self.imports.values() {
                            if modules[idx].registry.contains(lookup_name)
                                && self.type_is_pub_in(idx, lookup_name)
                            {
                                imported_owners.insert(idx);
                            }
                        }
                    }
                    let imported_owner = (imported_owners.len() == 1)
                        .then(|| *imported_owners.iter().next().unwrap());
                    let local_type = self.registry.contains(lookup_name)
                        && match canonical_owner {
                            Some(owner) => owner == self.module_idx,
                            None => !name.contains("::") && !name.contains('.'),
                        };
                    if !is_core_generic
                        && !local_type
                        && imported_owner.is_none()
                    {
                        self.diags.push(Diagnostic::error(
                            "E0119",
                        format!("there's no type called `{}`", name),
                            "generic types must name a struct or enum you defined".to_string(),
                            "check the spelling, or define the type first".to_string(),
                            Some(span),
                        ));
                    }
                    if !is_core_generic {
                        let expected_owner = canonical_owner
                            .or(imported_owner)
                            .or(local_type.then_some(self.module_idx));
                        let expected = expected_owner.and_then(|idx| {
                            if idx == self.module_idx {
                                self.trait_reg
                                    .struct_params
                                    .get(lookup_name)
                                    .or_else(|| self.trait_reg.enum_params.get(lookup_name))
                                    .cloned()
                            } else {
                                self.modules.and_then(|modules| {
                                    modules[idx]
                                        .trait_reg
                                        .struct_params
                                        .get(lookup_name)
                                        .or_else(|| modules[idx].trait_reg.enum_params.get(lookup_name))
                                        .cloned()
                                })
                            }
                        });
                        if let Some(params) = expected {
                            if params.len() != args.len() {
                                self.diags.push(Diagnostic::error(
                                    "E0119",
                                    format!(
                                        "`{}` expects {} type argument{}, got {}",
                                        name,
                                        params.len(),
                                        if params.len() == 1 { "" } else { "s" },
                                        args.len()
                                    ),
                                    "every generic parameter needs a matching type argument"
                                        .to_string(),
                                    format!(
                                        "write `{}`<{}>",
                                        name,
                                        params
                                            .iter()
                                            .map(|p| p.name.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    ),
                                    Some(span),
                                ));
                            }
                        } else if !args.is_empty() {
                            self.diags.push(Diagnostic::error(
                                "E0119",
                                format!("`{}` isn't generic", name),
                                "only types declared with type parameters accept `<…>`".to_string(),
                                format!("use `{}` without type arguments", name),
                                Some(span),
                            ));
                        }
                    }
                    for arg in args {
                        // D-MEM-VIEWRET1: `View<str>` is the named string-view
                        // spelling. `str` is not a free-standing type — only
                        // this View argument slot may name it.
                        if lookup_name == "View"
                            && matches!(arg, Type::Named(inner) if inner == "str")
                        {
                            continue;
                        }
                        self.check_declared_type_rules(arg, span);
                    }
                }
                Type::TraitObject(ts) => {
                    for t in ts {
                        if !self.trait_reg.is_trait_name(t) {
                            self.diags.push(Diagnostic::error(
                                "E0119",
                                format!("there's no trait called `{t}`"),
                                "a trait name in type position must name a declared trait".to_string(),
                                format!("add `trait {t} {{ … }}` first"),
                                Some(span),
                            ));
                        }
                    }
                }
                Type::Option(inner) => {
                    if matches!(**inner, Type::Option(_)) {
                        self.diags.push(Diagnostic::error(
                            "E0309",
                            "an optional type can't hold another optional type".to_string(),
                            format!(
                                "`{}??` isn't supported — use one `?` only (S32)",
                                inner.name()
                            ),
                            "drop the inner `?` or unwrap before wrapping again".to_string(),
                            Some(span),
                        ));
                    }
                    self.check_declared_type_rules(inner, span);
                }
                Type::List(inner) | Type::Shared(inner) => {
                    self.check_declared_type_rules(inner, span)
                }
                Type::Map {
                    key,
                    key_span,
                    value,
                } => {
                    self.check_declared_type_rules(key, span);
                    self.check_declared_type_rules(value, span);
                    if !is_map_key_type(key) {
                        self.diags.push(Diagnostic::error(
                            "E0502",
                            format!("`{}` can't be a map key type yet", key.name()),
                            "map keys must be Int, String, Bool, Char, or a payload-free enum"
                                .to_string(),
                            "pick a simpler key type, or store a struct as the value".to_string(),
                            Some(key_span.unwrap_or(span)),
                        ));
                    }
                }
                Type::Char => {}
                Type::Result { ok, err } => {
                    self.check_declared_type_rules(ok, span);
                    self.check_declared_type_rules(err, span);
                }
                Type::Union(members) => {
                    // D-UNIONTYPE1=A: only concrete closed member types.
                    let param_names_owned = self
                        .type_param_scope
                        .iter()
                        .map(|p| p.name.clone())
                        .collect::<Vec<_>>();
                    let param_names = param_names_owned
                        .iter()
                        .map(String::as_str)
                        .collect::<std::collections::HashSet<_>>();
                    for m in members {
                        let mut mentioned = std::collections::HashSet::new();
                        crate::Generics::collect_type_param_mentions(
                            m,
                            &param_names,
                            &mut mentioned,
                        );
                        if union_member_has_open_shape(m) || !mentioned.is_empty() {
                            self.diags.push(Diagnostic::error(
                                "E0363",
                                format!("`{}` can't be a union member", m.name()),
                                "anonymous unions hold concrete closed types — not type parameters, trait objects, or function types".to_string(),
                                "use a named enum when a member needs an open shape".to_string(),
                                Some(span),
                            ));
                        }
                        self.check_declared_type_rules(m, span);
                    }
                }
                Type::Fn { params, ret, .. } => {
                    for p in params {
                        self.check_declared_type_rules(p, span);
                    }
                    if let Some(r) = ret {
                        self.check_declared_type_rules(r, span);
                    }
                }
                Type::Tuple(fields) => {
                    for (_, t) in fields {
                        self.check_declared_type_rules(t, span);
                    }
                }
                // Only a user-written D-QUAL4 tag needs a "declared" check — a
                // compiler `Internal` fact is synthesized after this pass runs
                // over source-written types, so it never reaches here anyway.
                Type::Tagged { marker: crate::AST::TagMarker::User(name), inner } => {
                    if !self.tag_is_declared(name) {
                        self.diags.push(undeclared_value_tag(
                            name,
                            self.closest_declared_tag(name).as_deref(),
                            span,
                        ));
                    }
                    self.check_declared_type_rules(inner, span);
                }
                Type::Tagged { inner, .. } => self.check_declared_type_rules(inner, span),
                _ => {}
            }
        }

        pub(crate) fn tag_is_declared(&self, name: &str) -> bool {
            crate::Syntax::BUILTIN_TAGS.contains(&name)
                || self.trait_reg.local_tags.contains(name)
                || self.modules.is_some_and(|modules| {
                    self.imports.values().copied().any(|idx| {
                        modules[idx].trait_reg.local_tags.contains(name)
                            && self.type_is_pub_in(idx, name)
                    })
                })
        }

        pub(crate) fn closest_declared_tag(&self, name: &str) -> Option<String> {
            let mut candidates = self
                .trait_reg
                .local_tags
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            candidates.extend(crate::Syntax::BUILTIN_TAGS.iter().map(|tag| (*tag).to_string()));
            if let Some(modules) = self.modules {
                for idx in self.imports.values().copied() {
                    candidates.extend(
                        modules[idx]
                            .trait_reg
                            .local_tags
                            .iter()
                            .filter(|tag| self.type_is_pub_in(idx, tag))
                            .cloned(),
                    );
                }
            }
            candidates.sort();
            candidates.dedup();
            candidates
                .into_iter()
                .map(|candidate| (edit_distance(name, &candidate), candidate))
                .filter(|(distance, _)| *distance <= 2)
                .min_by(|a, b| a.cmp(b))
                .map(|(_, candidate)| candidate)
        }
    
        /// Returns true when a diagnostic was emitted (the mismatch is already
        /// reported); callers may add a context-specific error otherwise.
        ///
        pub(crate) fn check_type_assignable(&mut self, want: &Type, got: &Type, span: Span) -> bool {
            if want == got {
                if !Type::obligations_satisfy(want, got) {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "this needs {}, but the callable obligations are not satisfied",
                            want.show()
                        ),
                        "the callable value does not provide the obligations required here"
                            .to_string(),
                        "pass a callable with matching effects, labels, and view provenance"
                            .to_string(),
                        Some(span),
                    ));
                    return true;
                }
                return false;
            }
            if let (
                Type::Fn {
                    params: want_params,
                    ret: want_ret,
                    ..
                },
                Type::Fn {
                    params: got_params,
                    ret: got_ret,
                    ..
                },
            ) = (want, got)
            {
                // Keep ordinary function-shape mismatches on their existing
                // generic diagnostic path. E0771 is only for the callable
                // contract after parameters and return type already match.
                if want_params == got_params && want_ret == got_ret {
                    if fn_types_compatible(want, got) {
                        return true;
                    }
                    self.diags.push(Diagnostic::error(
                        "E0771",
                        format!(
                            "this needs {}, but the function value is {}",
                            want.show(),
                            got.show()
                        ),
                        "public labels and parameter zones are part of a function's callable type"
                            .to_string(),
                        format!("use `{}` here", want.name()),
                        Some(span),
                    ));
                    return true;
                }
            }
            if Type::compute_tensor_compatible(want, got) {
                // The erased `Tensor` spelling is the storage boundary. A
                // shaped alias remains exact when both sides carry shape.
                return true;
            }
            if result_used_where_plain_expected(want, got) {
                self.diags.push(Diagnostic::error(
                    "E0401",
                    format!(
                        "this needs {}, but the value is {}",
                        want.show(),
                        got.show()
                    ),
                    "a fallible result must be checked before its value is used".to_string(),
                    format!(
                        "use `{}`, `{}`, or test with `== {}(...)` / `== {}(...)`",
                        Syntax::OP_TRY_SUFFIX,
                        Syntax::OP_FALLBACK,
                        Syntax::LIT_OK,
                        Syntax::LIT_ERR
                    ),
                    Some(span),
                ));
                return true;
            }
            if option_used_where_plain_expected(want, got) {
                self.diags.push(Diagnostic::error(
                    "E0310",
                    format!(
                        "this needs {}, but the value is {}",
                        want.show(),
                        got.show()
                    ),
                    "a plain value is required here, not an optional one".to_string(),
                    format!(
                        "test with `== {}(...)` or `== {}` first, e.g. `if x == {}(n) {{ ... }}`",
                        Syntax::LIT_VALUE,
                        Syntax::LIT_NULL,
                        Syntax::LIT_VALUE
                    ),
                    Some(span),
                ));
                return true;
            }
            if let Type::Option(inner) = got {
                if want.unwrap_option().is_some() {
                    if let Some(want_inner) = want.unwrap_option() {
                        if **inner != *want_inner {
                            self.report_option_mismatch(want, got, span);
                            return true;
                        }
                    }
                } else if **inner != *want {
                    self.report_option_mismatch(want, got, span);
                    return true;
                }
                return false;
            }
            if want.unwrap_option().is_some() && got.unwrap_option().is_none() {
                self.diags.push(Diagnostic::error(
                    "E0108",
                    format!(
                        "this needs {}, but the value is {}",
                        want.show(),
                        got.show()
                    ),
                    "an optional value is required here".to_string(),
                    format!("wrap it with `{}(...)`", Syntax::LIT_VALUE),
                    Some(span),
                ));
                return true;
            }
            match (want, got) {
                // D-FIXARR1: a `[T#N]` widens to `[T]` — codegen emits `.to_vec()`.
                (Type::List(want_elem), Type::FixedList { elem: got_elem, .. })
                    if want_elem == got_elem =>
                {
                    return false;
                }
                // D-UNIONTYPE1=A: a member value widens into its union.
                (Type::Union(members), got) if members.iter().any(|m| m == got) => {
                    return false;
                }
                (Type::TraitObject(trait_names), got) => {
                    for trait_name in trait_names {
                        if !self.trait_reg.type_implements_trait(got, trait_name) {
                            let needs_derive = trait_name == COMPARABLE || trait_name == "Serialize";
                            self.diags
                                .push(e0905(&got.name(), trait_name, span, needs_derive));
                            return true;
                        }
                    }
                    return false;
                }
                _ => {}
            }
            false
        }

        pub(crate) fn report_option_mismatch(&mut self, want: &Type, got: &Type, span: Span) {
            self.diags.push(Diagnostic::error(
                "E0108",
                format!(
                    "this needs {}, but the value is {}",
                    want.show(),
                    got.show()
                ),
                "the types must match".to_string(),
                type_fix_hint(want, got),
                Some(span),
            ));
        }
    
}

#[cfg(test)]
mod tests {
    use super::is_core_view_generic;
    use crate::AST::Type;

    #[test]
    fn returned_view_generic_recognition_is_exact() {
        for name in ["View", "ViewMut"] {
            assert!(is_core_view_generic(&Type::Apply {
                name: name.to_string(),
                args: vec![Type::Int],
            }));
            assert!(!is_core_view_generic(&Type::Named(name.to_string())));
        }
        assert!(!is_core_view_generic(&Type::Apply {
            name: "UserView".to_string(),
            args: vec![Type::Int],
        }));
    }
}
