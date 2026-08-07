use super::*;
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{
    CodeModule, ConstAttr, EnumDef, EnumLitArg, Expr, ForKind, Func, GenericModuleDef,
    GenericModuleParam, ImportKind, Item, LValue, LambdaBody, ModuleAliasDef, ModuleArg, OrFallback,
    Pattern, ProgramBundle, RustConstKind, Stmt, StrPart, StructPatField, Type, VariantPayload,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod GenericModules;
mod InlineCalls;
mod Outputs;
mod Validation;

pub(crate) use InlineCalls::{mangle_inline_sibling_calls, rewrite_inline_calls_stmts};

pub(crate) use GenericModules::expand_generic_module_aliases;
pub use GenericModules::specialize_function_types;
use GenericModules::{clone_enum, clone_struct};
use Outputs::{
    cli_entry_param_shape, is_fallible_void_entry_return, no_run_error, resolve_outputs,
    CLIEntryShape,
};
#[allow(unused_imports)]
pub(crate) use Validation::{
    check_func_body_bundle, check_module_bodies, collect_core_expr, collect_core_lvalue,
    collect_core_stmts, collect_used_core, expand_core_reachable_closure, fn_types_compatible,
    func_sig_to_fn_type, register_func_item,
};
use Validation::{
    apply_helper_layer_inference, qualified_effect_facts, taint_check_item,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IncrementalSemaStats {
    pub hits: u64,
    pub recomputes: u64,
    pub live_items: usize,
    pub live_item_bytes: usize,
}

#[derive(Clone)]
pub(super) struct CachedFunctionBody {
    pub input: Vec<u8>,
    pub function: Func,
    pub diagnostics: Vec<Diagnostic>,
    pub summaries: HashMap<String, EffectSummary>,
    pub comptime_inputs: Vec<crate::AST::ComptimeInput>,
    pub address_taken: HashSet<String>,
    pub anchors: HashMap<(String, usize, usize), DefinitionAnchorFact>,
    pub pending_diagnostics: Vec<PendingFunctionDiagnostic>,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingFunctionDiagnostic {
    pub function_key: String,
    pub function_span: Span,
    pub diagnostic: Diagnostic,
}

fn mark_failed_pending_functions(
    diagnostics: &[Diagnostic],
    pending: &[PendingFunctionDiagnostic],
    failed: &mut HashSet<String>,
) {
    for diagnostic in diagnostics {
        if !matches!(diagnostic.severity, crate::Diagnostics::Severity::Error) {
            continue;
        }
        let Some(span) = diagnostic.span else { continue };
        for candidate in pending {
            if span.start >= candidate.function_span.start
                && span.end <= candidate.function_span.end
            {
                failed.insert(candidate.function_key.clone());
            }
        }
    }
}

fn dedupe_unknown_names(diagnostics: &mut Vec<Diagnostic>) {
    let mut seen = HashSet::new();
    diagnostics.retain(|diagnostic| {
        diagnostic.code != "E0107"
            || diagnostic
                .span
                .is_none_or(|span| seen.insert((span.start, span.end)))
    });
}

fn check_fact_tags_and_states(
    bundle: &ProgramBundle,
    states: &[ModuleState],
    diags: &mut Vec<Diagnostic>,
) -> jet_foundation::Facts::FactRegistry {
    let mut facts = jet_foundation::Facts::FactRegistry::default();
    register_effect_facts(bundle, &mut facts);
    super::Taint::register_builtin_tag_facts(&mut facts);

    let mut scrubbers = HashMap::new();
    let mut returns = HashMap::new();
    let mut return_types = HashMap::new();
    let mut field_tags = HashMap::new();
    let mut field_types = HashMap::new();
    let mut known_sources = std::collections::BTreeSet::new();
    for module in &bundle.modules {
        super::Taint::collect_function_paths(
            &module.alias,
            &module.items,
            &mut known_sources,
        );
    }
    for module in &bundle.modules {
        super::Taint::collect_return_tag_facts(
            &module.items,
            &mut returns,
            &mut return_types,
        );
        super::Taint::collect_field_facts(
            &module.items,
            &mut field_tags,
            &mut field_types,
        );
        super::Taint::collect_tag_facts(
            &module.items,
            &mut facts,
            &mut scrubbers,
            &known_sources,
            diags,
            true,
        );
    }
    for module in &bundle.modules {
        super::Taint::collect_tag_facts(
            &module.items,
            &mut facts,
            &mut scrubbers,
            &known_sources,
            diags,
            false,
        );
    }
    for (index, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            taint_check_item(
                item,
                &scrubbers,
                &facts,
                &returns,
                &return_types,
                &field_tags,
                &field_types,
                &states[index].core_imports,
                diags,
            );
        }
    }

    let mut state_table = crate::Sema::StateTable::with_facts(facts);
    for module in &bundle.modules {
        state_table.add_items(&module.items);
    }
    if !state_table.is_empty() {
        for module in &bundle.modules {
            state_table.validate_declarations(&module.items, diags);
            crate::Sema::check_items_state(&module.items, &state_table, diags);
        }
    }
    state_table.into_facts()
}

fn register_effect_facts(
    bundle: &ProgramBundle,
    facts: &mut jet_foundation::Facts::FactRegistry,
) {
    use jet_foundation::Facts::FactKind;
    for effect in jet_foundation::Facts::EFFECT_ROOTS {
        facts.declare(FactKind::Effect, (*effect).to_string(), std::iter::empty());
    }
    for name in crate::Syntax::BUILTIN_EFFECT_LEAVES {
        let root = super::effect_root(name);
        let member = name.strip_prefix(root).unwrap().trim_start_matches('.');
        facts.declare_member(FactKind::Effect, root.to_string(), member.to_string());
    }
    fn collect(items: &[Item], facts: &mut jet_foundation::Facts::FactRegistry) {
        for item in items {
            match item {
                Item::EffectDecl(declaration) => {
                    let root = super::effect_root(&declaration.name);
                    if let Some(member) = declaration
                        .name
                        .strip_prefix(root)
                        .and_then(|suffix| suffix.strip_prefix('.'))
                    {
                        facts.declare_member(
                            jet_foundation::Facts::FactKind::Effect,
                            root.to_string(),
                            member.to_string(),
                        );
                    }
                }
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        collect(body, facts);
                    }
                }
                Item::GenericModule(module) => collect(&module.body, facts),
                _ => {}
            }
        }
    }
    for module in &bundle.modules {
        collect(&module.items, facts);
    }
}

fn validate_declared_effects(
    bundle: &ProgramBundle,
    facts: &jet_foundation::Facts::FactRegistry,
) -> Vec<Diagnostic> {
    fn check_name(
        name: &str,
        span: Span,
        facts: &jet_foundation::Facts::FactRegistry,
        diags: &mut Vec<Diagnostic>,
    ) {
        if let Err(suggestion) = super::resolve_effect_name(name, facts) {
            if super::parse_effect_name(name).is_some() {
                diags.push(super::undeclared_effect(
                    name,
                    suggestion.as_deref(),
                    Some(span),
                ));
            }
        }
    }
    fn check_type(
        ty: &Type,
        facts: &jet_foundation::Facts::FactRegistry,
        diags: &mut Vec<Diagnostic>,
    ) {
        match ty {
            Type::Fn {
                params,
                ret,
                effect_bound,
                ..
            } => {
                for parameter in params {
                    check_type(parameter, facts, diags);
                }
                if let Some(ret) = ret {
                    check_type(ret, facts, diags);
                }
                if let Some(names) = effect_bound {
                    for (name, span) in names {
                        if super::effect_row_var(name).is_none() {
                            check_name(name, *span, facts, diags);
                        }
                    }
                }
            }
            Type::List(inner)
            | Type::Option(inner)
            | Type::Shared(inner)
            | Type::Tagged { inner, .. }
            | Type::FixedList { elem: inner, .. } => check_type(inner, facts, diags),
            Type::Result { ok, err } => {
                check_type(ok, facts, diags);
                check_type(err, facts, diags);
            }
            Type::Apply { args, .. } | Type::Union(args) => {
                for argument in args {
                    check_type(argument, facts, diags);
                }
            }
            Type::Map { key, value, .. } => {
                check_type(key, facts, diags);
                check_type(value, facts, diags);
            }
            Type::Tuple(fields) => {
                for (_, field) in fields {
                    check_type(field, facts, diags);
                }
            }
            _ => {}
        }
    }
    fn check_stmts(
        body: &[Stmt],
        facts: &jet_foundation::Facts::FactRegistry,
        diags: &mut Vec<Diagnostic>,
    ) {
        for statement in body {
            match statement {
                Stmt::Val(binding) => {
                    if let Some(ty) = &binding.ty {
                        check_type(ty, facts, diags);
                    }
                }
                Stmt::CountedLoop { init, .. } => {
                    if let Some(ty) = &init.ty {
                        check_type(ty, facts, diags);
                    }
                }
                _ => {}
            }
            for nested in super::UnsafeObligations::nested_bodies(statement) {
                check_stmts(nested, facts, diags);
            }
        }
    }
    fn check_func(
        function: &Func,
        facts: &jet_foundation::Facts::FactRegistry,
        diags: &mut Vec<Diagnostic>,
    ) {
        if let Some(names) = &function.declared_effects {
            for (name, span) in names {
                let name = name.strip_prefix('!').unwrap_or(name);
                if super::effect_row_var(name).is_none() {
                    check_name(name, *span, facts, diags);
                }
            }
        }
        for parameter in &function.params {
            check_type(&parameter.ty, facts, diags);
        }
        if let Some(return_type) = &function.return_type {
            check_type(return_type, facts, diags);
        }
        check_stmts(&function.body, facts, diags);
    }
    fn check_items(
        items: &[Item],
        facts: &jet_foundation::Facts::FactRegistry,
        diags: &mut Vec<Diagnostic>,
    ) {
        for item in items {
            match item {
                Item::EffectDecl(declaration) => {
                    if super::parse_effect_name(&declaration.name).is_none() {
                        diags.push(super::unknown_effect(
                            &declaration.name,
                            declaration.name_span,
                        ));
                    } else if !declaration.name.contains('.') {
                        diags.push(super::effect_leaf_required(
                            &declaration.name,
                            Some(declaration.name_span),
                        ));
                    }
                }
                Item::Func(function) => check_func(function, facts, diags),
                Item::Struct(definition) => {
                    for field in &definition.fields {
                        check_type(&field.ty, facts, diags);
                    }
                    for function in &definition.methods {
                        check_func(function, facts, diags);
                    }
                    for implementation in &definition.trait_impls {
                        for function in &implementation.methods {
                            check_func(function, facts, diags);
                        }
                    }
                }
                Item::Enum(definition) => {
                    for variant in &definition.variants {
                        match &variant.payload {
                            crate::AST::VariantPayload::Unit => {}
                            crate::AST::VariantPayload::Single(ty, _) => {
                                check_type(ty, facts, diags);
                            }
                            crate::AST::VariantPayload::Named(fields) => {
                                for field in fields {
                                    check_type(&field.ty, facts, diags);
                                }
                            }
                        }
                    }
                    for function in &definition.methods {
                        check_func(function, facts, diags);
                    }
                    for implementation in &definition.trait_impls {
                        for function in &implementation.methods {
                            check_func(function, facts, diags);
                        }
                    }
                }
                Item::Impl(implementation) => {
                    for function in &implementation.methods {
                        check_func(function, facts, diags);
                    }
                }
                Item::Trait(definition) => {
                    for method in &definition.methods {
                        if let Some(names) = &method.declared_effects {
                            for (name, span) in names {
                                check_name(name, *span, facts, diags);
                            }
                        }
                        for parameter in &method.params {
                            check_type(&parameter.ty, facts, diags);
                        }
                        if let Some(return_type) = &method.return_type {
                            check_type(return_type, facts, diags);
                        }
                        if let Some(body) = &method.default_body {
                            check_stmts(body, facts, diags);
                        }
                    }
                }
                Item::Test(test) => check_stmts(&test.body, facts, diags),
                Item::Bench(bench) => check_stmts(&bench.body, facts, diags),
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        check_items(body, facts, diags);
                    }
                }
                Item::GenericModule(module) => check_items(&module.body, facts, diags),
                _ => {}
            }
        }
    }
    let mut diags = Vec::new();
    for module in &bundle.modules {
        check_items(&module.items, facts, &mut diags);
    }
    diags
}

#[derive(Default)]
pub struct IncrementalSemaCache {
    environment: Vec<u8>,
    functions: HashMap<String, CachedFunctionBody>,
    hits: u64,
    recomputes: u64,
}

impl IncrementalSemaCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> IncrementalSemaStats {
        IncrementalSemaStats {
            hits: self.hits,
            recomputes: self.recomputes,
            live_items: self.functions.len(),
            live_item_bytes: self.environment.len()
                + self
                    .functions
                    .iter()
                    .map(|(key, entry)| {
                        key.len()
                            + entry.input.len()
                            + format!("{:?}", entry.function).len()
                            + format!("{:?}", entry.diagnostics).len()
                            + format!("{:?}", entry.summaries).len()
                            + format!("{:?}", entry.comptime_inputs).len()
                            + format!("{:?}", entry.address_taken).len()
                            + format!("{:?}", entry.anchors).len()
                            + format!("{:?}", entry.pending_diagnostics).len()
                    })
                    .sum::<usize>(),
        }
    }

    pub fn clear(&mut self) {
        self.environment.clear();
        self.functions.clear();
    }

    fn begin_bundle(&mut self, bundle: &ProgramBundle) {
        let environment = incremental_environment(bundle);
        if self.environment != environment {
            self.environment = environment;
            self.functions.clear();
        }
    }

    pub(super) fn get(&mut self, key: &str, input: &[u8]) -> Option<CachedFunctionBody> {
        let hit = self
            .functions
            .get(key)
            .filter(|entry| entry.input == input)
            .cloned();
        if hit.is_some() {
            self.hits += 1;
        }
        hit
    }

    pub(super) fn store(&mut self, key: String, entry: CachedFunctionBody) {
        self.recomputes += 1;
        self.functions.insert(key, entry);
    }
}

