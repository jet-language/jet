use super::helpers::is_pod_uninit_type;
use crate::AST::{
    AccessConvention, BindPattern, Binding, CallArg, Expr, Lambda, MetaAttr, MetaField, Pattern,
    Stmt, StrPart, Type,
};
use crate::Diagnostics::{Diagnostic, Severity, TextEdit};
use crate::Sema::Captures::{lambda_body_refs_name, lambda_collect_captures, stmt_refs_name};
use crate::Sema::Diagnostics::{
    edit_distance, field_read_to_clone, is_task_type, type_fix_hint,
};
use crate::Sema::{Checker, LocalInfo};
use crate::Syntax;
use std::collections::{HashMap, HashSet, VecDeque};
fn core_source_target(names: &jet_foundation::Names::NameLedger, module: &str) -> Option<usize> {
    let source = jet_foundation::CoreModuleExports::core_source_module(module)?;
    names.module_by_alias(source.alias)
}

struct FragmentNominalIndex<'a> {
    modules: &'a [crate::Sema::ModuleState],
    names: &'a jet_foundation::Names::NameLedger,
    /// The module whose items a comptime fragment lowers in place. The
    /// fragment declares that module's nominals under their source names, so
    /// they keep that spelling; every other nominal is its canonical identity.
    fragment_module: Option<usize>,
}

impl FragmentNominalIndex<'_> {
    fn get(&self, owner: usize, name: &str) -> Option<&str> {
        let state = self.modules.get(owner)?;
        // A checked local type (including an alias) takes precedence over
        // the implicit original Prelude declarations.
        if self.names.declaration(owner, name).is_some_and(|declaration| declaration.kind == "type") {
            return match state.registry.types.get(name)? {
                crate::Sema::TypeDef::Struct { .. } | crate::Sema::TypeDef::Enum { .. } =>
                    self.names.declared_identity(owner, name),
                _ => None,
            };
        }
        let owner = if let Some(error_owner) = state.compiler_builtin_error_owner {
            if crate::Sema::Prelude::error_prelude_program().ok().is_some_and(|program|
                program.items.iter().any(|item| matches!(item,
                    crate::AST::Item::Struct(definition) if definition.name == name)))
            {
                error_owner
            } else {
                owner
            }
        } else {
            owner
        };
        match self.modules.get(owner)?.registry.types.get(name)? {
            crate::Sema::TypeDef::Struct { .. } | crate::Sema::TypeDef::Enum { .. } =>
                self.names.declared_identity(owner, name),
            _ => None,
        }
    }

    fn contains(&self, identity: &str) -> bool {
        let Some(owner) = self.names.nominal_module(identity) else { return false };
        let Some((_, name)) = identity.rsplit_once("::") else { return false };
        self.get(owner, name) == Some(identity)
    }

    /// How the fragment spells a canonical identity: the bare source name for
    /// a nominal its own module declares, the identity otherwise.
    fn spelling<'b>(&self, identity: &'b str) -> &'b str {
        let Some(module) = self.fragment_module else { return identity };
        match identity.rsplit_once("::") {
            Some((_, leaf)) if self.get(module, leaf) == Some(identity) => leaf,
            _ => identity,
        }
    }

    fn module_names(&self, owner: usize) -> impl Iterator<Item = (&str, &str)> {
        self.names.module_declarations(owner).filter_map(move |declaration|
            self.get(owner, &declaration.name).map(|identity| (declaration.name.as_str(), identity)))
    }
}

fn canonical_fragment_name(
    owner: usize,
    name: &str,
    modules: &[crate::Sema::ModuleState],
    source_paths: &jet_foundation::Names::NameLedger,
    struct_identities: &FragmentNominalIndex<'_>,
) -> Option<String> {
    let identity = fragment_identity(owner, name, modules, source_paths, struct_identities)?;
    let spelling = struct_identities.spelling(&identity);
    if spelling.len() < identity.len() {
        return Some(spelling.to_owned());
    }
    Some(identity)
}

fn fragment_identity(
    owner: usize,
    name: &str,
    modules: &[crate::Sema::ModuleState],
    source_paths: &jet_foundation::Names::NameLedger,
    struct_identities: &FragmentNominalIndex<'_>,
) -> Option<String> {
    if struct_identities.contains(name) {
        return Some(name.to_owned());
    }
    if let Some(identity) = source_paths.canonical_source_path(owner, name) {
        if struct_identities.contains(&identity) { return Some(identity) }
    }
    if let Some((namespace, leaf)) = name.rsplit_once('.') {
        let state = modules.get(owner)?;
        let target = state.imports.get(namespace).copied().or_else(|| state.core_imports
            .get(namespace).and_then(|module| core_source_target(source_paths, module)));
        if let Some(target) = target {
            let target = source_paths.namespace_owner(target, leaf).unwrap_or(target);
            if let Some(identity) = struct_identities.get(target, leaf) {
                return Some(identity.to_owned());
            }
        }
    }
    if let Some(state) = modules.get(owner) {
        if let Some(original) = state.core_item_imports.get(name) {
            let target = core_source_target(source_paths, state.core_imports.get(name)?)?;
            let leaf = original.rsplit_once('.').map_or(original.as_str(), |(_, leaf)| leaf);
            if let Some(identity) = struct_identities.get(target, leaf) {
                return Some(identity.to_owned());
            }
        }
        if let Some(error_owner) = state.compiler_builtin_error_owner {
            if crate::Sema::Prelude::error_prelude_program().ok().is_some_and(|program| {
                program.items.iter().any(|item| matches!(item,
                    crate::AST::Item::Struct(definition) if definition.name == name))
            }) {
                return struct_identities.get(error_owner, name).map(str::to_owned);
            }
        }
    }
    let owner = source_paths.generated_type_owner(name)
        .or_else(|| source_paths.namespace_owner(owner, name)).unwrap_or(owner);
    struct_identities.get(owner, name).map(str::to_owned)
}

fn canonical_fragment_type(
    ty: &Type,
    owner: usize,
    modules: &[crate::Sema::ModuleState],
    source_paths: &jet_foundation::Names::NameLedger,
    struct_identities: &FragmentNominalIndex<'_>,
) -> Type {
    let map_name = |name: &str| {
        canonical_fragment_name(owner, name, modules, source_paths, struct_identities)
    };
    let canonical = |ty: &Type| {
        canonical_fragment_type(ty, owner, modules, source_paths, struct_identities)
    };
    match ty {
        Type::Named(name) => map_name(name).map_or_else(|| ty.clone(), Type::Named),
        Type::List(inner) => Type::List(Box::new(canonical(inner))),
        Type::Map { key, key_span, value } => Type::Map {
            key: Box::new(canonical(key)),
            key_span: *key_span,
            value: Box::new(canonical(value)),
        },
        Type::Shared(inner) => Type::Shared(Box::new(canonical(inner))),
        Type::Option(inner) => Type::Option(Box::new(canonical(inner))),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(canonical(ok)),
            err: Box::new(canonical(err)),
        },
        Type::Fn {
            params, ret, effect_bound, param_contract, call_metadata,
            return_view_provenance,
        } => Type::Fn {
            params: params.iter().map(canonical).collect(),
            ret: ret.as_ref().map(|ret| Box::new(canonical(ret))),
            effect_bound: effect_bound.clone(),
            param_contract: param_contract.clone(),
            call_metadata: call_metadata.clone(),
            return_view_provenance: return_view_provenance.clone(),
        },
        Type::Apply { name, args } => Type::Apply {
            name: map_name(name).unwrap_or_else(|| name.clone()),
            args: args.iter().map(canonical).collect(),
        },
        Type::Tuple(fields) => Type::Tuple(fields.iter()
            .map(|(name, ty)| (name.clone(), Box::new(canonical(ty)))).collect()),
        Type::FixedList { elem, len } => Type::FixedList {
            elem: Box::new(canonical(elem)), len: len.clone(),
        },
        Type::InlineRange { base, lo, hi } => Type::InlineRange {
            base: Box::new(canonical(base)), lo: *lo, hi: *hi,
        },
        Type::Tagged { marker, inner } => Type::Tagged {
            marker: marker.clone(), inner: Box::new(canonical(inner)),
        },
        Type::Union(members) => crate::AST::canonicalize_union(
            members.iter().map(canonical).collect()),
        Type::Quantity { base, dimension } => Type::Quantity {
            base: Box::new(canonical(base)), dimension: dimension.clone(),
        },
        _ => ty.clone(),
    }
}

/// The declared signature of `function` in module `owner`, with every nominal
/// spelled by its canonical identity.
fn fragment_signature(
    function: &crate::AST::Func,
    owner: usize,
    modules: &[crate::Sema::ModuleState],
    source_paths: &jet_foundation::Names::NameLedger,
    struct_identities: &FragmentNominalIndex<'_>,
) -> crate::AST::MirFragmentCoreSourceSignature {
    let canonical = |ty: &Type| {
        canonical_fragment_type(ty, owner, modules, source_paths, struct_identities)
    };
    crate::AST::MirFragmentCoreSourceSignature {
        type_params: function.type_params.iter().map(|param| param.name.clone()).collect(),
        params: function.params.iter().map(|param| {
            let ty = canonical(&param.ty);
            let ty = if param.variadic { Type::List(Box::new(ty)) } else { ty };
            (param.convention, ty)
        }).collect(),
        return_type: function.return_type.as_ref().map(canonical),
        value_return_type: canonical(&function.callable_value_return_type()),
    }
}

/// Signatures of every cross-module call module `owner` can spell, keyed by
/// (source alias or local name, function name).
type FragmentImportSignatures =
    HashMap<(String, String), crate::AST::MirFragmentCoreSourceSignature>;

/// The module-import call surface of module `owner` for a comptime fragment:
/// its `use` aliases and member-imported functions keyed to the loaded
/// non-Core-row targets (user modules and Core runtime units, keyed by loader
/// alias), and the declared signatures of those callees. Public Core modules
/// keep their own Core route.
fn fragment_module_imports(
    owner: usize,
    modules: &[crate::Sema::ModuleState],
    source_paths: &jet_foundation::Names::NameLedger,
    struct_identities: &FragmentNominalIndex<'_>,
) -> (
    HashMap<String, String>,
    HashMap<String, (String, String)>,
    FragmentImportSignatures,
) {
    let mut module_imports = HashMap::new();
    let mut imported_functions = HashMap::new();
    let mut signatures = HashMap::new();
    let Some(state) = modules.get(owner) else {
        return (module_imports, imported_functions, signatures);
    };
    let user_module = |target: usize| {
        modules.get(target).filter(|state| {
            jet_foundation::CoreModuleExports::core_source_module_by_alias(
                &state.module_alias).is_none()
        })
    };
    let signature = |function: &crate::AST::Func, target: usize| {
        fragment_signature(function, target, modules, source_paths, struct_identities)
    };
    for (alias, &target) in &state.imports {
        let Some(target_state) = user_module(target) else { continue };
        module_imports.insert(alias.clone(), target_state.module_alias.clone());
        for item in &target_state.items {
            if let crate::AST::Item::Func(function) = item {
                signatures.insert((alias.clone(), function.name.clone()), signature(function, target));
            }
        }
    }
    for (local, (name, target)) in &state.unqualified_file {
        let Some(target_state) = user_module(*target) else { continue };
        let Some(function) = target_state.items.iter().find_map(|item| match item {
            crate::AST::Item::Func(function) if function.name == *name => Some(function),
            _ => None,
        }) else { continue };
        imported_functions.insert(local.clone(), (target_state.module_alias.clone(), name.clone()));
        signatures.insert((local.clone(), name.clone()), signature(function, *target));
    }
    // Project only mentioned namespace functions and qualified function values.
    let mut collect_namespace_function = |expression: &Expr| {
        let (namespace, leaf) = match expression {
            Expr::Call(call) => call.name.rsplit_once('.')
                .map_or((None, call.name.as_str()), |(alias, name)| (Some(alias), name)),
            Expr::Ident(name, _) => (None, name.as_str()),
            Expr::Field(receiver, name, _)
            | Expr::MethodCall { receiver, method: name, .. } => {
                let Expr::Ident(alias, _) = receiver.as_ref() else { return };
                (Some(alias.as_str()), name.as_str())
            }
            _ => return,
        };
        let target = match namespace {
            Some(alias) => match state.imports.get(alias) { Some(&target) => target, None => return },
            None => owner,
        };
        let defining = source_paths.namespace_owner(target, leaf).unwrap_or(target);
        if namespace.is_none() && defining == owner { return }
        let Some(defining_state) = user_module(defining) else { return };
        let Some(function) = defining_state.items.iter().find_map(|item| match item {
            crate::AST::Item::Func(function) if function.name == leaf => Some(function),
            _ => None,
        }) else { return };
        let local = namespace.map_or_else(|| leaf.to_owned(), |alias| format!("{alias}.{leaf}"));
        if imported_functions.contains_key(&local) { return }
        signatures.insert((namespace.unwrap_or(&local).to_owned(), leaf.to_owned()),
            signature(function, defining));
        imported_functions.insert(local, (defining_state.module_alias.clone(), leaf.to_owned()));
    };
    for item in &state.items {
        match item {
            crate::AST::Item::Func(function) => {
                for statement in &function.body { statement.for_each_expr(&mut collect_namespace_function); }
                for clause in function.pre.iter().chain(&function.post) {
                    clause.cond.for_each_expr(&mut collect_namespace_function);
                    clause.message_expr.for_each_expr(&mut collect_namespace_function);
                }
                for param in &function.params {
                    if let Some(default) = &param.default { default.for_each_expr(&mut collect_namespace_function); }
                }
            }
            crate::AST::Item::Const(constant) => constant.value.for_each_expr(&mut collect_namespace_function),
            _ => {}
        }
    }
    (module_imports, imported_functions, signatures)
}

