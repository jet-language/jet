//! D-FACT-GATE1=A: one compile-time read model for every written gate.
//!
//! Source markers and policy data remain the writers. This module only merges
//! their already-parsed facts, keeps provenance, and exposes stable projections
//! to inspect commands. No entry is carried into TIR or generated Rust.

use crate::Diagnostics::Span;
use crate::Policy::GateSet;
use crate::AST::{Expr, Func, Item, ProgramBundle, Stmt, StrPart};
pub use jet_foundation::Authority::{GateDiagnostic, GateEntry, GateKind, GateOperation};

#[derive(Debug, Clone, Default)]
pub struct GateLedger {
    inner: jet_foundation::Authority::GateLedger,
}

impl GateLedger {
    /// Collect the parser/sema-visible source gates and the existing unsafe
    /// obligation report. External readers append lock, trust, and invocation
    /// facts with [`Self::push`].
    pub fn collect(bundle: &ProgramBundle, gates: GateSet) -> Self {
        let inspection = crate::Sema::UnsafeObligations::inspect_with_gates(bundle, gates);
        let mut ledger = Self::default();
        for gate in inspection.gates {
            let discharged = gate.operations.iter().all(|operation| operation.discharged);
            let mut provenance = gate.provenance;
            provenance.push(format!(
                "{}:{}..{}",
                gate.source, gate.span.start, gate.span.end
            ));
            ledger.push(GateEntry {
                kind: GateKind::Unsafe,
                domain: "security".to_string(),
                scope: "source".to_string(),
                source: gate.source,
                span: Some(gate.span),
                subject: "#Unsafe".to_string(),
                reason: gate.reason,
                status: Some(if discharged { "discharged" } else { "missing" }.to_string()),
                detail: format!("mode={}", gate.mode),
                provenance,
                operations: gate
                    .operations
                    .into_iter()
                    .map(|operation| GateOperation {
                        kind: operation.kind,
                        span: operation.span,
                        required: operation.required,
                        asserted: operation.asserted,
                        discharged: operation.discharged,
                    })
                    .collect(),
            });
        }
        ledger.set_diagnostics(
            inspection
                .diagnostics
                .into_iter()
                .map(|entry| GateDiagnostic {
                    source: entry.source,
                    diagnostic: entry.diagnostic,
                })
                .collect(),
        );

        for module in &bundle.modules {
            visit_statements(&module.display, &module.script_body, &mut ledger);
            visit_items(&module.display, &module.items, &mut ledger);
        }
        append_fact_gates(&mut ledger, &bundle.build_facts);
        ledger.append_structure_facts(&bundle.name_ledger);
        ledger.sort();
        ledger
    }

    pub fn entries(&self) -> &[GateEntry] {
        self.inner.entries()
    }

    pub fn diagnostics(&self) -> &[GateDiagnostic] {
        self.inner.diagnostics()
    }

    pub fn set_diagnostics(&mut self, diagnostics: Vec<GateDiagnostic>) {
        self.inner.set_diagnostics(diagnostics);
    }

    /// Add one writer's fact while coalescing the same fact with another
    /// provenance source. The ledger never drops provenance.
    pub fn push(&mut self, entry: GateEntry) {
        self.inner.push(entry);
    }

    pub fn sort(&mut self) {
        self.inner.sort();
    }

    /// Project structure exceptions into the same gate ledger as every other
    /// written escape. A structure fact with no gate tightens silently and is
    /// deliberately absent from this projection.
    pub fn append_structure_facts(&mut self, facts: &jet_foundation::Names::NameLedger) {
        for fact in facts.structure_facts() {
            let Some(gate) = fact.gate.as_deref() else {
                continue;
            };
            self.push(GateEntry {
                kind: GateKind::Structure,
                domain: "structure".to_string(),
                scope: fact.kind.name().to_string(),
                source: fact.source.clone(),
                span: Some(fact.span),
                subject: fact.subject.clone(),
                reason: Some(gate.to_string()),
                status: Some(fact.status.clone()),
                detail: fact.detail.clone(),
                provenance: vec![format!(
                    "{}:{}..{}",
                    fact.source, fact.span.start, fact.span.end
                )],
                operations: Vec::new(),
            });
        }
    }
}