fn incremental_environment(bundle: &ProgramBundle) -> Vec<u8> {
    let mut out = crate::CanonicalAST::canonical_fragment(&(
        bundle.entry,
        &bundle.project_root,
        bundle.active_os,
        bundle.layer_ceiling,
    ));
    out.extend(format!("{:?}", bundle.project_root).into_bytes());
    for module in &bundle.modules {
        let metadata = (
            &module.path,
            &module.display,
            &module.alias,
            &module.imports,
            module.web_target_ceiling,
            module.pub_file,
            module.no_prelude,
            &module.html_path,
            &module.policy_declarations,
        );
        out.extend(crate::CanonicalAST::canonical_fragment(&metadata));
        out.extend(format!("{metadata:?}").into_bytes());
        for item in &module.items {
            let mut item = item.clone();
            clear_callable_bodies(&mut item);
            out.extend(crate::CanonicalAST::canonical_fragment(&item));
            // Canonical AST deliberately omits locations. The exact signature
            // locations are part of IDE facts, so include the body-free Debug
            // form as a conservative span fingerprint.
            out.extend(format!("{item:?}").into_bytes());
        }
    }
    out
}

fn clear_callable_bodies(item: &mut Item) {
    let clear = |function: &mut Func| {
        function.body.clear();
        // The declaration end includes the body. Normalize it so an edit in a
        // trailing function does not invalidate unrelated earlier functions.
        function.span = function.name_span;
    };
    match item {
        Item::Func(function) => clear(function),
        Item::Struct(definition) => {
            definition.methods.iter_mut().for_each(clear);
            for implementation in &mut definition.trait_impls {
                implementation.methods.iter_mut().for_each(clear);
            }
        }
        Item::Enum(definition) => {
            definition.methods.iter_mut().for_each(clear);
            for implementation in &mut definition.trait_impls {
                implementation.methods.iter_mut().for_each(clear);
            }
        }
        Item::Impl(implementation) => implementation.methods.iter_mut().for_each(clear),
        Item::CodeModule(module) => {
            if let Some(body) = &mut module.body {
                body.iter_mut().for_each(clear_callable_bodies);
            }
        }
        _ => {}
    }
}

pub fn bundle_has_comptime_evaluation(bundle: &ProgramBundle) -> bool {
    bundle
        .modules
        .iter()
        .flat_map(|module| &module.items)
        .any(item_has_comptime_evaluation)
}

fn item_has_comptime_evaluation(item: &Item) -> bool {
    let function = |function: &Func| stmts_have_comptime_evaluation(&function.body);
    match item {
        Item::Func(value) => function(value),
        Item::Struct(value) => {
            value.methods.iter().any(function)
                || value
                    .trait_impls
                    .iter()
                    .flat_map(|implementation| &implementation.methods)
                    .any(function)
        }
        Item::Enum(value) => {
            value.methods.iter().any(function)
                || value
                    .trait_impls
                    .iter()
                    .flat_map(|implementation| &implementation.methods)
                    .any(function)
        }
        Item::Trait(value) => value
            .methods
            .iter()
            .filter_map(|method| method.default_body.as_deref())
            .any(stmts_have_comptime_evaluation),
        Item::Impl(value) => value.methods.iter().any(function),
        Item::Const(value) => value.is_comptime,
        Item::Test(value) => stmts_have_comptime_evaluation(&value.body),
        Item::Bench(value) => stmts_have_comptime_evaluation(&value.body),
        Item::CodeModule(value) => value
            .body
            .as_deref()
            .is_some_and(|body| body.iter().any(item_has_comptime_evaluation)),
        Item::ErrorConv(value) => stmts_have_comptime_evaluation(&value.body),
        Item::UserDerive(value) => stmts_have_comptime_evaluation(&value.body),
        Item::GenericModule(value) => value.body.iter().any(item_has_comptime_evaluation),
        _ => false,
    }
}

fn stmts_have_comptime_evaluation(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Stmt::Expr(value) | Stmt::Yield(value, _) => expr_has_comptime_evaluation(value),
        Stmt::Val(binding) => {
            binding.is_comptime || expr_has_comptime_evaluation(&binding.init)
        }
        Stmt::Assign { value, .. } => expr_has_comptime_evaluation(value),
        Stmt::Return(Some(value), _) => expr_has_comptime_evaluation(value),
        Stmt::BreakValue(value, _) | Stmt::BreakLabelValue(_, _, value, _) => {
            expr_has_comptime_evaluation(value)
        }
        Stmt::Return(None, _) => false,
        Stmt::While { cond, body, .. } => {
            expr_has_comptime_evaluation(cond) || stmts_have_comptime_evaluation(body)
        }
        Stmt::For { kind, body, .. } => {
            let iterable = match kind {
                ForKind::Range { start, end, step, exclusive: _ } => {
                    expr_has_comptime_evaluation(start)
                        || expr_has_comptime_evaluation(end)
                        || step.as_ref().is_some_and(expr_has_comptime_evaluation)
                }
                ForKind::In { collection, step } => {
                    expr_has_comptime_evaluation(collection)
                        || step.as_ref().is_some_and(expr_has_comptime_evaluation)
                }
            };
            iterable || stmts_have_comptime_evaluation(body)
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            expr_has_comptime_evaluation(subject)
                || arms.iter().any(|arm| {
                    expr_has_comptime_evaluation(&arm.cond)
                        || stmts_have_comptime_evaluation(&arm.body)
                })
                || else_body
                    .as_deref()
                    .is_some_and(stmts_have_comptime_evaluation)
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            init.is_comptime
                || expr_has_comptime_evaluation(&init.init)
                || expr_has_comptime_evaluation(cond)
                || step
                    .as_deref()
                    .is_some_and(|step| {
                        stmts_have_comptime_evaluation(std::slice::from_ref(step))
                    })
                || stmts_have_comptime_evaluation(body)
        }
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Off { body, .. }
        | Stmt::DebugOnly { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::ScopeMember { body, .. } => stmts_have_comptime_evaluation(body),
        Stmt::ContextBlock { fields, body, .. } => {
            fields
                .iter()
                .any(|(_, value, _)| expr_has_comptime_evaluation(value))
                || stmts_have_comptime_evaluation(body)
        }
        Stmt::ComptimeIf { .. }
        | Stmt::ComptimeSwitch { .. }
        | Stmt::ComptimeBlock { .. } => true,
        Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(..)
        | Stmt::ContinueLabel(..) => false,
    })
}

fn expr_has_comptime_evaluation(expr: &Expr) -> bool {
    let argument = |arg: &crate::AST::CallArg| expr_has_comptime_evaluation(&arg.expr);
    match expr {
        Expr::Str(parts, _) => parts.iter().any(|part| match part {
            StrPart::Interp(value, _) => expr_has_comptime_evaluation(value),
            StrPart::Lit(_) => false,
        }),
        Expr::ListLit(values, _) => values.iter().any(expr_has_comptime_evaluation),
        Expr::MemberSpread { base, .. } => expr_has_comptime_evaluation(base),
        Expr::Spread(value, _)
        | Expr::Unary(_, value, _)
        | Expr::Deref(value, _)
        | Expr::RawOf(value, _)
        | Expr::Copy(value, _)
        | Expr::Place(value, _, _)
        | Expr::Field(value, _, _)
        | Expr::Tainted(value, _, _)
        | Expr::Present(value, _)
        | Expr::Ok(value, _)
        | Expr::Err(value, _)
        | Expr::Try(value, _, _)
        | Expr::Paren(value, _)
        | Expr::IncDec { operand: value, .. }
        | Expr::PtrFromAddr { addr: value, .. } => expr_has_comptime_evaluation(value),
        Expr::MapLit(entries, _) => entries.iter().any(|(key, value)| {
            expr_has_comptime_evaluation(key) || expr_has_comptime_evaluation(value)
        }),
        Expr::Index { base, index, .. } => {
            expr_has_comptime_evaluation(base) || expr_has_comptime_evaluation(index)
        }
        Expr::Slice {
            base, start, end, range, ..
        } => {
            expr_has_comptime_evaluation(base)
                || range.as_deref().map_or_else(
                    || {
                        expr_has_comptime_evaluation(start)
                            || expr_has_comptime_evaluation(end)
                    },
                    expr_has_comptime_evaluation,
                )
        }
        Expr::Range { start, end, .. } => {
            expr_has_comptime_evaluation(start) || expr_has_comptime_evaluation(end)
        }
        Expr::Call(call) => call.args.iter().any(argument),
        Expr::Binary(_, left, right, _) => {
            expr_has_comptime_evaluation(left) || expr_has_comptime_evaluation(right)
        }
        Expr::CompareChain { operands, .. } => {
            operands.iter().any(expr_has_comptime_evaluation)
        }
        Expr::OptField { base, .. } => expr_has_comptime_evaluation(base),
        Expr::MethodCall { receiver, args, .. } => {
            expr_has_comptime_evaluation(receiver) || args.iter().any(argument)
        }
        Expr::StructLit { fields, .. } => fields
            .iter()
            .any(|(_, _, value)| expr_has_comptime_evaluation(value)),
        Expr::TypedLit { body, .. } => {
            let mut hit = false;
            body.for_each_expr(|value| {
                if expr_has_comptime_evaluation(value) {
                    hit = true;
                }
            });
            hit
        }
        Expr::EnumLit { args, .. } => args.iter().any(|arg| match arg {
            EnumLitArg::Positional(value) | EnumLitArg::Named { expr: value, .. } => {
                expr_has_comptime_evaluation(value)
            }
        }),
        Expr::PatternTest {
            subject, pattern, ..
        } => {
            expr_has_comptime_evaluation(subject)
                || match pattern {
                    Pattern::Struct { fields, .. } => fields.iter().any(|field| match field {
                        StructPatField::Value { value, .. } => {
                            expr_has_comptime_evaluation(value)
                        }
                        StructPatField::Bind { .. } => false,
                    }),
                    _ => false,
                }
        }
        Expr::OrFallback {
            value, fallback, ..
        } => {
            expr_has_comptime_evaluation(value)
                || match fallback {
                    OrFallback::Value(value) | OrFallback::Return(Some(value), _) => {
                        expr_has_comptime_evaluation(value)
                    }
                    _ => false,
                }
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            expr_has_comptime_evaluation(cond)
                || stmts_have_comptime_evaluation(then_body)
                || expr_has_comptime_evaluation(then_value)
                || stmts_have_comptime_evaluation(else_body)
                || expr_has_comptime_evaluation(else_value)
        }
        Expr::TupleLit(fields, _, _) => fields
            .iter()
            .any(|(_, value)| expr_has_comptime_evaluation(value)),
        Expr::Lambda(lambda) => match &lambda.body {
            LambdaBody::Expr(value) => expr_has_comptime_evaluation(value),
            LambdaBody::Block(body) => stmts_have_comptime_evaluation(body),
        },
        Expr::CallValue { callee, args, .. } => {
            expr_has_comptime_evaluation(callee) || args.iter().any(argument)
        }
        Expr::ComptimeSplice { .. } => true,
        Expr::Int(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::Char(..)
        | Expr::StrMatchLit(..)
        | Expr::BinMatchLit(..)
        | Expr::Ident(..)
        | Expr::UnitLit { .. }
        | Expr::Absent(_)
        | Expr::Todo { .. }
        | Expr::NoElse(_)
        | Expr::ReduceMarker(..) => false,
    }
}

fn builtin_type_registry() -> TypeRegistry {
    let zero = Span::new(0, 0);
    let variants = ["Less", "Equal", "Greater"].into_iter().map(|name| {
        (name.to_string(), (zero, VariantPayload::Unit))
    }).collect::<HashMap<_, _>>();
    let mut types = HashMap::new();
    types.insert(Syntax::TYPE_ORDERING.to_string(), TypeDef::Enum {
        variants, variant_order: vec!["Less".to_string(), "Equal".to_string(), "Greater".to_string()],
        groups: HashMap::new(), methods: HashMap::new(), single_use: false,
        must_use: false, c_layout_tag: None,
    });
    let remove_by_variants = ["Val", "Slot"].into_iter().map(|name| {
        (name.to_string(), (zero, VariantPayload::Unit))
    }).collect::<HashMap<_, _>>();
    types.insert(crate::Syntax::TYPE_REMOVE_BY.to_string(), TypeDef::Enum {
        variants: remove_by_variants,
        variant_order: vec!["Val".to_string(), "Slot".to_string()],
        groups: HashMap::new(),
        methods: HashMap::new(),
        single_use: false,
        must_use: false,
        c_layout_tag: None,
    });
    TypeRegistry {
        types,
        unit_types: HashSet::new(),
        unit_facts: HashMap::new(),
        computed_fields: HashMap::new(),
        field_defaults: HashMap::new(),
    }
}

fn unit_fact(
    family: &crate::AST::UnitFamilyDef,
    type_name: &str,
    dimension: Option<crate::AST::Dimension>,
    package: PathBuf,
) -> Option<UnitFact> {
    let affine = family.base.is_some()
        && family
            .members
            .iter()
            .any(|member| member.offset != crate::AST::UnitRatio::zero());
    family.members.iter().find_map(|member| {
        let stem = crate::AST::UnitFamilyDef::type_name(&member.name);
        let kind = if affine {
            if type_name == format!("{stem}Point") {
                QuantityKind::Point
            } else if type_name == format!("{stem}Delta") {
                QuantityKind::Delta
            } else {
                return None;
            }
        } else if type_name == stem {
            QuantityKind::Linear
        } else {
            return None;
        };
        Some(UnitFact {
            package: package.clone(),
            family: family.family.clone(),
            member: member.name.clone(),
            dimension: dimension.clone(),
            scale: member.scale.clone(),
            scale_provenance: member.scale_provenance.clone(),
            offset: if kind == QuantityKind::Point {
                member.offset.clone()
            } else {
                crate::AST::UnitRatio::zero()
            },
            kind,
        })
    })
}


fn package_scope_for(path: &Path, project_root: &Path) -> PathBuf {
    let norm_path = normalize_sem_path(path);
    let norm_root = normalize_sem_path(project_root);
    if norm_path.starts_with(&norm_root) {
        return norm_root;
    }
    norm_path
        .parent()
        .map(normalize_sem_path)
        .unwrap_or(norm_path)
}

fn normalize_sem_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// D-MOD2: inside an inline `module math { … }`, a call to a sibling function
/// `helper(x)` must lower to the mangled `math__helper`. This pre-pass rewrites

pub fn check_bundle(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts_for_output(bundle, mode, false, false, None, None).0
}

/// Check one explicitly addressed runnable Output. Sema marks that resolved
/// callable as the sole runtime entry; lower tiers consume only the fact.
pub fn check_bundle_for_output(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    output: &str,
) -> Vec<Diagnostic> {
    check_bundle_opts_for_output(bundle, mode, false, false, Some(output), None).0
}

pub fn check_bundle_for_output_opts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    output: &str,
    freestanding: bool,
    allow_impure: bool,
) -> Vec<Diagnostic> {
    check_bundle_opts_for_output(
        bundle,
        mode,
        freestanding,
        allow_impure,
        Some(output),
        None,
    )
    .0
}