/// Every type spelling a body of user module `owner` can write, to its
/// canonical identity: its own declarations, member-imported types,
/// `alias.Type` through its imports. Already-canonical names remain unchanged.
fn fragment_module_nominals(
    owner: usize,
    modules: &[crate::Sema::ModuleState],
    source_paths: &jet_foundation::Names::NameLedger,
    struct_identities: &FragmentNominalIndex<'_>,
) -> HashMap<String, String> {
    // Canonical keys need no self→self projection: leaving a canonical name
    // unchanged is already the reader's default.
    let mut identities = HashMap::new();
    for (source, path) in source_paths.canonical_paths(owner) {
        if struct_identities.contains(&path) { identities.insert(source, path); }
    }
    for (leaf, identity) in struct_identities.module_names(owner) {
        identities.insert(leaf.to_owned(), identity.to_owned());
    }
    if let Some(state) = modules.get(owner) {
        for (alias, &target) in &state.imports {
            for (leaf, identity) in struct_identities.module_names(target) {
                identities.insert(format!("{alias}.{leaf}"), identity.to_owned());
            }
        }
    }
    identities
}

/// S47/M2: a bound lambda used only by direct local calls borrows the names it
/// writes, so every write lands on the owner. Without a written capture, it
/// owns copyable values at closure creation. Non-copyable/view captures retain
/// the conservative escape route or the direct borrow route respectively.
fn stmt_uses_name_only_as_direct_call(stmt: &Stmt, name: &str) -> bool {
    let mut nested_capture = false;
    stmt.for_each_expr(|expr| {
        let Expr::Lambda(lambda) = expr else {
            return;
        };
        if lambda.params.iter().any(|param| param.name == name)
            || (!lambda.take_names.iter().any(|(capture, _)| capture == name)
                && !lambda_body_refs_name(&lambda.body, name))
        {
            return;
        }
        nested_capture = true;
    });
    if nested_capture {
        return false;
    }

    // Remove each direct invocation, then ask the canonical capture walker
    // whether any use remains. This keeps calls nested in ordinary argument
    // expressions direct while rejecting passing, storing, field access, and
    // assignment of the callable itself.
    let mut residual = stmt.clone();
    residual.for_each_expr_mut(|expr| {
        let direct = match expr {
            Expr::Call(call) => call.name == name,
            Expr::CallValue { callee, .. } => {
                fn is_name(expr: &Expr, name: &str) -> bool {
                    match expr {
                        Expr::Ident(candidate, _) => candidate == name,
                        Expr::Paren(inner, _) => is_name(inner, name),
                        _ => false,
                    }
                }
                is_name(callee, name)
            }
            Expr::MethodCall {
                receiver, method, ..
            } if method == "call" => {
                fn is_name(expr: &Expr, name: &str) -> bool {
                    match expr {
                        Expr::Ident(candidate, _) => candidate == name,
                        Expr::Paren(inner, _) => is_name(inner, name),
                        _ => false,
                    }
                }
                is_name(receiver, name)
            }
            _ => false,
        };
        if direct {
            let span = expr.span();
            *expr = Expr::Absent(span);
        }
    });
    !stmt_refs_name(&residual, name)
}

impl<'a> Checker<'a> {
    /// Read the statement-tail frame installed by `check_block_inner`.
    /// A null frame means the caller did not provide lexical-use context, so
    /// retain the safe escaping default.
    fn bound_lambda_escapes(&self, name: &str, lambda: &Lambda) -> bool {
        if self.stmt_tail_ptr.is_null() {
            return true;
        }
        let mut saw_direct_use = false;
        let mut saw_other_use = false;
        let mut inspect_tail = |tail: &[Stmt]| {
            for stmt in tail {
                if !stmt_refs_name(stmt, name) {
                    continue;
                }
                if stmt_uses_name_only_as_direct_call(stmt, name) {
                    saw_direct_use = true;
                } else {
                    saw_other_use = true;
                }
            }
        };
        // SAFETY: these slices point into the live Program AST while checking
        // the current statement; `check_block_inner` installs and restores
        // each frame around the recursive check.
        let tail = unsafe { std::slice::from_raw_parts(self.stmt_tail_ptr, self.stmt_tail_len) };
        inspect_tail(tail);
        for &(ptr, len) in self.liveness_frames.iter().rev() {
            if !ptr.is_null() && len > 0 {
                let frame = unsafe { std::slice::from_raw_parts(ptr, len) };
                inspect_tail(frame);
            }
        }
        if saw_other_use || !saw_direct_use {
            return true;
        }

        let param_names: HashSet<String> = lambda
            .params
            .iter()
            .map(|param| param.name.clone())
            .collect();
        let take_names: HashSet<String> = lambda
            .take_names
            .iter()
            .map(|(capture, _)| capture.clone())
            .collect();
        let mut read_caps = HashSet::new();
        let mut mut_caps = HashSet::new();
        lambda_collect_captures(&lambda.body, &param_names, &mut read_caps, &mut mut_caps);
        read_caps.extend(lambda.take_names.iter().map(|(capture, _)| capture.clone()));
        // A written `:=` local is a mutable borrow of its owner; a copy at
        // creation would silently discard the write. `Shared<T>` edits go
        // through the cell, and views already borrow.
        let written_locals: HashSet<&String> = mut_caps
            .iter()
            .filter(|capture| {
                !param_names.contains(*capture)
                    && !take_names.contains(*capture)
                    && !self.is_view(capture)
                    && self
                        .lookup(capture)
                        .is_some_and(|info| info.mutable && !matches!(&info.ty, Type::Shared(_)))
            })
            .collect();
        for capture in read_caps.iter().chain(mut_caps.iter()) {
            if param_names.contains(capture) || take_names.contains(capture) {
                continue;
            }
            // A view is already a deliberate borrow window. Keep direct
            // invocation on that path; ordinary values below use the
            // copy-at-creation ownership route.
            if self.is_view(capture) {
                continue;
            }
            let cap_ty = self
                .lookup(capture)
                .map(|info| info.ty.clone())
                .or_else(|| self.const_type(capture).cloned());
            let Some(cap_ty) = cap_ty else {
                continue;
            };
            if written_locals.contains(capture) {
                continue;
            }
            if written_locals.is_empty() && self.is_cloneable_type(&cap_ty) {
                return true;
            }
            if self.is_resource_type(&cap_ty) && !mut_caps.contains(capture) {
                return true;
            }
        }
        false
    }
}
impl<'a> Checker<'a> {
    /// Snapshot the checked nominal/import rows needed by an isolated
    /// comptime fragment. The ordinary bundle context already owns these
    /// rows; the fragment must carry its own projection because it does not
    /// share that context.
    pub(super) fn checked_comptime_nominals(
        &self,
    ) -> Option<std::sync::Arc<crate::AST::MirFragmentNominalFacts>> {
        let modules = self.modules?;
        if let Some(facts) = modules.get(self.module_idx)?.checked_nominals.as_ref() {
            return Some(std::sync::Arc::clone(facts));
        }
        checked_comptime_nominals_for_context(modules, self.module_idx, self.name_ledger)
    }
}
/// A comptime projection one `ModuleState` remembers. It is closed (`None`)
/// unless a [`FragmentMemoScope`] opened it, and a scope is opened only while
/// its caller holds the module states shared and the name ledger's lookup
/// tables final (body checking), so a remembered projection cannot go stale.
/// A cloned state starts closed.
pub(crate) struct FragmentMemo<T>(std::sync::Mutex<Option<Option<std::sync::Arc<T>>>>);

impl<T> Default for FragmentMemo<T> {
    fn default() -> Self {
        Self(std::sync::Mutex::new(None))
    }
}

impl<T> Clone for FragmentMemo<T> {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl<T> FragmentMemo<T> {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Option<std::sync::Arc<T>>>> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The remembered projection, or a fresh one that an open memo keeps.
    /// An open memo stays locked while projecting, so parallel bodies of one
    /// module project it once.
    fn get_or_project(&self, project: impl FnOnce() -> T) -> std::sync::Arc<T> {
        let mut memo = self.lock();
        match &mut *memo {
            Some(Some(value)) => std::sync::Arc::clone(value),
            Some(slot) => std::sync::Arc::clone(slot.insert(std::sync::Arc::new(project()))),
            None => {
                drop(memo);
                std::sync::Arc::new(project())
            }
        }
    }
}

/// Keeps the memos it opened open, and closes them when dropped. A memo
/// already open belongs to an outer scope.
pub(crate) struct FragmentMemoScope<'m, T> {
    opened: Vec<&'m FragmentMemo<T>>,
}

impl<'m, T> FragmentMemoScope<'m, T> {
    fn open(memos: impl IntoIterator<Item = &'m FragmentMemo<T>>) -> Self {
        let opened = memos.into_iter().filter(|memo| {
            let mut memo = memo.lock();
            let closed = memo.is_none();
            if closed {
                *memo = Some(None);
            }
            closed
        }).collect();
        Self { opened }
    }
}

impl<T> Drop for FragmentMemoScope<'_, T> {
    fn drop(&mut self) {
        for memo in &self.opened {
            *memo.lock() = None;
        }
    }
}

/// Share each module's loaded-callee projection across every evaluating
/// module of one body-checking pass, which holds `states` shared and the
/// name ledger's lookup tables final.
pub(crate) fn open_fragment_module_memos(
    states: &[crate::Sema::ModuleState],
) -> FragmentMemoScope<'_, FragmentModuleRows> {
    FragmentMemoScope::open(states.iter().map(|state| &state.fragment_rows))
}

/// Share one module's evaluating-context projection across its bodies while
/// they are checked. It is closed afterwards: only this module reads it.
pub(crate) fn open_fragment_context_memo(
    state: &crate::Sema::ModuleState,
) -> FragmentMemoScope<'_, crate::AST::MirFragmentNominalFacts> {
    FragmentMemoScope::open([&state.fragment_facts])
}

/// A checked nominal row a loaded module exports to comptime fragments.
enum FragmentNominalRow {
    Enum(crate::AST::EnumDef),
    Struct(crate::AST::StructDef),
}

/// One module's projection as a module a comptime fragment loads: its source
/// row and the nominal rows it exports, by canonical identity. It does not
/// depend on the evaluating module, so every context shares it.
pub(crate) struct FragmentModuleRows {
    row_key: String,
    body: std::sync::Arc<crate::AST::MirFragmentCoreSourceModule>,
    core_source_sigs: Vec<((String, String), crate::AST::MirFragmentCoreSourceSignature)>,
    nominals: Vec<(String, FragmentNominalRow)>,
}

fn project_fragment_module(
    target: usize,
    state: &crate::Sema::ModuleState,
    modules: &[crate::Sema::ModuleState],
    source_paths: &jet_foundation::Names::NameLedger,
    struct_identities: &FragmentNominalIndex<'_>,
) -> FragmentModuleRows {
    let source_module =
        jet_foundation::CoreModuleExports::core_source_module_by_alias(&state.module_alias);
    let mut body = crate::AST::MirFragmentCoreSourceModule {
        alias: state.module_alias.clone(),
        module_identity: source_paths
            .module_identity(target)
            .unwrap_or_else(|| state.module_path.clone()),
        core_imports: state.core_imports.clone(),
        functions: HashMap::new(),
        function_signatures: HashMap::new(),
        constants: Vec::new(),
        module_imports: HashMap::new(),
        imported_functions: HashMap::new(),
        import_signatures: HashMap::new(),
        nominal_identities: HashMap::new(),
    };
    let mut core_source_sigs = Vec::new();
    for item in &state.items {
        let function = match item {
            crate::AST::Item::Func(function) => function,
            crate::AST::Item::Const(constant) => {
                body.constants.push(constant.clone());
                continue;
            }
            _ => continue,
        };
        body.functions.insert(function.name.clone(), function.clone());
        let signature = fragment_signature(function, target, modules, source_paths, struct_identities);
        body.function_signatures.insert(function.name.clone(), signature.clone());
        let Some(source) = source_module else {
            continue;
        };
        let registered_target = crate::Syntax::CORE_CALLS.iter()
            .any(|row| row.targets_source_function(source.module, &function.name));
        if !registered_target && (!function.is_pub
            || !jet_foundation::CoreModuleExports::core_source_owns(source.module, &function.name)) {
            continue;
        }
        core_source_sigs.push(((source.module.to_string(), function.name.clone()), signature));
    }
    // A Core row is keyed by its Core module path; a user file module's
    // row by its loader alias, the name its import aliases resolve to.
    let row_key = match source_module {
        Some(source) => source.module.to_string(),
        None => {
            let (module_imports, imported_functions, import_signatures) =
                fragment_module_imports(target, modules, source_paths, struct_identities);
            body.module_imports = module_imports;
            body.imported_functions = imported_functions;
            body.import_signatures = import_signatures;
            body.nominal_identities =
                fragment_module_nominals(target, modules, source_paths, struct_identities);
            state.module_alias.clone()
        }
    };
    // A Core module exposes only its public leaves; every nominal of a
    // user module is declared, since its demanded bodies may name a
    // private type.
    let source_leaf_visible = |name: &str| {
        source_module.is_none_or(|source| {
            jet_foundation::CoreModuleExports::core_leaf_kind(source.module, name).is_some()
        })
    };
    let canonicalize = |ty: &mut Type| {
        *ty = canonical_fragment_type(ty, target, modules, source_paths, struct_identities);
    };
    let mut nominals = Vec::new();
    for item in &state.items {
        let name = match item {
            crate::AST::Item::Enum(def) => &def.name,
            crate::AST::Item::Struct(def) => &def.name,
            _ => continue,
        };
        if !source_leaf_visible(name) && !matches!(item,
            crate::AST::Item::Struct(definition) if definition.compiler_builtin_error)
        {
            continue;
        }
        let Some(identity) = struct_identities.get(target, name).map(str::to_owned) else {
            continue;
        };
        let row = match item {
            crate::AST::Item::Enum(def) => {
                let mut row = enum_row_without_members(def);
                row.name = identity.clone();
                for variant in &mut row.variants {
                    match &mut variant.payload {
                        crate::AST::VariantPayload::Unit => {}
                        crate::AST::VariantPayload::Single(ty, _) => canonicalize(ty),
                        crate::AST::VariantPayload::Named(fields) => {
                            for field in fields {
                                canonicalize(&mut field.ty);
                            }
                        }
                    }
                }
                FragmentNominalRow::Enum(row)
            }
            crate::AST::Item::Struct(def) => {
                let mut row = struct_row_without_members(def);
                row.name = identity.clone();
                for field in &mut row.fields {
                    canonicalize(&mut field.ty);
                }
                FragmentNominalRow::Struct(row)
            }
            _ => continue,
        };
        nominals.push((identity, row));
    }
    FragmentModuleRows { row_key, body: std::sync::Arc::new(body), core_source_sigs, nominals }
}

