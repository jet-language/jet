//! Declared fact-tag dataflow (D-FACTMODEL1=A, D-TAG-SURFACE1=A).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Effects::core_effect;
use crate::Sema::FlowFacts::{Facts, Plane};
use crate::AST::{
    EnumLitArg, Expr, ForKind, Item, LValue, Lambda, LambdaBody, OrFallback, Stmt, StrPart, Type,
};
use std::collections::{BTreeSet, HashMap};

pub type TagSet = BTreeSet<String>;
pub type FieldTags = HashMap<(String, String), TagSet>;
pub type FieldTypes = HashMap<(String, String), String>;
pub type ReturnTypes = HashMap<String, String>;

/// D-TAG-SURFACE1=A: credential log/print/serialize sinks. A `#Credential`
/// value reaching `core.io.print`, `core.io.eprint`, `jet.log.*`, or
/// `core.encoding.*.to_string*` is E0722.
fn is_credential_sink(module: &str, method: &str) -> bool {
    match module {
        "core.io" => matches!(method, "print" | "eprint"),
        "jet.log" | "core.log" => true, // all log methods are credential sinks
        "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
        | "core.encoding.yaml" | "core.encoding.cbor" | "core.encoding.xml" => {
            matches!(method, "to_string" | "to_string_pretty" | "to_bytes" | "to_bytes_canonical")
        }
        _ => false,
    }
}

/// Per-function taint analyzer. Carries the program-level facts (which functions
/// are exact-tag scrubbers, how Core aliases resolve to modules) and the running set of
/// tainted locals while it walks one function body.
struct TaintCtx<'a> {
    scrubbers: &'a HashMap<String, String>,
    facts: &'a jet_foundation::Facts::FactRegistry,
    returns: &'a HashMap<String, TagSet>,
    return_types: &'a ReturnTypes,
    field_tags: &'a FieldTags,
    field_types: &'a FieldTypes,
    /// Core import aliases in scope for the module owning this body
    /// (alias → resolved module path, e.g. `db` → `jet.db`). Used to classify a
    /// `MethodCall` on a Core alias as a sink.
    core_imports: &'a HashMap<String, String>,
    /// The taint plane of the one flow-fact store.
    locals: Facts<Taint>,
    /// The type each local carries, so a field read can find its tags.
    local_types: Facts<LocalType>,
    diags: Vec<Diagnostic>,
    /// Diagnostics CheckerCore already produced before this pass runs — read
    /// only to check whether a switch was already proven exhaustive
    /// (`FlowFacts::switch_proven_exhaustive`); never written here.
    existing_diags: &'a [Diagnostic],
}

/// D-TAG-SURFACE1: the fact tags a value carries. Suspicion is a hazard, so it
/// spreads across a merge: a value tainted on any path is tainted after it.
pub(crate) enum Taint {}

impl Plane for Taint {
    type Fact = TagSet;

    fn join(left: Option<&TagSet>, right: Option<&TagSet>) -> Option<TagSet> {
        match (left, right) {
            (Some(left), Some(right)) => Some(left.union(right).cloned().collect()),
            (Some(one), None) | (None, Some(one)) => Some(one.clone()),
            (None, None) => None,
        }
    }
}

/// The declared type of a local, tracked beside its tags so a field read can
/// look up the owner's field tags.
pub(crate) enum LocalType {}

impl Plane for LocalType {
    type Fact = String;

    fn join(left: Option<&String>, right: Option<&String>) -> Option<String> {
        match (left, right) {
            (Some(left), Some(right)) if left == right => Some(left.clone()),
            (Some(one), None) | (None, Some(one)) => Some(one.clone()),
            _ => None,
        }
    }
}

/// Both taint planes as one path leaves them.
#[derive(Clone)]
struct TaintFacts(Facts<Taint>, Facts<LocalType>);

impl<'a> TaintCtx<'a> {
    fn new(
        scrubbers: &'a HashMap<String, String>,
        facts: &'a jet_foundation::Facts::FactRegistry,
        returns: &'a HashMap<String, TagSet>,
        return_types: &'a ReturnTypes,
        field_tags: &'a FieldTags,
        field_types: &'a FieldTypes,
        core_imports: &'a HashMap<String, String>,
        existing_diags: &'a [Diagnostic],
    ) -> Self {
        TaintCtx {
            scrubbers,
            facts,
            returns,
            return_types,
            field_tags,
            field_types,
            core_imports,
            locals: Facts::new(),
            local_types: Facts::new(),
            diags: Vec::new(),
            existing_diags,
        }
    }