/// Like `check_bundle` but also returns effect facts for D-SEMINDEX1.
pub fn check_bundle_with_effect_facts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    check_bundle_opts_for_output(bundle, mode, false, false, None, None)
}

/// Check the compiler-host build entry with the read-only compiler API enabled.
///
/// This is a separate authority from ordinary runtime/check sema. The caller
/// must have selected the package/workspace `fn build`; the checker still
/// limits the exception to that entry function in the entry module.
pub fn check_bundle_with_effect_facts_for_build(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    check_bundle_opts_for_output_with_context(
        bundle,
        mode,
        false,
        false,
        None,
        None,
        true,
    )
}

pub fn check_bundle_with_effect_facts_incremental(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    cache: &mut IncrementalSemaCache,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    check_bundle_opts_for_output(bundle, mode, false, false, None, Some(cache))
}

/// Like `check_bundle` but with extra build options (E2-M15).
pub fn check_bundle_freestanding(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts_for_output(bundle, mode, true, false, None, None).0
}

/// Like `check_bundle` but with D-CTEFFECT1 `--allow-impure` flag.
pub fn check_bundle_allow_impure(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts_for_output(bundle, mode, false, true, None, None).0
}

fn check_bundle_opts_for_output(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    allow_impure: bool,
    explicit_output: Option<&str>,
    incremental: Option<&mut IncrementalSemaCache>,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    check_bundle_opts_for_output_with_context(
        bundle,
        mode,
        freestanding,
        allow_impure,
        explicit_output,
        incremental,
        false,
    )
}

fn check_bundle_opts_for_output_with_context(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    allow_impure: bool,
    explicit_output: Option<&str>,
    incremental: Option<&mut IncrementalSemaCache>,
    allow_compiler_api: bool,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    let edition = bundle.edition.clone();
    super::Edition::with_package_edition(&edition, || {
        check_bundle_opts_for_output_inner(
            bundle,
            mode,
            freestanding,
            allow_impure,
            explicit_output,
            incremental,
            allow_compiler_api,
        )
    })
}