/// Project every evaluating context under one loaded-module memo scope.
/// Without this scope, each context deep-copies the same reachable function
/// bodies before retaining them in its facts: memory grows with contexts times
/// reachable AST size instead of one shared body projection per loaded module.
pub(crate) fn checked_comptime_nominals_for_modules(
    modules: &[crate::Sema::ModuleState],
    name_ledger: &jet_foundation::Names::NameLedger,
) -> std::sync::Arc<[std::sync::Arc<crate::AST::MirFragmentNominalFacts>]> {
    let _modules = open_fragment_module_memos(modules);
    modules.iter().enumerate().map(|(module_idx, _)| {
        checked_comptime_nominals_for_context(modules, module_idx, name_ledger)
            .expect("module index came from the checked states")
    }).collect()
}

/// Project the checked nominal/import rows needed by an isolated comptime
/// fragment before a `Checker` exists for the current module. Derive and
/// marker expansion runs at that earlier bundle phase, but it must carry the
/// same declaration identities that body checking later supplies. While body
/// checking holds the memos open, each module projects its context once and
/// each loaded module its rows once, shared by every evaluation.
pub(crate) fn checked_comptime_nominals_for_context(
    modules: &[crate::Sema::ModuleState],
    module_idx: usize,
    name_ledger: &jet_foundation::Names::NameLedger,
) -> Option<std::sync::Arc<crate::AST::MirFragmentNominalFacts>> {
    let current = modules.get(module_idx)?;
    Some(current.fragment_facts.get_or_project(|| {
        let mut facts = project_comptime_nominals(modules, module_idx, current, name_ledger);
        facts.name_ledger = name_ledger.body_snapshot();
        facts
    }))
}

/// A struct row for fragment facts: every field but its methods, trait impls
/// and derives, which the facts never carry. Built field by field so those
/// members (whole function bodies) are never deep-cloned only to be cleared;
/// exhaustive destructuring keeps a new field from being silently dropped.
fn struct_row_without_members(def: &crate::AST::StructDef) -> crate::AST::StructDef {
    let crate::AST::StructDef {
        span, compiler_builtin_error, is_pub, is_package_pub, name, name_span, type_params, fields, state,
        methods: _, cli_bindings, trait_impls: _, derives: _, auto_derive_default, is_published_schema,
        published_schema_span, is_single_use, single_use_span, is_must_use, must_use_span, layout, layout_span,
        serde_markers, type_markers, validate_block, validate_span,
    } = def;
    crate::AST::StructDef {
        span: span.clone(), compiler_builtin_error: *compiler_builtin_error, is_pub: *is_pub,
        is_package_pub: *is_package_pub, name: name.clone(), name_span: name_span.clone(),
        type_params: type_params.clone(), fields: fields.clone(), state: state.clone(), methods: Vec::new(),
        cli_bindings: cli_bindings.clone(), trait_impls: Vec::new(), derives: Vec::new(),
        auto_derive_default: *auto_derive_default, is_published_schema: *is_published_schema,
        published_schema_span: published_schema_span.clone(), is_single_use: *is_single_use,
        single_use_span: single_use_span.clone(), is_must_use: *is_must_use, must_use_span: must_use_span.clone(),
        layout: layout.clone(), layout_span: layout_span.clone(), serde_markers: serde_markers.clone(),
        type_markers: type_markers.clone(), validate_block: validate_block.clone(), validate_span: validate_span.clone(),
    }
}

/// An enum row for fragment facts: every field but its methods, trait impls
/// and derives (see struct_row_without_members).
fn enum_row_without_members(def: &crate::AST::EnumDef) -> crate::AST::EnumDef {
    let crate::AST::EnumDef {
        span, is_pub, is_package_pub, name, name_span, type_params, variants, methods: _, trait_impls: _,
        derives: _, auto_derive_default, is_single_use, single_use_span, is_must_use, must_use_span,
        serde_markers, type_markers, groups,
    } = def;
    crate::AST::EnumDef {
        span: span.clone(), is_pub: *is_pub, is_package_pub: *is_package_pub, name: name.clone(),
        name_span: name_span.clone(), type_params: type_params.clone(), variants: variants.clone(),
        methods: Vec::new(), trait_impls: Vec::new(), derives: Vec::new(), auto_derive_default: *auto_derive_default,
        is_single_use: *is_single_use, single_use_span: single_use_span.clone(), is_must_use: *is_must_use,
        must_use_span: must_use_span.clone(), serde_markers: serde_markers.clone(), type_markers: type_markers.clone(),
        groups: groups.clone(),
    }
}

fn project_comptime_nominals(
    modules: &[crate::Sema::ModuleState],
    module_idx: usize,
    current: &crate::Sema::ModuleState,
    name_ledger: &jet_foundation::Names::NameLedger,
) -> crate::AST::MirFragmentNominalFacts {
    let reexports = &current.reexports;
    let struct_identities = FragmentNominalIndex { modules, names: name_ledger, fragment_module: None };
    // The fragment lowers this module's own functions beside its own items,
    // so their signatures spell its nominals as those items declare them.
    let fragment_identities = FragmentNominalIndex { fragment_module: Some(module_idx), ..struct_identities };

    // Resolve source keys on demand, rather than enumerate every module's
    // package aliases anew for each isolated comptime context.
    let source_paths = name_ledger;

    let error_owner = current.compiler_builtin_error_owner;
    let mut queue = VecDeque::new();
    let mut queued = HashSet::new();
    queued.insert(module_idx);
    queue.push_back(module_idx);
    if let Some(error_owner) = current.compiler_builtin_error_owner {
        if queued.insert(error_owner) {
            queue.push_back(error_owner);
        }
    }
    for &target in current.imports.values() {
        if queued.insert(target) {
            queue.push_back(target);
        }
    }
    for (_, (_, target)) in &current.unqualified_file {
        if queued.insert(*target) {
            queue.push_back(*target);
        }
    }
    for (_, (_, target)) in reexports {
        if queued.insert(*target) {
            queue.push_back(*target);
        }
    }
    for module in current.core_imports.values() {
        if let Some(target) = core_source_target(source_paths, module) {
            if queued.insert(target) {
                queue.push_back(target);
            }
        }
    }
    // D-MOD-CYCLE1=A: a package is one namespace, so a sibling file's
    // nominals are visible here by bare name. Registration evaluates
    // compile-time items before imports resolve, so read the loader's
    // namespace instead of this module's import table.
    let siblings: Vec<_> = name_ledger.namespace_siblings(module_idx).collect();
    for &sibling in &siblings {
        if queued.insert(sibling) {
            queue.push_back(sibling);
        }
    }
    if let Some(owner) = error_owner {
        if queued.insert(owner) {
            queue.push_back(owner);
        }
    }

    let mut facts = crate::AST::MirFragmentNominalFacts {
        module_idx,
        module_identity: name_ledger.module_identity(module_idx)
            .unwrap_or_else(|| current.module_path.clone()),
        compiler_owned_default_err_identity: current.compiler_builtin_error_owner
            .and_then(|owner| name_ledger.declared_identity(owner, crate::Syntax::TYPE_ERR).map(str::to_owned)),
        ..Default::default()
    };
    let function_signatures = std::sync::Arc::make_mut(&mut facts.function_signatures);
    for item in &current.items {
        if let crate::AST::Item::Func(function) = item {
            function_signatures.insert(function.name.clone(),
                fragment_signature(function, module_idx, modules, source_paths, &fragment_identities));
        }
        if let crate::AST::Item::Enum(def) = item {
            let row = enum_row_without_members(def);
            facts.enums.insert(row.name.clone(), row);
        }
        if let crate::AST::Item::Struct(def) = item {
            let Some(identity) = struct_identities.get(module_idx, &def.name).map(str::to_owned) else {
                continue;
            };
            let mut row = struct_row_without_members(def);
            row.name = identity.clone();
            for field in &mut row.fields {
                field.ty = canonical_fragment_type(
                    &field.ty,
                    module_idx,
                    modules,
                    &source_paths,
                    &struct_identities,
                );
            }
            facts.foreign_modules.insert(identity.clone(), current.module_alias.clone());
            facts.structs.insert(identity.clone(), row);
        }
    }
    for (alias, &target) in &current.imports {
        if let Some(state) = modules.get(target) {
            facts
                .import_modules
                .insert(alias.clone(), state.module_alias.clone());
        }
    }
    for (alias, module) in &current.core_imports {
        if let Some(target) = core_source_target(source_paths, module) {
            if let Some(state) = modules.get(target) {
                facts
                    .import_modules
                    .insert(alias.clone(), state.module_alias.clone());
            }
        }
    }

    let mut bodies = HashMap::new();
    let mut visited_namespaces = HashSet::new();
    while let Some(target) = queue.pop_front() {
        let Some(state) = modules.get(target) else {
            continue;
        };
        if let Some(namespace) = name_ledger.module_namespace(target) {
            if visited_namespaces.insert(namespace) {
                for sibling in name_ledger.namespace_siblings(target) {
                    if queued.insert(sibling) { queue.push_back(sibling); }
                }
            }
        }
        let nested_targets = state
            .imports
            .values()
            .copied()
            .chain(state.unqualified_file.values().map(|(_, nested)| *nested))
            .collect::<Vec<_>>();
        for nested in nested_targets {
            if queued.insert(nested) {
                queue.push_back(nested);
            }
        }
        for module in state.core_imports.values() {
            if let Some(nested) = core_source_target(source_paths, module) {
                if queued.insert(nested) {
                    queue.push_back(nested);
                }
            }
        }
        let rows = state.fragment_rows.get_or_project(|| {
            project_fragment_module(target, state, modules, source_paths, &struct_identities)
        });
        facts.core_source_sigs.extend(rows.core_source_sigs.iter().cloned());
        bodies.insert(rows.row_key.clone(), std::sync::Arc::clone(&rows.body));
        if target == module_idx {
            // The original Prelude supplier publishes its own checked struct
            // rows (private fields and declared defaults included) with its
            // facts; every other module's own rows stay in its item set.
            if current.compiler_builtin_error_owner == Some(module_idx) {
                for (identity, row) in &rows.nominals {
                    if let FragmentNominalRow::Struct(def) = row {
                        facts.structs.entry(identity.clone()).or_insert_with(|| def.clone());
                    }
                }
            }
            continue;
        }
        for (identity, row) in &rows.nominals {
            facts.foreign_modules.insert(identity.clone(), state.module_alias.clone());
            match row {
                FragmentNominalRow::Enum(def) => {
                    facts.enums.entry(identity.clone()).or_insert_with(|| def.clone());
                }
                FragmentNominalRow::Struct(def) => {
                    facts.structs.entry(identity.clone()).or_insert_with(|| def.clone());
                }
            }
        }
    }
    facts.core_source_bodies = std::sync::Arc::new(bodies);
    let (_, imported_functions, import_signatures) =
        fragment_module_imports(module_idx, modules, source_paths, &struct_identities);
    facts.imported_functions = imported_functions;
    facts.import_signatures = import_signatures;

    let foreign_identities: HashSet<String> = facts.foreign_modules.keys().cloned().collect();
    for identity in &foreign_identities {
        facts
            .nominal_identities
            .insert(identity.clone(), identity.clone());
    }
    for (source, path) in name_ledger.canonical_paths(module_idx) {
        if foreign_identities.contains(&path) {
            facts.nominal_identities.insert(source, path);
        }
    }
    if let Some(owner) = current.compiler_builtin_error_owner.filter(|owner| *owner != module_idx) {
        if let Ok(program) = crate::Sema::Prelude::error_prelude_program() {
            for item in &program.items {
                let crate::AST::Item::Struct(definition) = item else { continue };
                if name_ledger.declaration(module_idx, &definition.name)
                    .is_some_and(|declaration| declaration.kind == "type")
                {
                    continue;
                }
                if let Some(identity) = name_ledger.declared_identity(owner, &definition.name) {
                    facts.nominal_identities.entry(definition.name.clone())
                        .or_insert_with(|| identity.to_owned());
                }
            }
        }
    }
    for (alias, &target) in &current.imports {
        for (leaf, identity) in struct_identities.module_names(target) {
            if foreign_identities.contains(identity) {
                facts
                    .nominal_identities
                    .insert(format!("{alias}.{leaf}"), identity.to_owned());
            }
        }
    }
    for (alias, module) in &current.core_imports {
        let Some(target) = core_source_target(source_paths, module) else {
            continue;
        };
        for (leaf, identity) in struct_identities.module_names(target) {
            if foreign_identities.contains(identity) {
                facts
                    .nominal_identities
                    .insert(format!("{alias}.{leaf}"), identity.to_owned());
            }
        }
    }
    for (local, original) in &current.core_item_imports {
        let Some(module) = current.core_imports.get(local) else {
            continue;
        };
        let Some(target) = core_source_target(source_paths, module) else {
            continue;
        };
        let leaf = original
            .rsplit_once('.')
            .map_or(original.as_str(), |(_, leaf)| leaf);
        if let Some(identity) = struct_identities.get(target, leaf) {
            if foreign_identities.contains(identity) {
                facts
                    .nominal_identities
                    .insert(local.clone(), identity.to_owned());
            }
        }
    }
    for (local, (original, target)) in &current.unqualified_file {
        if let Some(identity) = struct_identities.get(*target, original) {
            if foreign_identities.contains(identity) {
                facts
                    .nominal_identities
                    .insert(local.clone(), identity.to_owned());
            }
        }
    }
    for &sibling in &siblings {
        let Some(state) = modules.get(sibling) else {
            continue;
        };
        for item in state.nominal_items() {
            let name = match item {
                crate::AST::Item::Struct(def) => &def.name,
                crate::AST::Item::Enum(def) => &def.name,
                _ => continue,
            };
            if let Some(identity) = struct_identities.get(sibling, name) {
                if foreign_identities.contains(identity) {
                    facts
                        .nominal_identities
                        .entry(name.clone())
                        .or_insert_with(|| identity.to_owned());
                }
            }
        }
    }
    for (local, (original, target)) in reexports {
        if let Some(identity) = struct_identities.get(*target, original) {
            if foreign_identities.contains(identity) {
                facts
                    .nominal_identities
                    .insert(local.clone(), identity.to_owned());
            }
        }
    }

    facts
}

