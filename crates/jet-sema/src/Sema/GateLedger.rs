//! D-FACT-GATE1=A: one compile-time read model for every written gate.
//!
//! Source markers and policy data remain the writers. This module only merges
//! their already-parsed facts, keeps provenance, and exposes stable projections
//! to inspect commands. No entry is carried into TIR or generated Rust.

use crate::AST::{Expr, Func, Item, ProgramBundle, Stmt, StrPart};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Policy::GateSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GateKind {
    Unsafe,
    Impure,
    DependencyGrant,
    BuildFlag,
    SessionFlag,
    TrustGrant,
    ForcePin,
    TaintScrub,
    DutyDrop,
    PrecisionDemotion,
    Nondeterministic,
}

impl GateKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unsafe => "unsafe",
            Self::Impure => "impure",
            Self::DependencyGrant => "dependency_grant",
            Self::BuildFlag => "build_flag",
            Self::SessionFlag => "session_flag",
            Self::TrustGrant => "trust_grant",
            Self::ForcePin => "force_pin",
            Self::TaintScrub => "taint_scrub",
            Self::DutyDrop => "duty_drop",
            Self::PrecisionDemotion => "precision_demotion",
            Self::Nondeterministic => "nondeterministic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "unsafe" | "unsafe_region" | "unsafe_fn" => Some(Self::Unsafe),
            "impure" => Some(Self::Impure),
            "dependency" | "dependency_grant" | "grant" => Some(Self::DependencyGrant),
            "build" | "build_flag" => Some(Self::BuildFlag),
            "session" | "session_flag" => Some(Self::SessionFlag),
            "trust" | "trust_grant" => Some(Self::TrustGrant),
            "force" | "force_pin" => Some(Self::ForcePin),
            "scrub" | "taint" | "taint_scrub" => Some(Self::TaintScrub),
            "drop" | "detach" | "duty" | "duty_drop" => Some(Self::DutyDrop),
            "approx"
            | "precision"
            | "precision_demotion"
            | "rounded"
            | "wrapping"
            | "saturating"
            | "checked" => Some(Self::PrecisionDemotion),
            "nondeterministic" | "determinism" => Some(Self::Nondeterministic),
            _ => None,
        }
    }

    pub const fn is_security(self) -> bool {
        matches!(
            self,
            Self::Unsafe
                | Self::Impure
                | Self::DependencyGrant
                | Self::BuildFlag
                | Self::SessionFlag
                | Self::TrustGrant
                | Self::ForcePin
                | Self::Nondeterministic
        )
    }

    pub const fn is_rights_kind(self) -> bool {
        self.is_security()
    }

    const fn display_order(self) -> u8 {
        match self {
            Self::Unsafe => 0,
            Self::Impure => 1,
            Self::Nondeterministic => 2,
            Self::DependencyGrant => 3,
            Self::BuildFlag => 4,
            Self::SessionFlag => 5,
            Self::TrustGrant => 6,
            Self::ForcePin => 7,
            Self::TaintScrub => 8,
            Self::DutyDrop => 9,
            Self::PrecisionDemotion => 10,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateOperation {
    pub kind: String,
    pub span: Span,
    pub required: Vec<String>,
    pub asserted: Vec<String>,
    pub discharged: bool,
}

#[derive(Debug, Clone)]
pub struct GateEntry {
    pub kind: GateKind,
    /// Broad fact plane used by `--scope` and by the human security-first view.
    pub domain: String,
    /// Lexical/build scope of the written gate (`function`, `block`, `package`, …).
    pub scope: String,
    pub source: String,
    pub span: Option<Span>,
    pub subject: String,
    pub reason: Option<String>,
    pub status: Option<String>,
    pub detail: String,
    pub provenance: Vec<String>,
    pub operations: Vec<GateOperation>,
}

#[derive(Debug, Clone)]
pub struct GateDiagnostic {
    pub source: String,
    pub diagnostic: Diagnostic,
}

#[derive(Debug, Clone, Default)]
pub struct GateLedger {
    entries: Vec<GateEntry>,
    diagnostics: Vec<GateDiagnostic>,
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
            provenance.push(format!("{}:{}..{}", gate.source, gate.span.start, gate.span.end));
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
        ledger.diagnostics = inspection
            .diagnostics
            .into_iter()
            .map(|entry| GateDiagnostic {
                source: entry.source,
                diagnostic: entry.diagnostic,
            })
            .collect();

        for module in &bundle.modules {
            visit_statements(&module.display, &module.script_body, &mut ledger);
            visit_items(&module.display, &module.items, &mut ledger);
        }
        ledger.sort();
        ledger
    }

    pub fn entries(&self) -> &[GateEntry] {
        &self.entries
    }

    pub fn diagnostics(&self) -> &[GateDiagnostic] {
        &self.diagnostics
    }

    /// Add one writer's fact while coalescing the same fact with another
    /// provenance source. The ledger never drops provenance.
    pub fn push(&mut self, mut entry: GateEntry) {
        if entry.provenance.is_empty() {
            entry.provenance.push(entry.source.clone());
        }
        if let Some(existing) = self.entries.iter_mut().find(|candidate| same_fact(candidate, &entry)) {
            for provenance in entry.provenance {
                if !existing.provenance.contains(&provenance) {
                    existing.provenance.push(provenance);
                }
            }
            if existing.reason.is_none() {
                existing.reason = entry.reason;
            }
            if existing.status.is_none() {
                existing.status = entry.status;
            }
            existing.provenance.sort();
            return;
        }
        entry.provenance.sort();
        self.entries.push(entry);
    }

    pub fn sort(&mut self) {
        self.entries.sort_by(|left, right| {
            (
                !left.kind.is_security(),
                left.kind.display_order(),
                left.kind.name(),
                left.source.as_str(),
                left.span.map(|span| span.start).unwrap_or(usize::MAX),
                left.span.map(|span| span.end).unwrap_or(usize::MAX),
                left.subject.as_str(),
                left.detail.as_str(),
            )
                .cmp(&(
                    !right.kind.is_security(),
                    right.kind.display_order(),
                    right.kind.name(),
                    right.source.as_str(),
                    right.span.map(|span| span.start).unwrap_or(usize::MAX),
                    right.span.map(|span| span.end).unwrap_or(usize::MAX),
                    right.subject.as_str(),
                    right.detail.as_str(),
                ))
        });
    }
}

fn same_fact(left: &GateEntry, right: &GateEntry) -> bool {
    left.kind == right.kind
        && left.domain == right.domain
        && left.scope == right.scope
        && left.subject == right.subject
        && left.detail == right.detail
        && match (left.span, right.span) {
            (None, None) => true,
            (Some(left_span), Some(right_span)) => {
                left_span == right_span && left.source == right.source
            }
            _ => false,
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
            Item::Bench(bench) => visit_statements(source, &bench.body, ledger),
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
                    visit_statements(source, body, ledger);
                }
            }
            _ => {}
        }
    }
}

fn visit_function(source: &str, function: &Func, ledger: &mut GateLedger) {
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
            name if name == crate::Syntax::KW_IMPURE => (GateKind::Impure, "#Impure", "impure function"),
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
        Expr::Call(call)
            if call.widen_approx || call.name == crate::Syntax::BUILTIN_APPROX =>
        {
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
                crate::Syntax::BUILTIN_CHECKED => {
                    "overflow fact represented as an optional result"
                }
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
                args.first().and_then(|argument| literal_text(Some(&argument.expr))),
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
        && args.iter().any(|arg| {
            matches!(arg.label.as_ref(), Some((label, _)) if label == "digits")
        })
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
                if reason.is_some() { "recorded" } else { "missing" },
            )),
            Stmt::Grant { caps, span, .. } => ledger.push(source_entry(
                GateKind::DependencyGrant,
                "security",
                "block",
                source,
                *span,
                "#grant",
                None,
                &format!(
                    "capabilities: {}",
                    caps.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(",")
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
        source_entry(kind, "test", "test", source, Span::new(span, span + 1), "x", None, "x", "recorded")
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
}