    fn union<'e>(&self, expressions: impl IntoIterator<Item = &'e Expr>) -> TagSet {
        let mut tags = TagSet::new();
        for expression in expressions {
            tags.extend(self.tags_of(expression));
        }
        tags
    }

    fn source_tags(&self, destinations: &[String]) -> TagSet {
        self.facts
            .iter_kind(jet_foundation::Facts::FactKind::Tag)
            .filter(|fact| {
                fact.from.iter().any(|source| {
                    destinations.iter().any(|destination| {
                        jet_foundation::Facts::fact_covers(source, destination)
                    })
                })
            })
            .map(|fact| fact.name.clone())
            .collect()
    }

    fn method_destinations(
        &self,
        receiver: &Expr,
        method: &str,
        recv_type: Option<&str>,
    ) -> Vec<String> {
        let mut destinations = Vec::new();
        if let Some(owner) = recv_type {
            destinations.push(format!("{owner}.{method}"));
            destinations.push(format!("{owner}::{method}"));
        }
        let Expr::Ident(alias, _) = receiver else {
            return destinations;
        };
        let Some(module) = self.core_imports.get(alias) else {
            destinations.push(format!("{alias}.{method}"));
            return destinations;
        };
        destinations.push(format!("{module}.{method}"));
        if let Some(effect) = core_effect(module, method) {
            destinations.push(effect.name().to_string());
        }
        if is_credential_sink(module, method) {
            destinations.push("Log".to_string());
        }
        destinations
    }

    fn type_name(ty: &Type) -> Option<String> {
        match ty {
            Type::Named(name) => Some(name.clone()),
            Type::Apply { name, .. } => Some(name.clone()),
            Type::Tagged { inner, .. } => Self::type_name(inner),
            _ => None,
        }
    }

    fn type_of(&self, expression: &Expr) -> Option<String> {
        match expression {
            Expr::Ident(name, _) => self.local_types.get(name).cloned(),
            Expr::StructLit { type_name, .. } => Some(type_name.clone()),
            Expr::Call(call) => self.return_types.get(&call.name).cloned(),
            Expr::MethodCall { recv_type, method, .. } => recv_type
                .as_ref()
                .and_then(|owner| self.return_types.get(&format!("{owner}::{method}")))
                .cloned(),
            Expr::Field(base, field, _) | Expr::OptField { base, member: field, .. } => {
                let owner = self.type_of(base)?;
                self.field_types.get(&(owner, field.clone())).cloned()
            }
            Expr::Paren(inner, _)
            | Expr::Copy(inner, _)
            | Expr::Present(inner, _)
            | Expr::Ok(inner, _)
            | Expr::Err(inner, _)
            | Expr::Try(inner, _, _) => self.type_of(inner),
            _ => None,
        }
    }

    fn tags_of(&self, expression: &Expr) -> TagSet {
        match expression {
            Expr::Tainted(inner, tag, _) => {
                let mut tags = self.tags_of(inner);
                tags.insert(tag.clone().unwrap_or_else(|| "Input".to_string()));
                tags
            }
            Expr::Ident(name, _) => self.locals.get(name).cloned().unwrap_or_default(),
            Expr::Call(call) => {
                let mut tags = self.union(call.args.iter().map(|argument| &argument.expr));
                tags.extend(self.source_tags(std::slice::from_ref(&call.name)));
                tags.extend(self.returns.get(&call.name).cloned().unwrap_or_default());
                if let Some(tag) = self.scrubbers.get(&call.name) {
                    tags.remove(tag);
                }
                tags
            }
            Expr::MethodCall { receiver, method, recv_type, args, .. } => {
                let mut tags = self.tags_of(receiver);
                tags.extend(self.union(args.iter().map(|argument| &argument.expr)));
                let destinations = self.method_destinations(receiver, method, recv_type.as_deref());
                tags.extend(self.source_tags(&destinations));
                let key = recv_type.as_ref().map(|ty| format!("{ty}::{method}"));
                if let Some(returned) = key.as_ref().and_then(|key| self.returns.get(key)) {
                    tags.extend(returned.iter().cloned());
                }
                if let Some(tag) = key.as_ref().and_then(|key| self.scrubbers.get(key)) {
                    tags.remove(tag);
                }
                // A verified token yields typed public claims, not the original
                // credential text. Core owns this declassification boundary.
                let verifies_credential = matches!(receiver.as_ref(), Expr::Ident(alias, _)
                    if self.core_imports.get(alias).map(String::as_str) == Some("core.auth"))
                    && matches!(method.as_str(), "verify_jwt" | "verify_paseto");
                if verifies_credential {
                    tags.remove(crate::Syntax::KW_CREDENTIAL);
                }
                tags
            }
            Expr::CallValue { callee, args, .. } => {
                let mut tags = self.tags_of(callee);
                tags.extend(self.union(args.iter().map(|argument| &argument.expr)));
                tags
            }
            Expr::Binary(_, left, right, _)
            | Expr::Range { start: left, end: right, .. } => self.union([left.as_ref(), right.as_ref()]),
            Expr::CompareChain { operands, .. } | Expr::ListLit(operands, _) => {
                self.union(operands)
            }
            Expr::Field(inner, field, _) => {
                let mut tags = self.tags_of(inner);
                if let Some(owner) = self.type_of(inner) {
                    tags.extend(
                        self.field_tags
                            .get(&(owner, field.clone()))
                            .into_iter()
                            .flat_map(|tags| tags.iter().cloned()),
                    );
                }
                tags
            }
            Expr::Unary(_, inner, _)
            | Expr::IncDec { operand: inner, .. }
            | Expr::Deref(inner, _)
            | Expr::RawOf(inner, _)
            | Expr::Copy(inner, _)
            | Expr::Place(inner, _, _)
            | Expr::Present(inner, _)
            | Expr::Ok(inner, _)
            | Expr::Err(inner, _)
            | Expr::Try(inner, _, _)
            | Expr::Paren(inner, _)
            | Expr::Spread(inner, _) => self.tags_of(inner),
            Expr::MemberSpread { base, .. } => self.tags_of(base),
            Expr::OptField { base, member, .. } => {
                let mut tags = self.tags_of(base);
                if let Some(owner) = self.type_of(base) {
                    tags.extend(
                        self.field_tags
                            .get(&(owner, member.clone()))
                            .into_iter()
                            .flat_map(|tags| tags.iter().cloned()),
                    );
                }
                tags
            }
            Expr::Index { base, index, .. } => self.union([base.as_ref(), index.as_ref()]),
            Expr::Slice {
                base,
                start,
                end,
                range,
                ..
            } => {
                let mut tags = self.tags_of(base);
                if let Some(range) = range {
                    tags.extend(self.tags_of(range));
                } else {
                    tags.extend(self.tags_of(start));
                    tags.extend(self.tags_of(end));
                }
                tags
            }
            Expr::MapLit(entries, _) => self.union(
                entries.iter().flat_map(|(key, value)| [key, value]),
            ),
            Expr::TupleLit(fields, _, _) => self.union(fields.iter().map(|(_, value)| value)),
            Expr::StructLit { fields, .. } => {
                self.union(fields.iter().map(|(_, _, value)| value))
            }
            Expr::TypedLit { body, .. } => {
                let mut tags = TagSet::new();
                body.for_each_expr(|value| tags.extend(self.tags_of(value)));
                tags
            }
            Expr::EnumLit { args, .. } => self.union(args.iter().map(|argument| match argument {
                EnumLitArg::Positional(value) => value,
                EnumLitArg::Named { expr, .. } => expr,
            })),
            Expr::Str(parts, _) => self.union(parts.iter().filter_map(|part| match part {
                StrPart::Interp(value, _) => Some(value.as_ref()),
                _ => None,
            })),
            Expr::OrFallback { value, fallback, .. } => {
                let mut tags = self.tags_of(value);
                if let OrFallback::Value(other) | OrFallback::Return(Some(other), _) = fallback {
                    tags.extend(self.tags_of(other));
                }
                tags
            }
            Expr::PatternTest { subject, .. } => self.tags_of(subject),
            Expr::If { then_value, else_value, .. } => {
                self.union([then_value.as_ref(), else_value.as_ref()])
            }
            Expr::PtrFromAddr { addr, .. } => self.tags_of(addr),
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Char(..)
            | Expr::Absent(_)
            | Expr::ReduceMarker(_, _)
            | Expr::Todo { .. }
        | Expr::NoElse(_)
            | Expr::Lambda(_)
            | Expr::UnitLit { .. }
            | Expr::ComptimeName { .. }
            | Expr::StrMatchLit(_, _)
            | Expr::BinMatchLit(_, _) => TagSet::new(),
        }
    }

    fn denied_tag(&self, tags: &TagSet, destinations: &[String]) -> Option<String> {
        tags.iter().find_map(|tag| {
            self.facts
                .get(jet_foundation::Facts::FactKind::Tag, tag)
                .and_then(|fact| {
                fact.deny.iter().any(|deny| {
                    destinations.iter().any(|destination| {
                        jet_foundation::Facts::fact_covers(deny, destination)
                    })
                }).then(|| tag.clone())
                })
        })
    }

    /// Walk an expression for sink violations:
    /// - E0721: a tainted (any kind) value reaches an injection sink (DB/Exec/Net).
    /// - E0722: a `#Credential` value reaches a log/print/serialize sink.
    /// Recurses into every sub-expression so a nested sink is still checked.
    fn check_expr(&mut self, e: &Expr) {
        match e {
            Expr::MethodCall {
                receiver,
                method,
                method_span,
                args,
                recv_type,
                ..
            } => {
                let destinations =
                    self.method_destinations(receiver, method, recv_type.as_deref());
                for argument in args {
                    if let Some(tag) = self.denied_tag(&self.tags_of(&argument.expr), &destinations) {
                        let api = match receiver.as_ref() {
                            Expr::Ident(alias, _) => format!("{alias}.{method}"),
                            _ => method.clone(),
                        };
                        self.diags.push(if tag == crate::Syntax::KW_CREDENTIAL
                            && destinations.iter().any(|destination| destination == "Log")
                        {
                            e0722(&api, *method_span)
                        } else {
                            e0721(&tag, &api, &destinations, *method_span)
                        });
                        break;
                    }
                }
                self.check_expr(receiver);
                for a in args {
                    self.check_expr(&a.expr);
                }
            }
            Expr::Tainted(inner, _, _) => self.check_expr(inner),
            Expr::Call(c) => {
                let destinations = if matches!(c.name.as_str(), "print" | "eprint") {
                    vec!["Log".to_string()]
                } else {
                    vec![c.name.clone()]
                };
                for argument in &c.args {
                    if let Some(tag) = self.denied_tag(&self.tags_of(&argument.expr), &destinations) {
                        self.diags.push(if tag == crate::Syntax::KW_CREDENTIAL
                            && destinations.iter().any(|destination| destination == "Log")
                        {
                            e0722(&c.name, c.name_span)
                        } else {
                            e0721(&tag, &c.name, &destinations, c.name_span)
                        });
                        break;
                    }
                }
                for a in &c.args {
                    self.check_expr(&a.expr);
                }
            }
            Expr::CallValue { callee, args, .. } => {
                self.check_expr(callee);
                for a in args {
                    self.check_expr(&a.expr);
                }
            }
            Expr::Binary(_, l, r, _) => {
                self.check_expr(l);
                self.check_expr(r);
            }
            Expr::CompareChain { operands, .. } => {
                for e in operands {
                    self.check_expr(e);
                }
            }
            Expr::Unary(_, inner, _)
            | Expr::IncDec { operand: inner, .. }
            | Expr::Deref(inner, _)
            | Expr::RawOf(inner, _)
            | Expr::Copy(inner, _)
            | Expr::Place(inner, _, _)
            | Expr::Field(inner, _, _)
            | Expr::Present(inner, _)
            | Expr::Ok(inner, _)
            | Expr::Err(inner, _)
            | Expr::Try(inner, _, _) => self.check_expr(inner),
            Expr::OptField { base, .. } => self.check_expr(base),
            Expr::Index { base, index, .. } => {
                self.check_expr(base);
                self.check_expr(index);
            }
            Expr::Slice {
                base, start, end, range, ..
            } => {
                self.check_expr(base);
                if let Some(range) = range {
                    self.check_expr(range);
                } else {
                    self.check_expr(start);
                    self.check_expr(end);
                }
            }
            Expr::Range { start, end, .. } => {
                self.check_expr(start);
                self.check_expr(end);
            }
            Expr::ListLit(elems, _) => elems.iter().for_each(|el| self.check_expr(el)),
            Expr::MapLit(entries, _) => entries.iter().for_each(|(k, v)| {
                self.check_expr(k);
                self.check_expr(v);
            }),
            Expr::TupleLit(fields, _, _) => fields.iter().for_each(|(_, e)| self.check_expr(e)),
            Expr::StructLit { fields, .. } => {
                fields.iter().for_each(|(_, _, f)| self.check_expr(f))
            }
            Expr::TypedLit { body, .. } => {
                body.for_each_expr(|f| self.check_expr(f))
            }
            Expr::EnumLit { args, .. } => args.iter().for_each(|a| match a {
                EnumLitArg::Positional(e) => self.check_expr(e),
                EnumLitArg::Named { expr, .. } => self.check_expr(expr),
            }),
            Expr::Str(parts, _) => parts.iter().for_each(|p| {
                if let StrPart::Interp(e, _) = p {
                    self.check_expr(e);
                }
            }),
            Expr::OrFallback {
                value, fallback, ..
            } => {
                self.check_expr(value);
                match fallback {
                    OrFallback::Value(e) => self.check_expr(e),
                    OrFallback::Return(Some(e), _) => self.check_expr(e),
                    _ => {}
                }
            }
            Expr::PatternTest { subject, .. } => self.check_expr(subject),
            Expr::If {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                ..
            } => {
                self.check_expr(cond);
                let before = self.snapshot();
                self.check_block(then_body);
                self.check_expr(then_value);
                let then_path = self.snapshot();
                self.locals = before.0.clone();
                self.local_types = before.1.clone();
                self.check_block(else_body);
                self.check_expr(else_value);
                let else_path = self.snapshot();
                self.merge_tags(&before, &[then_path, else_path]);
            }
            Expr::PtrFromAddr { addr, .. } => self.check_expr(addr),
            Expr::Lambda(l) => self.check_lambda(l),
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Char(..)
            | Expr::Ident(..)
            | Expr::Absent(_)
            | Expr::ReduceMarker(_, _)
            | Expr::Todo { .. }
        | Expr::NoElse(_)
            | Expr::UnitLit { .. }
            | Expr::ComptimeName { .. }
            // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
            // literal, no nested `Expr` to recurse into.
            | Expr::StrMatchLit(_, _)
            | Expr::BinMatchLit(_, _) => {}
            Expr::Paren(inner, _) => self.check_expr(inner),
            Expr::Spread(inner, _) => self.check_expr(inner),
            Expr::MemberSpread { base, .. } => self.check_expr(base),
        }
    }

    fn check_lambda(&mut self, l: &Lambda) {
        match &l.body {
            LambdaBody::Expr(e) => self.check_expr(e),
            LambdaBody::Block(b) => self.check_block(b),
        }
    }

    fn check_block(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.check_stmt(s);
        }
    }

    fn check_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) | Stmt::Yield(e, _) => self.check_expr(e),
            Stmt::BreakValue(e, _) | Stmt::BreakLabelValue(_, _, e, _) => self.check_expr(e),
            Stmt::Val(b) => {
                self.check_expr(&b.init);
                let mut tags = self.tags_of(&b.init);
                if let Some(ty) = &b.ty {
                    tags.extend(type_tags(ty));
                }
                if let Some(pat) = &b.pattern {
                    for name in pattern_names(pat) {
                        self.set_tags(&name, tags.clone());
                    }
                } else if !b.name.is_empty() {
                    self.set_tags(&b.name, tags);
                    if let Some(type_name) = b
                        .ty
                        .as_ref()
                        .and_then(Self::type_name)
                        .or_else(|| self.type_of(&b.init))
                    {
                        self.local_types.set(&b.name, type_name);
                    }
                }
            }
            Stmt::Assign {
                target, op, value, ..
            } => {
                self.check_expr(value);
                if let LValue::Local { name, .. } = target {
                    let mut tags = self.tags_of(value);
                    if op.is_some() {
                        tags.extend(self.locals.get(name).cloned().unwrap_or_default());
                    }
                    self.set_tags(name, tags);
                } else {
                    // Field/index assign targets are also walked for nested sinks.
                    if let LValue::Index { base, index, .. } = target {
                        self.check_expr(base);
                        self.check_expr(index);
                    }
                    if let LValue::Field { base, .. } = target {
                        self.check_expr(base);
                    }
                }
            }
            Stmt::Return(Some(e), _) => self.check_expr(e),
            Stmt::Return(None, _) => {}
            Stmt::While { cond, body, .. } => {
                self.check_expr(cond);
                self.check_loop_body(body);
            }
            Stmt::For {
                kind,
                body,
                var,
                var2,
                ..
            } => {
                let collection_tags = match kind {
                    ForKind::Range { start, end, step, exclusive: _ } => {
                        self.check_expr(start);
                        self.check_expr(end);
                        if let Some(s) = step {
                            self.check_expr(s);
                        }
                        TagSet::new()
                    }
                    ForKind::In { collection, step } => {
                        self.check_expr(collection);
                        if let Some(step) = step { self.check_expr(step); }
                        self.tags_of(collection)
                    }
                };
                self.set_tags(var, collection_tags.clone());
                if let Some((v2, _)) = var2 {
                    self.set_tags(v2, collection_tags);
                }
                self.check_loop_body(body);
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                span,
            }
            | Stmt::ComptimeSwitch {
                subject,
                arms,
                else_body,
                span,
            } => {
                self.check_expr(subject);
                let before = self.snapshot();
                let mut paths = Vec::new();
                for a in arms {
                    self.locals = before.0.clone();
                    self.local_types = before.1.clone();
                    self.check_expr(&a.cond);
                    self.check_block(&a.body);
                    paths.push(self.snapshot());
                }
                match else_body {
                    Some(b) => paths.push(self.walk_path(&before, b)),
                    // No `else`: skipping every arm is itself a path, unless
                    // CheckerCore already proved this pattern table exhaustive.
                    None if !crate::Sema::FlowFacts::switch_proven_exhaustive(
                        arms,
                        self.existing_diags,
                        *span,
                    ) =>
                    {
                        paths.push(before.clone());
                    }
                    None => {}
                }
                self.merge_tags(&before, &paths);
            }
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                self.check_expr(&init.init);
                self.check_expr(cond);
                let before = self.snapshot();
                self.check_block(body);
                if let Some(step) = step {
                    self.check_stmt(step);
                }
                let after_body = self.snapshot();
                self.locals = Facts::after_loop(&before.0, &after_body.0, &mut Vec::new());
                self.local_types = Facts::after_loop(&before.1, &after_body.1, &mut Vec::new());
            }
            Stmt::Loop { body, .. } => self.check_loop_body(body),
            Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Switched { body, .. }
            | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::ScopeMember { body, .. }
            | Stmt::Live { body, .. } => self.check_block(body),
            // D-CTMARKER1: comptime block erases; walk body conservatively.
            Stmt::ComptimeBlock { body, .. } => self.check_block(body),
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                self.check_expr(cond);
                let before = self.snapshot();
                let then_path = self.walk_path(&before, then_body);
                let other_path = match else_body {
                    Some(b) => self.walk_path(&before, b),
                    None => before.clone(),
                };
                self.merge_tags(&before, &[then_path, other_path]);
            }
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields {
                    self.check_expr(e);
                }
                self.check_block(body);
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {
            }
        }
    }

    fn set_tags(&mut self, name: &str, tags: TagSet) {
        if tags.is_empty() {
            self.locals.remove(name);
        } else {
            self.locals.set(name, tags);
        }
    }

    /// Join every path that meets here. Suspicion spreads: a value tainted on
    /// any path stays tainted after the paths meet.
    fn merge_tags(&mut self, before: &TaintFacts, paths: &[TaintFacts]) {
        self.locals = Facts::merge_paths(&before.0, &paths.iter().map(|p| p.0.clone()).collect::<Vec<_>>(), &mut Vec::new());
        self.local_types = Facts::merge_paths(&before.1, &paths.iter().map(|p| p.1.clone()).collect::<Vec<_>>(), &mut Vec::new());
    }

    fn snapshot(&self) -> TaintFacts {
        TaintFacts(self.locals.clone(), self.local_types.clone())
    }

    /// Walk one path from the facts that reach it and report where it ends.
    fn walk_path(&mut self, before: &TaintFacts, body: &[Stmt]) -> TaintFacts {
        self.locals = before.0.clone();
        self.local_types = before.1.clone();
        self.check_block(body);
        self.snapshot()
    }

    /// The shared loop rule, stated once in `Facts::after_loop`.
    fn check_loop_body(&mut self, body: &[Stmt]) {
        let before = self.snapshot();
        self.check_block(body);
        let after_body = self.snapshot();
        self.locals = Facts::after_loop(&before.0, &after_body.0, &mut Vec::new());
        self.local_types = Facts::after_loop(&before.1, &after_body.1, &mut Vec::new());
    }
}