fn direct_fixed_constructor(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::MethodCall { receiver, method, .. }
            if matches!(&**receiver, Expr::Field(_, name, _) if name == "Fixed")
                && matches!(method.as_str(), "new" | "over")
    )
}

impl<'a> Checker<'a> {
    fn normalized_path_string_source(&self, expr: &Expr) -> Option<String> {
        let Expr::MethodCall {
            receiver: normalized,
            method,
            args,
            ..
        } = expr.without_parens()
        else {
            return None;
        };
        if method != "to_string" || !args.is_empty() {
            return None;
        }
        let Expr::MethodCall {
            receiver: path,
            method,
            args,
            recv_type,
            ..
        } = normalized.without_parens()
        else {
            return None;
        };
        if method != "normalize" || !args.is_empty() {
            return None;
        }
        if self.registry.contains("Path") {
            return None;
        }
        // `recv_type` is the normal proof.  The built-in `Path.from` shape is
        // the same typed proof when the static constructor has not carried its
        // receiver marker through a nested chain; a user-defined `Path` keeps
        // the existing registry guard and remains quiet.
        let direct_path_constructor = matches!(
            path.without_parens(),
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } if method == "from"
                && args.len() == 1
                && matches!(receiver.without_parens(), Expr::Ident(name, _) if name == "Path")
                && !self.registry.contains("Path")
        );
        if recv_type.as_deref() != Some("Path") && !direct_path_constructor {
            return None;
        }
        let start = crate::Sema::source_expr_start(path);
        let end = crate::Sema::source_expr_end(self.source, normalized)?;
        self.source.get(start..end).map(str::to_string)
    }
}

fn interrupt_callback_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(name, _) => Some(name),
        Expr::Paren(inner, _) => interrupt_callback_name(inner),
        _ => None,
    }
}

fn interrupt_callback_lambda(expr: &Expr) -> Option<&crate::AST::Lambda> {
    match expr {
        Expr::Lambda(lambda) => Some(lambda),
        Expr::Paren(inner, _) => interrupt_callback_lambda(inner),
        _ => None,
    }
}

fn contains_taskgroup(ty: &Type) -> bool {
    match ty {
        Type::Named(name) => name == Syntax::TYPE_TASKGROUP,
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::FixedList { elem: inner, .. }
        | Type::Tagged { inner, .. }
        | Type::InlineRange { base: inner, .. } => contains_taskgroup(inner),
        Type::Map { key, value, .. }
        | Type::Result {
            ok: key,
            err: value,
        } => contains_taskgroup(key) || contains_taskgroup(value),
        // Function signatures have their own direct Group parameter/return
        // checks; do not duplicate those diagnostics at the binding site.
        Type::Fn { .. } => false,
        Type::Apply { args, .. } | Type::Union(args) => args.iter().any(contains_taskgroup),
        Type::Tuple(fields) => fields.iter().any(|(_, field)| contains_taskgroup(field)),
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::TraitObject(_)
        | Type::IntN { .. }
        | Type::Float32 => false,
        Type::Quantity { .. } => false,
        Type::Measure(_) => false,
    }
}
pub(crate) fn check_meta_attr_fields(meta: &MetaAttr) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut seen_category = false;
    let mut seen_tunable = false;
    let mut seen_maturity = false;
    for field in &meta.fields {
        match field {
            MetaField::Category { value, span } => {
                if seen_category {
                    diags.push(Diagnostic::error(
                        "E0346",
                        "`#Meta` repeats `category`".to_string(),
                        "`category` has one value; writing it twice would make tooling choose between two labels".to_string(),
                        "keep one `category: \"...\"` field".to_string(),
                        Some(*span),
                    ));
                }
                seen_category = true;
                match value {
                    Expr::Str(parts, _) => match parts.as_slice() {
                        [StrPart::Lit(s)] if s.is_empty() => diags.push(Diagnostic::error(
                            "E0348",
                            "`#Meta` category cannot be empty".to_string(),
                            "Canvas groups use this text as a visible label".to_string(),
                            "write a non-empty category, e.g. `category: \"Movement\"`".to_string(),
                            Some(value.span()),
                        )),
                        [StrPart::Lit(_)] => {}
                        _ => diags.push(Diagnostic::error(
                            "E0347",
                            "`#Meta` category needs plain quoted text".to_string(),
                            "`category` is compile-time tooling data, not a runtime string"
                                .to_string(),
                            "write a string literal, e.g. `category: \"Movement\"`".to_string(),
                            Some(value.span()),
                        )),
                    },
                    _ => diags.push(Diagnostic::error(
                        "E0347",
                        "`#Meta` category needs plain quoted text".to_string(),
                        "`category` is compile-time tooling data, not a runtime value".to_string(),
                        "write a string literal, e.g. `category: \"Movement\"`".to_string(),
                        Some(value.span()),
                    )),
                }
            }
            MetaField::Tunable { span } => {
                if seen_tunable {
                    diags.push(Diagnostic::error(
                        "E0346",
                        "`#Meta` repeats `tunable`".to_string(),
                        "`tunable` is a flag; writing it twice does not add meaning".to_string(),
                        "keep one `tunable` field".to_string(),
                        Some(*span),
                    ));
                }
                seen_tunable = true;
            }
            MetaField::Maturity { value, span } => {
                if seen_maturity {
                    diags.push(Diagnostic::error(
                        "E0346",
                        "`#Meta` repeats `maturity`".to_string(),
                        "a declaration has one maturity value".to_string(),
                        "keep one `maturity: .Experimental`, `.Tested`, or `.Hardened` field"
                            .to_string(),
                        Some(*span),
                    ));
                }
                seen_maturity = true;
                let valid = matches!(value,
                    Expr::EnumLit { type_name, variant, args, .. }
                        if type_name.is_empty()
                            && args.is_empty()
                            && matches!(variant.as_str(),
                                Syntax::MARKER_EXPERIMENTAL
                                    | Syntax::MARKER_TESTED
                                    | Syntax::MARKER_HARDENED));
                if !valid {
                    diags.push(Diagnostic::error(
                        "E0352",
                        "`#Meta` maturity needs a known maturity value".to_string(),
                        "maturity metadata is a closed documentation scale".to_string(),
                        "write `maturity: .Experimental`, `.Tested`, or `.Hardened`".to_string(),
                        Some(value.span()),
                    ));
                }
            }
            MetaField::Unknown { name, span, .. } => {
                let suggestion = if edit_distance(name, Syntax::META_FIELD_CATEGORY) <= 2 {
                    Some(Syntax::META_FIELD_CATEGORY)
                } else if edit_distance(name, Syntax::META_FIELD_TUNABLE) <= 2 {
                    Some(Syntax::META_FIELD_TUNABLE)
                } else if edit_distance(name, Syntax::META_FIELD_MATURITY) <= 2 {
                    Some(Syntax::META_FIELD_MATURITY)
                } else {
                    None
                };
                // Borrow the suggestion for the prose so the edit below can
                // still own it.
                let fix = match suggestion.as_ref() {
                    Some(suggestion) => format!("did you mean `{suggestion}`?"),
                    None => format!(
                        "use one of `{}`, `{}`, `{}`",
                        Syntax::META_FIELD_CATEGORY,
                        Syntax::META_FIELD_TUNABLE,
                        Syntax::META_FIELD_MATURITY
                    ),
                };
                let mut diagnostic = Diagnostic::error(
                    "E0345",
                    format!("`#Meta` does not have a `{}` field", name),
                    "`#Meta` fields are owner-ratified tooling metadata".to_string(),
                    fix,
                    Some(*span),
                );
                if let Some(suggestion) = suggestion {
                    diagnostic = diagnostic.with_edit(TextEdit {
                        span: *span,
                        new_text: suggestion.to_string(),
                    });
                }
                diags.push(diagnostic);
            }
        }
    }
    diags
}

impl<'a> Checker<'a> {
    /// Can codegen write this folded value back out as Rust?
    ///
    /// `CtValue::serialize` renders a struct or enum as `__jet_<Name> { … }`,
    /// which only names a real Rust type when `<Name>` is a user-declared type.
    /// The comptime evaluator also models builtins as structs with an internal
    /// field encoding, and those have no such Rust type. A container is only
    /// emittable when everything inside it is.
    /// As `ct_value_is_emittable`, and additionally that the value's literal
    /// form has the Rust type this binding declares. `Iter<T>` lowers to
    /// `JetIter<T>` but folds to a plain list, so writing the list back out
    /// would assign a `Vec` to a `JetIter` binding.
    pub(crate) fn ct_value_fits_binding(&self, value: &crate::AST::CtValue, ty: &Type) -> bool {
        use crate::AST::{CtReport, CtValue};
        // D-PATCH1: patch literals are intentionally partial source values.
        // Their runtime carrier completes omitted fields, so a raw comptime
        // struct would erase that shape and emit missing/mismatched fields.
        if matches!(ty, Type::Named(name) if name.ends_with(".Patch")) {
            return false;
        }
        if !self.ct_value_is_emittable(value) {
            return false;
        }
        match (value, ty) {
            (CtValue::List(_) | CtValue::Bytes(_), Type::List(_) | Type::FixedList { .. }) => true,
            (CtValue::List(_) | CtValue::Bytes(_), _) => false,
            (CtValue::Map(_), Type::Map { .. }) => true,
            (CtValue::Map(_), _) => false,
            // D-FAIL-CARRIER1=A: one carrier, so a payload fits either view of
            // it; only the report says which view the value came from.
            (CtValue::Present(_), Type::Option(_) | Type::Result { .. }) => true,
            (CtValue::Present(_), _) => false,
            (CtValue::Failed(CtReport::Clean(_)), Type::Option(_)) => true,
            (CtValue::Failed(CtReport::Clean(_)), _) => false,
            (CtValue::Failed(CtReport::Told(_)), Type::Result { .. }) => true,
            (CtValue::Failed(CtReport::Told(_)), _) => false,
            // A folded Unit is a real value only when the binding is itself
            // Unit-typed. `CtValue::Unit` also stands in as a placeholder for
            // constructs the evaluator can't represent as a literal (e.g. a
            // `scope.guard`/`on_commit`/`on_rollback` closure registration —
            // eval/exprs.rs pushes the guard and returns `Ok(CtValue::Unit)`
            // because real execution drains it procedurally at scope exit).
            // Accepting that placeholder as a fold for a non-Unit binding
            // would serialize a bare `()` in place of the real construct.
            (CtValue::Unit, Type::Named(n)) if n == "Unit" => true,
            (CtValue::Unit, _) => false,
            _ => true,
        }
    }