/// D-FACT-GATE1=A: a system/fleet `.Force` writer is a build gate, so it is
/// recorded in the same ledger as every other written move away from safety.
/// The effective snapshot already owns the complete provenance chain; this
/// adapter only projects the forced writers into the ledger's build kind.
fn append_fact_gates(ledger: &mut GateLedger, facts: &jet_foundation::Facts::BuildFactSnapshot) {
    for fact in facts.contributions.values() {
        for contribution in fact
            .provenance
            .iter()
            .filter(|contribution| contribution.force)
        {
            ledger.push(GateEntry {
                kind: GateKind::BuildFlag,
                domain: "build".to_string(),
                scope: contribution.layer.name().to_string(),
                source: contribution.source.clone(),
                span: Some(contribution.span),
                subject: fact.key.name.clone(),
                reason: contribution
                    .force_reason
                    .clone()
                    .or_else(|| Some(".Force".to_string())),
                status: Some("recorded".to_string()),
                detail: format!(".Force {}", contribution.value.display()),
                provenance: vec![format!(
                    "{}:{}..{}",
                    contribution.source, contribution.span.start, contribution.span.end
                )],
                operations: Vec::new(),
            });
        }
    }
}

fn source_entry(
    kind: GateKind,
    domain: &str,
    scope: &str,
    source: &str,
    span: Span,
    subject: &str,
    reason: Option<String>,
    detail: &str,
    status: &str,
) -> GateEntry {
    GateEntry {
        kind,
        domain: domain.to_string(),
        scope: scope.to_string(),
        source: source.to_string(),
        span: Some(span),
        subject: subject.to_string(),
        reason,
        status: Some(status.to_string()),
        detail: detail.to_string(),
        provenance: vec![format!("{}:{}..{}", source, span.start, span.end)],
        operations: Vec::new(),
    }
}

fn visit_items(source: &str, items: &[Item], ledger: &mut GateLedger) {
    for item in items {
        match item {
            Item::Func(function) => visit_function(source, function, ledger),
            Item::Const(constant) => visit_expression(source, &constant.value, ledger),
            Item::Struct(definition) => {
                for function in &definition.methods {
                    visit_function(source, function, ledger);
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        visit_function(source, function, ledger);
                    }
                }
                visit_statements(source, &definition.validate_block, ledger);
                for field in &definition.fields {
                    if let Some(expression) = &field.computed {
                        visit_expression(source, expression, ledger);
                    }
                    if let Some(expression) = &field.default {
                        visit_expression(source, expression, ledger);
                    }
                }
            }
            Item::Enum(definition) => {
                for function in &definition.methods {
                    visit_function(source, function, ledger);
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        visit_function(source, function, ledger);
                    }
                }
            }
            Item::Impl(implementation) => {
                for function in &implementation.methods {
                    visit_function(source, function, ledger);
                }
            }
            Item::Test(test) => visit_statements(source, &test.body, ledger),
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    visit_items(source, body, ledger);
                }
            }
            Item::GenericModule(module) => visit_items(source, &module.body, ledger),
            Item::Module(module) => {
                for expression in &module.imports {
                    visit_expression(source, expression, ledger);
                }
                for expression in &module.members {
                    visit_expression(source, expression, ledger);
                }
                for contribution in &module.contributions {
                    if let crate::AST::ContribValue::Expr(expression) = &contribution.value {
                        visit_expression(source, expression, ledger);
                    }
                }
            }
            Item::MarkerDecl(marker) => {
                if let Some(body) = &marker.body {
                    visit_template_body(source, body, ledger);
                }
                // D-BOUND-SINK1=A: checked text contracts are source code too.
                // Walk both comptime expressions so an audited `.raw()` in a
                // library-declared check or hole remains visible in the one
                // precision-demotion ledger.
                if let Some(text) = &marker.text {
                    visit_expression(source, &text.check, ledger);
                    visit_expression(source, &text.hole, ledger);
                }
            }
            _ => {}
        }
    }
}