/// Names bound by a destructuring pattern (S74), flattened.
fn pattern_names(pat: &crate::AST::BindPattern) -> Vec<String> {
    use crate::AST::BindPattern;
    match pat {
        BindPattern::Struct { fields, .. } => {
            fields.iter().map(|b| b.local_name().to_string()).collect()
        }
        BindPattern::List { elems, .. } | BindPattern::Tuple { elems, .. } => {
            elems.iter().map(|b| b.name.clone()).collect()
        }
    }
}

fn type_tags(ty: &Type) -> TagSet {
    match ty {
        Type::Tagged { marker, inner } => {
            let mut tags = type_tags(inner);
            tags.insert(marker.clone());
            tags
        }
        _ => TagSet::new(),
    }
}

pub fn check_func_taint(
    function: &crate::AST::Func,
    owner: Option<&str>,
    scrubbers: &HashMap<String, String>,
    facts: &jet_foundation::Facts::FactRegistry,
    returns: &HashMap<String, TagSet>,
    return_types: &ReturnTypes,
    field_tags: &FieldTags,
    field_types: &FieldTypes,
    core_imports: &HashMap<String, String>,
    existing_diags: &[Diagnostic],
) -> Vec<Diagnostic> {
    let mut ctx = TaintCtx::new(
        scrubbers,
        facts,
        returns,
        return_types,
        field_tags,
        field_types,
        core_imports,
        existing_diags,
    );
    for parameter in &function.params {
        ctx.set_tags(&parameter.name, type_tags(&parameter.ty));
        let type_name = match (&parameter.ty, owner) {
            (Type::Named(name), Some(owner)) if name == "Self" => Some(owner.to_string()),
            (ty, _) => TaintCtx::type_name(ty),
        };
        if let Some(type_name) = type_name {
            ctx.local_types.set(&parameter.name, type_name);
        }
    }
    ctx.check_block(&function.body);
    ctx.diags
}