    pub(crate) fn ct_value_is_emittable(&self, value: &crate::AST::CtValue) -> bool {
        use crate::AST::{CtReport, CtValue};
        match value {
            CtValue::Struct { type_name, fields } => {
                self.registry.is_user_struct(type_name)
                    // D-MIGRATE1: a `#PublishedSchema` struct's generated Rust
                    // type carries a hidden `__jet_unknown_fields` carrier the
                    // comptime value does not model. Serializing the fold would
                    // emit a literal missing that field (rustc E0063 — an I2
                    // internal compiler error), so decline and let the ordinary
                    // runtime literal path build the value (it injects the
                    // carrier).
                    && !self.registry.is_published_schema_struct(type_name)
                    && fields
                        .iter()
                        .all(|(_, field)| self.ct_value_is_emittable(field))
            }
            CtValue::Enum {
                type_name, args, ..
            } => {
                self.registry.is_user_enum(type_name)
                    && args.iter().all(|(_, arg)| self.ct_value_is_emittable(arg))
            }
            CtValue::List(items) => items.iter().all(|item| self.ct_value_is_emittable(item)),
            CtValue::Map(entries) => entries
                .values()
                .all(|entry| self.ct_value_is_emittable(entry)),
            CtValue::Present(inner) | CtValue::Failed(CtReport::Told(inner)) => {
                self.ct_value_is_emittable(inner)
            }
            // A closure has no literal form at all.
            CtValue::Closure(_) => false,
            _ => true,
        }
    }

    pub(crate) fn check_meta_attr(&mut self, meta: &mut MetaAttr) {
        if let Some(mut marker) = self.take_rule_fact(Syntax::MARKER_META, meta.span) {
            let Some(arguments) = self.validate_rule_signature(&mut marker) else {
                return;
            };
            let mut normalized = Vec::with_capacity(meta.fields.len());
            for (source_index, mut field) in meta.fields.drain(..).enumerate() {
                let parameter = arguments
                    .bindings
                    .iter()
                    .find(|binding| binding.source_index == source_index)
                    .and_then(|binding| binding.parameter_index);
                match (
                    parameter,
                    arguments.constant_for_source(source_index),
                    &mut field,
                ) {
                    (
                        Some(0),
                        Some(crate::Comptime::CtValue::Str(value)),
                        MetaField::Category {
                            value: expression, ..
                        },
                    ) => {
                        let span = expression.span();
                        *expression = Expr::Str(vec![StrPart::Lit(value.clone())], span);
                        normalized.push(field);
                    }
                    (Some(1), Some(crate::Comptime::CtValue::Bool(true)), _) => {
                        normalized.push(MetaField::Tunable {
                            span: marker.args[source_index].span(),
                        })
                    }
                    (Some(1), Some(crate::Comptime::CtValue::Bool(false)), _) => {}
                    _ => normalized.push(field),
                }
            }
            meta.fields = normalized;
        }
        self.diags.extend(check_meta_attr_fields(meta));
    }

    /// D-UNINIT-SENTINEL2: `name := Type.{ uninit }` — gate on `use core.mem`,
    /// restrict to plain-data types (E0423), declare the binding, and record
    /// it as not-yet-written so the dataflow can prove write-before-read
    /// (E0420). Reuses D-UNINIT1's engine unchanged; only the surface syntax
    /// moved (SENTINEL1 annotated RHS → SENTINEL2 typed-literal body).
    pub(crate) fn check_uninit_binding(&mut self, b: &mut Binding) {
        let has_mem = self
            .core_imports
            .values()
            .any(|m| m == Syntax::CORE_MEM_MODULE);
        if !has_mem {
            self.diags.push(Diagnostic::error(
                "E0424",
                format!("`{}` needs the low-level memory tier", Syntax::KW_UNINIT),
                format!(
                    "`{}` skips the automatic zero-fill — an expert-tier operation",
                    Syntax::KW_UNINIT
                ),
                format!(
                    "add `use {}` at the top of this file to opt in",
                    Syntax::CORE_MEM_MODULE
                ),
                Some(b.name_span),
            ));
        }
        let (ty, ty_span) = match (&b.ty, b.ty_span) {
            (Some(t), Some(s)) => (self.resolve_type(t.clone()), s),
            _ => return,
        };
        if let Some(slot) = b.ty.as_mut() {
            *slot = ty.clone();
        }
        self.check_declared_type(&ty, ty_span);
        if !is_pod_uninit_type(&ty) {
            self.diags.push(Diagnostic::error(
                    "E0423",
                    format!("`{}` needs a plain-data type", Syntax::KW_UNINIT),
                    format!(
                        "`{}` may own heap memory or need cleanup, so leaving it uninitialized is unsafe",
                        ty.show()
                    ),
                    "use plain data — a number, `Bool`, `Char`, `U8`, or a fixed array of those (e.g. `[4096]U8`)".to_string(),
                    Some(ty_span),
                ));
        }
        let state = match &ty {
            Type::FixedList { len, .. } => super::super::UninitState::fixed(len.require_literal()),
            _ => super::super::UninitState::scalar(),
        };
        self.declare(
            &b.name,
            b.name_span,
            LocalInfo {
                def_span: b.name_span,
                binding_sigil_span: b.sigil_span,
                ty,
                mutable: true,
                param_conv: None,
                decl_loop_depth: self.loop_depth,
                interrupt_sendable: false,
                reactive_local: false,
                reactive_shared: false,
                single_use_span: None,
                invalid: false,
            },
        );
        self.flow.uninit.set(&b.name, state);
    }

    /// A write-convention argument is not a definite-initialization proof:
    /// Jet has no callee contract that guarantees one scalar, or every fixed
    /// slot, was written. Keep the state and reject the unproved handoff.
    pub(crate) fn clear_uninit_mut_args(&mut self, args: &[CallArg]) {
        if self.flow.uninit.is_empty() {
            return;
        }
        for arg in args {
            if arg.convention == AccessConvention::Write {
                if let Expr::Ident(n, span) = &arg.expr {
                    if self.flow.uninit.contains(n) {
                        self.diags.push(Diagnostic::error(
                                "E0420",
                                format!("`{n}` may be read before it is given a value"),
                                format!(
                                    "passing `{n}` with write access does not prove that the callee initializes it"
                                ),
                                format!(
                                    "write every value in `{n}` here before passing it to another function"
                                ),
                                Some(*span),
                            ));
                    }
                }
            }
        }
    }

    /// D-CHOOSE-TEST1=A: lower a subject-first pattern test into one
    /// statement binding. The pattern validator supplies capture types;
    /// the route checker proves the failed branch leaves before captures
    /// enter scope.
    pub(crate) fn check_refutable_binding(&mut self, b: &mut Binding) {
        let Some(BindPattern::Refutable {
            mut pattern,
            mut fallback,
            names,
            span,
            synthesized,
        }) = b.pattern.clone()
        else {
            return;
        };
        let subject_span = b.init.span();
        let subject = std::mem::replace(&mut b.init, Expr::Absent(subject_span));
        let mut subject = Box::new(subject);
        let (subject_ty, bindings) =
            self.check_pattern_test_typed(&mut subject, &mut pattern, span, true);
        b.init = *subject;
        let Some(subject_ty) = subject_ty else {
            for name in names.iter() {
                self.declare_bound(name.local_name(), name.span, Type::Int, false, b.sigil_span);
            }
            return;
        };
        self.check_diverging_fallback(&mut fallback, &subject_ty, span);
        // A refutable binding stores the checked subject type, not a capture
        // type. Lowering must carry this nominal owner into its TPattern.
        b.ty = Some(subject_ty);
        // Rejected text/byte holes have no binding fact. Do not install a
        // recovery declaration over the value E0118 already protects.
        let text_or_byte_holes =
            matches!(&pattern, Pattern::StrMatch { .. } | Pattern::BinMatch { .. });
        if let Some(BindPattern::Refutable {
            pattern: stored_pattern,
            fallback: stored_fallback,
            ..
        }) = b.pattern.as_mut()
        {
            *stored_pattern = pattern;
            *stored_fallback = fallback;
        }
        for name in names.iter() {
            if text_or_byte_holes
                && !bindings.contains_key(name.local_name())
                && (self.lookup(name.local_name()).is_some()
                    || self.const_type(name.local_name()).is_some())
            {
                continue;
            }
            let ty = bindings
                .get(name.local_name())
                .or_else(|| bindings.get(&name.name))
                .cloned()
                .unwrap_or(Type::Int);
            // D-FLOWTYPE1=A: sema's rewrite of an exiting `x == None` guard
            // into `x == .Val(x) ?? { … }` refines the stable Optional `x`
            // itself, like `if x == .Val(x)` does in a branch; the payload
            // holds for the rest of this scope. A written binding that
            // reuses the subject's name is an ordinary shadow (E0118).
            let refines_subject = synthesized
                && matches!(
                    &b.init,
                    Expr::Ident(subject, _) if subject == name.local_name()
                )
                && self.is_optional_flow_refine(name.local_name(), &ty);
            if refines_subject {
                // The payload replaces the tested carrier under the same name.
                self.flow.moved.remove(name.local_name());
                self.record_optional_flow_narrow(name.local_name(), name.span, ty);
            } else {
                self.declare_bound(name.local_name(), name.span, ty, false, b.sigil_span);
            }
        }
    }