/// D-META-CODE1=A / D-META-USER1=A: marker and derive bodies retain typed
/// item templates until expansion. Walk both branches here so gate inspection
/// sees expressions in generated items and compile-time control flow without
/// inventing a source-string parser path.
fn visit_template_body(source: &str, body: &[crate::AST::DeriveBodyItem], ledger: &mut GateLedger) {
    for body_item in body {
        match body_item {
            crate::AST::DeriveBodyItem::Stmt(statement) => {
                visit_statements(source, std::slice::from_ref(statement), ledger);
            }
            crate::AST::DeriveBodyItem::Item(item) => {
                visit_items(source, std::slice::from_ref(item.as_ref()), ledger);
            }
            crate::AST::DeriveBodyItem::Loop {
                source: expression,
                body,
                ..
            } => {
                visit_expression(source, expression, ledger);
                visit_template_body(source, body, ledger);
            }
        }
    }
}

fn visit_function(source: &str, function: &Func, ledger: &mut GateLedger) {
    if let Some(transition) = &function.state_transition {
        let from = transition.from.as_deref().unwrap_or("_");
        let spelling = format!("#Transition({from}, {})", transition.to);
        let detail = format!("state transition {from} -> {}", transition.to);
        ledger.push(source_entry(
            GateKind::StateTransition,
            "knowledge",
            "function",
            source,
            transition.span,
            &spelling,
            Some(spelling.clone()),
            &detail,
            "recorded",
        ));
    }
    if let Some(tag) = &function.scrub_tag {
        let span = function
            .markers
            .iter()
            .find(|marker| marker.name == crate::Syntax::KW_SCRUB)
            .map(|marker| marker.span)
            .unwrap_or(function.name_span);
        ledger.push(source_entry(
            GateKind::TaintScrub,
            "knowledge",
            "function",
            source,
            span,
            &format!("#Scrub({tag})"),
            None,
            &format!("taint removed: {tag}"),
            "cleared",
        ));
    }
    for marker in &function.markers {
        let (kind, subject, detail) = match marker.name.as_str() {
            name if name == crate::Syntax::MARKER_NONDETERMINISTIC => (
                GateKind::Nondeterministic,
                "#Nondeterministic",
                "determinism escape",
            ),
            name if name == crate::Syntax::KW_IMPURE => {
                (GateKind::Impure, "#Impure", "impure function")
            }
            _ => continue,
        };
        ledger.push(source_entry(
            kind,
            "security",
            "function",
            source,
            marker.span,
            subject,
            literal_text(marker.args.first()),
            detail,
            "recorded",
        ));
    }
    visit_statements(source, &function.body, ledger);
}

fn visit_statements(source: &str, body: &[Stmt], ledger: &mut GateLedger) {
    for statement in body {
        visit_statement_expressions(source, statement, ledger);
    }
    visit_statement_gates(source, body, ledger);
}

fn visit_statement_expressions(source: &str, statement: &Stmt, ledger: &mut GateLedger) {
    let mut copy = statement.clone();
    copy.for_each_expr_mut(|expression| visit_expression_value(source, expression, ledger));
}

fn visit_expression(source: &str, expression: &Expr, ledger: &mut GateLedger) {
    let mut copy = expression.clone();
    copy.for_each_expr_mut(|value| visit_expression_value(source, value, ledger));
}