pub fn check_body_tags(
    body: &[Stmt],
    scrubbers: &HashMap<String, String>,
    facts: &jet_foundation::Facts::FactRegistry,
    returns: &HashMap<String, TagSet>,
    return_types: &ReturnTypes,
    field_tags: &FieldTags,
    field_types: &FieldTypes,
    core_imports: &HashMap<String, String>,
    existing_diags: &[Diagnostic],
) -> Vec<Diagnostic> {
    let mut ctx = TaintCtx::new(
        scrubbers,
        facts,
        returns,
        return_types,
        field_tags,
        field_types,
        core_imports,
        existing_diags,
    );
    ctx.check_block(body);
    ctx.diags
}

pub fn collect_return_tag_facts(
    items: &[Item],
    returns: &mut HashMap<String, TagSet>,
    return_types: &mut ReturnTypes,
) {
    fn register(
        function: &crate::AST::Func,
        key: String,
        returns: &mut HashMap<String, TagSet>,
        return_types: &mut ReturnTypes,
    ) {
        let tags = function
            .return_type
            .as_ref()
            .map(type_tags)
            .unwrap_or_default();
        if !tags.is_empty() {
            returns.insert(key.clone(), tags);
        }
        if let Some(type_name) = function.return_type.as_ref().and_then(TaintCtx::type_name) {
            return_types.insert(key, type_name);
        }
    }

    for item in items {
        match item {
            Item::Func(function) => {
                register(function, function.name.clone(), returns, return_types)
            }
            Item::Impl(implementation) => {
                for method in &implementation.methods {
                    register(
                        method,
                        format!("{}::{}", implementation.type_name, method.name),
                        returns,
                        return_types,
                    );
                }
            }
            Item::Struct(definition) => {
                for method in &definition.methods {
                    register(
                        method,
                        format!("{}::{}", definition.name, method.name),
                        returns,
                        return_types,
                    );
                }
                for implementation in &definition.trait_impls {
                    for method in &implementation.methods {
                        register(
                            method,
                            format!("{}::{}", definition.name, method.name),
                            returns,
                            return_types,
                        );
                    }
                }
            }
            Item::Enum(definition) => {
                for method in &definition.methods {
                    register(
                        method,
                        format!("{}::{}", definition.name, method.name),
                        returns,
                        return_types,
                    );
                }
            }
            _ => {}
        }
    }
}