fn check_bundle_opts_for_output_inner(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    allow_impure: bool,
    explicit_output: Option<&str>,
    mut incremental: Option<&mut IncrementalSemaCache>,
    allow_compiler_api: bool,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    let mut diags = Vec::new();
    diags.extend(inject_units_prelude(bundle));
    diags.extend(super::Casing::validate_bundle(bundle));
    diags.extend(resolve_unit_dimensions(bundle));
    // D-OSTARGET2=B (ratified 2026-07-03): fold every `#Known if build.os == {
    // … }` switch to the arm matching this build's active OS *before* any other
    // pass sees a body — so OS-gating checks, the type-checker, and codegen only
    // meet the taken arm. Rewrites into a `#Known if` chain (reuses D-WHEN1).
    diags.extend(super::desugar_os_switches(bundle));
    // D-MIGRATE4: desugar each `change … via { (old) => … }` converter on a
    // decodable `#PublishedSchema` type into a synthetic top-level converter
    // function, so the runtime migration step (codegen) can call it. Runs before
    // registration/checking so those synthetic functions are type-checked and
    // lowered through the normal pipeline. Sets `conv_fn` on the `change` op.
    super::desugar_migrations(bundle);
    // D-SPREAD1=A: expand `prefix.[a, b]` to field lists (spliced in list
    // position) before inference sees bodies.
    super::desugar_member_spreads(bundle);
    // D-GENMOD2=A: expand module aliases into concrete CodeModules before any
    // sibling-call mangling or registration sees the items.
    expand_generic_module_aliases(bundle, &mut diags);
    // D-MOD2: rewrite inline-module sibling calls to their mangled names before any
    // registration/checking/codegen sees the bodies.
    mangle_inline_sibling_calls(bundle);
    // D-UNSAFE-OBLIG1=A: run after compile-time branch selection and generic
    // module expansion, but before registration/TIR. Assertions are checked and
    // erased here so no generated or untaken body bypasses the policy.
    diags.extend(super::UnsafeObligations::check_and_strip(bundle));
    let mut states: Vec<ModuleState> = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(module_idx, m)| ModuleState {
            module_path: m.display.clone(),
            module_alias: m.alias.clone(),
            func_spans: HashMap::new(),
            const_spans: HashMap::new(),
            import_spans: HashMap::new(),
            package_scope: package_scope_for(&m.path, &bundle.project_root),
            allow_compiler_api: allow_compiler_api && module_idx == bundle.entry,
            funcs: HashMap::new(),
            func_pub: HashMap::new(),
            func_pkg_pub: HashMap::new(),
            type_pub: HashMap::new(),
            type_pkg_pub: HashMap::new(),
            method_pub: HashMap::new(),
            method_pkg_pub: HashMap::new(),
            field_pub: HashMap::new(),
            field_pkg_pub: HashMap::new(),
            registry: builtin_type_registry(),
            consts: HashMap::new(),
            imports: HashMap::new(),
            core_imports: HashMap::new(),
            tests: HashMap::new(),
            trait_reg: TraitRegistry::default(),
            declared_states: m
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::StateDecl(state) => Some((
                        state.type_name.clone(),
                        state
                            .states
                            .iter()
                            .map(|(name, _)| name.clone())
                            .collect(),
                    )),
                    _ => None,
                })
                .collect(),
            policy_declarations: m.policy_declarations.clone(),
            rule_facts: m.rule_facts.clone(),
            code_modules: HashMap::new(),
            code_module_identities: HashMap::new(),
            unqualified: HashMap::new(),
            unqualified_file: HashMap::new(),
            reexports: HashMap::new(),
        })
        .collect();

    // Generic-instance declarations have one AST/codegen owner, while every
    // consumer registry receives the same nominal metadata. This is not a
    // declaration clone: generated Rust/TIR still sees the owner item once.
    let shared_instance_nominals: Vec<(usize, Item)> = bundle.modules.iter().enumerate().flat_map(|(owner, module)| {
        let prefixes: Vec<String> = module.items.iter().filter_map(|item| match item {
            Item::CodeModule(cm) if cm.instance_identity.is_some() =>
                Some(GenericModules::module_type_prefix(&cm.name)),
            _ => None,
        }).collect();
        module.items.iter().filter_map(move |item| match item {
            Item::Struct(def) if prefixes.iter().any(|prefix| def.name.starts_with(prefix)) => Some((owner, Item::Struct(clone_struct(def)))),
            Item::Enum(def) if prefixes.iter().any(|prefix| def.name.starts_with(prefix)) => Some((owner, Item::Enum(clone_enum(def)))),
            _ => None,
        })
    }).collect();
    for (owner, item) in &shared_instance_nominals {
        for (consumer, st) in states.iter_mut().enumerate() {
            if consumer == *owner { continue; }
            match item {
                Item::Struct(def) => {
                    register_struct(def, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub.insert(def.name.clone(), def.is_pub && !def.is_package_pub);
                    st.type_pkg_pub.insert(def.name.clone(), def.is_package_pub);
                }
                Item::Enum(def) => {
                    register_enum(def, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub.insert(def.name.clone(), def.is_pub && !def.is_package_pub);
                    st.type_pkg_pub.insert(def.name.clone(), def.is_package_pub);
                }
                _ => unreachable!(),
            }
        }
    }

    // D-METADERIVE1=A orphan law needs a bundle-wide provider view: a derive
    // may be supplied by the entry module for an imported type, or imported
    // for an entry-local type.  Clone provider bodies/helpers before mutating
    // modules so expansion can attach generated items beside the target type.
    let derive_providers: Vec<(
        usize,
        String,
        String,
        Vec<crate::AST::Stmt>,
        HashMap<String, Func>,
    )> = bundle
        .modules
        .iter()
        .enumerate()
        .flat_map(|(origin, module)| {
            let helpers: HashMap<String, Func> = module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Func(f) => Some((f.name.clone(), f.clone())),
                    _ => None,
                })
                .collect();
            module.items.iter().filter_map(move |item| match item {
                Item::UserDerive(d) => Some((
                    origin,
                    d.trait_name.clone(),
                    d.type_param.clone(),
                    d.body.clone(),
                    helpers.clone(),
                )),
                _ => None,
            })
        })
        .collect();

    // D-MARK-VOCAB1 (card #518): the dynamic half of the `#Rule` vocabulary
    // vocabulary — every `derive T.Name { … }` provider in the bundle, not
    // just this module's own, per the same bundle-wide orphan-rule view as
    // `derive_providers` above.
    let known_derive_names: HashSet<String> =
        derive_providers.iter().map(|(_, name, _, _, _)| name.clone()).collect();
    let ct_core_imports: Vec<HashMap<String, String>> = bundle
        .modules
        .iter()
        .map(|module| {
            module
                .imports
                .iter()
                .filter_map(|import| {
                    Some((import.import_alias(), import.core_module_path()?))
                })
                .collect()
        })
        .collect();
    let mut top_level_embed_inputs = Vec::new();

    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        super::Protocol::expand_module_protocols(&mut module.items, &mut diags);
        // D-DOTSCOPE1: validate contextual `.member { … }` scope statements
        // against each marker's declared vocabulary (E0614/E0615/E0616/E0617/E0618).
        diags.extend(super::ScopeMembers::check(&module.items));
        // D-FIELDPOL1: computed-field cycle check (E0338) + `self.field`
        // rewrite + synthesized getter methods, before anything else.
        process_computed_fields(&mut module.items, &mut diags);
        // D-VALIDATE1 (card #506): `validate { … }` block shape check +
        // synthesized `Type.validate(value)`, same pre-registration timing.
        process_validate_blocks(&mut module.items, &mut diags);
        // D-PATCH1: synthetic `T.Patch` before struct registration.
        inject_patchable_types(&mut module.items, &mut diags);
        let base = module
            .path
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let mut comptime_types = HashMap::new();
        eval_comptime_items(
            &mut module.items,
            &mut comptime_types,
            &base,
            &mut diags,
            &ct_core_imports[idx],
            Some(&mut top_level_embed_inputs),
        );
        super::CheckerMarkers::resolve_static_rule_products(
            module,
            &base,
            &ct_core_imports[idx],
            &mut diags,
        );
        // Card #436: `CFFI::assemble` (jetpack crate) drains every
        // `#Extern`/`#Bindgen module` out of its declaring file and re-homes
        // it in a synthetic per-lib module (`<c.lib>`) with an empty
        // registry of its own — so a struct/enum/distinct declared in an
        // ordinary file was NEVER visible to `is_c_abi_type`'s `Type::Named`
        // lookup (`c_named_type_ok`, Sema/FFI.rs), and every named type was
        // silently rejected at the C boundary regardless of its shape. Real
        // modules are always processed before any synthetic one (assemble
        // only appends), so by this iteration every preceding module's
        // registry is already fully populated; merge them once here so a
        // same-project named type resolves. Type names are unique
        // program-wide (a duplicate definition is its own error elsewhere),
        // so this union is sound.
        let ffi_named_types: Option<HashMap<String, TypeDef>> = if module
            .items
            .iter()
            .any(|i| matches!(i, Item::CModule(_)))
        {
            Some(
                states[..idx]
                    .iter()
                    .flat_map(|s| s.registry.types.iter())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        } else {
            None
        };
        let st = &mut states[idx];
        for import in &module.imports {
            if !matches!(import.kind, crate::AST::ImportKind::Unqualified { .. }) {
                st.import_spans.insert(import.import_alias(), import.alias_span);
            }
        }
        for item in &module.items {
            match item {
                Item::Func(f) => {
                    st.func_spans.insert(f.name.clone(), f.name_span);
                }
                Item::Const(c) => {
                    st.const_spans.insert(c.name.clone(), c.name_span);
                }
                _ => {}
            }
            match item {
                Item::Func(f) => register_func_item(f, st, &mut diags),
                Item::Struct(s) => {
                    register_struct(
                        s,
                        &mut st.registry,
                        &mut diags,
                        &st.funcs,
                        &st.consts,
                    );
                    st.type_pub
                        .insert(s.name.clone(), s.is_pub && !s.is_package_pub);
                    st.type_pkg_pub.insert(s.name.clone(), s.is_package_pub);
                    for fld in &s.fields {
                        st.field_pub.insert(
                            (s.name.clone(), fld.name.clone()),
                            fld.is_pub && !fld.is_package_pub,
                        );
                        st.field_pkg_pub
                            .insert((s.name.clone(), fld.name.clone()), fld.is_package_pub);
                    }
                    for m in &s.methods {
                        st.method_pub.insert(
                            (s.name.clone(), m.name.clone()),
                            m.is_pub && !m.is_package_pub,
                        );
                        st.method_pkg_pub
                            .insert((s.name.clone(), m.name.clone()), m.is_package_pub);
                    }
                }
                Item::Enum(e) => {
                    register_enum(e, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub
                        .insert(e.name.clone(), e.is_pub && !e.is_package_pub);
                    st.type_pkg_pub.insert(e.name.clone(), e.is_package_pub);
                    for m in &e.methods {
                        st.method_pub.insert(
                            (e.name.clone(), m.name.clone()),
                            m.is_pub && !m.is_package_pub,
                        );
                        st.method_pkg_pub
                            .insert((e.name.clone(), m.name.clone()), m.is_package_pub);
                    }
                }
                Item::Impl(i) => {
                    if !i.type_name.contains('.') && !st.registry.contains(&i.type_name) {
                        diags.push(Diagnostic::error(
                            "E0301",
                            format!("`impl {}` names a type that doesn't exist", i.type_name),
                            format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                            format!(
                                "define `struct {}` or `enum {}` first",
                                i.type_name, i.type_name
                            ),
                            Some(i.type_span),
                        ));
                    } else if !i.type_name.contains('.') {
                        for m in &i.methods {
                            st.method_pub.insert(
                                (i.type_name.clone(), m.name.clone()),
                                m.is_pub && !m.is_package_pub,
                            );
                            st.method_pkg_pub
                                .insert((i.type_name.clone(), m.name.clone()), m.is_package_pub);
                        }
                    }
                }
                Item::Const(c) => {
                    if let Some(meta) = &c.meta {
                        diags.extend(CheckerCore::check_meta_attr_fields(meta));
                    }
                    register_const(c, &mut st.consts, &mut diags, &st.funcs, &st.registry)
                }
                Item::Distinct(d) => {
                    register_distinct(d, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub
                        .insert(d.name.clone(), d.is_pub && !d.is_package_pub);
                    st.type_pkg_pub.insert(d.name.clone(), d.is_package_pub);
                }
                Item::TypeAlias(a) => {
                    register_type_alias(a, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub
                        .insert(a.name.clone(), a.is_pub && !a.is_package_pub);
                    st.type_pkg_pub.insert(a.name.clone(), a.is_package_pub);
                }
                Item::Tag(t) => {
                    st.type_pub
                        .insert(t.name.clone(), t.is_pub && !t.is_package_pub);
                    st.type_pkg_pub.insert(t.name.clone(), t.is_package_pub);
                }
                // D-QUAL3: a unit family lowers to one `#Numeric` distinct type
                // per member, each erasing to `Float`.
                Item::UnitFamily(uf) => {
                    let dimension = uf.resolved_dimension.clone();
                    for d in uf.distinct_defs() {
                        register_distinct(&d, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                        st.registry.unit_types.insert(d.name.clone());
                        // D-DIMENSION-OPEN1=D: a family that names a base
                        // unit relates its members by scale, dimension or not.
                        // Without a base — currency, plain tags — members stay
                        // unrelated nominal types with no conversion.
                        if let Some(owner) = uf
                            .resolved_owner
                            .as_deref()
                            .filter(|_| uf.base.is_some() || dimension.is_some())
                        {
                            if let Some(fact) = unit_fact(
                                uf,
                                &d.name,
                                dimension.clone(),
                                PathBuf::from(owner),
                            ) {
                                st.registry.unit_facts.insert(d.name.clone(), fact);
                            }
                        }
                        st.type_pub
                            .insert(d.name.clone(), d.is_pub && !d.is_package_pub);
                        st.type_pkg_pub.insert(d.name.clone(), d.is_package_pub);
                    }
                }
                Item::Test(t) => {
                    let Some(name) = &t.name else {
                        continue;
                    };
                    if name_defined(name, &st.funcs, &st.registry, &st.consts)
                        || st.tests.contains_key(name)
                    {
                        diags.push(defined_twice(
                            name,
                            "every test needs a unique name so failures are easy to find",
                            t.name_span,
                        ));
                    } else {
                        st.tests.insert(name.clone(), t.name_span);
                    }
                }
                // D-BENCH1: `#Bench` blocks define no referenceable name; codegen
                // discovers them straight from the AST, so registration is a no-op.
                Item::Bench(_) => {}
                Item::ExternRust(block) => {
                    if check_extern_block(block, &st.registry, &mut diags) {
                        for ef in &block.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                &st.registry,
                                &st.consts,
                                &mut diags,
                                false,
                            );
                        }
                    }
                }
                Item::CModule(cm) => {
                    // Card #436: check named C-ABI types (struct/enum/distinct)
                    // against the merged cross-file view built above, not the
                    // synthetic module's own (empty) registry. See the comment
                    // at `ffi_named_types`'s construction.
                    let merged_registry = ffi_named_types.as_ref().map(|extra| {
                        let mut types = st.registry.types.clone();
                        for (k, v) in extra {
                            types.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                        TypeRegistry {
                            types,
                            unit_types: st.registry.unit_types.clone(),
                            unit_facts: st.registry.unit_facts.clone(),
                            computed_fields: st.registry.computed_fields.clone(),
                            field_defaults: st.registry.field_defaults.clone(),
                        }
                    });
                    let check_registry = merged_registry.as_ref().unwrap_or(&st.registry);
                    if check_c_module(cm, check_registry, &mut diags) {
                        for ef in &cm.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                check_registry,
                                &st.consts,
                                &mut diags,
                                true,
                            );
                            // C FFI functions are callable across the `use c.<lib>`
                            // alias — expose them like any pub item.
                            st.func_pub.insert(ef.name.clone(), true);
                        }
                    }
                }
                Item::Trait(t) => {
                    st.type_pub
                        .insert(t.name.clone(), t.is_pub && !t.is_package_pub);
                    st.type_pkg_pub.insert(t.name.clone(), t.is_package_pub);
                }
                Item::Module(_) => {}
                Item::CodeModule(cm) => {
                    if let Some(body) = &cm.body {
                        // D-MOD2: register inline module functions under mangled names
                        // (`math__double`) so call-site sema can check them.
                        st.code_modules.insert(cm.name.clone(), cm.name.clone());
                        st.code_module_identities.insert(
                            cm.name.clone(),
                            cm.instance_identity.as_ref()
                                .map(|identity| format!("instance:{}", identity.fingerprint))
                                .unwrap_or_else(|| format!("module:{}::{}", st.module_path, cm.name)),
                        );
                        for inner in body {
                            if let Item::Func(f) = inner {
                                let mangled = format!("{}__{}", cm.name, f.name);
                                st.func_spans.insert(mangled.clone(), f.name_span);
                                st.funcs.insert(mangled.clone(), func_to_sig(f));
                                if !f.type_params.is_empty() {
                                    st.trait_reg
                                        .fn_params
                                        .insert(mangled.clone(), f.type_params.clone());
                                }
                                st.func_pub.insert(mangled, f.is_pub && !f.is_package_pub);
                                st.func_pkg_pub
                                    .insert(format!("{}__{}", cm.name, f.name), f.is_package_pub);
                            }
                        }
                    }
                }
                Item::ErrorConv(_) => {}
                // D-MIGRATE1: migration decls are handled by the schema diff pass; no registration needed.
                Item::Migration(_) => {}
                // D-STATE-DECL: state-set decls are sema-only (I3); no type to register.
                Item::StateDecl(_) => {}
                // D-PROTO1/D-PROTO2: expanded before registration; declaration erases.
                Item::ProtocolDecl(_) => {}
                // D-METADERIVE1=A: user-authored derive blocks are expanded below; skip here.
                Item::UserDerive(_) => {}
                // D-META-NAME1/FORM1: registration is #1457's/#1458's job; no
                // registry row for #1456's declaration-side parse.
                Item::EffectDecl(_)
                | Item::MarkerDecl(_)
                | Item::GenericModule(_)
                | Item::ModuleAlias(_) => {}
            }
        }
        // D-METADERIVE1=A: user-derive expansion — run after struct/func registration so
        // derive bodies can call helper functions and access TypeInfo. Re-entry (D-CTCODEGEN1=A):
        // emitted fragments go through the full lexer→parser pipeline and are appended as items.
        {
            if !derive_providers.is_empty() {
                let struct_infos: Vec<&crate::AST::StructDef> = module
                    .items
                    .iter()
                    .filter_map(|i| {
                        if let Item::Struct(s) = i {
                            Some(s)
                        } else {
                            None
                        }
                    })
                    .collect();

                let mut new_items: Vec<Item> = Vec::new();

                for s in &struct_infos {
                    for (derive_name, derive_span) in &s.derives {
                        // Prefer an entry-local provider, then one beside the target.
                        // Remaining imported/imported pairs violate the orphan law:
                        // either provider or target must be entry-local.
                        let provider = derive_providers
                            .iter()
                            .filter(|(_, name, _, _, _)| name == derive_name)
                            .min_by_key(|(origin, _, _, _, _)| {
                                if *origin == 0 {
                                    0
                                } else if *origin == idx {
                                    1
                                } else {
                                    2
                                }
                            });
                        let Some((provider_idx, _, type_param, body, helper_funcs)) = provider else {
                            continue;
                        };
                        if idx > 0 && *provider_idx > 0 {
                            diags.push(Diagnostic::error(
                                "E2711",
                                format!(
                                    "derive orphan rule: neither `derive T.{}` nor `{}` is local",
                                    derive_name, s.name
                                ),
                                "a generated implementation is owned locally only when the derive provider or target type lives in the entry module".to_string(),
                                format!(
                                    "define `derive T.{}` or `{}` in the entry module",
                                    derive_name, s.name
                                ),
                                // The violating marker belongs to an imported source file;
                                // the bundled diagnostic currently renders against the entry
                                // file, so omit a misleading entry-file caret.
                                None,
                            ));
                            continue;
                        }
                        let actual_funcs: HashMap<String, &Func> = helper_funcs
                            .iter()
                            .map(|(name, func)| (name.clone(), func))
                            .collect();
                        let states = module
                            .items
                            .iter()
                            .find_map(|item| match item {
                                Item::StateDecl(state) if state.type_name == s.name => Some(
                                    state
                                        .states
                                        .iter()
                                        .map(|(name, _)| name.clone())
                                        .collect::<Vec<_>>(),
                                ),
                                _ => None,
                            })
                            .unwrap_or_default();
                        let type_info =
                            crate::Comptime::build_struct_type_info_with_states(s, &states);

                        match crate::Comptime::evaluate_derive_body(
                            body,
                            type_param,
                            type_info,
                            &actual_funcs,
                            &bundle.project_root,
                        ) {
                                Ok(fragments) => {
                                    for fragment in fragments {
                                        let (toks, lex_diags) = crate::Lexer::lex(&fragment);
                                        if !lex_diags.is_empty() {
                                            let detail = lex_diags
                                                .first()
                                                .map(|d| d.what.as_str())
                                                .unwrap_or("the generated text could not be read");
                                            diags.push(Diagnostic::error(
                                                "E2710",
                                                format!(
                                                    "`derive T.{}` generated invalid Jet while expanding `#{}` on `{}`",
                                                    derive_name, derive_name, s.name
                                                ),
                                                format!(
                                                    "generated source did not pass the ordinary lexer and parser: {detail}"
                                                ),
                                                "fix the `derive` body so every emitted fragment is valid Jet source".to_string(),
                                                Some(*derive_span),
                                            ));
                                            continue;
                                        }
                                        match crate::Parser::parse(&toks) {
                                            Ok(mut prog) => new_items.extend(prog.items.drain(..)),
                                            Err(parse_diags) => {
                                                let detail = parse_diags
                                                    .first()
                                                    .map(|d| d.what.as_str())
                                                    .unwrap_or("the generated text was not valid Jet");
                                                diags.push(Diagnostic::error(
                                                    "E2710",
                                                    format!(
                                                        "`derive T.{}` generated invalid Jet while expanding `#{}` on `{}`",
                                                        derive_name, derive_name, s.name
                                                    ),
                                                    format!(
                                                        "generated source did not pass the ordinary lexer and parser: {detail}"
                                                    ),
                                                    "fix the `derive` body so every emitted fragment is valid Jet source".to_string(),
                                                    Some(*derive_span),
                                                ));
                                            }
                                        }
                                    }
                                }
                                // E2710: derive body failed at comptime. Wrap with context
                                // pointing at the #TraitName trigger on the struct.
                            Err(inner) => diags.push(Diagnostic::error(
                                    "E2710",
                                    format!(
                                        "`derive T.{}` body failed while expanding `#{}` on `{}`",
                                        derive_name, derive_name, s.name
                                    ),
                                    inner.what.clone(),
                                    "fix the `derive` body so it generates valid Jet at compile time".to_string(),
                                    Some(*derive_span),
                            )),
                        }
                    }
                }

                // Register new items before synthesis so they go through
                // the normal sema pipeline.
                for item in &new_items {
                    match item {
                        Item::Func(f) => register_func_item(f, st, &mut diags),
                        Item::Struct(s) => {
                            register_struct(
                                s,
                                &mut st.registry,
                                &mut diags,
                                &st.funcs,
                                &st.consts,
                            );
                            st.type_pub
                                .insert(s.name.clone(), s.is_pub && !s.is_package_pub);
                            st.type_pkg_pub.insert(s.name.clone(), s.is_package_pub);
                            for field in &s.fields {
                                st.field_pub.insert(
                                    (s.name.clone(), field.name.clone()),
                                    field.is_pub && !field.is_package_pub,
                                );
                                st.field_pkg_pub.insert(
                                    (s.name.clone(), field.name.clone()),
                                    field.is_package_pub,
                                );
                            }
                        }
                        Item::Enum(e) => {
                            register_enum(
                                e,
                                &mut st.registry,
                                &mut diags,
                                &st.funcs,
                                &st.consts,
                            );
                            st.type_pub
                                .insert(e.name.clone(), e.is_pub && !e.is_package_pub);
                            st.type_pkg_pub.insert(e.name.clone(), e.is_package_pub);
                        }
                        Item::Tag(t) => {
                            st.type_pub
                                .insert(t.name.clone(), t.is_pub && !t.is_package_pub);
                            st.type_pkg_pub.insert(t.name.clone(), t.is_package_pub);
                        }
                        Item::Impl(i) => {
                            for m in &i.methods {
                                st.method_pub
                                    .insert((i.type_name.clone(), m.name.clone()), m.is_pub);
                            }
                        }
                        _ => {}
                    }
                }
                module.items.extend(new_items);
            }
        }

        st.consts.extend(comptime_types);
        // D-SERDE2=A/R11: built-in codecs re-enter as ordinary Jet source in
        // bundle builds too; this is the production multi-file path.
        super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);

        // S62 + D-LIB2: synthesis must happen before register_impl_methods
        // so the synthesised Func nodes appear in the type registry.
        synthesize_impls(&mut module.items);
        register_type_methods(&module.items, &mut st.registry, &mut diags);
        register_patchable_methods(&module.items, &mut st.registry);
        register_impl_methods(&module.items, &mut st.registry, &mut diags);
        // D-TXN-ROLLBACK layer 2: ensure Rollback is known before user impl blocks.
        st.trait_reg.register_synthetic_rollback();
        st.trait_reg.register_synthetic_display_debug();
        st.trait_reg.register_synthetic_close();
        st.trait_reg.register_synthetic_operators();
        st.trait_reg.register_synthetic_iter_index();
        st.trait_reg.register_synthetic_io();
        st.trait_reg.register_synthetic_driver();
        st.trait_reg.register_items(&module.items, &mut diags);
        for type_name in &st.registry.unit_types {
            st.trait_reg
                .trait_impls
                .insert((type_name.clone(), crate::Generics::DISPLAY.to_string()));
        }
        for (type_name, fact) in &st.registry.unit_facts {
            {
                st.trait_reg.trait_impls.insert((
                    type_name.clone(),
                    crate::Generics::quantity_bound(&fact.family, fact.kind.name()),
                ));
                for capability in [crate::Generics::ENCODE, crate::Generics::DECODE] {
                    st.trait_reg
                        .derives
                        .entry(type_name.clone())
                        .or_default()
                        .insert(capability.to_string());
                }
            }
        }
        // D-SERDE: validate `#[Codable]`/`#[Encode]`/`#[Decode]` markers (E2407–E2412)
        // now that the trait registry resolves field/variant types — keeps the emitted
        // `impl`s rustc-clean (I2).
        diags.extend(validate_serde_items(&module.items, &st.trait_reg));
        // D-MARK-VOCAB1 (card #518): a marker name outside the registered
        // `@`/`#` plane vocabulary is E0927, instead of silently doing
        // nothing (the parser accepts any PascalCase name structurally).
        diags.extend(check_marker_vocabulary(&module.items, &known_derive_names));
        // D-CLIFLAG1: validate `#[CLI]`-derived structs (E1305/E1306), same
        // timing as the serde pass above (trait registry must be built so
        // `CLI` is visible on `s.derives`).
        diags.extend(validate_cli_items(&module.items, &st.trait_reg));
        // D-MIGRATE1: schema diff pass (E0910) — runs after struct registration (I3).
        diags.extend(check_schema_migrations(
            &module.items,
            &bundle.project_root,
            &st.trait_reg,
        ));
        // D-SHARED-CYCLE1=C: strong Shared cycles are beginner-rejected (E0221);
        // expert cycles use Shared.Weak and are admitted.
        check_strong_shared_cycles(&st.registry, &mut diags);
    }
    let bundle_auto_derives = TraitRegistry::bundle_auto_derives(bundle);
    for (state, auto_derives) in states.iter_mut().zip(&bundle_auto_derives) {
        state.trait_reg.merge_auto_derives(auto_derives);
    }
    bundle.comptime_inputs.extend(top_level_embed_inputs);
    diags.extend(super::BudgetSpecs::validate_bundle(bundle));

    // S62 E2401: delegation validation — check field exists and implements trait.
    // Runs after all m9 registrations so implements_trait is populated.
    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &states[idx];
        for item in &module.items {
            if let Item::Impl(i) = item {
                if let (Some(trait_name), Some(field_name)) = (&i.trait_name, &i.delegation_field) {
                    if let Some(fields) = st.registry.struct_fields(&i.type_name) {
                        if let Some((_, _, field_ty, _)) =
                            fields.iter().find(|(n, _, _, _)| n == field_name)
                        {
                            let field_type_name = field_ty.name();
                            if !st.trait_reg.implements_trait(&field_type_name, trait_name) {
                                diags.push(Diagnostic::error(
                                    "E2401",
                                    format!(
                                        "`{}` doesn't implement `{}`, so it can't delegate",
                                        field_type_name, trait_name
                                    ),
                                    format!(
                                        "`impl {}.{} using {}` forwards `{}` methods to the `{}` field, but `{}` doesn't implement `{}`",
                                        i.type_name, trait_name, field_name,
                                        trait_name, field_name,
                                        field_type_name, trait_name
                                    ),
                                    format!(
                                        "implement `impl {}: {}` on the field's type, or choose a different field",
                                        field_type_name, trait_name
                                    ),
                                    Some(i.type_span),
                                ));
                            }
                        } else {
                            diags.push(Diagnostic::error(
                                "E2401",
                                format!("`{}` has no field `{}`", i.type_name, field_name),
                                format!(
                                    "`impl {}.{} using {}` needs `{}` to have a field named `{}`",
                                    i.type_name, trait_name, field_name, i.type_name, field_name
                                ),
                                format!("add `{}: Type` to `struct {}`", field_name, i.type_name),
                                Some(i.type_span),
                            ));
                        }
                    }
                }
            }
        }
    }

    // D-MOD3/4: Unqualified imports (`use alias.Item`) are processed in a
    // dedicated pass *after* file-module aliases land in `st.imports` below.

    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &mut states[idx];
        for imp in &module.imports {
            // Unqualified imports are handled in the dedicated pass below.
            if matches!(&imp.kind, ImportKind::Unqualified { .. }) {
                continue;
            }
            let alias = imp.import_alias();
            if st.imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if st.core_imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if let ImportKind::Module(name, _) = &imp.kind {
                if crate::Syntax::is_legacy_std_import(name) {
                    diags.push(Diagnostic::error(
                        "E0019",
                        format!("`{name}` is the old standard-library import spelling"),
                        "the standard library module was renamed to `core`".to_string(),
                        format!(
                            "use `import {}` or `import {}.fs as fs`",
                            Syntax::CORE_SHORT,
                            Syntax::CORE_SHORT
                        ),
                        Some(imp.span),
                    ));
                    continue;
                }
                // D-CORENS1 / E0341: old `jet.<ring>` spelling → teach the new `core.<ring>`.
                if let Some(ring) = name.strip_prefix("jet.") {
                    if crate::Syntax::is_ring_module(ring) {
                        diags.push(Diagnostic::error(
                            "E0341",
                            format!("`use jet.{ring}` is the old first-party library spelling"),
                            "first-party libraries moved to the `core.*` namespace (D-CORENS1)"
                                .to_string(),
                            format!("write `use core.{ring}` instead"),
                            Some(imp.span),
                        ));
                        continue;
                    }
                }
            }
            if let Some(module) = imp.core_module_path() {
                if !crate::Syntax::is_known_core_module(&module) {
                    diags.push(Diagnostic::error(
                        "E1001",
                        format!("there is no core module `{}`", module),
                        "`core` is compiler-known in M10, and only the frozen core modules exist"
                            .to_string(),
                        format!("import one of: {}", crate::Syntax::core_modules_list()),
                        Some(imp.span),
                    ));
                    continue;
                }
                // D-RINGLAYER1=A: infer minimum layer and enforce optional ceiling.
                if let Some(mod_layer) = crate::Syntax::core_module_layer(&module) {
                    if let Some(ceiling) = bundle.layer_ceiling {
                        if mod_layer > ceiling {
                            diags.push(crate::Syntax::layer_ceiling_exceeded(
                                &module,
                                mod_layer,
                                ceiling,
                                Some(imp.span),
                                Some(&format!("`use {module}`")),
                            ));
                            continue;
                        }
                    }
                    if mod_layer > bundle.inferred_layer {
                        bundle.inferred_layer = mod_layer;
                    }
                }
                st.core_imports.insert(alias, module);
                continue;
            }
            // S59 (E2-M14): C `use` forms bind to a synthetic merged module
            // resolved by `CFFI::assemble` (E3204 already reported there).
            if imp.is_c_import() {
                if let Some(target) = bundle.cffi.target_for(idx, &alias) {
                    st.imports.insert(alias, target);
                }
                continue;
            }
            if let Some(target) = bundle.import_targets.get(&(idx, imp.span)).copied() {
                st.imports.insert(alias, target);
            }
        }
    }

    // D-MOD3/4: process `use alias.Item` unqualified imports now that file-module
    // aliases are registered in `st.imports`. `pub use` additionally re-exports the
    // item onto this module's public surface (`reexports`).
    for (idx, module) in bundle.modules.iter().enumerate() {
        for imp in &module.imports {
            let ImportKind::Unqualified {
                module_alias,
                module_alias_span,
                items,
                ..
            } = &imp.kind
            else {
                continue;
            };
            let st = &mut states[idx];
            if let Some(canonical) = st.code_modules.get(module_alias.as_str()) {
                // Inline module: items are mangled as `{alias}__{item}`.
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let mangled = format!("{}__{}", canonical, orig);
                    if !st.funcs.contains_key(&mangled) {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", orig, module_alias),
                            "check the module body for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !st.func_pub.get(&mangled).copied().unwrap_or(false)
                        && !st.func_pkg_pub.get(&mangled).copied().unwrap_or(false)
                    {
                        diags.push(Diagnostic::error(
                            "E0609",
                            format!("`{}` is private in module `{}`", orig, module_alias),
                            "only `pub` items can be brought into scope with `use`".to_string(),
                            format!(
                                "add `pub` before `fn {}` in module `{}`",
                                orig, module_alias
                            ),
                            Some(*module_alias_span),
                        ));
                    } else {
                        st.unqualified.insert(local.to_string(), mangled.clone());
                        if imp.is_pub {
                            st.reexports.insert(local.to_string(), (mangled, idx));
                        }
                    }
                }
            } else if module_alias == "core" || module_alias == "jet" {
                // Std namespace prefix: `use core.mem` → bind each item as a Core import.
                // Each item `x` becomes `core.x` in the known-modules table.
                let st = &mut states[idx];
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let full = format!("core.{}", orig);
                    if !crate::Syntax::is_known_core_module(&full) {
                        diags.push(Diagnostic::error(
                            "E1001",
                            format!("there is no core module `{}`", full),
                            "`core` is compiler-known in M10, and only the frozen core modules exist".to_string(),
                            format!("import one of: {}", crate::Syntax::core_modules_list()),
                            Some(*module_alias_span),
                        ));
                    } else if st.core_imports.contains_key(local) {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{}` is used twice", local),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                    } else {
                        // D-RINGLAYER1=A M2: unqualified `use core.X` obeys the same layer rules.
                        if let Some(mod_layer) = crate::Syntax::core_module_layer(&full) {
                            if let Some(ceiling) = bundle.layer_ceiling {
                                if mod_layer > ceiling {
                                    diags.push(crate::Syntax::layer_ceiling_exceeded(
                                        &full,
                                        mod_layer,
                                        ceiling,
                                        Some(*module_alias_span),
                                        Some(&format!("`use core.{orig}`")),
                                    ));
                                    continue;
                                }
                            }
                            if mod_layer > bundle.inferred_layer {
                                bundle.inferred_layer = mod_layer;
                            }
                        }
                        st.core_imports.insert(local.to_string(), full);
                    }
                }
            } else if st.imports.contains_key(module_alias.as_str()) {
                // File module: look up items in the target module's state.
                let target_idx = st.imports[module_alias.as_str()];
                let is_reexport = imp.is_pub;
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let same_pkg = states[target_idx].package_scope == states[idx].package_scope;
                    let is_pub = states[target_idx]
                        .func_pub
                        .get(orig.as_str())
                        .copied()
                        .unwrap_or(false)
                        || (same_pkg
                            && states[target_idx]
                                .func_pkg_pub
                                .get(orig.as_str())
                                .copied()
                                .unwrap_or(false));
                    let exists = states[target_idx].funcs.contains_key(orig.as_str());
                    if !exists {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", orig, module_alias),
                            "check the module for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !is_pub {
                        diags.push(Diagnostic::error(
                            "E0609",
                            format!("`{}` is private in module `{}`", orig, module_alias),
                            "only `pub` items can be brought into scope with `use`".to_string(),
                            format!("add `pub` before `fn {}` in the imported file", orig),
                            Some(*module_alias_span),
                        ));
                    } else {
                        states[idx]
                            .unqualified_file
                            .insert(local.to_string(), (orig.clone(), target_idx));
                        if is_reexport {
                            states[idx]
                                .reexports
                                .insert(local.to_string(), (orig.clone(), target_idx));
                        }
                    }
                }
            } else {
                // Module alias not found — E0610.
                diags.push(Diagnostic::error(
                    "E0610",
                    format!("no module named `{}` in scope", module_alias),
                    "the alias must refer to a module imported earlier in this file".to_string(),
                    format!("add `import … as {}`  before this `use`", module_alias),
                    Some(*module_alias_span),
                ));
            }
        }
    }

    for idx in 0..bundle.modules.len() {
        for item in &bundle.modules[idx].items {
            let Item::Impl(i) = item else { continue };
            if !i.type_name.contains('.') {
                continue;
            }
            if !impl_type_exists(
                &i.type_name,
                &states[idx].registry,
                &states[idx].imports,
                Some(&states),
            ) {
                diags.push(Diagnostic::error(
                    "E0301",
                    format!("`impl {}` names a type that doesn't exist", i.type_name),
                    format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                    format!(
                        "define `struct {}` or `enum {}` first",
                        i.type_name, i.type_name
                    ),
                    Some(i.type_span),
                ));
            } else {
                for m in &i.methods {
                    states[idx]
                        .method_pub
                        .insert((i.type_name.clone(), m.name.clone()), m.is_pub);
                }
            }
        }
    }

    // D-SHAPE-OUTPUT-CALLABLE1: freeze every runnable Output to the ordinary
    // function it resolves to before entry selection or lowering can inspect it.
    resolve_outputs(bundle, &states, mode, explicit_output, &mut diags);

    // Parity with the single-file path: `@static` and address-taken consts
    // must lower to Rust `static` in bundle mode too.
    for module in bundle.modules.iter_mut() {
        let const_names: Vec<String> = module
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Const(c) => Some(c.name.clone()),
                _ => None,
            })
            .collect();
        let mut address_taken: HashSet<String> = HashSet::new();
        for item in &module.items {
            match item {
                Item::Func(f) => {
                    walk_stmts_for_const_refs(&f.body, &const_names, &mut address_taken)
                }
                Item::Struct(s) => {
                    for m in &s.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Impl(i) => {
                    for m in &i.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Test(t) => {
                    walk_stmts_for_const_refs(&t.body, &const_names, &mut address_taken)
                }
                Item::Bench(b) => {
                    walk_stmts_for_const_refs(&b.body, &const_names, &mut address_taken)
                }
                Item::EffectDecl(_)
            | Item::MarkerDecl(_)
            | Item::Const(_)
            | Item::ExternRust(_)
            | Item::Trait(_)
            | Item::Tag(_) // D-QUAL2: tags erase
            | Item::Module(_)
            | Item::Distinct(_)
            | Item::TypeAlias(_) // D-TYPEALIAS1: erases
            | Item::UnitFamily(_) // D-QUAL3: lowered to distinct types
            | Item::CModule(_) | Item::CodeModule(_)
            | Item::ErrorConv(_)
            | Item::Migration(_) // D-MIGRATE1
            | Item::StateDecl(_) // D-STATE-DECL: erases
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: already expanded above
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
            }
        }
        for item in &mut module.items {
            if let Item::Const(c) = item {
                let force_static = c.attrs.contains(&ConstAttr::ForceStatic);
                c.rust_kind = if force_static || address_taken.contains(&c.name) {
                    RustConstKind::Static
                } else {
                    RustConstKind::Const
                };
            }
        }
    }

    // Each non-entry module becomes a Rust `mod user_<alias>`; a type in the
    // entry file with the same name would collide in the type namespace.
    for (idx, m) in bundle.modules.iter().enumerate() {
        if idx == bundle.entry {
            continue;
        }
        if states[bundle.entry].registry.contains(&m.alias) {
            diags.push(Diagnostic::error(
                "E0105",
                format!(
                    "the type `{}` clashes with the imported file `{}`",
                    m.alias, m.display
                ),
                "a type and an imported module can't share a name".to_string(),
                format!(
                    "rename the type, or import with `{} other_name`",
                    Syntax::KW_AS
                ),
                None,
            ));
        }
    }

    let entry = &states[bundle.entry];
    if mode == CompileMode::Run || mode == CompileMode::Eval {
        let entry_items = &bundle.modules[bundle.entry].items;
        let has_selected_output = entry_items.iter().any(|item| {
            matches!(item, Item::Const(value) if value.resolved_output.as_ref().is_some_and(|output| output.selected))
        });
        if has_selected_output {
            // The selected Output contract was checked before body checking.
        } else if let Some(run_fn) = entry_items.iter().find_map(|i| match i {
            Item::Func(f) if f.name == "run" => Some(f),
            _ => None,
        }) {
            // S12/D-S80-RUN1/D-CLIFLAG1: `run` is the only program entry name.
            // It is zero-arg (optionally `-> () ?`), or one typed CLI-spec
            // parameter (`#[CLI]` struct / enum).
            if run_fn.params.is_empty() {
                if mode == CompileMode::Run
                    && run_fn
                        .return_type
                        .as_ref()
                        .is_some_and(|ret| !is_fallible_void_entry_return(ret, entry))
                {
                    diags.push(Diagnostic::error(
                        "E0122",
                        "`run` returns the wrong kind of value".to_string(),
                        "`run` is where running starts; it either returns nothing or reports top-level errors with `() ?`"
                            .to_string(),
                        "write `fn run() { ... }`, or `fn run() => () ? { ... }` if the entry uses `?`"
                            .to_string(),
                        Some(run_fn.name_span),
                    ));
                }
            } else if run_fn.params.len() == 1 {
                let param = &run_fn.params[0];
                let cli_module = jet_foundation::CLISchema::entry_type_module(bundle)
                    .unwrap_or(bundle.entry);
                match cli_entry_param_shape(
                    &bundle.modules[cli_module].items,
                    &param.ty,
                    &states[cli_module].trait_reg,
                ) {
                    CLIEntryShape::Struct | CLIEntryShape::Enum => {}
                    CLIEntryShape::EnumBadVariants(bad) => diags.extend(bad),
                    CLIEntryShape::Invalid => diags.push(e1308(Some(param.ty_span))),
                }
            } else {
                diags.push(e1308(Some(run_fn.name_span)));
            }
        } else {
            diags.push(no_run_error());
        }
    }
    match mode {
        CompileMode::Test if entry.tests.is_empty()
            && !bundle.modules[bundle.entry].items.iter().any(|item| {
                matches!(item, Item::Const(value) if value.resolved_output.as_ref().is_some_and(|output| output.selected && output.kind == crate::AST::OutputKind::Check))
            }) => {
            diags.push(Diagnostic::error(
                "E0601",
                format!("no `#{}` blocks found to run", Syntax::KW_TEST),
                format!(
                    "add at least one top-level block: #{} \"describes what this checks\" {{ ... }}",
                    Syntax::KW_TEST
                ),
                format!(
                    "use `{}` and `{}` inside the block to check results",
                    Syntax::BUILTIN_REQUIRE,
                    Syntax::BUILTIN_REQUIRE_EQ
                ),
                None,
            ));
        }
        // `jet bench` checks the AST for `#Bench` blocks before entering Bench
        // mode and falls back to whole-program timing otherwise, so an empty
        // bench set is never an error here.
        CompileMode::Bench
        | CompileMode::Test
        | CompileMode::Run
        | CompileMode::Check
        | CompileMode::Eval => {}
    }

    // D-EFF1: collect effect summaries across every module, then run the
    // whole-program fixpoint and enforce each `#(…)` bound once.
    let mut declared_effect_facts = jet_foundation::Facts::FactRegistry::default();
    register_effect_facts(bundle, &mut declared_effect_facts);
    diags.extend(validate_declared_effects(bundle, &declared_effect_facts));

    // D-CTEFFECT1 Tier-1: accumulate embed inputs from all module checks.
    // Use a temporary to avoid simultaneous &mut borrows of `bundle`.
    if mode == CompileMode::Check {
        if let Some(cache) = incremental.as_deref_mut() {
            cache.begin_bundle(bundle);
        }
    } else {
        incremental = None;
    }
    let mut embed_inputs = std::mem::take(&mut bundle.comptime_inputs);
    let mut effect_summaries: HashMap<String, EffectSummary> = HashMap::new();
    let mut reference_anchors = HashMap::new();
    let mut module_effect_summaries: Vec<(String, HashMap<String, EffectSummary>)> = Vec::new();
    let mut module_pending_diagnostics = Vec::new();
    // D-METHODMACRO1=A: top-level function names whose address was taken
    // anywhere in the bundle, accumulated across every module below; the
    // `#Inline(Always)` address-taken pass (E0918) runs after the loop, once
    // this set is complete across the whole bundle.
    let mut global_addr_taken: HashSet<String> = HashSet::new();
    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        let mut local_summaries = HashMap::new();
        let mut local_pending_diagnostics = Vec::new();
        let mut module_diags = check_module_bodies(
            module,
            idx,
            &states,
            &declared_effect_facts,
            mode,
            freestanding,
            allow_impure,
            &mut local_summaries,
            &mut embed_inputs,
            &mut global_addr_taken,
            &mut reference_anchors,
            &mut local_pending_diagnostics,
            incremental.as_deref_mut(),
        );
        dedupe_unknown_names(&mut module_diags);
        diags.extend(module_diags);
        for pending in &mut local_pending_diagnostics {
            pending.function_key = format!("{}::{}", module.alias, pending.function_key);
        }
        module_pending_diagnostics.push(local_pending_diagnostics);
        seed_trait_dispatch_effects(&module.items, &mut local_summaries);
        apply_effect_via(&module.items, &mut local_summaries, &mut Vec::new());
        effect_summaries.extend(local_summaries.clone());
        module_effect_summaries.push((module.alias.clone(), local_summaries));
    }
    bundle.comptime_inputs = embed_inputs;
    // D-METHODMACRO1=A: E0918 (address-taken) needs every module's function
    // bodies checked first. Methods can't appear in `global_addr_taken`
    // (Jet's grammar has no way to read a method's bare name as a value), so
    // this only ever fires for top-level functions.
    let mut failed_diagnostic_phases = HashSet::new();
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let phase_diagnostic_start = diags.len();
        for item in &module.items {
            if let Item::Func(f) = item {
                if f.is_inline_always && global_addr_taken.contains(&f.name) {
                    diags.push(e0918_address_taken(
                        &f.name,
                        f.inline_span.unwrap_or(f.name_span),
                    ));
                }
            }
        }
        mark_failed_pending_functions(
            &diags[phase_diagnostic_start..],
            &module_pending_diagnostics[module_index],
            &mut failed_diagnostic_phases,
        );
    }
    // D-EFF2 (`#(via f)`): seed each via-fn's summary with its callback's bound
    // before the fixpoint, so its published effect set is a tight pass-through.
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let phase_diagnostic_start = diags.len();
        apply_effect_via(&module.items, &mut effect_summaries, &mut diags);
        mark_failed_pending_functions(
            &diags[phase_diagnostic_start..],
            &module_pending_diagnostics[module_index],
            &mut failed_diagnostic_phases,
        );
    }
    // File modules need qualified facts: bare top-level names overwrite one
    // another, while D-EFFECT-OMIT1 requires one cross-package solver answer.
    let (public_summaries, public_solved) = qualified_effect_facts(&module_effect_summaries);
    // The Output carries the same solved effect row used by diagnostics and
    // semantic-index consumers. Tooling never re-walks the callable body.
    for module in &mut bundle.modules {
        let display = module.display.clone();
        for item in &mut module.items {
            let Item::Const(value) = item else { continue };
            let Some(output) = &mut value.resolved_output else { continue };
            let alias = &states[output.module].module_alias;
            output.effects = public_solved
                .get(&format!("{alias}::{}", output.semantic_name))
                .map(|effects| effects.iter().cloned().collect())
                .unwrap_or_default();
            reference_anchors.insert(
                (display.clone(), output.reference.start, output.reference.end),
                super::Effects::DefinitionAnchorFact {
                    module_path: output.source_path.clone(),
                    kind: "function".to_string(),
                    def_span: output.definition,
                    semantic_identity: Some(format!("{alias}::{}", output.semantic_name)),
                },
            );
        }
    }
    // `public_summaries` also carries unique short aliases for tooling. Run
    // diagnostics only over canonical module-qualified nodes so each source
    // obligation is reported once.
    let module_aliases = bundle
        .modules
        .iter()
        .map(|module| format!("{}::", module.alias))
        .collect::<Vec<_>>();
    let validation_summaries = public_summaries
        .iter()
        .filter(|(key, _)| module_aliases.iter().any(|prefix| key.starts_with(prefix)))
        .map(|(key, summary)| (key.clone(), summary.clone()))
        .collect::<HashMap<_, _>>();
    // D-CRYPTO-DIAG1: candidate facts survive only when their entire function
    // remains error-free through the solved effect phases below.
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let phase_diagnostic_start = diags.len();
        let prefix = format!("{}::", module.alias);
        let local_solved = public_solved
            .iter()
            .filter_map(|(key, row)| key.strip_prefix(&prefix).map(|key| (key.to_string(), row.clone())))
            .collect::<HashMap<_, _>>();
        let local_summaries = validation_summaries
            .iter()
            .map(|(key, summary)| {
                if let Some(key) = key.strip_prefix(&prefix) {
                    let mut summary = summary.clone();
                    summary.edges = summary
                        .edges
                        .iter()
                        .map(|edge| edge.strip_prefix(&prefix).unwrap_or(edge).to_string())
                        .collect();
                    for call in &mut summary.memory.calls {
                        call.callee = call
                            .callee
                            .strip_prefix(&prefix)
                            .unwrap_or(&call.callee)
                            .to_string();
                    }
                    (key.to_string(), summary)
                } else {
                    (key.clone(), summary.clone())
                }
            })
            .collect::<HashMap<_, _>>();
        check_effect_boundaries(
            &module.items,
            &local_solved,
            &local_summaries,
            &mut diags,
        );
        super::Effects::check_inferred_purity(
            &module.items,
            &module.alias,
            &validation_summaries,
            &public_solved,
            &mut diags,
        );
        check_replayable_effects(&module.items, &local_solved, &mut diags);
        check_secret_grants(
            &module.items,
            &module.alias,
            &validation_summaries,
            &mut diags,
        );
        mark_failed_pending_functions(
            &diags[phase_diagnostic_start..],
            &module_pending_diagnostics[module_index],
            &mut failed_diagnostic_phases,
        );
    }
    check_region_caps(&validation_summaries, &public_solved, &mut failed_diagnostic_phases, &mut diags);
    // D-EFF2: callback param effect bounds (E0747).
    check_callback_bounds(&validation_summaries, &public_solved, &mut failed_diagnostic_phases, &mut diags);
    for pending in module_pending_diagnostics.into_iter().flatten() {
        if !failed_diagnostic_phases.contains(&pending.function_key) {
            diags.push(pending.diagnostic);
        }
    }

    // D-WASM1=A (c123 M1): JS/WASM partition inference and boundary checks.
    // D-MEM-FACTS1: module `#Policy(no_alloc)` declarations are checked only
    // after the same qualified, dependency-complete graph reaches its fixpoint.
    // #657 feeds the other scope levels and the two remaining fact values into
    // this declaration surface; reachability itself stays single-mechanism.
    let (memory_summaries, memory_declarations) =
        super::MemoryFacts::bundle_memory_inputs(bundle, &public_summaries);
    let memory_projections = memory_declarations
        .iter()
        .flat_map(|declaration| {
            declaration.roots.iter().map(|root| {
                (
                    (root.clone(), declaration.fact),
                    super::MemoryFacts::project_memory_fact(
                        declaration.fact,
                        root,
                        &memory_summaries,
                    ),
                )
            })
        })
        .collect::<HashMap<_, _>>();
    diags.extend(super::MemoryFacts::check_memory_facts(
        &memory_declarations,
        &memory_summaries,
    ));
    diags.extend(check_web_partition(
        bundle,
        &public_summaries,
        &public_solved,
    ));

    // D-WEBAPP1=D / D-WEBAUTHOR1=D (Tower #438): one sema-known application graph.
    let (web_app_graph, web_app_diags) = super::WebApp::extract_web_app_graph(bundle);
    diags.extend(web_app_diags);

    // D-OSTARGET1=A (ratified 2026-07-01, c134): native OS platform gating —
    // mixed-axis conflicts and unmatched cross-gate calls.
    diags.extend(check_os_target(bundle));

    // D-FACTMODEL1=A: one erased fact model for tags, effects, and states.
    // Keep the pass in its own frame; this bundle checker already carries the
    // compiler's largest solved graphs.
    let fact_registry = check_fact_tags_and_states(bundle, &states, &mut diags);

    let (mut used_core, usage_spans, ffi_callback_fns) = collect_used_core(bundle, &states);
    // D-CLIFLAG1: a `#[CLI]`-derived struct's generated `__jet_cli_spec_*`/
    // `__jet_cli_decode_*` functions (and the synthesized `fn main` for a
    // typed `fn run`) call straight into `core.args`'s `JetArgsSpec`/
    // `JetParsedArgs` prelude — but they're pure codegen text, not a Jet
    // method call `collect_used_core` can see by walking function bodies.
    // Force the same `CORELIB_PRELUDE` inclusion a hand-written
    // `use core.args` would trigger (any key works; the caller only checks
    // "is this set empty").
    if bundle.modules.iter().any(|m| {
        m.items
            .iter()
            .any(|i| matches!(i, Item::Struct(s) if s.derives.iter().any(|(t, _)| t == "CLI")))
    }) {
        used_core.insert("core.args::spec".to_string());
    }
    // D-MEM1 S6: `Shared<T>`/`Pool<T>`/`Id<T>` need `CORELIB_PRELUDE`'s `jet_std`
    // module (`JetShared`/`JetPool`/`JetId`), but need no `use core.X` import to
    // reach them (unlike `tasks.spawn` etc.) — `collect_used_core` only walks
    // import aliases, so it never sees them. Same forced-insert shape as
    // D-CLIFLAG1 above; a cheap source-text scan is deliberately over-eager (a
    // false positive just includes the prelude when it wasn't strictly needed —
    // harmless, `#![allow(warnings)]` covers the unused code).
    if bundle.modules.iter().any(|m| {
        m.source.contains("Pool<")
            || m.source.contains("Shared<")
            || m.source.contains("Shared.new(")
            || m.source.contains("Cell<")
            || m.source.contains("Cell.new(")
            || m.source.contains("Id<")
    }) {
        used_core.insert("core.mem::pool_shared".to_string());
    }
    // D-VALIDATE1 (card #506): a `validate { … }` block synthesizes
    // `Type.validate(value)`, which returns `[jet_std::FieldError]` — same
    // forced-insert shape as D-CLIFLAG1/D-MEM1 S6 above, since declaring the
    // block needs no `use core.X` import to reach `CORELIB_PRELUDE`.
    if bundle.modules.iter().any(|m| {
        m.items
            .iter()
            .any(|i| matches!(i, Item::Struct(s) if !s.validate_block.is_empty()))
    }) {
        used_core.insert("core.validate::field_error".to_string());
    }
    // D-EMAIL-SMTP-CONFIG1=A: sema canonicalizes `email.Limits.safe()` to a
    // static `Limits.safe()` call before this late usage walk. Preserve CoreLib
    // reachability for type-only SMTP policy programs.
    if bundle.modules.iter().zip(states.iter()).any(|(module, state)| {
        module.source.contains(".Limits")
            && state.core_imports.values().any(|path| path == "core.email")
    }) {
        used_core.insert("core.email::Limits.safe".to_string());
    }
    // D-CORE-SOURCE-AUTHORITY1=A: late sema-generated helpers join the same
    // source-owned package and audited ABI closure as explicit calls.
    expand_core_reachable_closure(&mut used_core);
    bundle.used_core = used_core;
    bundle.ffi_callback_fns = ffi_callback_fns;
    diags.extend(super::MemoryFacts::annotate_scoped_gc_promotions(bundle));
    apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);
    (
        diags,
        super::Effects::SemIndexEffectFacts {
            summaries: public_summaries,
            solved: public_solved,
            memory_declarations,
            memory_projections,
            reference_anchors,
            web_app: web_app_graph,
            fact_registry,
        },
    )
}