fn visit_expression_value(source: &str, expression: &Expr, ledger: &mut GateLedger) {
    match expression {
        Expr::Call(call) if call.widen_approx || call.name == crate::Syntax::BUILTIN_APPROX => {
            ledger.push(source_entry(
                GateKind::PrecisionDemotion,
                "knowledge",
                "expression",
                source,
                call.name_span,
                "approx",
                None,
                "exact value widened to an approximation",
                "recorded",
            ));
        }
        Expr::Call(call)
            if matches!(
                call.name.as_str(),
                crate::Syntax::BUILTIN_WRAPPING
                    | crate::Syntax::BUILTIN_SATURATING
                    | crate::Syntax::BUILTIN_CHECKED
            ) =>
        {
            let detail = match call.name.as_str() {
                crate::Syntax::BUILTIN_SATURATING => {
                    "overflow fact replaced by saturating arithmetic"
                }
                crate::Syntax::BUILTIN_CHECKED => "overflow fact represented as an optional result",
                _ => "overflow fact replaced by wrapping arithmetic",
            };
            ledger.push(source_entry(
                GateKind::PrecisionDemotion,
                "knowledge",
                "expression",
                source,
                call.name_span,
                &call.name,
                None,
                detail,
                "recorded",
            ));
        }
        Expr::MethodCall {
            method,
            method_span,
            args,
            ..
        } if is_rounded_conversion(method, args) => {
            ledger.push(source_entry(
                GateKind::PrecisionDemotion,
                "knowledge",
                "expression",
                source,
                *method_span,
                method,
                None,
                "unit conversion rounded with an explicit mode",
                "recorded",
            ));
        }
        Expr::MethodCall {
            method,
            method_span,
            args,
            ..
        } if method == crate::Syntax::METHOD_DROP => {
            ledger.push(source_entry(
                GateKind::DutyDrop,
                "duty",
                "expression",
                source,
                *method_span,
                ".drop",
                args.first()
                    .and_then(|argument| literal_text(Some(&argument.expr))),
                "result duty explicitly discharged",
                "discharged",
            ));
        }
        Expr::MethodCall {
            method,
            method_span,
            ..
        } if method == crate::Syntax::TASK_DETACH => {
            ledger.push(source_entry(
                GateKind::DutyDrop,
                "duty",
                "expression",
                source,
                *method_span,
                "detach",
                None,
                "join duty explicitly abandoned",
                "discharged",
            ));
        }
        Expr::MethodCall {
            method,
            method_span,
            ..
        } if method == "raw" => {
            ledger.push(source_entry(
                GateKind::PrecisionDemotion,
                "knowledge",
                "expression",
                source,
                *method_span,
                ".raw",
                None,
                "typed fact extracted as a raw value",
                "recorded",
            ));
        }
        Expr::RawOf(_, span) => {
            ledger.push(source_entry(
                GateKind::PrecisionDemotion,
                "knowledge",
                "expression",
                source,
                *span,
                ".raw",
                None,
                "typed fact extracted as a raw value",
                "recorded",
            ));
        }
        _ => {}
    }
}

fn is_rounded_conversion(method: &str, args: &[crate::AST::CallArg]) -> bool {
    // The checker owns validity of the source, mode, and result. The ledger
    // only recognizes the canonical destination-owned spelling and its
    // required `digits:` slot, so it does not become a second authority.
    method.starts_with("from_")
        && method.ends_with("_rounded")
        && args.len() == 3
        && args
            .iter()
            .any(|arg| matches!(arg.label.as_ref(), Some((label, _)) if label == "digits"))
}

fn literal_text(expression: Option<&Expr>) -> Option<String> {
    let Some(Expr::Str(parts, _)) = expression else {
        return None;
    };
    let mut text = String::new();
    for part in parts {
        let StrPart::Lit(value) = part else {
            return None;
        };
        text.push_str(value);
    }
    Some(text)
}

