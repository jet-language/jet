//! D-MEM-FACTS1: transitive memory-policy facts over the Effects call graph.

use crate::Diagnostics::{Diagnostic, Span};
use std::collections::{HashMap, HashSet, VecDeque};

use super::Effects::EffectSummary;
use crate::AST::{Expr, Func, GcPromotion, GcPromotionEdge, Item, LValue, ProgramBundle, Stmt, Type};
use crate::Policy::{self, PolicyDeclaration, PolicyError, PolicyKey, PolicyScope, PolicyValue};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MemoryFact {
    NoAlloc,
    ZeroRc,
    ArenaBounded(u64),
}

impl MemoryFact {
    pub fn display(self) -> String {
        match self {
            Self::NoAlloc => "no_alloc".to_string(),
            Self::ZeroRc => "zero_rc".to_string(),
            Self::ArenaBounded(bytes) => format!("arena_bounded({bytes})"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryEventKind {
    Allocation,
    RetainRelease,
    /// A proven upper bound for this arena operation; `None` is deliberately
    /// unprovable rather than an estimate.
    ArenaBytes(Option<u64>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryEvent {
    pub kind: MemoryEventKind,
    pub span: Span,
    pub source: String,
    pub operation: String,
    pub provenance: String,
    pub executions: Option<u64>,
}

impl MemoryEvent {
    pub(crate) fn new(kind: MemoryEventKind, span: Span, operation: impl Into<String>) -> Self {
        Self {
            kind,
            span,
            source: String::new(),
            operation: operation.into(),
            provenance: "checked Jet body".to_string(),
            executions: Some(1),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenMemoryDispatch {
    pub span: Span,
    pub source: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MemorySummary {
    pub events: Vec<MemoryEvent>,
    pub open_dispatches: Vec<OpenMemoryDispatch>,
    pub regions: Vec<MemoryPolicyRegion>,
    pub unbounded_control: Vec<Span>,
    pub calls: Vec<MemoryCall>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryCall {
    pub callee: String,
    pub span: Span,
    pub source: String,
    pub executions: Option<u64>,
}

pub(crate) fn bundle_memory_inputs(
    bundle: &ProgramBundle,
    qualified: &HashMap<String, EffectSummary>,
) -> (HashMap<String, EffectSummary>, Vec<MemoryFactDeclaration>) {
    let mut summaries = qualified.clone();
    let package = bundle.modules.first().into_iter()
        .flat_map(|module| module.policy_declarations.iter())
        .filter(|declaration| declaration.scope == PolicyScope::Package)
        .cloned()
        .collect::<Vec<_>>();
    let mut declarations = Vec::new();
    for module in &bundle.modules {
        let mut outer = package.clone();
        outer.extend(
            module
                .policy_declarations
                .iter()
                .filter(|declaration| declaration.scope == PolicyScope::Module)
                .cloned(),
        );
        if let Some(span) = module.no_alloc_policy {
            if !outer.iter().any(|declaration| declaration.key == PolicyKey::NoAlloc) {
                outer.push(PolicyDeclaration {
                    key: PolicyKey::NoAlloc,
                    value: PolicyValue::Enabled,
                    scope: PolicyScope::Module,
                    span,
                    target: None,
                    source: module.display.clone(),
                });
            }
        }
        let mut functions = Vec::new();
        collect_function_keys(&module.items, &module.alias, &mut functions);
        for (function_span, function_key) in functions {
            let Some(summary) = qualified.get(&function_key) else { continue };
            let mut chain = outer.clone();
            chain.extend(
                module
                    .policy_declarations
                    .iter()
                    .filter(|declaration| {
                        declaration.scope == PolicyScope::Function
                            && declaration.target == Some(function_span)
                    })
                    .cloned(),
            );
            append_effective(&chain, vec![function_key.clone()], &module.display, &mut declarations);
            for region in &summary.memory.regions {
                let mut region_chain = chain.clone();
                region_chain.extend(region.declarations.clone());
                let synthetic = format!(
                    "{}#policy@{}..{}",
                    function_key, region.span.start, region.span.end
                );
                summaries.insert(
                    synthetic.clone(),
                    EffectSummary {
                        edges: region.edges.clone(),
                        memory: MemorySummary {
                            events: region.events.clone(),
                            open_dispatches: region.open_dispatches.clone(),
                            regions: Vec::new(),
                            unbounded_control: region.unbounded_control.clone(),
                            calls: region.calls.clone(),
                        },
                        ..Default::default()
                    },
                );
                append_effective(&region_chain, vec![synthetic], &module.display, &mut declarations);
            }
        }
    }
    (summaries, declarations)
}

/// D-OPTGC1=A: attach the effective scoped-GC proof to allocations whose
/// owned value crosses the current function boundary. This runs after normal
/// ownership/type checking, so codegen consumes a decision instead of
/// rediscovering policy or escape facts.
pub(crate) fn annotate_scoped_gc_promotions(bundle: &mut ProgramBundle) -> Vec<Diagnostic> {
    let mut package = Vec::new();
    for declaration in bundle
        .modules
        .iter()
        .flat_map(|module| module.policy_declarations.iter())
        .filter(|declaration| declaration.scope == PolicyScope::Package)
    {
        if !package.contains(declaration) {
            package.push(declaration.clone());
        }
    }
    let mut diagnostics = Vec::new();
    for module in &mut bundle.modules {
        let mut outer = package.clone();
        outer.extend(
            module
                .policy_declarations
                .iter()
                .filter(|declaration| declaration.scope == PolicyScope::Module)
                .cloned(),
        );
        let function_policies = module.policy_declarations.clone();
        annotate_items(
            &mut module.items,
            &outer,
            &function_policies,
            &mut diagnostics,
        );
    }
    // Propagate hidden root transfer through ordinary calls until stable.
    for _ in 0..bundle.modules.iter().map(|m| m.items.len()).sum::<usize>().max(1) {
        let promoted = bundle
            .modules
            .iter()
            .flat_map(|module| module.items.iter())
            .filter_map(|item| match item {
                Item::Func(function) if function.gc_return => Some(function.name.clone()),
                _ => None,
            })
            .collect::<HashSet<_>>();
        let mut changed = false;
        for module in &mut bundle.modules {
            for item in &mut module.items {
                if let Item::Func(function) = item {
                    changed |= propagate_gc_transfers(function, &promoted);
                }
            }
        }
        if !changed {
            break;
        }
    }
    let promoted = bundle
        .modules
        .iter()
        .flat_map(|module| module.items.iter())
        .filter_map(|item| match item {
            Item::Func(function) if function.gc_return => Some(function.name.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    for module in &bundle.modules {
        for item in &module.items {
            let Item::Func(function) = item else { continue };
            if function.gc_scope {
                continue;
            }
            for stmt in &function.body {
                let Stmt::Val(binding) = stmt else { continue };
                if matches!(&binding.init, Expr::Call(call) if promoted.contains(&call.name)) {
                    diagnostics.push(Diagnostic::error(
                        "E2111",
                        format!("`{}` cannot leave its scoped GC policy here", binding.name),
                        "the called function returns a collector-owned graph, but this function is ownership-only".to_string(),
                        "add `#Policy(gc)` to this function or convert the returned graph to ordinary ownership before the boundary".to_string(),
                        Some(binding.name_span),
                    ));
                }
            }
        }
    }
    diagnostics
}

fn annotate_items(
    items: &mut [Item],
    outer: &[PolicyDeclaration],
    declarations: &[PolicyDeclaration],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item in items {
        match item {
            Item::Func(function) => annotate_function(function, outer, declarations, diagnostics),
            Item::Struct(definition) => {
                for method in &mut definition.methods {
                    annotate_function(method, outer, declarations, diagnostics);
                }
                for implementation in &mut definition.trait_impls {
                    for method in &mut implementation.methods {
                        annotate_function(method, outer, declarations, diagnostics);
                    }
                }
            }
            Item::Enum(definition) => {
                for implementation in &mut definition.trait_impls {
                    for method in &mut implementation.methods {
                        annotate_function(method, outer, declarations, diagnostics);
                    }
                }
            }
            Item::Impl(implementation) => {
                for method in &mut implementation.methods {
                    annotate_function(method, outer, declarations, diagnostics);
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &mut module.body {
                    annotate_items(body, outer, declarations, diagnostics);
                }
            }
            _ => {}
        }
    }
}

fn annotate_function(
    function: &mut Func,
    outer: &[PolicyDeclaration],
    declarations: &[PolicyDeclaration],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut chain = outer.to_vec();
    chain.extend(
        declarations
            .iter()
            .filter(|declaration| {
                declaration.scope == PolicyScope::Function
                    && declaration.target == Some(function.span)
            })
            .cloned(),
    );
    function.gc_scope = effective_gc_policy(&chain, diagnostics).is_some();
    function.gc_return = annotate_scope(&mut function.body, &chain, diagnostics);
}

fn annotate_scope(
    stmts: &mut [Stmt],
    chain: &[PolicyDeclaration],
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let direct_returns = returned_names(stmts);
    let mut promoted_names = direct_returns.clone();
    let binding_names = stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Val(binding) => Some(binding.name.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    loop {
        let mut discovered = HashSet::new();
        for stmt in stmts.iter() {
            match stmt {
                Stmt::Val(binding) if promoted_names.contains(&binding.name) => {
                    collect_expr_idents(&binding.init, &mut discovered);
                }
                Stmt::Assign { target, value, .. }
                    if lvalue_root(target).is_some_and(|name| promoted_names.contains(name)) =>
                {
                    collect_expr_idents(value, &mut discovered);
                }
                _ => {}
            }
        }
        discovered.retain(|name| binding_names.contains(name));
        let before = promoted_names.len();
        promoted_names.extend(discovered);
        if promoted_names.len() == before {
            break;
        }
    }
    let effective = effective_gc_policy(chain, diagnostics);
    let mut promoted_return = false;
    for stmt in stmts {
        match stmt {
            Stmt::Val(binding)
                if promoted_names.contains(&binding.name)
                    && binding
                        .ty
                        .as_ref()
                        .map(type_may_own_heap)
                        .unwrap_or_else(|| expr_may_own_heap(&binding.init)) =>
            {
                if let Some(policy) = &effective {
                    let scope = policy
                        .provenance
                        .last()
                        .map(|declaration| declaration.scope.name())
                        .unwrap_or("package")
                        .to_string();
                    let mut edges = promotion_edges(&binding.init)
                        .into_iter()
                        .filter(|edge| {
                            edge.binding != binding.name
                                && promoted_names.contains(&edge.binding)
                        })
                        .collect::<Vec<_>>();
                    edges.sort_by(|left, right| {
                        (&left.slot, left.group, &left.binding)
                            .cmp(&(&right.slot, right.group, &right.binding))
                    });
                    edges.dedup();
                    binding.gc_promotion = Some(GcPromotion {
                        span: binding.name_span,
                        scope,
                        policy_provenance: Policy::explain(policy),
                        reason: "returned heap graph requires identity beyond its lexical owner"
                            .to_string(),
                        edges,
                        collection_len: match &binding.init {
                            Expr::ListLit(items, _) => Some(items.len()),
                            _ => None,
                        },
                    });
                    promoted_return |= direct_returns.contains(&binding.name);
                }
            }
            Stmt::Policy { declarations, body, .. } => {
                let mut nested = chain.to_vec();
                nested.extend(declarations.clone());
                promoted_return |= annotate_scope(body, &nested, diagnostics);
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::CountedLoop { body, .. } => {
                promoted_return |= annotate_scope(body, chain, diagnostics);
            }
            Stmt::Switch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    promoted_return |= annotate_scope(&mut arm.body, chain, diagnostics);
                }
                if let Some(body) = else_body {
                    promoted_return |= annotate_scope(body, chain, diagnostics);
                }
            }
            _ => {}
        }
    }
    promoted_return
}

fn effective_gc_policy(
    chain: &[PolicyDeclaration],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Policy::EffectivePolicy> {
    match Policy::resolve(PolicyKey::ScopedGc, chain.iter().cloned()) {
        Ok(policy) => policy.filter(|policy| policy.value == PolicyValue::Enabled),
        Err(error) => {
            diagnostics.push(policy_resolution_diagnostic(error));
            None
        }
    }
}

fn policy_resolution_diagnostic(error: PolicyError) -> Diagnostic {
    let span = match error {
        PolicyError::ProhibitedScope { span, .. } | PolicyError::Widening { span, .. } => span,
        PolicyError::Conflict { second, .. } => second,
    };
    Diagnostic::error(
        "E0355",
        "invalid effective `gc` policy".to_string(),
        "the package, module, function, and block declarations must form one valid policy ladder".to_string(),
        "remove the conflicting declaration or tighten the enclosing policy".to_string(),
        Some(span),
    )
}

fn propagate_gc_transfers(function: &mut Func, promoted: &HashSet<String>) -> bool {
    let returned = returned_names(&function.body);
    let mut changed = false;
    for stmt in &mut function.body {
        if let Stmt::Val(binding) = stmt {
            if function.gc_scope
                && matches!(&binding.init, Expr::Call(call) if promoted.contains(&call.name))
                && !binding.gc_transferred
            {
                binding.gc_transferred = true;
                changed = true;
            }
            if returned.contains(&binding.name)
                && (binding.gc_promotion.is_some() || binding.gc_transferred)
                && !function.gc_return
            {
                function.gc_return = true;
                changed = true;
            }
        }
    }
    changed
}

fn returned_names(stmts: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    for stmt in stmts {
        match stmt {
            Stmt::Return(Some(Expr::Ident(name, _)), _) => {
                names.insert(name.clone());
            }
            Stmt::Policy { body, .. } => names.extend(returned_names(body)),
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::CountedLoop { body, .. } => names.extend(returned_names(body)),
            Stmt::Switch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    names.extend(returned_names(&arm.body));
                }
                if let Some(body) = else_body {
                    names.extend(returned_names(body));
                }
            }
            _ => {}
        }
    }
    names
}

fn collect_expr_idents(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Ident(name, _) => {
            out.insert(name.clone());
        }
        Expr::Call(call) => {
            for arg in &call.args {
                collect_expr_idents(&arg.expr, out);
            }
        }
        Expr::MethodCall { receiver, args, .. }
        | Expr::CallValue {
            callee: receiver,
            args,
            ..
        } => {
            collect_expr_idents(receiver, out);
            for arg in args {
                collect_expr_idents(&arg.expr, out);
            }
        }
        Expr::Binary(_, left, right, _) => {
            collect_expr_idents(left, out);
            collect_expr_idents(right, out);
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Field(inner, _, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => collect_expr_idents(inner, out),
        Expr::OptField { base, .. } => collect_expr_idents(base, out),
        Expr::Index { base, index, .. } => {
            collect_expr_idents(base, out);
            collect_expr_idents(index, out);
        }
        Expr::Slice {
            base, start, end, range, ..
        } => {
            collect_expr_idents(base, out);
            if let Some(range) = range {
                collect_expr_idents(range, out);
            } else {
                collect_expr_idents(start, out);
                collect_expr_idents(end, out);
            }
        }
        Expr::ListLit(items, _) => {
            for item in items {
                collect_expr_idents(item, out);
            }
        }
        Expr::MapLit(pairs, _) => {
            for (key, value) in pairs {
                collect_expr_idents(key, out);
                collect_expr_idents(value, out);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, value) in fields {
                collect_expr_idents(value, out);
            }
        }
        Expr::TypedLit { body, .. } => {
            body.for_each_expr(|value| collect_expr_idents(value, out));
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, value) in fields {
                collect_expr_idents(value, out);
            }
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    crate::AST::EnumLitArg::Positional(value)
                    | crate::AST::EnumLitArg::Named { expr: value, .. } => {
                        collect_expr_idents(value, out);
                    }
                }
            }
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let crate::AST::StrPart::Interp(value, _) = part {
                    collect_expr_idents(value, out);
                }
            }
        }
        _ => {}
    }
}

fn promotion_edges(expr: &Expr) -> Vec<GcPromotionEdge> {
    fn add(expr: &Expr, slot: String, group: usize, out: &mut Vec<GcPromotionEdge>) {
        let mut names = HashSet::new();
        collect_expr_idents(expr, &mut names);
        let mut names = names.into_iter().collect::<Vec<_>>();
        names.sort();
        out.extend(names.into_iter().map(|binding| GcPromotionEdge {
            binding,
            slot: slot.clone(),
            group,
        }));
    }

    let mut out = Vec::new();
    match expr {
        Expr::StructLit { fields, .. } => {
            for (field, _, value) in fields {
                add(value, format!("field:{field}"), 0, &mut out);
            }
        }
        Expr::TypedLit { body, .. } => {
            body.for_each_expr(|value| add(value, "typed_lit".to_string(), 0, &mut out));
        }
        Expr::ListLit(items, _) => {
            for (index, item) in items.iter().enumerate() {
                add(item, "collection".to_string(), index, &mut out);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (field, value) in fields {
                add(value, format!("field:{field}"), 0, &mut out);
            }
        }
        _ => add(expr, "value".to_string(), 0, &mut out),
    }
    out
}

fn lvalue_root(target: &LValue) -> Option<&str> {
    match target {
        LValue::Local { name, .. } => Some(name),
        LValue::Field { base, .. } | LValue::Index { base, .. } => match base.as_ref() {
            Expr::Ident(name, _) => Some(name),
            _ => None,
        },
    }
}

fn expr_may_own_heap(expr: &Expr) -> bool {
    !matches!(
        expr,
        Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Char(..)
            | Expr::UnitLit { .. }
            | Expr::ReduceMarker(..)
    )
}

fn type_may_own_heap(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::Char | Type::IntN { .. } | Type::Float32 => false,
        Type::FixedList { elem, .. } | Type::Option(elem) | Type::Tagged { inner: elem, .. } => {
            type_may_own_heap(elem)
        }
        Type::Tuple(fields) => fields.iter().any(|(_, ty)| type_may_own_heap(ty)),
        _ => true,
    }
}

fn append_effective(
    chain: &[PolicyDeclaration],
    roots: Vec<String>,
    source: &str,
    out: &mut Vec<MemoryFactDeclaration>,
) {
    for key in [PolicyKey::NoAlloc, PolicyKey::ZeroRc, PolicyKey::ArenaBounded] {
        let Ok(Some(effective)) = Policy::resolve(key, chain.iter().cloned()) else { continue };
        let fact = match (key, effective.value) {
            (PolicyKey::NoAlloc, PolicyValue::Enabled) => MemoryFact::NoAlloc,
            (PolicyKey::ZeroRc, PolicyValue::Enabled) => MemoryFact::ZeroRc,
            (PolicyKey::ArenaBounded, PolicyValue::Limit(limit)) => MemoryFact::ArenaBounded(limit),
            _ => continue,
        };
        let Some(last) = effective.provenance.last() else { continue };
        out.push(MemoryFactDeclaration {
            fact,
            roots: roots.clone(),
            span: last.span,
            source: source.to_string(),
            provenance: Policy::explain(&effective),
        });
    }
}

fn collect_function_keys(items: &[Item], alias: &str, out: &mut Vec<(Span, String)>) {
    fn one(
        function: &crate::AST::Func,
        owner: Option<&str>,
        alias: &str,
        out: &mut Vec<(Span, String)>,
    ) {
        out.push((function.span, format!("{alias}::{}", super::effect_key(owner, &function.name))));
    }
    for item in items {
        match item {
            Item::Func(function) => one(function, None, alias, out),
            Item::Impl(implementation) => {
                for method in &implementation.methods { one(method, Some(&implementation.type_name), alias, out); }
            }
            Item::Struct(definition) => {
                for method in &definition.methods { one(method, Some(&definition.name), alias, out); }
                for block in &definition.trait_impls {
                    for method in &block.methods { one(method, Some(&definition.name), alias, out); }
                }
            }
            Item::Enum(definition) => {
                for method in &definition.methods { one(method, Some(&definition.name), alias, out); }
                for block in &definition.trait_impls {
                    for method in &block.methods { one(method, Some(&definition.name), alias, out); }
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body { collect_function_keys(body, alias, out); }
            }
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryPolicyRegion {
    pub declarations: Vec<crate::Policy::PolicyDeclaration>,
    pub span: Span,
    pub events: Vec<MemoryEvent>,
    pub edges: std::collections::BTreeSet<String>,
    pub open_dispatches: Vec<OpenMemoryDispatch>,
    pub unbounded_control: Vec<Span>,
    pub calls: Vec<MemoryCall>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryFactDeclaration {
    pub fact: MemoryFact,
    pub roots: Vec<String>,
    pub span: Span,
    pub source: String,
    pub provenance: String,
}

/// Stable inspect/API/semver projection: consumers compare facts, never parse
/// diagnostics or rescan source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryProjection {
    Proven,
    Violated { call_path: Vec<String>, operation: String },
    OpenWorld { call_path: Vec<String>, reason: String },
}

pub fn project_memory_fact(
    fact: MemoryFact,
    root: &str,
    summaries: &HashMap<String, EffectSummary>,
) -> MemoryProjection {
    match shortest_violation(fact, root, summaries) {
        None => MemoryProjection::Proven,
        Some(Finding { event: Some(event), path, .. }) => MemoryProjection::Violated {
            call_path: path,
            operation: event.operation,
        },
        Some(Finding { open: Some(open), path, .. }) => MemoryProjection::OpenWorld {
            call_path: path,
            reason: open.reason,
        },
        Some(_) => MemoryProjection::OpenWorld {
            call_path: vec![root.to_string()],
            reason: "memory proof is incomplete".to_string(),
        },
    }
}

#[derive(Clone, Debug)]
struct Finding {
    event: Option<MemoryEvent>,
    open: Option<OpenMemoryDispatch>,
    path: Vec<String>,
    arena_used: u64,
}

/// Check declarations over the already-qualified, complete pre-TIR effects
/// graph. Breadth-first traversal gives the shortest violating call path;
/// lexical sorting makes ties deterministic.
pub fn check_memory_facts(
    declarations: &[MemoryFactDeclaration],
    summaries: &HashMap<String, EffectSummary>,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for declaration in declarations {
        let mut emitted = HashSet::new();
        let mut roots = declaration.roots.clone();
        roots.sort();
        for root in roots {
            if let Some(finding) = shortest_violation(declaration.fact, &root, summaries) {
                let identity = if let Some(event) = &finding.event {
                    format!("event:{}:{}:{}", event.source, event.span.start, event.span.end)
                } else if let Some(open) = &finding.open {
                    format!("open:{}:{}:{}", open.source, open.span.start, open.span.end)
                } else {
                    continue;
                };
                if emitted.insert(identity) {
                    out.push(memory_violation(declaration, finding));
                }
            }
        }
    }
    out
}

fn shortest_violation(
    fact: MemoryFact,
    root: &str,
    summaries: &HashMap<String, EffectSummary>,
) -> Option<Finding> {
    if let MemoryFact::ArenaBounded(limit) = fact {
        return match arena_bound(
            root,
            summaries,
            &mut HashSet::new(),
            vec![root.to_string()],
        ) {
            Ok(proof) if proof.bytes <= limit => None,
            Ok(proof) => {
                let ArenaProof { bytes, witness } = proof;
                let (event, open, path) = match witness {
                    Some((event, path)) => (Some(event), None, path),
                    None => (
                        None,
                        Some(OpenMemoryDispatch {
                            span: Span::new(0, 0),
                            source: root.to_string(),
                            reason: format!(
                                "the verified upper bound is {bytes} bytes, above the declared {limit} bytes"
                            ),
                        }),
                        vec![root.to_string()],
                    ),
                };
                Some(Finding { event, open, path, arena_used: bytes })
            }
            Err(finding) => Some(finding),
        };
    }
    let mut queue = VecDeque::from([(root.to_string(), vec![root.to_string()], 0_u64)]);
    let mut seen = HashSet::new();
    while let Some((node, path, arena_used)) = queue.pop_front() {
        if !seen.insert((node.clone(), arena_used)) {
            continue;
        }
        let Some(summary) = summaries.get(&node) else {
            return Some(Finding {
                event: None,
                open: Some(OpenMemoryDispatch {
                    span: Span::new(0, 0),
                    source: node.clone(),
                    reason: "callee body is unavailable and has no verified signed memory summary".to_string(),
                }),
                path,
                arena_used,
            });
        };
        let mut events = summary.memory.events.clone();
        events.sort_by_key(|event| (event.span.start, event.span.end));
        let mut next_arena = arena_used;
        for event in events {
            let violates = match (fact, event.kind) {
                (MemoryFact::NoAlloc, MemoryEventKind::Allocation) => true,
                (MemoryFact::ZeroRc, MemoryEventKind::RetainRelease) => true,
                (MemoryFact::ArenaBounded(limit), MemoryEventKind::ArenaBytes(Some(bytes))) => {
                    next_arena = next_arena.saturating_add(bytes);
                    next_arena > limit
                }
                (MemoryFact::ArenaBounded(_), MemoryEventKind::ArenaBytes(None)) => true,
                _ => false,
            };
            if violates {
                return Some(Finding { event: Some(event), open: None, path, arena_used: next_arena });
            }
        }
        if let Some(open) = summary.memory.open_dispatches.first() {
            return Some(Finding {
                event: None,
                open: Some(open.clone()),
                path,
                arena_used: next_arena,
            });
        }
        let mut edges = summary.edges.iter().cloned().collect::<Vec<_>>();
        edges.sort();
        for edge in edges {
            let mut next_path = path.clone();
            next_path.push(edge.clone());
            queue.push_back((edge, next_path, next_arena));
        }
    }
    None
}

#[derive(Clone, Debug)]
struct ArenaProof {
    bytes: u64,
    witness: Option<(MemoryEvent, Vec<String>)>,
}

fn arena_bound(
    node: &str,
    summaries: &HashMap<String, EffectSummary>,
    visiting: &mut HashSet<String>,
    path: Vec<String>,
) -> Result<ArenaProof, Finding> {
    if !visiting.insert(node.to_string()) {
        return Err(Finding {
            event: None,
            open: Some(OpenMemoryDispatch {
                span: Span::new(0, 0),
                source: node.to_string(),
                reason: "recursive call multiplicity is not statically bounded for arena accounting"
                    .to_string(),
            }),
            path,
            arena_used: 0,
        });
    }
    let result = arena_bound_inner(node, summaries, visiting, path);
    visiting.remove(node);
    result
}

fn arena_bound_inner(
    node: &str,
    summaries: &HashMap<String, EffectSummary>,
    visiting: &mut HashSet<String>,
    path: Vec<String>,
) -> Result<ArenaProof, Finding> {
    let Some(summary) = summaries.get(node) else {
        return Err(Finding {
            event: None,
            open: Some(OpenMemoryDispatch {
                span: Span::new(0, 0),
                source: node.to_string(),
                reason: "callee body is unavailable and has no verified signed arena summary"
                    .to_string(),
            }),
            path,
            arena_used: 0,
        });
    };
    if let Some(open) = summary.memory.open_dispatches.first() {
        return Err(Finding {
            event: None,
            open: Some(open.clone()),
            path,
            arena_used: 0,
        });
    }

    let mut total = 0_u64;
    let mut witness = None;
    let mut events = summary.memory.events.clone();
    events.sort_by_key(|event| (event.span.start, event.span.end));
    for event in events {
        let MemoryEventKind::ArenaBytes(bytes) = event.kind else { continue };
        let Some(bytes) = bytes else {
            return Err(Finding {
                event: Some(event),
                open: None,
                path,
                arena_used: total,
            });
        };
        let Some(executions) = event.executions else {
            return Err(Finding {
                event: None,
                open: Some(OpenMemoryDispatch {
                    span: event.span,
                    source: event.source,
                    reason: "loop iteration count is not statically bounded for arena accounting"
                        .to_string(),
                }),
                path,
                arena_used: total,
            });
        };
        let Some(contribution) = bytes.checked_mul(executions) else {
            return Err(arena_overflow(&event.source, event.span, path, total));
        };
        let Some(next) = total.checked_add(contribution) else {
            return Err(arena_overflow(&event.source, event.span, path, total));
        };
        total = next;
        witness = Some((event, path.clone()));
    }

    let called = summary
        .memory
        .calls
        .iter()
        .map(|call| call.callee.as_str())
        .collect::<HashSet<_>>();
    if let Some(edge) = summary
        .edges
        .iter()
        .find(|edge| !called.contains(edge.as_str()) && may_reach_arena(edge, summaries, &mut HashSet::new()))
    {
        return Err(Finding {
            event: None,
            open: Some(OpenMemoryDispatch {
                span: Span::new(0, 0),
                source: edge.clone(),
                reason: "call-site multiplicity is unavailable for arena accounting".to_string(),
            }),
            path,
            arena_used: total,
        });
    }

    let mut calls = summary.memory.calls.clone();
    calls.sort_by_key(|call| (call.span.start, call.span.end, call.callee.clone()));
    for call in calls {
        let mut call_path = path.clone();
        call_path.push(call.callee.clone());
        let proof = arena_bound(&call.callee, summaries, visiting, call_path.clone())?;
        if proof.bytes == 0 {
            continue;
        }
        let Some(executions) = call.executions else {
            return Err(Finding {
                event: None,
                open: Some(OpenMemoryDispatch {
                    span: call.span,
                    source: call.source,
                    reason: "loop iteration count is not statically bounded for arena accounting"
                        .to_string(),
                }),
                path: call_path,
                arena_used: total,
            });
        };
        let Some(contribution) = proof.bytes.checked_mul(executions) else {
            return Err(arena_overflow(&call.source, call.span, call_path, total));
        };
        let Some(next) = total.checked_add(contribution) else {
            return Err(arena_overflow(&call.source, call.span, call_path, total));
        };
        total = next;
        witness = proof.witness.or_else(|| {
            Some((
                MemoryEvent {
                    kind: MemoryEventKind::ArenaBytes(Some(contribution)),
                    span: call.span,
                    source: call.source,
                    operation: format!("call to `{}`", call.callee),
                    provenance: "transitive checked arena bound".to_string(),
                    executions: Some(executions),
                },
                call_path,
            ))
        });
    }
    Ok(ArenaProof { bytes: total, witness })
}

fn arena_overflow(source: &str, span: Span, path: Vec<String>, used: u64) -> Finding {
    Finding {
        event: None,
        open: Some(OpenMemoryDispatch {
            span,
            source: source.to_string(),
            reason: "arena upper-bound arithmetic overflowed".to_string(),
        }),
        path,
        arena_used: used,
    }
}

fn may_reach_arena(
    node: &str,
    summaries: &HashMap<String, EffectSummary>,
    visiting: &mut HashSet<String>,
) -> bool {
    if !visiting.insert(node.to_string()) {
        return true;
    }
    let Some(summary) = summaries.get(node) else {
        visiting.remove(node);
        return true;
    };
    let direct = summary
        .memory
        .events
        .iter()
        .any(|event| matches!(event.kind, MemoryEventKind::ArenaBytes(_)));
    let result = direct
        || summary
            .edges
            .iter()
            .any(|edge| may_reach_arena(edge, summaries, visiting));
    visiting.remove(node);
    result
}

fn memory_violation(declaration: &MemoryFactDeclaration, finding: Finding) -> Diagnostic {
    let fact = declaration.fact.display();
    let path = finding.path.join(" -> ");
    if let Some(event) = finding.event {
        let arena = match declaration.fact {
            MemoryFact::ArenaBounded(_) => format!("; the proven path total is {} bytes", finding.arena_used),
            _ => String::new(),
        };
        return Diagnostic::error(
            "E0921",
            format!(
                "{} at {} violates the effective `{}` declared at {}",
                event.operation, event.source, fact, declaration.source
            ),
            format!(
                "{} is reachable through {} from code governed by `{}`{}; declaration provenance: {}; operation provenance: {}",
                event.source, path, fact, arena, declaration.provenance, event.provenance
            ),
            "remove or replace the incompatible operation, call an implementation whose transitive memory facts satisfy the contract, or move the call outside this policy scope".to_string(),
            Some(event.span),
        );
    }
    let open = finding.open.expect("finding has event or open dispatch");
    Diagnostic::error(
        "E0921",
        format!("`{fact}` cannot be proved through the open dispatch at {}", open.source),
        format!(
            "{} is reachable through {}; a strict transitive fact cannot assume an unknown future target is compatible; declaration provenance: {}",
            open.reason, path, declaration.provenance
        ),
        "seal the target set, consume a verified signed dependency summary that proves the fact, or move the dispatch outside this policy scope".to_string(),
        Some(if open.span.start == open.span.end { declaration.span } else { open.span }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arena_event(bytes: u64) -> MemoryEvent {
        MemoryEvent {
            kind: MemoryEventKind::ArenaBytes(Some(bytes)),
            span: Span::new(1, 2),
            source: "dep.jet".to_string(),
            operation: "arena allocation".to_string(),
            provenance: "checked body".to_string(),
            executions: Some(1),
        }
    }

    #[test]
    fn arena_bound_counts_each_call_site() {
        let mut summaries = HashMap::new();
        summaries.insert(
            "dep".to_string(),
            EffectSummary {
                memory: MemorySummary { events: vec![arena_event(4)], ..Default::default() },
                ..Default::default()
            },
        );
        let calls = [3, 5]
            .into_iter()
            .map(|start| MemoryCall {
                callee: "dep".to_string(),
                span: Span::new(start, start + 1),
                source: "root.jet".to_string(),
                executions: Some(1),
            })
            .collect::<Vec<_>>();
        summaries.insert(
            "root".to_string(),
            EffectSummary {
                edges: ["dep".to_string()].into_iter().collect(),
                memory: MemorySummary { calls, ..Default::default() },
                ..Default::default()
            },
        );
        assert_eq!(
            project_memory_fact(MemoryFact::ArenaBounded(8), "root", &summaries),
            MemoryProjection::Proven
        );
        assert!(matches!(
            project_memory_fact(MemoryFact::ArenaBounded(7), "root", &summaries),
            MemoryProjection::Violated { .. }
        ));
    }

    #[test]
    fn missing_or_recursive_body_never_proves_a_strict_fact() {
        assert!(matches!(
            project_memory_fact(MemoryFact::NoAlloc, "missing", &HashMap::new()),
            MemoryProjection::OpenWorld { .. }
        ));
        let mut summaries = HashMap::new();
        summaries.insert(
            "recursive".to_string(),
            EffectSummary {
                edges: ["recursive".to_string()].into_iter().collect(),
                memory: MemorySummary {
                    calls: vec![MemoryCall {
                        callee: "recursive".to_string(),
                        span: Span::new(1, 2),
                        source: "recursive.jet".to_string(),
                        executions: Some(1),
                    }],
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert!(matches!(
            project_memory_fact(
                MemoryFact::ArenaBounded(8),
                "recursive",
                &summaries
            ),
            MemoryProjection::OpenWorld { .. }
        ));
    }
}