/// Load the standard dimension catalog from ordinary Jet source. Local names
/// shadow Prelude members; physical dimension behavior remains explicit opt-in.
fn inject_units_prelude(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    const SOURCE: &str = include_str!("../../../jet-codegen/src/Prelude/Units.jet");
    let (tokens, mut diagnostics) = crate::Lexer::lex_generated(SOURCE);
    let mut prelude = match crate::Parser::parse(&tokens) {
        Ok(program) => program
            .items
            .into_iter()
            .filter_map(|item| match item {
                Item::UnitFamily(family) => Some(family),
                _ => None,
            })
            .collect::<Vec<_>>(),
        Err(mut parse_diagnostics) => {
            diagnostics.append(&mut parse_diagnostics);
            return diagnostics;
        }
    };
    resolve_standard_unit_dimensions(&mut prelude);

    for module in &mut bundle.modules {
        if module.no_prelude {
            continue;
        }
        let occupied = module
            .items
            .iter()
            .flat_map(|item| match item {
                Item::UnitFamily(family) => family
                    .distinct_defs()
                    .into_iter()
                    .map(|definition| definition.name)
                    .collect::<Vec<_>>(),
                Item::Distinct(definition) => vec![definition.name.clone()],
                Item::Struct(definition) => vec![definition.name.clone()],
                Item::Enum(definition) => vec![definition.name.clone()],
                Item::TypeAlias(definition) => vec![definition.name.clone()],
                _ => Vec::new(),
            })
            .collect::<HashSet<_>>();
        let mut selected = prelude
            .iter()
            .filter(|family| {
                source_mentions_identifier(&module.source, &family.family)
                    || family
                        .members
                        .iter()
                        .any(|member| source_mentions_unit_member(&module.source, &member.name))
            })
            .map(|family| family.family.clone())
            .collect::<HashSet<_>>();
        loop {
            let mut added = false;
            for family in &prelude {
                if !selected.contains(&family.family) {
                    continue;
                }
                let Some(crate::AST::UnitDimensionDecl::Derived(expression)) = &family.dimension
                else {
                    continue;
                };
                for dependency in dimension_dependencies(expression) {
                    added |= selected.insert(dependency);
                }
            }
            if !added {
                break;
            }
        }
        for standard in &prelude {
            if module
                .items
                .iter()
                .any(|item| matches!(item, Item::UnitFamily(local) if local.family == standard.family))
            {
                continue;
            }
            let mut standard = standard.clone();
            let used_members = standard
                .members
                .iter()
                .filter(|member| source_mentions_unit_member(&module.source, &member.name))
                .map(|member| member.name.clone())
                .collect::<HashSet<_>>();
            if !selected.contains(&standard.family) {
                continue;
            }
            standard.members.retain(|member| {
                let is_base = standard
                    .base
                    .as_ref()
                    .is_some_and(|base| base.0 == member.name);
                (is_base || used_members.contains(&member.name))
                    && !occupied.contains(&crate::AST::UnitFamilyDef::type_name(&member.name))
            });
            module.items.push(Item::UnitFamily(standard));
        }
    }
    diagnostics
}

