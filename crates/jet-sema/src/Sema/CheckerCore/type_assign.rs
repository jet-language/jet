use super::helpers::no_any_type;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{e0905, e0909, generic_depth_exceeded, substitute_type, COMPARABLE};
use crate::Sema::Bundle::fn_types_compatible;
use crate::Sema::CheckerCoreLib::{
    core_type_known, data_renamed_to_datatree, layout_handle_renamed_to_layout,
    phantom_fact_menu_diag, retired_acronym_spelling_diag, retired_authority_vocabulary_diag,
};
use crate::Sema::Diagnostics::{
    option_used_where_plain_expected, plain_used_where_result_expected,
    result_used_where_plain_expected, soft_public_use, suggest_field, type_fix_hint,
    undeclared_value_tag,
};
use crate::Sema::{Checker, KnowledgeGate, KnowledgePlane, TypeDef};
use crate::Syntax;
use crate::AST::Type;

fn union_member_has_open_shape(ty: &Type) -> bool {
    match ty {
        Type::TraitObject(_) | Type::Fn { .. } => true,
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::Tagged { inner, .. }
        | Type::FixedList { elem: inner, .. } => union_member_has_open_shape(inner),
        Type::Map { key, value, .. }
        | Type::Result {
            ok: key,
            err: value,
        } => union_member_has_open_shape(key) || union_member_has_open_shape(value),
        Type::Apply { args, .. } | Type::Union(args) => {
            args.iter().any(union_member_has_open_shape)
        }
        Type::Tuple(fields) => fields
            .iter()
            .any(|(_, field)| union_member_has_open_shape(field)),
        _ => false,
    }
}

pub(crate) fn is_core_view_generic(ty: &Type) -> bool {
    // D-PIN1=A / D-PIN3=A: `Pin<T>` joins the borrowed-window family. It is a
    // core generic like `View`/`ViewMut` — declarable on fields, parameters,
    // and returns, and constructed only by `mem.pin`.
    matches!(
        ty,
        Type::Apply { name, .. }
            if matches!(name.as_str(), "View" | "ViewMut" | Syntax::TYPE_PIN | Syntax::TYPE_VIEW_ITER)
    )
}