    pub(crate) fn check_binding(&mut self, b: &mut Binding) {
        if let Some(meta) = &mut b.meta {
            self.check_meta_attr(meta);
        }
        // D-FAIL-BIND1=A: a user binding with the ambient spelling remains
        // legal, but it is easy to mistake for the fallback report and is
        // shadowed by that report inside a fallible `??` fallback.
        if b.name == Syntax::AMBIENT_ERR {
            self.diags.push(Diagnostic::lint(
                    "L0511",
                    "binding `err` shadows the fallback's ambient failure report".to_string(),
                    "`err` has a special meaning inside a fallible `??` fallback, so the same name is easy to misread as the report there".to_string(),
                    "rename the binding, or use it outside a fallback".to_string(),
                    Some(b.name_span),
                ));
        }
        // D-DETACH1: record the binding name so report_unsendable can flag view-capturing tasks.
        let prev_binding_name = self.current_binding_name.take();
        self.current_binding_name = Some(b.name.clone());
        if matches!(&b.pattern, Some(BindPattern::Refutable { .. })) {
            self.check_refutable_binding(b);
            self.current_binding_name = prev_binding_name;
            return;
        }
        if b.pattern.is_some() {
            self.check_destructuring_binding(b);
            self.current_binding_name = prev_binding_name;
            return;
        }
        if b.uninit {
            self.check_uninit_binding(b);
            self.current_binding_name = prev_binding_name;
            return;
        }
        let mut annot_valid = true;
        // An unannotated binding takes its initializer's own type: the
        // expectation of an enclosing value (a value-if arm, a block tail)
        // never reaches it. Only a written annotation gives the initializer
        // an expected type.
        let saved_expected = self.expected_type.take();
        if let (Some(ty), Some(span)) = (&mut b.ty, b.ty_span) {
            let t = self.resolve_type(ty.clone());
            *ty = t.clone();
            self.expected_type = Some(t.clone());
            let before = self.diags.len();
            self.check_declared_type(&t, span);
            if self.diags.len() > before {
                annot_valid = false;
            }
        }
        if !b.mutable {
            if let Expr::Ident(n, nspan) = &mut b.init {
                if let Some(info) = self.lookup(n) {
                    if !info.ty.is_scalar()
                        && matches!(
                            info.param_conv,
                            Some(AccessConvention::Read) | Some(AccessConvention::Write)
                        )
                    {
                        let source_ty = info.ty.clone();
                        let diagnostic = Diagnostic::error(
                            "E0120",
                            format!(
                                "`{}` was not moved here, so it cannot be taken with the move marker `^`",
                                n
                            ),
                            "this function has read access only and does not own the value"
                                .to_string(),
                            format!(
                                "copy it instead: `{} {} {}`",
                                b.name,
                                if b.mutable {
                                    Syntax::SIGIL_BIND_MUT
                                } else {
                                    Syntax::SIGIL_BIND_IMMUT
                                },
                                Syntax::copy_call(n)
                            ),
                            Some(*nspan),
                        );
                        self.diags.push(self.with_ownership_copy_edit(
                            diagnostic,
                            *nspan,
                            Some(&source_ty),
                        ));
                    }
                }
            }
        }
        let saved_esc = self.lambda_escapes;
        let saved_bind = self.lambda_binding.clone();
        if let Expr::Lambda(lambda) = &b.init {
            // S47: a local callable borrowed by direct calls can retain mutable
            // capture borrows; keep the owning clone only when a later use can
            // retain or move the callable.
            self.lambda_escapes = self.bound_lambda_escapes(&b.name, lambda);
            self.lambda_binding = Some(b.name.clone());
        }
        // D-META-STAGE1=B: a marked name in a compile-time binding RHS is an
        // ordinary read of a marked name; set
        // `in_comptime` before `infer()` so the RHS types in the compile-time
        // world (the evaluator runs after, independently).
        if b.is_comptime {
            self.in_comptime = true;
        }
        // D-SHAPE-PLACE1=A: a bare maximal place bound locally is a checked
        // read window, never an implicit move/copy. Decide this before
        // `infer`'s owning-position rewrite (#642).
        //
        // The field carve-out below is root-sensitive, because the two laws
        // that meet here answer for different roots:
        //
        //   * Rooted in a LOCAL owned value, `s :: p.name` is an owning
        //     position and materializes, so the generated Rust never moves
        //     a field out of its struct (`field_read_clones_instead_of_
        //     moving`, `owning_nonscalar_field_read_clones` — the latter
        //     pins the emit byte-exactly and runs it).
        //   * Rooted in a READ/WRITE PARAMETER, the projection lives inside
        //     a borrow, and spec.md:338-339 promises that a read call is
        //     "allocation-free for every non-scalar shape" while
        //     spec.md:339-340 and 438-439 say a bare `::` binding of a
        //     place — "a name followed by its maximal field, index, or
        //     range projection" — stays a read window. Materializing there
        //     allocates off the borrow and drops the loan on the owner.
        //
        // This is the same `param_conv` fact `infer_checked`'s
        // `borrowed_param_place` and `implicit_copy_target` already use to
        // separate a borrowed projection from an ordinary owning read; the
        // binding path was the one place that did not consult it.
        // Excluding `Shared<T>` is not a carve-out for convenience: under
        // D-CONC-SHARE1=A a field read on a shared handle IS a locked read
        // (`desugar_shared_field_read` rewrites it during inference), so it
        // never was a window and must not be wrapped as one.
        let borrowed_param_place =
            crate::Sema::Diagnostics::expr_root_ident(&b.init).is_some_and(|root| {
                self.lookup(root).is_some_and(|info| {
                    matches!(
                        info.param_conv,
                        Some(AccessConvention::Read) | Some(AccessConvention::Write)
                    ) && !self.type_contains_shared(&info.ty)
                })
            });
        // A `Copy` field is not a window. `type_is_copy` names the types
        // every execution tier duplicates on read (scalars, `Char`, `U8`,
        // `Complex`, `Range`), so a field of one of those has no storage to
        // keep borrowed and no allocation to avoid — the read IS the copy.
        // Keeping it as a window records the owned value as a live borrow of
        // the receiver, which is why `step :: self.step` could no longer
        // cross into a task (E1102) even though it is an `Int`: the fact,
        // not the type, refused it. `infer_checked`'s twin
        // `borrowed_param_place` already carries this `!type_is_copy` gate,
        // and before the window was restored for read-parameter projections
        // `field_read_to_clone` kept every field read out of the wrap; this
        // narrows the restoration back to the shapes a window is for.
        //
        // Only a FIELD projection or a bare name is judged here, because only
        // there is the type exact: `compound_expr_type`'s index arm answers
        // with the element type even when the index is a range (`xs[band]`),
        // which is a real window over many elements. A bare name matters for
        // write parameters: `held :: a` with `a: &Int` copies the Int, so it
        // must not stay a live read view that blocks the later `a = b`.
        let copy_field_read = match &b.init {
            Expr::Field(..) => self
                .compound_expr_type(&b.init)
                .is_some_and(|ty| crate::Sema::Diagnostics::type_is_copy(&ty)),
            Expr::Ident(name, _) => self
                .lookup(name)
                .is_some_and(|info| crate::Sema::Diagnostics::type_is_copy(&info.ty)),
            _ => false,
        };
        if !b.mutable
            && !copy_field_read
            && !matches!(b.init, Expr::Copy(..) | Expr::Place(..))
            && self.place_from_expr(&b.init).is_some()
            && (borrowed_param_place || !field_read_to_clone(&b.init, self.registry, self.imports))
        {
            let span = b.init.span();
            let inner = std::mem::replace(&mut b.init, Expr::Absent(span));
            b.init = Expr::Place(Box::new(inner), crate::AST::PlaceAccess::Read, span);
        }
        // `g :: param.field ?? fallback` reads the payload through the same
        // non-owning window as `o :: param.field` then `g :: o ?? fallback`:
        // a read view of the borrowed parameter, not an owned copy.
        if !b.mutable {
            if let Expr::OrFallback { value, .. } = &mut b.init {
                let place: &Expr = value;
                let borrowed_view = matches!(place, Expr::Field(..) | Expr::Index { .. })
                    && crate::Sema::Diagnostics::expr_root_ident(place).is_some_and(|root| {
                        self.lookup(root).is_some_and(|info| {
                            matches!(
                                info.param_conv,
                                Some(AccessConvention::Read) | Some(AccessConvention::Write)
                            ) && !self.type_contains_shared(&info.ty)
                        })
                    })
                    && self.place_from_expr(place).is_some();
                if borrowed_view {
                    let span = value.span();
                    let inner = std::mem::replace(&mut **value, Expr::Absent(span));
                    **value = Expr::Place(Box::new(inner), crate::AST::PlaceAccess::Read, span);
                }
            }
        }
        let saved_fixed_constructor = self.allow_fixed_constructor;
        self.allow_fixed_constructor = direct_fixed_constructor(&b.init);
        if let Expr::CallValue { callee, .. } = &mut b.init {
            if let Expr::Lambda(lambda) = callee.as_mut() {
                if lambda.meta.result_loop || lambda.meta.collecting_loop {
                    lambda.meta.loop_label = Some((b.name.clone(), b.name_span));
                }
            }
        }
        // Capture allocator provenance before inference when the receiver's
        // binding type is already available. Inference also records the
        // canonical allocator receiver type, so the post-inference fallback
        // below covers the producer's resolved shape.
        let arena_alloc_source = self.arena_alloc_source(&b.init);
        let diagnostics_before_init = self.diags.len();
        let invalid_reads_before_init = self.invalid_binding_reads;
        // An immutable `::` binding is a read window. Let the existing
        // field-read clone rule inspect the selected type without treating
        // the read as an owning escape; the clone is inserted immediately
        // after inference when the field is duplicable. Mutable bindings
        // and all other owning positions use the default materialization
        // path; `copies: .Explicit` keeps the refusal.
        let implicit_field_read = !b.mutable
            && !matches!(b.init, Expr::Copy(..) | Expr::Place(..))
            && field_read_to_clone(&b.init, self.registry, self.imports);
        let saved_borrow_ctx = self.borrow_ctx;
        if implicit_field_read {
            self.borrow_ctx = true;
        }
        // A distinct-range constructor's Result must stay visible until the
        // binding-level admission below can distinguish a runtime value from
        // a compile-time literal; ordinary call propagation would rewrite it
        // to `Try` first.
        let distinct_range_constructor = matches!(
            &b.init,
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } if args.len() == 1
                && Syntax::numeric_conversion_source(method).is_some()
                && matches!(
                    receiver.as_ref(),
                    Expr::Ident(type_name, _)
                        if self.registry.distinct_range(type_name).is_some()
                )
        );
        // A checked-text runtime boundary deliberately exposes its Result
        // for ordinary .Ok/.Err matching, just like a range admission.
        let checked_text_constructor = matches!(
            b.init.without_parens(),
            Expr::MethodCall { receiver, method, .. }
                if method == "from"
                    && matches!(receiver.without_parens(), Expr::Ident(name, _)
                        if self.is_checked_text_type_name(name))
        );
        // D-MEM-COPYSEM1: a value-if is not a maximal place. When it feeds a
        // non-view binding, its arm tails occupy the same owning destination
        // as the conditional result; route them through `infer_owning_value`.
        // A declared `View`/`ViewMut` result keeps the view path unchanged.
        let owning_if_value = matches!(b.init.without_parens(), Expr::If { .. })
            && !matches!(
                b.ty.as_ref(),
                Some(Type::Apply { name, .. }) if name == "View" || name == "ViewMut"
            );
        let mut it = if distinct_range_constructor || checked_text_constructor {
            self.infer_without_auto_propagation(&mut b.init)
        } else if b.mutable || owning_if_value {
            self.infer_owning_value(&mut b.init)
        } else {
            self.infer(&mut b.init)
        };
        self.borrow_ctx = saved_borrow_ctx;
        let arena_alloc_source = arena_alloc_source.or_else(|| self.arena_alloc_source(&b.init));
        // D-MEM-COPYSEM1=A (`infer_checked`'s field-read rule): a borrowed
        // window is not a value to duplicate. `rec.view_field` bound here IS
        // the view; an `Expr::Copy` would materialize an owned value under a
        // binding typed as the view and erase its owner relation (#4392).
        if implicit_field_read
            && it.as_ref().is_some_and(|ty| {
                self.is_cloneable_type(ty) && !crate::Sema::CheckerCore::is_core_view_generic(ty)
            })
        {
            let span = b.init.span();
            let inner = std::mem::replace(&mut b.init, Expr::Absent(span));
            b.init = Expr::Copy(Box::new(inner), span);
        }
        let init_has_error = self.diags[diagnostics_before_init..]
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error);
        self.allow_fixed_constructor = saved_fixed_constructor;
        if let (
            Some(Type::Result { ok, .. }),
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            },
        ) = (&it, &b.init)
        {
            if let Type::Named(type_name) = ok.as_ref() {
                if self.registry.distinct_range(type_name).is_some()
                    && matches!(receiver.as_ref(), Expr::Ident(name, _) if name == type_name)
                    && Syntax::numeric_conversion_source(method).is_some()
                    && args.len() == 1
                {
                    self.diags.push(Diagnostic::error(
                            "E0136",
                            format!("making a `{type_name}` from a runtime value can fail"),
                            "only a literal is checked at compile time; a runtime number needs the fallible form so a bad value is handled".to_string(),
                            format!("write `{type_name}.{method}(raw)?` and handle the failure"),
                            Some(args[0].expr.span()),
                        ));
                    it = Some(Type::Named(type_name.clone()));
                }
            }
        }
        let inline_target = match &it {
            Some(Type::Result { ok, .. }) => match ok.as_ref() {
                Type::InlineRange { lo, hi, .. } => Some((*lo, *hi, ok.as_ref().clone())),
                _ => None,
            },
            _ => None,
        };
        if let Some((lo, hi, target)) = inline_target {
            if let Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } = &b.init
            {
                let descriptor = matches!(receiver.as_ref(), Expr::Call(call)
                        if call.resolved_ret.as_ref().is_some_and(|ty| ty == &target));
                if descriptor
                    && method.as_str() == Syntax::conversion_method_for_source("Int")
                    && args.len() == 1
                {
                    self.diags
                        .push(crate::Sema::Diagnostics::inline_range_runtime_conversion(
                            lo,
                            hi,
                            method,
                            args[0].expr.span(),
                        ));
                    it = Some(target);
                }
            }
        }
        if b.is_comptime {
            self.in_comptime = false;
        }
        self.lambda_escapes = saved_esc;
        self.lambda_binding = saved_bind;
        self.expected_type = saved_expected;
        self.reject_borrowed_param_subplace(&mut b.init, it.as_ref(), "be stored in a binding");
        if it
            .as_ref()
            .is_some_and(crate::Sema::Diagnostics::contains_expiring_secret_loan)
        {
            self.diags.push(Diagnostic::error(
                    "E0201",
                    "an ExpiringSecret loan cannot be stored".to_string(),
                    "the callback parameter is a temporary read-only loan that ends when `.with` returns".to_string(),
                    "use the loan inside the callback and store only a non-secret result".to_string(),
                    Some(b.init.span()),
                ));
        }

        // E2502 (E2-M7): a line stream — `FileReader.lines()` / `StdinHandle
        // .lines()` — is a loop-source-only value. It may only be consumed
        // directly by `loop line in handle.lines()`; binding it to a name lets it
        // escape loop position, where there is no meaningful lowering. (Codegen
        // previously emitted a placeholder that rustc rejected — an I2 hole. This
        // moves the guarantee into sema, c109/I3.)
        if let Some(Type::Named(n)) = &it {
            if n == "FileLines" || n == "StdinLines" || n == "ProcessLines" {
                self.diags.push(Diagnostic::error(
                        "E2502",
                        "a line stream can only be used directly in a loop".to_string(),
                        "`.lines()` hands back a lazy line reader meant to be iterated in place; storing it in a name would let it leave the loop, where it has no use".to_string(),
                        format!(
                            "iterate it directly: `loop {} in handle.lines() {{ … }}`",
                            if b.name.is_empty() { "line" } else { b.name.as_str() }
                        ),
                        Some(b.init.span()),
                    ));
            }
        }

        if let Expr::Lambda(lam) = &b.init {
            if let Some(capture) = self.once_lambda_sites.get(&lam.span).cloned() {
                self.once_lambdas.insert(b.name_span, capture);
            }
            if lam.meta.escapes {
                for name in &lam.meta.mut_captures {
                    self.lambda_mut_borrow_stack
                        .last_mut()
                        .unwrap()
                        .insert(name.clone());
                }
                for name in &lam.meta.moved_captures {
                    self.mark_moved_exact(name.clone(), lam.span, "an escaping lambda");
                }
            }
        }

        // `a :: b` moves `b` when the type isn't a scalar (M2 model:
        // assignment moves). Borrowed parameters can't be moved at all.
        if let Expr::Ident(n, nspan) = &b.init {
            if let Some(info) = self.lookup(n) {
                if !info.ty.is_scalar() {
                    if info.param_conv.is_none() {
                        self.mark_moved(n.clone(), *nspan, format!("the binding `{}`", b.name));
                    }
                }
            }
        }

        // Poison a binding when inference has no usable type, including a
        // recovery type produced by an initializer type error (E0801/E3203).
        // Ownership, sendability, and contract errors still leave the
        // binding well-typed, so later reports about that name survive:
        // `.detach()` on an unsendable task (E1103/E1106, D-DETACH1) and an
        // unchecked `.join()` (E0402) are exactly such secondary reports.
        let initializer_type_error = self.diags[diagnostics_before_init..]
            .iter()
            .any(|diagnostic| matches!(diagnostic.code.as_str(), "E0801" | "E3203"));
        // A read of an invalid binding infers nothing without a new
        // diagnostic. An initializer that yields no type after such a read
        // is that earlier error's cascade: poison this binding as well, so
        // later uses of it stay silent instead of reporting `Unknown`.
        let init_read_invalid = self.invalid_binding_reads != invalid_reads_before_init;
        let mut init_type_unusable = (b.ty.is_none()
            && it.is_none()
            && (init_has_error || init_read_invalid))
            || (init_has_error && initializer_type_error)
            || (b.is_comptime && init_read_invalid);
        if b.ty.is_none() && it.is_none() && !init_has_error && !init_read_invalid {
            if let Expr::MethodCall {
                method,
                method_span,
                ..
            } = b.init.without_parens()
            {
                self.diags.push(Diagnostic::error(
                    "E0116",
                    format!("`.{method}()` doesn't hand back a value"),
                    "a call is a value only when its function declares a return type after `->`"
                        .to_string(),
                    format!(
                        "put `{method}` on its own line, or declare it as `fn {method}(…) -> Type {{ … }}`"
                    ),
                    Some(*method_span),
                ));
            }
        }
        let mut final_ty = match (&b.ty, it) {
            (Some(_), Some(actual)) if !annot_valid => actual,
            (Some(annot), Some(actual)) => {
                let annot = self.resolve_type(annot.clone());
                let mut actual = self.resolve_type(actual.clone());
                let preserve_clock_provenance = crate::Sema::Diagnostics::is_clock_type(&annot)
                    && (crate::Sema::Diagnostics::is_deterministic_clock_type(&actual)
                        || crate::Sema::Diagnostics::is_system_clock_type(&actual));
                if annot != actual && self.implicitly_convert_unit(&mut b.init, &annot, &actual) {
                    actual = annot.clone();
                }
                // D-OPT-LIFT1=A: a written `T?` annotation is a slot; a plain
                // payload fills it.
                if let Some(lifted) = self.lift_optional_slot(&annot, &actual, &mut b.init) {
                    actual = lifted;
                }
                // D-SG9: a fixed-width literal is range-checked and re-typed in
                // `infer` (E1003), so it arrives matching `annot`. A non-literal
                // width mismatch falls to E0108 below — no implicit narrowing or
                // widening between integer widths.
                if annot != actual {
                    // D-DIST1/D-DIST3 (E0128): distinct-type coercion is never implicit.
                    let distinct_name = if let Type::Named(n) = &annot {
                        if self.registry.is_distinct(n) {
                            Some(n.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(dt) = distinct_name {
                        self.diags.push(Diagnostic::error(
                                "E0128",
                                format!("a `{}` can't be used where a `{}` is expected", actual.name(), dt),
                                format!("`{}` and `{}` are different types — even though `{}` is built on `{}`, one is never accepted in place of the other", dt, actual.name(), dt, self.registry.distinct_base(&dt).map(|t| t.name()).unwrap_or_default()),
                                format!("construct a `{}`: `{}({})`", dt, dt, "expr"),
                                Some(b.init.span()),
                            ));
                    } else if let Some(diag) = crate::Sema::Diagnostics::typed_text_mismatch(
                        &annot,
                        &actual,
                        b.init.span(),
                    ) {
                        self.diags.push(diag);
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0108",
                            format!(
                                "`{}` says its type is {}, but the value is {}",
                                b.name,
                                annot.show(),
                                actual.show()
                            ),
                            "the type written after `:` must match the value".to_string(),
                            type_fix_hint(&annot, &actual),
                            Some(b.init.span()),
                        ));
                    }
                }
                if preserve_clock_provenance {
                    actual
                } else {
                    annot
                }
            }
            (Some(annot), None) => self.resolve_type(annot.clone()),
            (None, Some(actual)) => actual,
            // Keep an unresolved binding explicitly unknown after the
            // initializer's diagnostic; never invent Int and leak it into TIR.
            (None, None) => Type::Named("Unknown".to_string()),
        };
        if contains_taskgroup(&final_ty)
            && !matches!(&final_ty, Type::Named(name) if name == Syntax::TYPE_TASKGROUP)
        {
            self.diags.push(Diagnostic::error(
                    "E1110",
                    "`Group` cannot be stored inside another value".to_string(),
                    "a task group is call-stack-only spawn authority; aliases and aggregates could outlive its owner"
                        .to_string(),
                    "pass `group: Group` directly to a helper function or method and use it there"
                        .to_string(),
                    Some(b.name_span),
                ));
        }
        if b.ty.is_none() {
            b.ty = Some(final_ty.clone());
        }
        self.report_lending_view_escape(&b.init, "be stored in a binding");
        // D-DECIMAL1: default-on float-money lint for money-like binding names.
        if final_ty.is_float() && crate::Numeric::is_money_like_name(&b.name) {
            self.diags.push(Diagnostic::lint(
                "L0504",
                format!("binding `{}` looks like money but has type `Float`", b.name),
                "floating-point money loses cents on common values like `0.1 + 0.2`".to_string(),
                "use `Decimal` for exact money: `price: Decimal := Decimal(\"9.99\")`".to_string(),
                Some(b.name_span),
            ));
        }
        if !self.defer_ct_evaluation
            && b.is_comptime
            && !init_type_unusable
            && !crate::Comptime::check_build_time_io(&b.init, self.ct_base_dir, &mut self.diags)
        {
            // D-CTIO1: the path law already reported against the call.
            init_type_unusable = true;
        } else if !self.defer_ct_evaluation && b.is_comptime && !init_type_unusable {
            let is_patch_binding =
                matches!(&final_ty, Type::Named(name) if name.ends_with(".Patch"));
            let globals = self.current_ct_globals().into_owned();
            let binding_types = self.current_ct_binding_types(&globals);
            // D-META-EFFECT1: pass core_imports so the interpreter can
            // resolve effect-approved Core calls (e.g. `math.sqrt(x)`).
            // D-CTEFFECT1: pass impure context so bindings inside #Impure blocks
            // start with the gate already open.
            let mut mutated = std::collections::HashMap::new();
            let checked_nominals = self.checked_comptime_nominals();
            let ct_checked_funcs = self.ct_checked_funcs_for_evaluation();
            let folded = crate::Comptime::evaluate_owned_with_imports_opts_collecting_items(
                &b.init,
                ct_checked_funcs,
                self.ct_externs,
                self.ct_base_dir,
                &globals,
                &binding_types,
                self.core_imports,
                self.gates,
                self.ct_impure_depth,
                self.ct_items,
                checked_nominals,
                Some(&mut mutated),
            );
            self.apply_ct_mutations(mutated);
            match folded {
                Ok((v, inputs)) => {
                    // A compile-time initializer reading other `@` names infers
                    // no type while it is checked in compile-time context (the
                    // evaluator reads those names). The evaluated value is the
                    // binding's type then, the same fallback a later read of
                    // the name uses, so `@ratio :: @LIMIT /% @BASE` is an Int.
                    if matches!(&final_ty, Type::Named(name) if name == "Unknown") {
                        final_ty = v.jet_type();
                        if matches!(&b.ty, Some(Type::Named(name)) if name == "Unknown") {
                            b.ty = Some(final_ty.clone());
                        }
                    }
                    // A value codegen cannot write back out must not become
                    // literal data.
                    if self.ct_value_fits_binding(&v, &final_ty) {
                        b.ct = Some(v.clone());
                    }
                    if !is_patch_binding {
                        self.ct_scopes.last_mut().unwrap().insert(b.name.clone(), v);
                    }
                    // D-CTEFFECT1 Tier-1: accumulate embed inputs for .jet/lock.
                    self.ct_embed_inputs.extend(inputs);
                }
                Err(d) => {
                    // The checked type still exists, but this binding has no
                    // compile-time value. Keep the original evaluation failure
                    // and poison dependent reads before another fragment runs.
                    init_type_unusable = true;
                    self.diags.push(d);
                }
            }
        }
        if b.name == "_" {
            if is_task_type(&final_ty) && !self.in_taskgroup_spawn {
                self.diags
                    .push(crate::Sema::CheckerOwnership::l1101_unjoined_task(
                        "this task",
                        "discarding it into `_` skips the join before the task finishes",
                        b.name_span,
                    ));
            } else if self.type_is_single_use(&final_ty) {
                self.diags
                    .push(crate::Sema::CheckerOwnership::e0140_discarded_wildcard(
                        b.name_span,
                    ));
            }
            // An ordinary fallible call already propagates its failure in
            // statement position. Its Unit success has nothing to bind.
            if !init_has_error
                && !b.is_comptime
                && matches!(&final_ty, Type::Named(name) if name == Syntax::INTERNAL_UNIT_TYPE)
                && matches!(
                    b.init.without_parens(),
                    Expr::Try(inner, ..)
                        if matches!(
                            inner.without_parens(),
                            Expr::Call(..) | Expr::MethodCall { .. }
                        )
                )
            {
                self.diags
                    .push(Diagnostic::from_row("L0531", &[], Some(b.name_span)));
            }
            self.current_binding_name = prev_binding_name;
            return;
        }
        let binding_sendable = if let Expr::Lambda(lam) = &b.init {
            self.lambda_value_sendable(lam, &final_ty)
        } else {
            self.sendability_problem(&final_ty, true).is_none()
        };
        let interrupt_sendable = if !matches!(&final_ty, Type::Fn { .. }) {
            false
        } else {
            if let Some(name) = interrupt_callback_name(&b.init) {
                self.lookup(name)
                    .map(|info| info.param_conv.is_none() && info.interrupt_sendable)
                    .unwrap_or_else(|| {
                        self.funcs.contains_key(name)
                            || self.unqualified.contains_key(name)
                            || self.has_file_function(name)
                    })
            } else if let Some(lambda) = interrupt_callback_lambda(&b.init) {
                self.lambda_interrupt_sendable(lambda, &final_ty)
            } else {
                false
            }
        };
        // D-LIN1: a binding that owns a `#SingleUse` value carries the duty to
        // consume it exactly once. The duty transfers on `y :: x` (the move marks
        // `x` consumed via `note_move_if_direct_ident`; `y` now owns it).
        let single_use_span = if self.type_is_single_use(&final_ty) {
            Some(b.name_span)
        } else {
            None
        };
        // D-ALLOC2: `x :: arena.alloc(v)` makes `x` a scope-bound view into
        // `arena`. Flag the binding for codegen (it lowers to a `&mut T`, read
        // through a deref); register the fact after declaration so its binding
        // identity is available to E0631 / E0632. E0631: a binding whose
        // *initializer is itself a view name* (`y :: x`) would move the view to
        // a new — possibly longer-lived — binding; reject it (views are
        // non-reassignable non-escaping locals, I8).
        if arena_alloc_source.is_some() {
            b.arena_view = true;
        } else if let Expr::Ident(src, src_span) = &b.init {
            if self.is_arena_view(src) || self.is_fixed_backing_view(src) {
                self.report_view_escape(src, "be stored in another binding", *src_span);
            }
        }
        if matches!(&final_ty, Type::Named(name) if name == "Fixed") {
            if let Some(owner) = self.fixed_backing_source(&b.init) {
                self.record_fixed_backing(&b.name, owner, b.name_span);
            }
        }
        let direct_mutable_view =
            matches!(&final_ty, Type::Apply { name, .. } if name == "ViewMut");
        let transferred_view_root = direct_mutable_view
            .then(|| self.mutable_view_aggregate_root(&b.init))
            .flatten();
        // D-SHAPE-PLACE1: `x :: list[a..b]` makes `x` a scope-bound window
        // into `list`. E2305 fires the same way E0631 does for arena views —
        // rebinding a live view to a second name is itself an escape (views
        // are non-reassignable non-escaping locals, I8), not re-tracked.
        for (output_path, place, kind, access) in self.view_call_sources(&b.init) {
            let access = if direct_mutable_view && output_path.is_empty() {
                crate::Sema::ViewAccess::Write
            } else {
                access
            };
            self.record_list_view(
                &b.name,
                output_path,
                place,
                kind,
                access,
                b.name_span,
                &final_ty,
                transferred_view_root.as_deref(),
            );
        }
        self.finish_mutable_view_aggregate_transfer(
            transferred_view_root.as_deref(),
            b.init.span(),
            format!("the binding `{}`", b.name),
        );
        if let Expr::Ident(src, src_span) = &b.init {
            let _ = src_span;
            self.transfer_named_view(&b.name, src, b.name_span);
        }
        // D-MEM1 stage S5: `x :: s.trim()` / `x :: s.after(sep)` / `x ::
        // s.before(sep)` makes `x` a scope-bound string view into `s` — the
        // same E2305 reasoning as `View<T>`, reported as E2307 since `String`
        // has no distinct view type for codegen to key off (`b.string_view`
        // flags the binding itself instead). Immutable (`::`) only, matching
        // "views are non-reassignable non-escaping locals" (I8) — a `:=`
        // binding can be reassigned to an ordinary owned `String` later, and
        // codegen's `&str` place has nowhere to put that; fall back to the
        // ordinary eager/owned lowering for `:=` (unchanged pre-S5 behavior).
        if !b.mutable {
            if let Some(owner) = self.string_view_call_source(&b.init) {
                b.string_view = true;
                self.record_string_view(&b.name, owner, b.name_span);
            }
        }
        // No dedicated "rebound to another binding" check here — the general
        // E2307 check on `Expr::Ident` reads (this binding's init was already
        // inferred above) already caught `y :: d` for a live view `d`; a
        // second check here would double-report the same span.
        let concrete_unit_value = (!b.mutable)
            .then(|| self.concrete_unit_value(&b.init))
            .flatten();
        let normalized_path_source =
            (!b.mutable && !init_has_error && matches!(&final_ty, Type::String))
                .then(|| self.normalized_path_string_source(&b.init))
                .flatten();
        self.declare_with_sendability(
            &b.name,
            b.name_span,
            LocalInfo {
                def_span: b.name_span,
                binding_sigil_span: if b.is_comptime { None } else { b.sigil_span },
                ty: final_ty.clone(),
                mutable: b.mutable && !b.is_comptime,
                param_conv: None,
                decl_loop_depth: self.loop_depth,
                interrupt_sendable,
                reactive_local: b.reactive_local(),
                reactive_shared: b.reactive_shared(),
                single_use_span,
                invalid: init_type_unusable,
            },
            binding_sendable,
        );
        if let Some(source) = normalized_path_source {
            self.flow.path_strings.set_at(
                &b.name,
                self.scope_depth(),
                crate::Sema::FlowFacts::PathStringFact { source },
            );
        }
        if let Some(arena) = arena_alloc_source {
            self.record_arena_view(&b.name, arena, b.name_span);
        }
        if b.track() && !b.name.is_empty() {
            let depth = self
                .flow
                .origins
                .depth_of(&b.name)
                .unwrap_or_else(|| self.scope_depth());
            let (line, column) = crate::Diagnostics::span_line_col(&self.source, b.name_span.start);
            self.flow.origins.set_at(
                &b.name,
                depth,
                crate::Sema::FlowFacts::OriginFact {
                    tracked: true,
                    source: Some(b.name.clone()),
                    line: Some(line),
                    column: Some(column),
                    ambiguity: false,
                },
            );
        }
        // D-CONC-FREEZE1=A: publish the freeze provenance in the shared
        // flow store after the declaration owns the new binding.
        if let Some(site) = self.frozen_expr_site(&b.init) {
            let depth = self
                .binding_fact_depth(&b.name)
                .unwrap_or_else(|| self.scope_depth());
            self.flow.frozen.set_at(&b.name, depth, site);
        }
        if b.reactive_shared() && crate::Sema::CheckerInfer::is_reactive_handle_ty(&final_ty) {
            self.note_reactive_upgrade(&b.name, &final_ty, "#Shared pin");
            b.reactive_upgrade = true;
        }
        if let Some(value) = concrete_unit_value {
            self.concrete_unit_values
                .last_mut()
                .expect("binding scope exists")
                .insert(b.name.clone(), value);
        }
        self.current_binding_name = prev_binding_name;
    }

    /// S74: a `val`/`var` binding that destructures a struct (`Point { x, y }`)
    /// or a list (`[a, b]`). Each bound name is declared separately; move and
    /// mutability follow the per-name M2 rules. Struct destructuring is
    /// irrefutable (you may bind any subset of fields); list destructuring is
    /// guarded by a runtime length check in codegen, and a literal of the wrong
    /// length is caught here (E0315).
    pub(crate) fn check_destructuring_binding(&mut self, b: &mut Binding) {
        let inferred = self.infer(&mut b.init);
        let pattern = b
            .pattern
            .clone()
            .expect("destructuring binding has a pattern");
        let Some(it) = inferred else {
            // The initializer itself didn't type-check; declare error
            // placeholders so the bound names don't cascade into E0107.
            for n in pattern.names() {
                self.declare_bound(n.local_name(), n.span, Type::Int, b.mutable, b.sigil_span);
            }
            return;
        };
        let it = self.resolve_type(it);
        match &pattern {
            BindPattern::Struct {
                type_name,
                type_span,
                fields,
                rest,
                span: pat_span,
            } => {
                let display_type_name = self.display_type_name(type_name, None);
                let actual = match &it {
                    Type::Named(n) => Some(n.clone()),
                    Type::Apply { name, .. } => Some(name.clone()),
                    _ => None,
                };
                let is_struct = actual.as_deref().is_some_and(|n| {
                    let (import_ns, lookup_name) = self.struct_type_name_parts(n);
                    self.struct_owner_module(lookup_name, import_ns)
                        .and_then(|m| self.struct_fields_of(m, lookup_name))
                        .is_some()
                });
                if !is_struct {
                    self.diags.push(Diagnostic::error(
                        "E0313",
                        format!(
                            "`{} {{ … }}` can only destructure a `{}` value, but this is {}",
                            display_type_name,
                            display_type_name,
                            it.show()
                        ),
                        "destructuring with `{ }` pulls fields out of a struct value".to_string(),
                        format!(
                            "destructure a `{}`, or bind the whole value with a name",
                            display_type_name
                        ),
                        Some(*type_span),
                    ));
                    for n in pattern.names() {
                        self.declare_bound(
                            n.local_name(),
                            n.span,
                            Type::Int,
                            b.mutable,
                            b.sigil_span,
                        );
                    }
                    return;
                }
                let actual = actual.unwrap();
                let pattern_matches = match &it {
                    Type::Apply {
                        name: actual_name, ..
                    } => {
                        let (pattern_ns, pattern_name) = self.struct_type_name_parts(type_name);
                        let (actual_ns, actual_name) = self.struct_type_name_parts(actual_name);
                        pattern_name == actual_name
                            && matches!(
                                (
                                    self.struct_owner_module(pattern_name, pattern_ns),
                                    self.struct_owner_module(actual_name, actual_ns),
                                ),
                                (Some(pattern_owner), Some(actual_owner))
                                    if pattern_owner == actual_owner
                            )
                    }
                    _ => self.nominal_type_identity(&Type::Named(type_name.clone()), &it),
                };
                if !pattern_matches {
                    self.diags.push(Diagnostic::error(
                        "E0313",
                        format!(
                            "this value is a `{}`, not a `{}`",
                            actual, display_type_name
                        ),
                        "the type named before `{ }` must match the value you destructure"
                            .to_string(),
                        format!("write `{} {{ … }}` to match the value", actual),
                        Some(*type_span),
                    ));
                    for n in pattern.names() {
                        self.declare_bound(
                            n.local_name(),
                            n.span,
                            Type::Int,
                            b.mutable,
                            b.sigil_span,
                        );
                    }
                    return;
                }
                for f in fields {
                    // `field_type` resolves the field's type and reports E0302
                    // with a suggestion if the field name is unknown.
                    let fty = self.field_type(&it, &f.name, f.span).unwrap_or(Type::Int);
                    self.declare_bound(f.local_name(), f.span, fty, b.mutable, b.sigil_span);
                }
                // D-DESTRUCT1: `..` is mandatory whenever the pattern doesn't
                // name every field, and redundant when it already does.
                let (import_ns, lookup_name) = self.struct_type_name_parts(&actual);
                let total_fields = self
                    .struct_owner_module(lookup_name, import_ns)
                    .and_then(|m| self.struct_fields_of(m, lookup_name))
                    .map(|fs| fs.len());
                if let Some(total) = total_fields {
                    let partial = fields.len() < total;
                    if partial && rest.is_none() {
                        self.diags.push(Diagnostic::error(
                                "E0326",
                                format!("this pattern leaves out fields of `{}`", display_type_name),
                                "a destructure that doesn't name every field must end with `..` so the skipped fields are visible at a glance".to_string(),
                                "add `, ..` before the closing `}`, or name the remaining fields".to_string(),
                                Some(*pat_span),
                            ));
                    } else if !partial {
                        if let Some(rest_span) = rest {
                            self.diags.push(Diagnostic::error(
                                    "E0327",
                                    format!("`..` is redundant — this pattern already names every field of `{}`", display_type_name),
                                    "a trailing `..` only makes sense when some fields are left unnamed".to_string(),
                                    "remove the `..`".to_string(),
                                    Some(*rest_span),
                                ));
                        }
                    }
                }
            }
            BindPattern::List { elems, span } => {
                let (elem_ty, fixed_len) = match &it {
                    Type::List(inner) => ((**inner).clone(), None),
                    // S76: [T#N] can be destructured; E0963 if count doesn't match.
                    Type::FixedList { elem, len } => {
                        ((**elem).clone(), Some(len.require_literal()))
                    }
                    _ => {
                        self.diags.push(Diagnostic::error(
                            "E0313",
                            format!(
                                "`[ … ]` can only destructure a list, but this is {}",
                                it.show()
                            ),
                            "destructuring with `[ ]` pulls elements out of a list value"
                                .to_string(),
                            "destructure a list, or bind the whole value with a name".to_string(),
                            Some(*span),
                        ));
                        for n in pattern.names() {
                            self.declare_bound(
                                n.local_name(),
                                n.span,
                                Type::Int,
                                b.mutable,
                                b.sigil_span,
                            );
                        }
                        return;
                    }
                };
                // E0963: destructure count must match the fixed-size length.
                if let Some(fixed) = fixed_len {
                    if elems.len() as u64 != fixed {
                        self.diags.push(Diagnostic::error(
                                "E0963",
                                format!(
                                    "destructuring with {} name{}, but this fixed-size list has {} element{}",
                                    elems.len(),
                                    if elems.len() == 1 { "" } else { "s" },
                                    fixed,
                                    if fixed == 1 { "" } else { "s" }
                                ),
                                "a fixed-size list `[T#N]` has a known length — the pattern must match exactly".to_string(),
                                format!(
                                    "use {} name{} in the pattern",
                                    fixed,
                                    if fixed == 1 { "" } else { "s" }
                                ),
                                Some(*span),
                            ));
                    }
                }
                // A list literal has a known length: a mismatch is a compile
                // error rather than a runtime length failure.
                if let Expr::ListLit(items, _) = &b.init {
                    if items.len() != elems.len() {
                        self.diags.push(Diagnostic::error(
                            "E0315",
                            format!(
                                "this pattern binds {} item{}, but the list has {}",
                                elems.len(),
                                if elems.len() == 1 { "" } else { "s" },
                                items.len()
                            ),
                            "a list pattern must name exactly as many items as the list holds"
                                .to_string(),
                            format!(
                                "name {} item{} to match the list",
                                items.len(),
                                if items.len() == 1 { "" } else { "s" }
                            ),
                            Some(*span),
                        ));
                    }
                }
                for e in elems {
                    self.declare_bound(&e.name, e.span, elem_ty.clone(), b.mutable, b.sigil_span);
                }
            }
            BindPattern::Tuple { elems, span } => {
                if let Type::Apply { name, args } = &it {
                    // The applied run is the reserved generic `VjpRun<T>` even
                    // when resolution qualified its head through the
                    // non-generic source-owned `core.compute` record.
                    if Self::split_type_name(name).1 == "VJPRun" && args.len() == 1 && elems.len() == 2 {
                        let pull_ty = Type::Fn {
                            params: vec![Type::Named("Tensor".to_string())],
                            ret: Some(Box::new(args[0].clone())),
                            effect_bound: None,
                            param_contract: None,
                            call_metadata: None,
                            return_view_provenance: None,
                        };
                        let types = [Type::Named("Tensor".to_string()), pull_ty];
                        for (element, ty) in elems.iter().zip(types.iter()) {
                            self.declare_bound(
                                &element.name,
                                element.span,
                                ty.clone(),
                                b.mutable,
                                b.sigil_span,
                            );
                        }
                        return;
                    }
                }
                let Type::Tuple(fields) = &it else {
                    self.diags.push(Diagnostic::error(
                        "E0313",
                        format!(
                            "`( … )` can only destructure a tuple, but this is {}",
                            it.show()
                        ),
                        "destructuring with `( )` pulls named members out of a tuple value"
                            .to_string(),
                        "destructure a tuple, or bind the whole value with a name".to_string(),
                        Some(*span),
                    ));
                    for n in pattern.names() {
                        self.declare_bound(
                            n.local_name(),
                            n.span,
                            Type::Int,
                            b.mutable,
                            b.sigil_span,
                        );
                    }
                    return;
                };
                if elems.len() != fields.len() {
                    self.diags.push(Diagnostic::error(
                        "E0315",
                        format!(
                            "this pattern binds {} member{}, but the tuple has {}",
                            elems.len(),
                            if elems.len() == 1 { "" } else { "s" },
                            fields.len()
                        ),
                        "a tuple pattern must name exactly as many members as the tuple holds"
                            .to_string(),
                        format!(
                            "name {} member{} to match the tuple",
                            fields.len(),
                            if fields.len() == 1 { "" } else { "s" }
                        ),
                        Some(*span),
                    ));
                } else if let Expr::TupleLit(items, _, _) = &b.init {
                    if items.len() != elems.len() {
                        self.diags.push(Diagnostic::error(
                                "E0315",
                                format!(
                                    "this pattern binds {} member{}, but the tuple literal has {}",
                                    elems.len(),
                                    if elems.len() == 1 { "" } else { "s" },
                                    items.len()
                                ),
                                "a tuple pattern must name exactly as many members as the literal holds"
                                    .to_string(),
                                format!(
                                    "name {} member{} to match the tuple",
                                    items.len(),
                                    if items.len() == 1 { "" } else { "s" }
                                ),
                                Some(*span),
                            ));
                    }
                }
                for (e, (_, fty)) in elems.iter().zip(fields.iter()) {
                    self.declare_bound(&e.name, e.span, (**fty).clone(), b.mutable, b.sigil_span);
                }
            }
            BindPattern::Refutable { .. } => {
                unreachable!("refutable bindings use check_refutable_binding")
            }
        }
        // Move the initializer when it's an owned, non-scalar local (M2): the
        // whole value is consumed to produce the bound parts.
        if let Expr::Ident(n, nspan) = &b.init {
            if let Some(info) = self.lookup(n) {
                if !info.ty.is_scalar() && info.param_conv.is_none() {
                    self.mark_moved(n.clone(), *nspan, "the destructuring binding");
                }
            }
        }
    }
}