fn dimension_dependencies(expression: &crate::AST::Expr) -> Vec<String> {
    match expression {
        crate::AST::Expr::Ident(name, _) => vec![name.clone()],
        crate::AST::Expr::Binary(_, left, right, _) => {
            let mut dependencies = dimension_dependencies(left);
            dependencies.extend(dimension_dependencies(right));
            dependencies
        }
        _ => Vec::new(),
    }
}

fn source_mentions_identifier(source: &str, name: &str) -> bool {
    source.match_indices(name).any(|(start, _)| {
        let before = source[..start].chars().next_back();
        let after = source[start + name.len()..].chars().next();
        !before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            && !after.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
    })
}

fn source_mentions_unit_member(source: &str, member: &str) -> bool {
    source_mentions_unqualified_identifier(
        source,
        &crate::AST::UnitFamilyDef::type_name(member),
    ) || source.contains(&format!("from_{member}"))
        || source.match_indices(member).any(|(start, _)| {
            source[..start]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_digit())
        })
}

fn source_mentions_unqualified_identifier(source: &str, name: &str) -> bool {
    source.match_indices(name).any(|(start, _)| {
        let before = source[..start].chars().next_back();
        let after = source[start + name.len()..].chars().next();
        before != Some('.')
            && !before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            && !after.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
    })
}