/// Reject `Never` wherever a declaration stores an ordinary value. The only
/// exceptions are a callable's successful return slot and the already-ratified
/// failure-side `!Never` carrier.
pub(crate) fn reject_never_value_positions(
    ty: &Type,
    span: Span,
    allow_success_never: bool,
    diags: &mut Vec<Diagnostic>,
) {
    match ty {
        Type::Named(name) if name == Syntax::TYPE_NEVER && !allow_success_never => {
            diags.push(Diagnostic::from_row("E2422", &[], Some(span)));
        }
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::Tagged { inner, .. }
        | Type::FixedList { elem: inner, .. }
        | Type::InlineRange { base: inner, .. }
        | Type::Quantity { base: inner, .. } => {
            reject_never_value_positions(inner, span, false, diags);
        }
        Type::Map { key, value, .. } => {
            reject_never_value_positions(key, span, false, diags);
            reject_never_value_positions(value, span, false, diags);
        }
        Type::Result { ok, err } => {
            reject_never_value_positions(ok, span, allow_success_never, diags);
            reject_never_value_positions(err, span, true, diags);
        }
        Type::Fn { params, ret, .. } => {
            for param in params {
                reject_never_value_positions(param, span, false, diags);
            }
            if let Some(ret) = ret {
                reject_never_value_positions(ret, span, true, diags);
            }
        }
        Type::Apply { args, .. } | Type::Union(args) => {
            for arg in args {
                reject_never_value_positions(arg, span, false, diags);
            }
        }
        Type::Tuple(fields) => {
            for (_, field) in fields {
                reject_never_value_positions(field, span, false, diags);
            }
        }
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32
        | Type::TraitObject(_)
        | Type::Measure(_)
        | Type::Named(_) => {}
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn check_declared_type(&mut self, ty: &Type, span: Span) {
        self.check_declared_type_common(ty, span);
        self.reject_never_value_positions(ty, span, false);
    }

    /// Check a function's declared return contract. `Never` is legal only in
    /// this success slot (and nested function-type return slots); ordinary
    /// value declarations use `check_declared_type` instead.
    pub(crate) fn check_declared_return_type(&mut self, ty: &Type, span: Span) {
        self.check_declared_type_common(ty, span);
        self.reject_never_value_positions(ty, span, true);
    }

    fn check_declared_type_common(&mut self, ty: &Type, span: Span) {
        self.warn_soft_public_declared_type(ty, span);
        self.check_declared_type_rules(ty, span);
        if self.cell_guard_storage_is_unsupported(ty) {
            self.report_cell_guard_storage(
                format!("a Cell guard cannot be stored in `{}`", ty.show()),
                span,
            );
        }
    }

    fn reject_never_value_positions(&mut self, ty: &Type, span: Span, allow_success_never: bool) {
        reject_never_value_positions(ty, span, allow_success_never, &mut self.diags);
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
            Type::FixedList { elem, .. }
            | Type::Tagged { inner: elem, .. }
            | Type::InlineRange { base: elem, .. } => {
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
        self.warn_deprecated_type_name(name, span);
        let (owner, public_name) = if let Some((alias, leaf)) = name.rsplit_once('.') {
            (self.imports.get(alias).copied(), leaf)
        } else {
            let locally_owned = self.registry.contains(name) || {
                let declared = self
                    .name_ledger
                    .declaration(self.module_idx, name)
                    .is_some();
                self.modules.is_some_and(|modules| {
                    declared && modules[self.module_idx].trait_reg.is_trait_name(name)
                })
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

    pub(crate) fn warn_deprecated_type_name(&mut self, name: &str, span: Span) {
        let (import_ns, leaf) = self.struct_type_name_parts(name);
        let Some(owner) = self.struct_owner_module(leaf, import_ns) else {
            return;
        };
        let deprecation = if owner == self.module_idx {
            self.registry
                .types
                .get(leaf)
                .and_then(|definition| match definition {
                    TypeDef::Struct { deprecation, .. }
                    | TypeDef::Enum { deprecation, .. }
                    | TypeDef::Distinct { deprecation, .. }
                    | TypeDef::Alias { deprecation, .. } => deprecation.as_ref(),
                })
        } else {
            self.modules
                .and_then(|modules| modules.get(owner))
                .and_then(|module| module.registry.types.get(leaf))
                .and_then(|definition| match definition {
                    TypeDef::Struct { deprecation, .. }
                    | TypeDef::Enum { deprecation, .. }
                    | TypeDef::Distinct { deprecation, .. }
                    | TypeDef::Alias { deprecation, .. } => deprecation.as_ref(),
                })
        }
        .cloned();
        if let Some(deprecation) = deprecation {
            let item = import_ns.map_or_else(
                || leaf.to_string(),
                |namespace| format!("{namespace}.{leaf}"),
            );
            self.check_deprecation(&item, &deprecation, span);
        }
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
                    self.diags
                        .push(retired_acronym_spelling_diag(n, &canonical, span));
                    return;
                }
                if let Some(diag) = retired_authority_vocabulary_diag(n, span) {
                    self.diags.push(diag);
                    return;
                }
                if let Some((alias, leaf)) = n.split_once('.') {
                    if let Some(module) = self.core_imports.get(alias) {
                        if let Some(kind) =
                            jet_foundation::CoreModuleExports::core_leaf_kind(module, leaf)
                        {
                            match kind {
                                jet_foundation::CoreModuleExports::CoreLeafKind::Generic(arity) => {
                                    self.diags.push(Diagnostic::error(
                                        "E0119",
                                        format!(
                                            "`{n}` expects {arity} type argument{}, got 0",
                                            if arity == 1 { "" } else { "s" }
                                        ),
                                        "every generic Core type needs a matching type argument"
                                            .to_string(),
                                        format!(
                                            "write `{n}<...>` with exactly {arity} type argument{}",
                                            if arity == 1 { "" } else { "s" }
                                        ),
                                        Some(span),
                                    ));
                                }
                                _ => {}
                            }
                            return;
                        }
                    }
                }
                if !self.registry.contains(n) {
                    if let Some(arity) =
                        jet_foundation::CoreModuleExports::core_generic_arity(n)
                    {
                        self.diags.push(Diagnostic::error(
                            "E0119",
                            format!(
                                "`{n}` expects {arity} type argument{}, got 0",
                                if arity == 1 { "" } else { "s" }
                            ),
                            "every generic Core type needs a matching type argument".to_string(),
                            format!(
                                "write `{n}<...>` with exactly {arity} type argument{}",
                                if arity == 1 { "" } else { "s" }
                            ),
                            Some(span),
                        ));
                        return;
                    }
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
                    if let (Some(modules), Some(&index)) = (self.modules, self.imports.get(module))
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
                // D-FACT-HOME1=A: a phantom fact-menu name (`Effect`,
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
                let (import_ns, lookup_name) = Self::split_type_name(name);
                let canonical_owner = name
                    .contains("::")
                    .then(|| self.name_ledger.nominal_module(name))
                    .flatten();
                if lookup_name == "Any" {
                    self.diags.push(no_any_type(span));
                    for arg in args {
                        self.check_declared_type_rules(arg, span);
                    }
                    return;
                }
                let local_alias = canonical_owner.is_none() && import_ns.is_none();
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
                let core_generic_arity = match import_ns {
                    Some(namespace) => self
                        .core_imports
                        .get(namespace)
                        .and_then(|module| {
                            match jet_foundation::CoreModuleExports::core_leaf_kind(
                                module,
                                lookup_name,
                            ) {
                                Some(
                                    jet_foundation::CoreModuleExports::CoreLeafKind::Generic(
                                        arity,
                                    ),
                                ) => Some(arity),
                                _ => None,
                            }
                        }),
                    None => jet_foundation::CoreModuleExports::core_generic_arity(lookup_name),
                };
                if let Some(expected) = core_generic_arity {
                    if args.len() != expected {
                        self.diags.push(Diagnostic::error(
                            "E0119",
                            format!(
                                "`{name}` expects {expected} type argument{}, got {}",
                                if expected == 1 { "" } else { "s" },
                                args.len()
                            ),
                            "every generic Core type needs a matching type argument".to_string(),
                            format!("write `{name}<...>` with exactly {expected} type argument{}", if expected == 1 { "" } else { "s" }),
                            Some(span),
                        ));
                    }
                }
                let is_core_generic = core_generic_arity.is_some()
                    || (local_alias
                        && matches!(
                            lookup_name,
                            "Task"
                                | Syntax::TYPE_RECEIVER
                                | "Sender"
                                | "Ptr"
                                | "Tensor"
                                | "Vec"
                                | "Matrix"
                                | Syntax::TYPE_ATOMIC
                            | "Set" | Syntax::TYPE_TALLY | Syntax::TYPE_QUEUE
                            // D-ITERTOOLS1=A: expanded generic collection handles.
                            | Syntax::TYPE_RANK | "PriorityQueue" | "Cache"
                            | "Decimal"
                            // D-REACT1=B: reactive handle types.
                            | "Signal" | "Derived" | "Computed"
                            // D-EVENT1=D: first-party typed Event/Hook family.
                            | "Event" | "Hook" | "DecisionHook" | "HookDecision" | "HookOutcome"
                            | "DispatchReport"
                            // D-STREAMYIELD1: generator return type.
                            | "Stream"
                            // D-FOUND-COREAPI1 / #2853: event-time stream
                            // intermediate and window carriers are core generics.
                            | "StreamEventTime" | "KeyedStream" | "Window"
                            // D-QUERY-RETAIN1=A: one deferred query and
                            // generic grouped result replace the old wrappers.
                            | "Query" | "DataGroupedQuery" | "Group"
                            // D-DATAFRAME1=A: joins remain typed list products.
                            | "DataJoin"
                            | "Pool" | "Id"
                            // D-LOCALCELL1=A: one-thread cell and projected guard types.
                            | "Cell" | "CellReadGuard" | "CellEditGuard"
                            // The one closed secret-lifetime wrapper.
                            | "ExpiringSecret" | Syntax::TYPE_SHARED_GUARD
                            | Syntax::TYPE_SHARED_WEAK
                            // D-SHARED-REVISION1=A: owner-bound snapshot has
                            // source and projection type arguments.
                            | Syntax::TYPE_SHARED_SNAPSHOT
                            | "KeyRef" | "MutationPlan" | "VaultWrite" | "Rotation" | "WrappedImportPlan"
                            // D-SPACE-GEOMETRY1=A: point/delta carry a scalar
                            // and one nominal space; transforms carry two.
                            | "Point2" | "Delta2" | "Transform" | "Transform2" | "Ray2"
                        ))
                    || (local_alias && is_core_view_generic(ty));
                if is_core_generic
                    && matches!(
                        lookup_name,
                        "Point2" | "Delta2" | "Transform" | "Transform2" | "Ray2"
                    )
                {
                    let valid = match lookup_name {
                        "Point2" | "Delta2" => {
                            args.len() == 2
                                && args[0].is_float()
                                && matches!(&args[1], Type::Named(_))
                        }
                        "Transform" => {
                            args.len() == 2
                                && matches!((&args[0], &args[1]), (Type::Named(_), Type::Named(_)))
                        }
                        "Transform2" => {
                            args.len() == 3
                                && args[0].is_float()
                                && matches!((&args[1], &args[2]), (Type::Named(_), Type::Named(_)))
                        }
                        "Ray2" => {
                            args.len() == 3
                                && args[0].is_float()
                                && matches!((&args[1], &args[2]), (Type::Named(_), Type::Named(_)))
                        }
                        _ => false,
                    };
                    if !valid {
                        self.diags.push(Diagnostic::error(
                            "E0119",
                            format!("`{name}` is not a valid coordinate-space type"),
                            "Point2 and Delta2 are `<Float, Space>`; Transform is `<From, To>` and Transform2/Ray2 are `<Float, From, To>`".to_string(),
                            "use a known nominal space such as `Screen`, `World`, `View`, `Camera`, or `Device` and keep the argument order".to_string(),
                            Some(span),
                        ));
                    }
                    for arg in args {
                        self.check_declared_type_rules(arg, span);
                    }
                    return;
                }
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
                if is_core_generic && lookup_name == Syntax::TYPE_ATOMIC {
                    let valid = args.len() == 1
                        && args
                            .first()
                            .is_some_and(jet_foundation::Layout::atomic_scalar_type);
                    if !valid {
                        self.diags.push(Diagnostic::error(
                            "E0119",
                            format!(
                                "`{name}` accepts exactly one closed scalar: `Bool`, `I32`, `U32`, `I64`, `Int`, or `U64`"
                            ),
                            "Atomic values use one compiler-owned lock-free word; arbitrary widths and compound types have no portable representation".to_string(),
                            "use `Atomic<Bool>`, `Atomic<I32>`, `Atomic<U32>`, `Atomic<I64>`, `Atomic<Int>`, or `Atomic<U64>`".to_string(),
                            Some(span),
                        ));
                    }
                    for arg in args {
                        self.check_declared_type_rules(arg, span);
                    }
                    return;
                }
                if is_core_generic
                    && lookup_name == "ExpiringSecret"
                    && (args.len() != 1
                        || !args
                            .first()
                            .is_some_and(crate::Sema::Diagnostics::is_expiring_secret_member_type))
                {
                    self.diags.push(Diagnostic::error(
                            "E0112",
                            "`ExpiringSecret<T>` requires a secret type".to_string(),
                            "only Secret, SigningKey, and X25519SecretKey have the audited move-only zeroizing contract required by this wrapper".to_string(),
                            "use `ExpiringSecret<crypto.Secret>`, `ExpiringSecret<crypto.SigningKey>`, or `ExpiringSecret<crypto.X25519SecretKey>`".to_string(),
                            Some(span),
                        ));
                }
                let explicit_import_owner = import_ns
                    .filter(|namespace| !namespace.contains("::"))
                    .and_then(|namespace| self.struct_owner_module(lookup_name, Some(namespace)));
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
                let imported_owner =
                    (imported_owners.len() == 1).then(|| *imported_owners.iter().next().unwrap());
                let local_type = self.registry.contains(lookup_name)
                    && match canonical_owner {
                        Some(owner) => owner == self.module_idx,
                        None => import_ns.is_none(),
                    };
                if !is_core_generic && !local_type && imported_owner.is_none() {
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
                    if lookup_name == "View" && matches!(arg, Type::Named(inner) if inner == "str")
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
            Type::List(inner) | Type::Shared(inner) => self.check_declared_type_rules(inner, span),
            Type::Map {
                key,
                key_span,
                value,
            } => {
                self.check_declared_type_rules(key, span);
                self.check_declared_type_rules(value, span);
                if !self.map_key_type_eligible(key) {
                    self.diags.push(Diagnostic::error(
                        "E0502",
                        format!("`{}` can't be a map key type (D-MAP-KEY1)", key.name()),
                        "map keys must be Int, String, Bool, Char, U8/IntN, a payload-free enum, or a tuple/struct whose fields recursively follow D-MAP-KEY1; Float, views, Shared, functions, lists, maps, sets, and payload-carrying enums are not key-eligible".to_string(),
                        "use an eligible scalar, enum, tuple, or struct key; remove non-key fields or store the value separately".to_string(),
                        Some(key_span.unwrap_or(span)),
                    ));
                }
            }
            Type::Char => {}
            Type::Result { ok, err } => {
                self.check_declared_type_rules(ok, span);
                self.check_declared_type_rules(err, span);
                // D-FAILURE-FOUNDATION1=A: every explicit `!` domain is
                // checked after source/import spellings are resolved. Keep
                // this beside the recursive declared-type walk so callbacks,
                // aliases, fields, and function returns share one rule.
                let resolved_error = self.resolve_type((**err).clone());
                if !self.is_error_domain(&resolved_error) {
                    let domain = resolved_error.show();
                    self.diags.push(Diagnostic::from_row(
                        "E2417",
                        &[("domain", domain.as_str())],
                        Some(span),
                    ));
                }
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
                    crate::Generics::collect_type_param_mentions(m, &param_names, &mut mentioned);
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
            Type::Tagged {
                marker: crate::AST::TagMarker::User(name),
                inner,
            } => {
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
            Type::InlineRange { base, .. } => self.check_declared_type_rules(base, span),
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
        candidates.extend(
            crate::Syntax::BUILTIN_TAGS
                .iter()
                .map(|tag| (*tag).to_string()),
        );
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
        suggest_field(name, &candidates)
    }

    /// Find a checked-text slot that would receive a plain `String`.
    ///
    /// Most callers add their own context-specific mismatch after this method
    /// returns false. A few owning literal paths intentionally ignore that
    /// boolean, so checked text needs one shared rejection before those paths
    /// can manufacture a nominal outer value around an unchecked string.
    fn checked_text_string_target(&self, want: &Type, got: &Type) -> bool {
        match (want, got) {
            (Type::Named(name), Type::String)
                if self.is_checked_text_type_name(name)
                    || self.type_param_has_bound(want, crate::Generics::CHECKED_TEXT) =>
            {
                true
            }
            (Type::List(want), Type::List(got))
            | (Type::Shared(want), Type::Shared(got))
            | (Type::Option(want), Type::Option(got)) => self.checked_text_string_target(want, got),
            (Type::List(want), Type::FixedList { elem: got, .. })
            | (Type::FixedList { elem: want, .. }, Type::List(got)) => {
                self.checked_text_string_target(want, got)
            }
            (
                Type::Map {
                    key: want_key,
                    value: want_value,
                    ..
                },
                Type::Map {
                    key: got_key,
                    value: got_value,
                    ..
                },
            ) => {
                self.checked_text_string_target(want_key, got_key)
                    || self.checked_text_string_target(want_value, got_value)
            }
            (
                Type::Result {
                    ok: want_ok,
                    err: want_err,
                },
                Type::Result {
                    ok: got_ok,
                    err: got_err,
                },
            ) => {
                self.checked_text_string_target(want_ok, got_ok)
                    || self.checked_text_string_target(want_err, got_err)
            }
            (
                Type::Apply {
                    args: want_args, ..
                },
                Type::Apply { args: got_args, .. },
            ) => want_args
                .iter()
                .zip(got_args)
                .any(|(want, got)| self.checked_text_string_target(want, got)),
            (Type::Tuple(want_fields), Type::Tuple(got_fields)) => want_fields
                .iter()
                .zip(got_fields)
                .any(|((_, want), (_, got))| self.checked_text_string_target(want, got)),
            (Type::FixedList { elem: want, .. }, Type::FixedList { elem: got, .. }) => {
                self.checked_text_string_target(want, got)
            }
            (
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
            ) => {
                want_params
                    .iter()
                    .zip(got_params)
                    .any(|(want, got)| self.checked_text_string_target(want, got))
                    || want_ret
                        .as_deref()
                        .zip(got_ret.as_deref())
                        .is_some_and(|(want, got)| self.checked_text_string_target(want, got))
            }
            (Type::Tagged { inner: want, .. }, Type::Tagged { inner: got, .. })
            | (Type::InlineRange { base: want, .. }, Type::InlineRange { base: got, .. })
            | (Type::Quantity { base: want, .. }, Type::Quantity { base: got, .. }) => {
                self.checked_text_string_target(want, got)
            }
            (Type::Union(want_members), Type::Union(got_members)) => {
                want_members.iter().any(|want| {
                    got_members
                        .iter()
                        .any(|got| self.checked_text_string_target(want, got))
                })
            }
            _ => false,
        }
    }

    /// Returns true when a diagnostic was emitted or compatibility was
    /// handled; callers may add a context-specific error otherwise.
    ///
    pub(crate) fn check_type_assignable(&mut self, want: &Type, got: &Type, span: Span) -> bool {
        if want == got {
            if !Type::obligations_satisfy(want, got) {
                if matches!((want, got), (Type::Fn { .. }, Type::Fn { .. })) {
                    // Let callable-value callers report the concrete required
                    // and offered signatures instead of a generic E0108.
                    return false;
                }
                self.diags.push(Diagnostic::error(
                    "E0108",
                    format!(
                        "this needs {}, but the callable obligations are not satisfied",
                        want.show()
                    ),
                    "the callable value does not provide the obligations required here".to_string(),
                    "pass a callable with matching effects, labels, and view provenance"
                        .to_string(),
                    Some(span),
                ));
                return true;
            }
            return true;
        }
        // D-NEVER2=B: a callable promised to return `Never` cannot be
        // satisfied by a value-producing function. Check the normalized
        // callable returns so raw `fn() Int` and fallible `fn() Int !E`
        // both fail this contract.
        if let (
            Type::Fn {
                ret: Some(want_ret),
                ..
            },
            Type::Fn {
                ret: Some(got_ret),
                ..
            },
        ) = (
            want.with_effective_fn_returns(),
            got.with_effective_fn_returns(),
        ) {
            if want_ret.has_never_success() && !got_ret.has_never_success() {
                self.diags.push(Diagnostic::from_row("E2426", &[], Some(span)));
                return true;
            }
        }
        if self.checked_text_string_target(want, got) {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!(
                    "this needs {}, but the value is {}",
                    want.show(),
                    got.show()
                ),
                "a checked text type is nominal; a plain String needs validation before it crosses this boundary".to_string(),
                "construct the checked text with its checked literal or `from(text)`".to_string(),
                Some(span),
            ));
            return true;
        }
        // D-TYPE2-EXACT1: an inline range is proof attached to its carrier.
        // Reaching the carrier boundary erases that proof, so it is still
        // a knowledge loss even though the runtime representation is the
        // same integer.
        if matches!(got, Type::InlineRange { .. }) && got.erased_inline_ranges() == *want {
            self.require_knowledge_gate(
                KnowledgePlane::Range,
                KnowledgeGate::BoundedArithmetic,
                span,
            );
            return true;
        }
        // Callable contracts are directional even when the structural function
        // shape is identical. An erased function type carries no proof that it
        // can honor a strict positional/label-only zone; do not let it cross
        // that boundary. Labels remain declaration metadata here, not inferred
        // parameter-name identity, so direct named-function mismatches retain
        // their existing E0112 path below.
        if let (
            Type::Fn {
                param_contract: Some(want_contract),
                ..
            },
            Type::Fn {
                param_contract: None,
                ..
            },
        ) = (want, got)
        {
            let has_strict_zone = want_contract.iter().any(|(_, zone)| {
                matches!(
                    zone,
                    crate::AST::ParamZone::PositionalOnly | crate::AST::ParamZone::LabelOnly
                )
            });
            if fn_types_compatible(want, got) && has_strict_zone {
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
        // Function values are structurally assignable. Keep call metadata for
        // binding a value call, but it cannot make equal callable shapes into
        // different value types.
        if matches!((want, got), (Type::Fn { .. }, Type::Fn { .. }))
            && fn_types_compatible(want, got)
            && Type::obligations_satisfy(want, got)
        {
            return true;
        }
        // A qualified constructor carries the owning module in its nominal
        // spelling, while a value from an imported signature may still carry
        // the source leaf. Treat those spellings as one struct type before
        // the ordinary mismatch diagnostics run. The helper is deliberately
        // limited to registered structs and their type arguments; enum,
        // trait, and tagged nominal identity remain distinct mechanisms.
        if self.nominal_type_identity(want, got) {
            return true;
        }
        if Type::compute_tensor_compatible(want, got) {
            // The erased `Tensor` spelling is the storage boundary. A
            // shaped alias remains exact when both sides carry shape.
            return true;
        }
        if plain_used_where_result_expected(want, got) {
            // D-FAILCOMP1: `[T !E]{ok_value, …}` and other fallible slots can
            // store a plain success `T` without an explicit `Ok` wrapper.
            return true;
        }
        if result_used_where_plain_expected(want, got) {
            // An unannotated callback may widen its expected success row to
            // Result when a nested call can fail. The expression wrapper will
            // insert the canonical Try node after this argument check; retain
            // the carrier here instead of reporting the pre-elaboration shape.
            if self.failure_carrier_inference {
                if self.failure_carrier.is_none() {
                    self.failure_carrier = Some(got.clone());
                    self.ret = Some(got.clone());
                }
                self.task_body_propagates = true;
                return true;
            }
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
            return true;
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
                return true;
            }
            // D-UNIONTYPE1=A: a member value widens into its union.
            (Type::Union(members), got) if members.iter().any(|m| m == got) => {
                return true;
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
                return true;
            }
            _ => {}
        }
        false
    }

    /// S48 (syntax-decisions.md:1613; spec.md:839, ratified): a
    /// trait name in type position (`fn f(s: Shape)`) means dynamic dispatch
    /// with INVISIBLE boxing. A concrete value whose type implements the
    /// named trait therefore MEETS that slot — sema decides acceptance only;
    /// the box itself is materialised by the slot-driven boxing lowering
    /// already applies to a `[Shape]` list element (`TCallArg::box_as_trait`
    /// / `preserve_typed_list_shape`), never by a second mechanism here.
    ///
    /// Single-trait only (the S48 shape). A multi-name `TraitObject` is the
    /// D-ANY-JAI1 variadic loop element, which has no one `Box<dyn T>` to
    /// coerce toward and stays on the ordinary equality path.
    pub(crate) fn trait_slot_accepts(&self, want: &Type, got: &Type) -> bool {
        // A value that already IS the trait value needs no coercion: an
        // unresolved bare-trait parameter spelling can equal the argument's
        // own type, and a `TraitObject` argument is the resolved same thing.
        if want == got || matches!(got, Type::TraitObject(_)) {
            return false;
        }
        let trait_name = match want {
            Type::TraitObject(names) if names.len() == 1 => names.first(),
            // An unresolved signature type still spells the trait as a plain
            // nominal; `CheckerCore/types.rs:179` uses the same test.
            Type::Named(name)
                if self.trait_reg.is_trait_name(name) && !self.registry.contains(name) =>
            {
                Some(name)
            }
            _ => None,
        };
        trait_name.is_some_and(|trait_name| self.trait_reg.type_implements_trait(got, trait_name))
    }

    pub(crate) fn nominal_type_identity(&self, want: &Type, got: &Type) -> bool {
        if want == got {
            return true;
        }
        match (want, got) {
            (Type::Named(want_name), Type::Named(got_name))
                if Syntax::is_data_type_name(want_name) && Syntax::is_data_type_name(got_name) =>
            {
                true
            }
            (Type::Named(want_name), Type::Named(got_name)) => {
                self.same_declared_name_identity(want_name, got_name)
            }
            (
                Type::Apply {
                    name: want_name,
                    args: want_args,
                },
                Type::Apply {
                    name: got_name,
                    args: got_args,
                },
            ) => {
                want_args.len() == got_args.len()
                    && self.same_declared_name_identity(want_name, got_name)
                    && want_args
                        .iter()
                        .zip(got_args)
                        .all(|(want, got)| self.nominal_type_identity(want, got))
            }
            (Type::List(want), Type::List(got))
            | (Type::Shared(want), Type::Shared(got))
            | (Type::Option(want), Type::Option(got)) => self.nominal_type_identity(want, got),
            (
                Type::Result {
                    ok: want_ok,
                    err: want_err,
                },
                Type::Result {
                    ok: got_ok,
                    err: got_err,
                },
            ) => {
                self.nominal_type_identity(want_ok, got_ok)
                    && self.nominal_type_identity(want_err, got_err)
            }
            (
                Type::Map {
                    key: want_key,
                    value: want_value,
                    ..
                },
                Type::Map {
                    key: got_key,
                    value: got_value,
                    ..
                },
            ) => {
                self.nominal_type_identity(want_key, got_key)
                    && self.nominal_type_identity(want_value, got_value)
            }
            (
                Type::FixedList {
                    elem: want_elem,
                    len: want_len,
                    ..
                },
                Type::FixedList {
                    elem: got_elem,
                    len: got_len,
                    ..
                },
            ) => want_len == got_len && self.nominal_type_identity(want_elem, got_elem),
            (Type::Tuple(want_fields), Type::Tuple(got_fields)) => {
                want_fields.len() == got_fields.len()
                    && want_fields.iter().zip(got_fields).all(
                        |((want_name, want), (got_name, got))| {
                            want_name == got_name && self.nominal_type_identity(want, got)
                        },
                    )
            }
            (Type::Union(want_members), Type::Union(got_members)) => {
                if want_members.len() != got_members.len() {
                    return false;
                }
                let mut matched = vec![false; got_members.len()];
                want_members.iter().all(|want_member| {
                    let Some(index) =
                        got_members
                            .iter()
                            .enumerate()
                            .position(|(index, got_member)| {
                                !matched[index]
                                    && self.nominal_type_identity(want_member, got_member)
                            })
                    else {
                        return false;
                    };
                    matched[index] = true;
                    true
                })
            }
            (
                Type::Tagged {
                    marker: want_marker,
                    inner: want_inner,
                },
                Type::Tagged {
                    marker: got_marker,
                    inner: got_inner,
                },
            ) if want_marker == got_marker => self.nominal_type_identity(want_inner, got_inner),
            (
                Type::Quantity {
                    base: want_base,
                    dimension: want_dimension,
                },
                Type::Quantity {
                    base: got_base,
                    dimension: got_dimension,
                },
            ) if want_dimension == got_dimension => self.nominal_type_identity(want_base, got_base),
            _ => false,
        }
    }

    fn declared_nominal_identity(&self, spelling: &str) -> Option<String> {
        let (namespace, leaf) = Self::split_type_name(spelling);
        if let Some(owner) = self.struct_owner_module(leaf, namespace) {
            return Some(self.canonical_nominal_name(owner, leaf));
        }

        // Inline-module types have a compiler-owned leaf (`M...Account`) but
        // source names (`bank.Account`). A return path can add the enclosing
        // file alias (`vis2.bank.Account`) before display projection. Resolve
        // both through the ledger's one display map instead of comparing
        // spellings or inventing another alias table.
        let current_prefix = self
            .modules
            .and_then(|modules| modules.get(self.module_idx))
            .map(|module| format!("{}.", module.module_alias));
        // A named helper, not a closure: the returned iterator borrows the
        // caller's `name`, and a closure would tie that borrow to the
        // closure's own inferred lifetime.
        fn visible_spellings<'n>(
            name: &'n str,
            current_prefix: Option<&'n str>,
        ) -> impl Iterator<Item = &'n str> {
            std::iter::once(name).chain(current_prefix.and_then(|prefix| name.strip_prefix(prefix)))
        }
        let Some(modules) = self.modules else {
            return None;
        };
        for (owner, module) in modules.iter().enumerate() {
            for candidate in module.registry.types.keys() {
                if owner != self.module_idx && !self.type_is_pub_in(owner, candidate) {
                    continue;
                }
                let display =
                    self.name_ledger
                        .display_path(self.module_idx, candidate, Some(owner));
                let matches = display
                    .as_deref()
                    .into_iter()
                    .chain(std::iter::once(candidate.as_str()))
                    .any(|target| {
                        visible_spellings(spelling, current_prefix.as_deref())
                            .any(|name| name == target)
                    });
                if matches {
                    return Some(self.canonical_nominal_name(owner, candidate));
                }
            }
        }
        None
    }

    fn same_declared_name_identity(&self, want: &str, got: &str) -> bool {
        self.declared_nominal_identity(want)
            .zip(self.declared_nominal_identity(got))
            .is_some_and(|(want, got)| want == got)
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
