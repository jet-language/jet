//! Shared static-guarantee fact graph (card #25 / claim.static-guarantees).
//!
//! One inventory over refinements, contracts, taint (E3 IFC slice), effect
//! budgets, bounds proofs, and replay soundness. Individual sema passes remain
//! the enforcers; this graph is the shared, queryable fact surface those
//! passes feed and that acceptance / `jet prove` consume (I8).

use crate::AST::{Func, Item, Program, Type};
use crate::Diagnostics::Diagnostic;
use crate::Lexer;
use crate::Parser;
use std::collections::BTreeSet;

/// Domains that share the static-guarantees facts engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PolicyDomain {
    Refinement,
    Contract,
    /// D-TAINT1 — E3 information-flow slice (full IFC deferred post-E3).
    Taint,
    Budget,
    Bounds,
    Replay,
    Memory,
}

impl PolicyDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            PolicyDomain::Refinement => "refinement",
            PolicyDomain::Contract => "contract",
            PolicyDomain::Taint => "taint",
            PolicyDomain::Budget => "budget",
            PolicyDomain::Bounds => "bounds",
            PolicyDomain::Replay => "replay",
            PolicyDomain::Memory => "memory",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyFact {
    pub domain: PolicyDomain,
    pub subject: String,
    pub detail: String,
}

/// Canonical shared fact graph for static guarantees.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PolicyFactGraph {
    facts: Vec<PolicyFact>,
}

impl PolicyFactGraph {
    pub fn record(
        &mut self,
        domain: PolicyDomain,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.facts.push(PolicyFact {
            domain,
            subject: subject.into(),
            detail: detail.into(),
        });
    }

    pub fn facts(&self) -> &[PolicyFact] {
        &self.facts
    }

    pub fn domains(&self) -> BTreeSet<PolicyDomain> {
        self.facts.iter().map(|fact| fact.domain).collect()
    }

    pub fn has_domain(&self, domain: PolicyDomain) -> bool {
        self.facts.iter().any(|fact| fact.domain == domain)
    }

    pub fn subjects_in(&self, domain: PolicyDomain) -> Vec<&str> {
        self.facts
            .iter()
            .filter(|fact| fact.domain == domain)
            .map(|fact| fact.subject.as_str())
            .collect()
    }
}

/// Parse source and collect the shared static-guarantee fact graph.
pub fn collect_policy_facts(src: &str) -> Result<PolicyFactGraph, Vec<Diagnostic>> {
    let (toks, lex_diags) = Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let program = Parser::parse(&toks)?;
    let mut graph = collect_policy_facts_from_program(&program);
    // Value-fact tags are type-transparent; surface them from source so the
    // shared graph records introduction sites alongside `#Sanitizer` sinks.
    if src.contains("#Tainted") {
        graph.record(
            PolicyDomain::Taint,
            "source",
            "#Tainted value-fact enters the shared IFC/taint lattice",
        );
    }
    Ok(graph)
}

pub fn collect_policy_facts_from_program(program: &Program) -> PolicyFactGraph {
    let mut graph = PolicyFactGraph::default();
    for declaration in &program.policy_declarations {
        if matches!(
            declaration.key,
            crate::Policy::PolicyKey::NoAlloc
                | crate::Policy::PolicyKey::ZeroRc
                | crate::Policy::PolicyKey::ArenaBounded
        ) {
            graph.record(
                PolicyDomain::Memory,
                declaration.key.name(),
                format!(
                    "{} {} at {}..{}",
                    declaration.scope.name(),
                    declaration.value.display(),
                    declaration.span.start,
                    declaration.span.end
                ),
            );
        }
    }
    collect_items(&mut graph, &program.items);
    graph
}

fn collect_items(graph: &mut PolicyFactGraph, items: &[Item]) {
    for item in items {
        match item {
            Item::Distinct(def) => {
                if let Some((lo, hi, _)) = def.range {
                    graph.record(
                        PolicyDomain::Refinement,
                        def.name.clone(),
                        format!("#Invariant / range proves value in {lo}..{hi}"),
                    );
                    graph.record(
                        PolicyDomain::Bounds,
                        def.name.clone(),
                        "range-refined distinct feeds D-OOBPROOF1 fixed-list indexing",
                    );
                }
            }
            Item::Func(func) => collect_func(graph, func),
            Item::Impl(imp) => {
                for method in &imp.methods {
                    collect_func(graph, method);
                }
            }
            Item::Struct(def) => {
                for method in &def.methods {
                    collect_func(graph, method);
                }
                for block in &def.trait_impls {
                    for method in &block.methods {
                        collect_func(graph, method);
                    }
                }
            }
            Item::Enum(def) => {
                for method in &def.methods {
                    collect_func(graph, method);
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_items(graph, body);
                }
            }
            _ => {}
        }
    }
}

fn collect_func(graph: &mut PolicyFactGraph, func: &Func) {
    if !func.pre.is_empty() || !func.post.is_empty() {
        graph.record(
            PolicyDomain::Contract,
            func.name.clone(),
            format!("#Pre×{} #Post×{}", func.pre.len(), func.post.len()),
        );
    }
    if func.is_sanitizer {
        graph.record(
            PolicyDomain::Taint,
            func.name.clone(),
            "#Sanitizer clears taint before sinks (D-TAINT1)",
        );
    }
    if func.is_replayable {
        graph.record(
            PolicyDomain::Replay,
            func.name.clone(),
            "#Replayable forbids ambient Time/Rand/Net/Io (D-REPLAY1)",
        );
    }
    if let Some(effects) = func.declared_effects.as_ref() {
        if !effects.is_empty() {
            let names: Vec<&str> = effects.iter().map(|(name, _)| name.as_str()).collect();
            graph.record(
                PolicyDomain::Budget,
                func.name.clone(),
                format!(
                    "declared effect bound =[{}]=> feeds D-EFFBUDGET1",
                    names.join(", ")
                ),
            );
        }
    }
    if func_has_fixed_list_param(func) && func_has_named_index_param(func) {
        graph.record(
            PolicyDomain::Bounds,
            func.name.clone(),
            "fixed-list param + refined index param share D-OOBPROOF1 facts",
        );
    }
}

fn func_has_fixed_list_param(func: &Func) -> bool {
    func.params
        .iter()
        .any(|param| matches!(param.ty, Type::FixedList { .. }))
}

fn func_has_named_index_param(func: &Func) -> bool {
    func.params
        .iter()
        .any(|param| matches!(param.ty, Type::Named(_)))
}