/// Give the shared catalog one stable identity, independent of the package
/// whose module receives the ordinary Prelude declarations.
fn resolve_standard_unit_dimensions(prelude: &mut [crate::AST::UnitFamilyDef]) {
    let mut known = HashMap::<String, crate::AST::Dimension>::new();
    for family in prelude.iter() {
        if matches!(family.dimension, Some(crate::AST::UnitDimensionDecl::Base(_))) {
            known.insert(
                family.family.clone(),
                crate::AST::Dimension::base(format!("core.units::{}", family.family)),
            );
        }
    }
    loop {
        let mut progress = false;
        for family in prelude.iter() {
            if known.contains_key(&family.family) {
                continue;
            }
            let Some(crate::AST::UnitDimensionDecl::Derived(expression)) = &family.dimension else {
                continue;
            };
            let DimensionLookup::Found(dimension) = resolve_dimension_expression(
                expression,
                &|qualifier, name| {
                    if qualifier.is_none() {
                        known
                            .get(name)
                            .cloned()
                            .map_or(DimensionLookup::Missing, DimensionLookup::Found)
                    } else {
                        DimensionLookup::Missing
                    }
                },
            ) else {
                continue;
            };
            known.insert(family.family.clone(), dimension);
            progress = true;
        }
        if !progress {
            break;
        }
    }
    for family in prelude {
        family.resolved_dimension = known.get(&family.family).cloned();
        family.resolved_owner = Some("core.units".to_string());
    }
}

fn stable_unit_owner(bundle: &ProgramBundle, module: usize) -> (String, String) {
    let module = &bundle.modules[module];
    let (package_root, dependency_name) =
        GenericModules::owning_package(bundle, &module.path);
    let package = GenericModules::package_identity(bundle, package_root, dependency_name);
    let module_path = module
        .path
        .strip_prefix(package_root)
        .unwrap_or(&module.path)
        .to_string_lossy()
        .replace('\\', "/");
    (package.clone(), format!("{package}::{module_path}"))
}