pub fn collect_function_paths(
    module: &str,
    items: &[Item],
    paths: &mut BTreeSet<String>,
) {
    let mut register = |owner: Option<&str>, name: &str| {
        let local = owner
            .map(|owner| format!("{owner}.{name}"))
            .unwrap_or_else(|| name.to_string());
        paths.insert(local.clone());
        paths.insert(format!("{module}.{local}"));
    };
    for item in items {
        match item {
            Item::Func(function) => register(None, &function.name),
            Item::Impl(implementation) => {
                for method in &implementation.methods {
                    register(Some(&implementation.type_name), &method.name);
                }
            }
            Item::Struct(definition) => {
                for method in &definition.methods {
                    register(Some(&definition.name), &method.name);
                }
                for implementation in &definition.trait_impls {
                    for method in &implementation.methods {
                        register(Some(&definition.name), &method.name);
                    }
                }
            }
            Item::Enum(definition) => {
                for method in &definition.methods {
                    register(Some(&definition.name), &method.name);
                }
            }
            _ => {}
        }
    }
}

pub fn register_builtin_tag_facts(facts: &mut jet_foundation::Facts::FactRegistry) {
    let injection = vec!["DB".to_string(), "Exec".to_string(), "Net".to_string()];
    for tag in ["Input", "PII", "Secret"] {
        facts.declare_with_rules(
            jet_foundation::Facts::FactKind::Tag,
            tag,
            std::iter::empty(),
            injection.clone(),
            std::iter::empty(),
        );
    }
    let mut credential = injection;
    credential.push("Log".to_string());
    facts.declare_with_rules(
        jet_foundation::Facts::FactKind::Tag,
        "Credential",
        std::iter::empty(),
        credential,
        std::iter::empty(),
    );
}