fn visit_statement_gates(source: &str, body: &[Stmt], ledger: &mut GateLedger) {
    for statement in body {
        match statement {
            Stmt::Impure { reason, span, .. } => ledger.push(source_entry(
                GateKind::Impure,
                "security",
                "block",
                source,
                *span,
                "#Impure",
                reason.clone(),
                "ambient comptime effect gate",
                if reason.is_some() {
                    "recorded"
                } else {
                    "missing"
                },
            )),
            Stmt::Caps { caps, span, .. } => ledger.push(source_entry(
                GateKind::DependencyGrant,
                "security",
                "block",
                source,
                *span,
                "#Abilities",
                None,
                &format!(
                    "abilities: {}",
                    caps.iter()
                        .map(|(name, _)| name.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                "recorded",
            )),
            Stmt::AssumeDet { reason, span, .. } => ledger.push(source_entry(
                GateKind::Nondeterministic,
                "security",
                "block",
                source,
                *span,
                "assume_deterministic",
                Some(reason.clone()),
                "determinism escape",
                "recorded",
            )),
            _ => {}
        }
        if let Stmt::Unsafe { body, .. } = statement {
            visit_statement_gates(source, body, ledger);
        } else {
            for nested in crate::Sema::UnsafeObligations::nested_bodies(statement) {
                visit_statement_gates(source, nested, ledger);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: GateKind, source: &str, span: usize) -> GateEntry {
        source_entry(
            kind,
            "test",
            "test",
            source,
            Span::new(span, span + 1),
            "x",
            None,
            "x",
            "recorded",
        )
    }

    #[test]
    fn security_entries_sort_before_knowledge_entries() {
        let mut ledger = GateLedger::default();
        ledger.push(entry(GateKind::PrecisionDemotion, "a.jet", 1));
        ledger.push(entry(GateKind::Unsafe, "a.jet", 2));
        ledger.sort();
        assert_eq!(ledger.entries()[0].kind, GateKind::Unsafe);
    }

    #[test]
    fn duplicate_fact_keeps_both_provenance_rows() {
        let mut ledger = GateLedger::default();
        let mut first = entry(GateKind::TrustGrant, "trust", 0);
        first.provenance = vec!["trust store".to_string()];
        let mut second = first.clone();
        second.provenance = vec!["lockfile".to_string()];
        ledger.push(first);
        ledger.push(second);
        assert_eq!(ledger.entries().len(), 1);
        assert_eq!(ledger.entries()[0].provenance.len(), 2);
    }

    #[test]
    fn structure_facts_use_the_same_gate_ledger() {
        let mut names = jet_foundation::Names::NameLedger::default();
        names.record_structure_fact(jet_foundation::Names::StructureFact::new(
            jet_foundation::Names::StructureFactKind::Liveness,
            "files",
            "app.jet",
            Span::new(4, 9),
            "unused",
            "remove or rename to _files",
            Some("_name".to_string()),
        ));

        let mut ledger = GateLedger::default();
        ledger.append_structure_facts(&names);

        assert_eq!(ledger.entries().len(), 1);
        assert_eq!(ledger.entries()[0].kind, GateKind::Structure);
        assert_eq!(ledger.entries()[0].domain, "structure");
        assert_eq!(ledger.entries()[0].reason.as_deref(), Some("_name"));
    }

    #[test]
    fn forced_build_fact_is_one_build_gate_entry() {
        let span = Span::new(7, 12);
        let contribution = jet_foundation::Policy::FactContribution::new(
            "Build.Settings.tls",
            jet_foundation::Policy::FactValue::Bool(false),
            jet_foundation::Policy::SourceScope::Package,
            jet_foundation::Policy::ContributionLayer::System,
            "system.jet",
        )
        .at(span)
        .force_with_reason("fleet certification");
        let fact = jet_foundation::Policy::resolve(
            jet_foundation::Policy::FactKey::new("Build.Settings.tls"),
            [contribution],
        )
        .expect("system force is a valid build contribution")
        .expect("the forced writer resolves");
        let mut facts = jet_foundation::Facts::BuildFactSnapshot::default();
        facts.contributions.insert(fact.key.name.clone(), fact);

        let mut ledger = GateLedger::default();
        append_fact_gates(&mut ledger, &facts);

        assert_eq!(ledger.entries().len(), 1);
        let entry = &ledger.entries()[0];
        assert_eq!(entry.kind, GateKind::BuildFlag);
        assert_eq!(entry.domain, "build");
        assert_eq!(entry.subject, "Build.Settings.tls");
        assert_eq!(entry.source, "system.jet");
        assert_eq!(entry.reason.as_deref(), Some("fleet certification"));
        assert_eq!(entry.span, Some(span));
    }
}