/// Resolve open unit dimensions before registration. The declaration graph is
/// compile-time only; backends receive the normalized map already attached to
/// each family.
fn resolve_unit_dimensions(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    #[derive(Clone)]
    struct Declaration {
        module: usize,
        item: usize,
        family: String,
        span: Span,
        is_pub: bool,
        claim: crate::AST::UnitDimensionDecl,
        preset: Option<crate::AST::Dimension>,
    }

    let declarations = bundle
        .modules
        .iter()
        .enumerate()
        .flat_map(|(module_index, module)| {
            module
            .items
            .iter()
            .enumerate()
            .filter_map(move |(item_index, item)| match item {
                Item::UnitFamily(family) => family.dimension.clone().map(|claim| Declaration {
                    module: module_index,
                    item: item_index,
                    family: family.family.clone(),
                    span: family.family_span,
                    is_pub: family.is_pub,
                    claim,
                    preset: family.resolved_dimension.clone(),
                }),
                _ => None,
            })
        })
        .collect::<Vec<_>>();

    let imported_modules = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(module_index, module)| {
            module
                .imports
                .iter()
                .filter_map(|import| bundle.import_targets.get(&(module_index, import.span)).copied())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let import_aliases = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(module_index, module)| {
            module
                .imports
                .iter()
                .filter_map(|import| {
                    bundle
                        .import_targets
                        .get(&(module_index, import.span))
                        .copied()
                        .map(|target| (import.import_alias(), target))
                })
                .collect::<HashMap<_, _>>()
        })
        .collect::<Vec<_>>();
    let mut known = HashMap::<(usize, String), crate::AST::Dimension>::new();
    for declaration in &declarations {
        if let Some(dimension) = declaration.preset.clone() {
            known.insert(
                (declaration.module, declaration.family.clone()),
                dimension,
            );
        } else if matches!(declaration.claim, crate::AST::UnitDimensionDecl::Base(_)) {
            let (_, module_identity) = stable_unit_owner(bundle, declaration.module);
            known.insert(
                (declaration.module, declaration.family.clone()),
                crate::AST::Dimension::base(format!(
                    "{module_identity}::{}",
                    declaration.family
                )),
            );
        }
    }

    let mut pending = declarations
        .iter()
        .filter(|declaration| {
            declaration.preset.is_none()
                && matches!(declaration.claim, crate::AST::UnitDimensionDecl::Derived(_))
        })
        .cloned()
        .collect::<Vec<_>>();
    loop {
        let mut progress = false;
        pending.retain(|declaration| {
            let crate::AST::UnitDimensionDecl::Derived(expression) = &declaration.claim else {
                unreachable!()
            };
            let visible = |qualifier: Option<&str>, name: &str| {
                if let Some(alias) = qualifier {
                    let Some(target) = import_aliases[declaration.module].get(alias) else {
                        return DimensionLookup::Missing;
                    };
                    let Some(candidate) = declarations.iter().find(|candidate| {
                        candidate.module == *target
                            && candidate.is_pub
                            && candidate.family == name
                    }) else {
                        return DimensionLookup::Missing;
                    };
                    return known
                        .get(&(candidate.module, candidate.family.clone()))
                        .cloned()
                        .map_or(DimensionLookup::Missing, DimensionLookup::Found);
                }
                if declarations.iter().any(|candidate| {
                    candidate.module == declaration.module && candidate.family == name
                }) {
                    return known
                        .get(&(declaration.module, name.to_string()))
                        .cloned()
                        .map_or(DimensionLookup::Missing, DimensionLookup::Found);
                }
                let candidates = imported_modules[declaration.module]
                    .iter()
                    .copied()
                    .filter(|target| {
                        declarations.iter().any(|candidate| {
                            candidate.module == *target
                                && candidate.is_pub
                                && candidate.family == name
                        })
                    })
                    .collect::<HashSet<_>>();
                if candidates.len() > 1 {
                    return DimensionLookup::Ambiguous(name.to_string());
                }
                let Some(target) = candidates.into_iter().next() else {
                    return DimensionLookup::Missing;
                };
                known
                    .get(&(target, name.to_string()))
                    .cloned()
                    .map_or(DimensionLookup::Missing, DimensionLookup::Found)
            };
            let dimension = match resolve_dimension_expression(expression, &visible) {
                DimensionLookup::Found(dimension) => dimension,
                DimensionLookup::Missing | DimensionLookup::Ambiguous(_) => return true,
            };
            known.insert(
                (declaration.module, declaration.family.clone()),
                dimension,
            );
            progress = true;
            false
        });
        if !progress {
            break;
        }
    }

    let mut diagnostics = Vec::new();
    for declaration in &declarations {
        let resolved = known
            .get(&(declaration.module, declaration.family.clone()))
            .cloned();
        if resolved.is_none() {
            let ambiguity = match &declaration.claim {
                crate::AST::UnitDimensionDecl::Derived(expression) => {
                    let visible = |qualifier: Option<&str>, name: &str| {
                        if qualifier.is_some() {
                            return DimensionLookup::Missing;
                        }
                        if declarations.iter().any(|candidate| {
                            candidate.module == declaration.module
                                && candidate.family == name
                        }) {
                            return DimensionLookup::Missing;
                        }
                        let matches = imported_modules[declaration.module]
                            .iter()
                            .copied()
                            .filter(|target| {
                                declarations.iter().any(|candidate| {
                                    candidate.module == *target
                                        && candidate.is_pub
                                        && candidate.family == name
                                })
                            })
                            .collect::<HashSet<_>>();
                        if matches.len() > 1 {
                            DimensionLookup::Ambiguous(name.to_string())
                        } else {
                            DimensionLookup::Missing
                        }
                    };
                    match resolve_dimension_expression(expression, &visible) {
                        DimensionLookup::Ambiguous(name) => Some(name),
                        _ => None,
                    }
                }
                crate::AST::UnitDimensionDecl::Base(_) => None,
            };
            diagnostics.push(if let Some(name) = ambiguity {
                Diagnostic::error(
                    "E0905",
                    format!("dimension name `{name}` is ambiguous"),
                    "more than one imported module exports that dimension".to_string(),
                    format!("qualify it with the intended module alias, such as `dep.{name}`"),
                    Some(declaration.span),
                )
            } else {
                Diagnostic::error(
                    "E0905",
                    format!("dimension `{}` cannot be resolved", declaration.family),
                    "derived dimensions can use visible declared dimensions and cannot form a cycle"
                        .to_string(),
                    "import or declare every base dimension and remove any dimension cycle".to_string(),
                    Some(declaration.span),
                )
            });
        }
        let owner = stable_unit_owner(bundle, declaration.module).0;
        if let Item::UnitFamily(definition) =
            &mut bundle.modules[declaration.module].items[declaration.item]
        {
            definition.resolved_dimension = resolved;
            if definition.resolved_owner.is_none() {
                definition.resolved_owner = Some(owner);
            }
        }
    }

    // D-DIMENSION-OPEN1=D: a family that never claimed a dimension still needs
    // its owning package recorded, because its unit facts carry the scale,
    // offset, and kind that same-family conversion depends on.
    let owners = (0..bundle.modules.len())
        .map(|module| stable_unit_owner(bundle, module).0)
        .collect::<Vec<_>>();
    for (module, owner) in owners.into_iter().enumerate() {
        for item in &mut bundle.modules[module].items {
            if let Item::UnitFamily(definition) = item {
                if definition.resolved_owner.is_none() {
                    definition.resolved_owner = Some(owner.clone());
                }
            }
        }
    }
    diagnostics
}

enum DimensionLookup {
    Found(crate::AST::Dimension),
    Missing,
    Ambiguous(String),
}

fn resolve_dimension_expression(
    expression: &crate::AST::Expr,
    visible: &impl Fn(Option<&str>, &str) -> DimensionLookup,
) -> DimensionLookup {
    match expression {
        crate::AST::Expr::Ident(name, _) => visible(None, name),
        crate::AST::Expr::Field(base, name, _) => match base.as_ref() {
            crate::AST::Expr::Ident(alias, _) => visible(Some(alias), name),
            _ => DimensionLookup::Missing,
        },
        crate::AST::Expr::Binary(
            op @ (crate::AST::BinOp::Mul | crate::AST::BinOp::Div),
            left,
            right,
            _,
        ) => {
            let left = resolve_dimension_expression(left, visible);
            let right = resolve_dimension_expression(right, visible);
            match (left, right) {
                (DimensionLookup::Ambiguous(name), _)
                | (_, DimensionLookup::Ambiguous(name)) => DimensionLookup::Ambiguous(name),
                (DimensionLookup::Found(left), DimensionLookup::Found(right)) => {
                    let dimension = if *op == crate::AST::BinOp::Mul {
                        left.multiply(&right)
                    } else {
                        left.divide(&right)
                    };
                    dimension.map_or(DimensionLookup::Missing, DimensionLookup::Found)
                }
                _ => DimensionLookup::Missing,
            }
        }
        _ => DimensionLookup::Missing,
    }
}

#[cfg(test)]
mod structure_tests {
    use super::*;

    #[test]
    fn duplicate_unknown_name_at_one_span_is_reported_once() {
        let span = Span::new(4, 8);
        let unknown = || {
            Diagnostic::error(
                "E0107",
                "nothing named `x` exists here".to_string(),
                "a name must be declared before it is used".to_string(),
                "declare it first".to_string(),
                Some(span),
            )
        };
        let mut diagnostics = vec![unknown(), unknown()];
        dedupe_unknown_names(&mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn same_unknown_name_span_in_two_modules_is_preserved() {
        let unknown = || {
            Diagnostic::error(
                "E0107",
                "nothing named `x` exists here".to_string(),
                "a name must be declared before it is used".to_string(),
                "declare it first".to_string(),
                Some(Span::new(4, 8)),
            )
        };
        let mut first_module = vec![unknown(), unknown()];
        let mut second_module = vec![unknown(), unknown()];
        dedupe_unknown_names(&mut first_module);
        dedupe_unknown_names(&mut second_module);
        let diagnostics = [first_module, second_module].concat();
        assert_eq!(diagnostics.len(), 2);
    }

    fn incremental_bundle(source: &str) -> ProgramBundle {
        let (tokens, lexer_diagnostics) = crate::Lexer::lex(source);
        assert!(lexer_diagnostics.is_empty(), "{lexer_diagnostics:?}");
        let mut program = crate::Parser::parse(&tokens).unwrap();
        ProgramBundle {
            entry: 0,
            project_root: std::path::PathBuf::from("."),
            modules: vec![crate::AST::LoadedModule {
                path: std::path::PathBuf::from("cache-accounting.jet"),
                display: "cache-accounting.jet".to_string(),
                source: source.to_string(),
                alias: "main".to_string(),
                imports: std::mem::take(&mut program.imports),
                items: std::mem::take(&mut program.items),
                block_spans: std::mem::take(&mut program.block_spans),
                web_target_ceiling: program.web_target_ceiling,
                pub_file: program.pub_file,
                no_prelude: program.no_prelude,
                html_path: program.html_path,
                no_alloc_policy: program.no_alloc_policy,
                policy_declarations: program.policy_declarations,
                rule_facts: program.rule_facts,
            }],
            parse_teaching: Vec::new(),
            used_core: HashSet::new(),
            ffi_callback_fns: HashSet::new(),
            cffi: crate::AST::CFfi::default(),
            comptime_inputs: Vec::new(),
            import_targets: HashMap::new(),
            layer_ceiling: None,
            inferred_layer: crate::Syntax::RuntimeLayer::Core,
            web_partitions: HashMap::new(),
            web_partition_enforced: false,
            web_partition_report: None,
            dep_roots: HashMap::new(),
            active_os: crate::Syntax::OSTarget::host(),
            edition: "2027".to_string(),
        }
    }

    #[test]
    fn ordinary_units_prelude_supplies_standard_axes_and_provenance() {
        let mut bundle = incremental_bundle(
            "#UnitFamily(Token, dimension, base: token) { token }\n\
             #UnitFamily(TokenRate, dimension: Token / Time, base: token_per_second) { token_per_second }\n",
        );
        assert!(inject_units_prelude(&mut bundle).is_empty());
        assert!(resolve_unit_dimensions(&mut bundle).is_empty());

        let families = bundle.modules[0]
            .items
            .iter()
            .filter_map(|item| match item {
                Item::UnitFamily(family) => Some(family),
                _ => None,
            })
            .collect::<Vec<_>>();
        let token = families.iter().find(|family| family.family == "Token").unwrap();
        let time = families.iter().find(|family| family.family == "Time").unwrap();
        let rate = families
            .iter()
            .find(|family| family.family == "TokenRate")
            .unwrap();
        assert_eq!(
            rate.resolved_dimension,
            token
                .resolved_dimension
                .as_ref()
                .unwrap()
                .divide(time.resolved_dimension.as_ref().unwrap())
        );

        let mass = families.iter().find(|family| family.family == "Mass").unwrap();
        let dalton = mass
            .members
            .iter()
            .find(|member| member.name == "dalton")
            .unwrap();
        assert!(matches!(
            dalton.scale_provenance,
            crate::AST::UnitScaleProvenance::Measured { .. }
        ));
    }

    #[test]
    fn imported_public_dimension_participates_in_derived_claims() {
        let mut bundle = incremental_bundle(
            "use dep\n\
             #UnitFamily(InventoryRate, dimension: Inventory / Time, base: item_per_second) { item_per_second }\n",
        );
        let import_span = bundle.modules[0].imports[0].span;
        let mut dependency =
            incremental_bundle("pub #UnitFamily(Inventory, dimension, base: item) { item }\n")
                .modules
                .remove(0);
        dependency.path = "deps/dep.jet".into();
        dependency.display = "deps/dep.jet".to_string();
        dependency.alias = "dep".to_string();
        bundle.modules.push(dependency);
        bundle.import_targets.insert((0, import_span), 1);

        assert!(inject_units_prelude(&mut bundle).is_empty());
        assert!(resolve_unit_dimensions(&mut bundle).is_empty());
        let dimension = |module: usize, family: &str| {
            bundle.modules[module]
                .items
                .iter()
                .find_map(|item| match item {
                    Item::UnitFamily(definition) if definition.family == family => {
                        definition.resolved_dimension.clone()
                    }
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(
            dimension(0, "InventoryRate"),
            dimension(1, "Inventory")
                .divide(&dimension(0, "Time"))
                .unwrap()
        );
    }

    #[test]
    fn pending_diagnostics_have_exact_retained_byte_cost() {
        let mut bundle = incremental_bundle("fn run() {}\n");
        let mut cache = IncrementalSemaCache::new();
        let (diagnostics, _) = check_bundle_with_effect_facts_incremental(
            &mut bundle,
            CompileMode::Check,
            &mut cache,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(cache.functions.len(), 1);

        let before = cache.stats().live_item_bytes;
        let pending = PendingFunctionDiagnostic {
            function_key: "run".to_string(),
            function_span: Span::new(0, 11),
            diagnostic: Diagnostic::error(
                "E2702",
                "crypto API misuse".to_string(),
                "known nonce length".to_string(),
                "pass the exact nonce length".to_string(),
                Some(Span::new(3, 6)),
            ),
        };
        let expected_delta = format!("{:?}", vec![pending.clone()]).len()
            - format!("{:?}", Vec::<PendingFunctionDiagnostic>::new()).len();
        cache
            .functions
            .values_mut()
            .next()
            .unwrap()
            .pending_diagnostics
            .push(pending);

        assert_eq!(cache.stats().live_item_bytes - before, expected_delta);
    }

    #[test]
    fn bundle_stays_split_without_reordering_passes() {
        const MAX_MODULE_LINES: usize = 2500;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |relative: &str| std::fs::read_to_string(root.join(relative)).unwrap();
        let bundle = read("src/Sema/Bundle.rs");
        let generic = read("src/Sema/Bundle/GenericModules.rs");
        let substitution = read("src/Sema/Bundle/GenericModules/Substitution.rs");
        let outputs = read("src/Sema/Bundle/Outputs.rs");
        let inline_calls = read("src/Sema/Bundle/InlineCalls.rs");
        let validation = read("src/Sema/Bundle/Validation.rs");
        let production = bundle
            .split("#[cfg(test)]\nmod structure_tests")
            .next()
            .unwrap();
        for (relative, source) in [
            ("src/Sema/Bundle.rs", production),
            ("src/Sema/Bundle/GenericModules.rs", generic.as_str()),
            (
                "src/Sema/Bundle/GenericModules/Substitution.rs",
                substitution.as_str(),
            ),
            ("src/Sema/Bundle/InlineCalls.rs", inline_calls.as_str()),
            ("src/Sema/Bundle/Outputs.rs", outputs.as_str()),
            ("src/Sema/Bundle/Validation.rs", validation.as_str()),
        ] {
            assert!(
                source.lines().count() < MAX_MODULE_LINES,
                "{relative} must stay below the card #510 module boundary"
            );
            assert!(!source.contains("include!("));
            assert!(!source.contains("#[path"));
        }
        assert!(production.contains("\nmod GenericModules;\nmod InlineCalls;\nmod Outputs;\nmod Validation;\n"));
        assert!(generic.contains("\nmod Substitution;\n"));

        let ordered = [
            "expand_generic_module_aliases(bundle, &mut diags);",
            "mangle_inline_sibling_calls(bundle);",
            "super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);",
            "register_type_methods(&module.items, &mut st.registry, &mut diags);",
            "register_impl_methods(&module.items, &mut st.registry, &mut diags);",
            "let mut module_diags = check_module_bodies(",
            "collect_used_core(bundle, &states)",
            "apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);",
        ];
        let positions: Vec<usize> = ordered
            .iter()
            .map(|needle| production.find(needle).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