pub fn collect_field_facts(
    items: &[Item],
    field_tags: &mut FieldTags,
    field_types: &mut FieldTypes,
) {
    for item in items {
        let Item::Struct(definition) = item else {
            continue;
        };
        for field in &definition.fields {
            let key = (definition.name.clone(), field.name.clone());
            let tags = type_tags(&field.ty);
            if !tags.is_empty() {
                field_tags.insert(key.clone(), tags);
            }
            if let Some(type_name) = TaintCtx::type_name(&field.ty) {
                field_types.insert(key, type_name);
            }
        }
    }
}

pub fn collect_tag_facts(
    items: &[Item],
    facts: &mut jet_foundation::Facts::FactRegistry,
    scrubbers: &mut HashMap<String, String>,
    known_sources: &BTreeSet<String>,
    diags: &mut Vec<Diagnostic>,
    declarations_only: bool,
) {
    fn closest<'a>(
        name: &str,
        candidates: impl IntoIterator<Item = &'a str>,
    ) -> Option<&'a str> {
        candidates
            .into_iter()
            .map(|candidate| (crate::Syntax::edit_distance(name, candidate), candidate))
            .filter(|(distance, _)| *distance <= 3)
            .min_by_key(|(distance, candidate)| (*distance, *candidate))
            .map(|(_, candidate)| candidate)
    }

    fn register_scrubber(
        function: &crate::AST::Func,
        key: String,
        facts: &jet_foundation::Facts::FactRegistry,
        scrubbers: &mut HashMap<String, String>,
        diags: &mut Vec<Diagnostic>,
    ) {
        let Some(tag) = &function.scrub_tag else {
            return;
        };
        if facts
            .get(jet_foundation::Facts::FactKind::Tag, tag)
            .is_none()
        {
            diags.push(crate::Sema::Diagnostics::undeclared_value_tag(
                tag,
                None,
                function.name_span,
            ));
            return;
        }
        let consumes_tag = function
            .params
            .iter()
            .any(|parameter| type_tags(&parameter.ty).contains(tag));
        let returns_tag = function
            .return_type
            .as_ref()
            .is_some_and(|ty| type_tags(ty).contains(tag));
        if !consumes_tag || returns_tag {
            diags.push(Diagnostic::error(
                "E0736",
                format!("`#Scrub({tag})` does not match this function signature"),
                "a scrubber consumes a value carrying that tag and returns a value without it"
                    .to_string(),
                format!(
                    "accept a `#{tag} T` parameter and return the untagged result"
                ),
                Some(function.name_span),
            ));
            return;
        }
        scrubbers.insert(key, tag.clone());
    }

    if declarations_only {
        for item in items {
            let Item::Tag(tag) = item else {
                continue;
            };
            facts.declare_with_rules(
                jet_foundation::Facts::FactKind::Tag,
                tag.name.clone(),
                std::iter::empty(),
                tag.deny.iter().map(|(name, _)| name.clone()),
                tag.from.iter().map(|(name, _)| name.clone()),
            );
            for (destination, span) in &tag.deny {
                if crate::Sema::Effects::parse_effect_name(destination).is_none()
                    && destination != "Html"
                {
                    let effects = crate::Sema::Effects::Effect::all();
                    let suggestion = closest(
                        destination,
                        effects
                            .iter()
                            .map(String::as_str)
                            .chain(["Html"].into_iter()),
                    );
                    diags.push(Diagnostic::error(
                        "E0735",
                        format!("`{destination}` is not a known tag destination"),
                        "a `deny` entry names an effect or a registered sink".to_string(),
                        suggestion
                            .map(|candidate| format!("did you mean `{candidate}`?"))
                            .unwrap_or_else(|| {
                                "use a known effect such as `DB`, `Net`, `Exec`, `Log`, or `Html`"
                                    .to_string()
                            }),
                        Some(*span),
                    ));
                }
            }
            for (source, span) in &tag.from {
                if crate::Sema::Effects::parse_effect_name(source).is_none()
                    && !known_sources.contains(source)
                {
                    let suggestion = closest(
                        source,
                        known_sources.iter().map(String::as_str),
                    );
                    diags.push(Diagnostic::error(
                        "E0735",
                        format!("`{source}` is not a known tag source"),
                        "a `from` entry names an effect or a function path".to_string(),
                        suggestion
                            .map(|candidate| format!("did you mean `{candidate}`?"))
                            .unwrap_or_else(|| {
                                "use a known effect or a declared function path".to_string()
                            }),
                        Some(*span),
                    ));
                }
            }
        }
        return;
    }

    for item in items {
        match item {
            Item::Func(function) => register_scrubber(
                function,
                function.name.clone(),
                facts,
                scrubbers,
                diags,
            ),
            Item::Impl(i) => {
                for m in &i.methods {
                    register_scrubber(
                        m,
                        format!("{}::{}", i.type_name, m.name),
                        facts,
                        scrubbers,
                        diags,
                    );
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    register_scrubber(
                        m,
                        format!("{}::{}", s.name, m.name),
                        facts,
                        scrubbers,
                        diags,
                    );
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        register_scrubber(
                            m,
                            format!("{}::{}", s.name, m.name),
                            facts,
                            scrubbers,
                            diags,
                        );
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    register_scrubber(
                        m,
                        format!("{}::{}", e.name, m.name),
                        facts,
                        scrubbers,
                        diags,
                    );
                }
            }
            _ => {}
        }
    }
}

/// E0722: a `#Credential` value reaches a log/print/serialize
/// sink. Credentials must never appear in log files, stdout, or serialized output.
pub fn e0722(api: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0722",
        format!("a `Credential` value is denied at `{api}`"),
        format!(
            "`{api}` writes to a log, terminal, or serialized output, where a credential would leak"
        ),
        "log a non-secret field, or pass the value through a matching `#Scrub(Credential)` function"
            .to_string(),
        Some(span),
    )
}

pub fn e0721(tag: &str, api: &str, destinations: &[String], span: Span) -> Diagnostic {
    let destination = destinations.last().map(String::as_str).unwrap_or("sink");
    Diagnostic::error(
        "E0721",
        format!("a `{tag}` value is denied at `{api}`"),
        format!(
            "the declaration for `{tag}` denies `{destination}`, which covers this destination"
        ),
        format!(
            "remove the destination use, or pass the value through a matching `#Scrub({tag})` function"
        ),
        Some(span),
    )
}
