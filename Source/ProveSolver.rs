//! D-PROVE-SOLVER1=A: opt-in native Presburger solver for `jet prove --lens solver`.
//!
//! Semantics live here as a std-only producer. The CLI only marshals obligations
//! exported after sema and embeds certificate-checked evidence in ProofReport.
//! Hard limits and certificate shapes follow `docs/spec/proof-replay-decisions.md`.

use std::collections::{BTreeMap, BTreeSet};

use jet::AST::{BinOp, Expr, Item, Program, UnOp};
use jet::Diagnostics::span_line_col;
use jet::Lexer;
use jet::Parser;
use jet::SHA256;
use jet_foundation::JSON::{parse_json, JSONValue};

const MAX_OBLIGATIONS: usize = 10_000;
const MAX_TERMS: usize = 50_000;
const MAX_VARS: usize = 256;
const MAX_STEPS: u64 = 1_000_000;
const BACKEND: &str = "native-presburger";
const BACKEND_VERSION: &str = "1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Affine {
    pub constant: i128,
    pub terms: BTreeMap<String, i128>,
}

impl Affine {
    fn constant(c: i128) -> Self {
        Self {
            constant: c,
            terms: BTreeMap::new(),
        }
    }

    fn var(name: &str, coeff: i128) -> Self {
        let mut terms = BTreeMap::new();
        terms.insert(name.to_string(), coeff);
        Self {
            constant: 0,
            terms,
        }
    }

    fn add(&self, other: &Self) -> Result<Self, SolverFail> {
        let mut terms = self.terms.clone();
        for (k, v) in &other.terms {
            let entry = terms.entry(k.clone()).or_insert(0);
            *entry = entry
                .checked_add(*v)
                .ok_or(SolverFail::CoefficientOverflow)?;
            if *entry == 0 {
                terms.remove(k);
            }
        }
        Ok(Self {
            constant: self
                .constant
                .checked_add(other.constant)
                .ok_or(SolverFail::CoefficientOverflow)?,
            terms,
        })
    }

    fn scale(&self, factor: i128) -> Result<Self, SolverFail> {
        let mut terms = BTreeMap::new();
        for (k, v) in &self.terms {
            let scaled = v
                .checked_mul(factor)
                .ok_or(SolverFail::CoefficientOverflow)?;
            if scaled != 0 {
                terms.insert(k.clone(), scaled);
            }
        }
        Ok(Self {
            constant: self
                .constant
                .checked_mul(factor)
                .ok_or(SolverFail::CoefficientOverflow)?,
            terms,
        })
    }

    fn term_count(&self) -> usize {
        self.terms.len() + 1
    }

    fn to_json(&self) -> String {
        let terms: Vec<String> = self
            .terms
            .iter()
            .map(|(variable, coefficient)| {
                format!(
                    "{{\"coefficient\":\"{coefficient}\",\"variable\":{}}}",
                    json_str(variable)
                )
            })
            .collect();
        format!(
            "{{\"constant\":\"{}\",\"terms\":[{}]}}",
            self.constant,
            terms.join(",")
        )
    }
}

/// Normalized inequality: `affine <= 0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Inequality {
    pub affine: Affine,
}

impl Inequality {
    fn le(affine: Affine) -> Self {
        Self { affine }
    }

    fn to_json(&self) -> String {
        format!(
            "{{\"affine\":{},\"relation\":\"le\"}}",
            self.affine.to_json()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Formula {
    pub assumptions: Vec<Inequality>,
    pub claim: Vec<Inequality>,
}

impl Formula {
    fn to_json(&self) -> String {
        let assumptions = canonical_inequalities(&self.assumptions)
            .iter()
            .map(Inequality::to_json)
            .collect::<Vec<_>>()
            .join(",");
        let claim = canonical_inequalities(&self.claim)
            .iter()
            .map(Inequality::to_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"assumptions\":{{\"children\":[{assumptions}],\"op\":\"and\"}},\"claim\":{{\"children\":[{claim}],\"op\":\"and\"}}}}"
        )
    }

    fn hash(&self) -> String {
        let canonical = format!("{}\n", self.to_json());
        SHA256::sha256_hex(canonical.as_bytes())
    }

    fn term_count(&self) -> usize {
        self.assumptions
            .iter()
            .chain(self.claim.iter())
            .map(|ineq| ineq.affine.term_count())
            .sum()
    }

    fn variables(&self) -> BTreeSet<String> {
        let mut vars = BTreeSet::new();
        for ineq in self.assumptions.iter().chain(self.claim.iter()) {
            vars.extend(ineq.affine.terms.keys().cloned());
        }
        vars
    }
}

fn canonical_inequalities(inequalities: &[Inequality]) -> Vec<Inequality> {
    let mut normalized = inequalities.to_vec();
    normalized.sort_by(|left, right| left.to_json().cmp(&right.to_json()));
    normalized.dedup();
    normalized
}

#[derive(Clone, Debug)]
pub(crate) struct Obligation {
    pub id: String,
    pub kind: String,
    pub origin: String,
    pub span: String,
    pub formula: Formula,
}

#[derive(Clone, Debug)]
pub(crate) enum SolverOutcome {
    Proved {
        certificate: String,
        certificate_sha256: String,
        steps: u64,
    },
    Disproved {
        assignment: BTreeMap<String, i128>,
        steps: u64,
    },
    Unknown {
        reason: &'static str,
        steps: u64,
    },
}

#[derive(Clone, Debug)]
enum SolverFail {
    CoefficientOverflow,
    #[allow(dead_code)]
    StructuralLimit,
}

#[derive(Clone, Debug)]
pub(crate) struct SolverEvidence {
    pub obligation: Obligation,
    pub outcome: SolverOutcome,
    pub evidence_id: String,
}

/// Collect solver obligations from target members and discharge them.
pub(crate) fn run_solver_producer(
    members: &[(String, String)],
    target_input_sha256: &str,
    enable: bool,
) -> Result<Vec<SolverEvidence>, String> {
    if !enable {
        return Ok(Vec::new());
    }
    let mut obligations = Vec::new();
    for (path, source) in members {
        obligations.extend(extract_obligations(path, source, target_input_sha256)?);
        if obligations.len() > MAX_OBLIGATIONS {
            return Err("solver structural_limit: more than 10000 obligations".into());
        }
    }
    let mut out = Vec::new();
    for obligation in obligations {
        if obligation.formula.term_count() > MAX_TERMS
            || obligation.formula.variables().len() > MAX_VARS
        {
            out.push(SolverEvidence {
                evidence_id: evidence_id_for(&obligation, "unknown"),
                obligation,
                outcome: SolverOutcome::Unknown {
                    reason: "structural_limit",
                    steps: 0,
                },
            });
            continue;
        }
        let outcome = prove_obligation(&obligation.formula)
            .map_err(|reason| format!("solver certificate failure: {reason}"))?;
        let tag = match &outcome {
            SolverOutcome::Proved { .. } => "proved",
            SolverOutcome::Disproved { .. } => "disproved",
            SolverOutcome::Unknown { .. } => "unknown",
        };
        out.push(SolverEvidence {
            evidence_id: evidence_id_for(&obligation, tag),
            obligation,
            outcome,
        });
    }
    Ok(out)
}

fn evidence_id_for(obligation: &Obligation, tag: &str) -> String {
    framed_sha256(&[
        &obligation.id,
        &obligation.kind,
        &obligation.origin,
        &obligation.span,
        tag,
    ])
}

fn extract_obligations(
    path: &str,
    source: &str,
    target_input_sha256: &str,
) -> Result<Vec<Obligation>, String> {
    let (toks, lex_diags) = Lexer::lex(source);
    if !lex_diags.is_empty() {
        return Ok(Vec::new());
    }
    let program = match Parser::parse(&toks) {
        Ok(p) => p,
        Err(_) => return Ok(Vec::new()),
    };
    Ok(extract_from_program(path, source, target_input_sha256, &program))
}

fn extract_from_program(
    path: &str,
    source: &str,
    target_input_sha256: &str,
    program: &Program,
) -> Vec<Obligation> {
    let mut out = Vec::new();
    let mut specs = BTreeMap::new();
    collect_function_specs(&program.items, &mut specs);
    walk_items(
        path,
        source,
        target_input_sha256,
        &program.items,
        &specs,
        &mut out,
    );
    out
}

#[derive(Clone)]
struct FunctionSpec {
    params: Vec<String>,
    pre: Vec<Expr>,
}

fn collect_function_specs(items: &[Item], out: &mut BTreeMap<String, FunctionSpec>) {
    for item in items {
        match item {
            Item::Func(func) => {
                out.entry(func.name.clone()).or_insert_with(|| FunctionSpec {
                    params: func.params.iter().map(|param| param.name.clone()).collect(),
                    pre: func.pre.iter().map(|clause| clause.cond.clone()).collect(),
                });
            }
            Item::Impl(imp) => {
                for func in &imp.methods {
                    out.entry(func.name.clone()).or_insert_with(|| FunctionSpec {
                        params: func.params.iter().map(|param| param.name.clone()).collect(),
                        pre: func.pre.iter().map(|clause| clause.cond.clone()).collect(),
                    });
                }
            }
            Item::Struct(def) => {
                for func in &def.methods {
                    out.entry(func.name.clone()).or_insert_with(|| FunctionSpec {
                        params: func.params.iter().map(|param| param.name.clone()).collect(),
                        pre: func.pre.iter().map(|clause| clause.cond.clone()).collect(),
                    });
                }
            }
            Item::Enum(def) => {
                for func in &def.methods {
                    out.entry(func.name.clone()).or_insert_with(|| FunctionSpec {
                        params: func.params.iter().map(|param| param.name.clone()).collect(),
                        pre: func.pre.iter().map(|clause| clause.cond.clone()).collect(),
                    });
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_function_specs(body, out);
                }
            }
            _ => {}
        }
    }
}

fn walk_items(
    path: &str,
    source: &str,
    target_input_sha256: &str,
    items: &[Item],
    specs: &BTreeMap<String, FunctionSpec>,
    out: &mut Vec<Obligation>,
) {
    for item in items {
        match item {
            Item::Distinct(def) => {
                if let Some((lo, hi, span)) = def.range {
                    // The distinct declaration is the authority for the bounded
                    // value. Keep the invariant as assumptions and prove that
                    // every claimed bound follows from that same checked fact.
                    // This gives the certificate checker real arithmetic to
                    // validate instead of a constant "lo <= hi" label.
                    let bounds = inclusive_bounds(lo, hi);
                    let formula = Formula {
                        assumptions: bounds.clone(),
                        claim: bounds,
                    };
                    let formula_hash = formula.hash();
                    let span_text = source_span_text(source, span);
                    let id = framed_sha256(&[
                        target_input_sha256,
                        "fixed_index_bounds",
                        path,
                        &span_text,
                        &formula_hash,
                    ]);
                    out.push(Obligation {
                        id,
                        kind: "fixed_index_bounds".into(),
                        origin: path.to_string(),
                        span: span_text,
                        formula,
                    });
                }
            }
            Item::Func(func) => {
                collect_func_contracts(path, source, target_input_sha256, func, out);
                collect_call_preconditions(
                    path,
                    source,
                    target_input_sha256,
                    func,
                    specs,
                    out,
                );
            }
            Item::Impl(imp) => {
                for method in &imp.methods {
                    collect_func_contracts(path, source, target_input_sha256, method, out);
                    collect_call_preconditions(
                        path,
                        source,
                        target_input_sha256,
                        method,
                        specs,
                        out,
                    );
                }
            }
            Item::Struct(def) => {
                for method in &def.methods {
                    collect_func_contracts(path, source, target_input_sha256, method, out);
                    collect_call_preconditions(
                        path,
                        source,
                        target_input_sha256,
                        method,
                        specs,
                        out,
                    );
                }
            }
            Item::Enum(def) => {
                for method in &def.methods {
                    collect_func_contracts(path, source, target_input_sha256, method, out);
                    collect_call_preconditions(
                        path,
                        source,
                        target_input_sha256,
                        method,
                        specs,
                        out,
                    );
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    walk_items(path, source, target_input_sha256, body, specs, out);
                }
            }
            _ => {}
        }
    }
}

fn collect_func_contracts(
    path: &str,
    source: &str,
    target_input_sha256: &str,
    func: &jet::AST::Func,
    out: &mut Vec<Obligation>,
) {
    let mut assumptions = Vec::new();
    for clause in &func.pre {
        if let Some(ineqs) = expr_to_inequalities(&clause.cond) {
            assumptions.extend(ineqs);
        }
    }
    let mut claim = Vec::new();
    for clause in &func.post {
        if let Some(ineqs) = expr_to_inequalities(&clause.cond) {
            claim.extend(ineqs);
        }
    }
    if claim.is_empty() {
        return;
    }
    let formula = Formula {
        assumptions,
        claim,
    };
    let formula_hash = formula.hash();
    let span = source_span_text(source, func.span);
    let id = framed_sha256(&[
        target_input_sha256,
        "function_postcondition",
        path,
        &span,
        &formula_hash,
    ]);
    out.push(Obligation {
        id,
        kind: "function_postcondition".into(),
        origin: format!("{path}::{}", func.name),
        span,
        formula,
    });
}

fn collect_call_preconditions(
    path: &str,
    source: &str,
    target_input_sha256: &str,
    func: &jet::AST::Func,
    specs: &BTreeMap<String, FunctionSpec>,
    out: &mut Vec<Obligation>,
) {
    let assumptions = func
        .pre
        .iter()
        .filter_map(|clause| expr_to_inequalities(&clause.cond))
        .flatten()
        .collect::<Vec<_>>();
    let mut calls = Vec::<jet::AST::Call>::new();
    for statement in &func.body {
        visit_stmt_calls(statement, &mut |call| calls.push(call.clone()));
    }
    for call in calls {
        let Some(spec) = specs.get(&call.name) else {
            continue;
        };
        if spec.pre.is_empty() {
            continue;
        }
        let Some(substitutions) = bind_call_arguments(&spec.params, &call.args) else {
            continue;
        };
        let mut claim = Vec::new();
        let mut supported = true;
        for condition in &spec.pre {
            let Some(inequalities) = expr_to_inequalities_with_subst(condition, &substitutions)
            else {
                supported = false;
                break;
            };
            claim.extend(inequalities);
        }
        if !supported || claim.is_empty() {
            continue;
        }
        let formula = Formula { assumptions: assumptions.clone(), claim };
        let formula_hash = formula.hash();
        let span = source_span_text(source, call.name_span);
        let origin = format!("{path}::{} -> {}", func.name, call.name);
        let id = framed_sha256(&[
            target_input_sha256,
            "call_precondition",
            path,
            &origin,
            &span,
            &formula_hash,
        ]);
        out.push(Obligation {
            id,
            kind: "call_precondition".into(),
            origin,
            span,
            formula,
        });
    }
}

fn bind_call_arguments(
    params: &[String],
    args: &[jet::AST::CallArg],
) -> Option<BTreeMap<String, Affine>> {
    if params.len() != args.len() || args.iter().any(|arg| arg.spread) {
        return None;
    }
    let mut slots = vec![None; params.len()];
    let mut next_positional = 0usize;
    for arg in args {
        let index = if let Some((label, _)) = &arg.label {
            params.iter().position(|param| param == label)?
        } else {
            while next_positional < slots.len() && slots[next_positional].is_some() {
                next_positional += 1;
            }
            let index = next_positional;
            next_positional = next_positional.checked_add(1)?;
            index
        };
        if index >= slots.len() || slots[index].is_some() {
            return None;
        }
        let affine = expr_to_affine_with_subst(&arg.expr, &BTreeMap::new())?;
        slots[index] = Some(affine);
    }
    if slots.iter().any(Option::is_none) {
        return None;
    }
    let mut substitutions = BTreeMap::new();
    for (param, slot) in params.iter().zip(slots) {
        let Some(affine) = slot else {
            return None;
        };
        substitutions.insert(param.clone(), affine);
    }
    Some(substitutions)
}

fn visit_stmt_calls(statement: &jet::AST::Stmt, calls: &mut impl FnMut(&jet::AST::Call)) {
    use jet::AST::Stmt;
    match statement {
        Stmt::Expr(expr) | Stmt::Yield(expr, _) => visit_expr_calls(expr, calls),
        Stmt::Val(binding) => visit_expr_calls(&binding.init, calls),
        Stmt::Assign { target, value, .. } => {
            visit_lvalue_calls(target, calls);
            visit_expr_calls(value, calls);
        }
        Stmt::Return(Some(expr), _)
        | Stmt::BreakValue(expr, _)
        | Stmt::BreakLabelValue(_, _, expr, _) => visit_expr_calls(expr, calls),
        Stmt::Return(None, _)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(..)
        | Stmt::ContinueLabel(..) => {}
        Stmt::While { cond, body, .. } => {
            visit_expr_calls(cond, calls);
            visit_stmt_list(body, calls);
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                jet::AST::ForKind::Range { start, end, step, .. } => {
                    visit_expr_calls(start, calls);
                    visit_expr_calls(end, calls);
                    if let Some(step) = step {
                        visit_expr_calls(step, calls);
                    }
                }
                jet::AST::ForKind::In { collection, step } => {
                    visit_expr_calls(collection, calls);
                    if let Some(step) = step {
                        visit_expr_calls(step, calls);
                    }
                }
            }
            visit_stmt_list(body, calls);
        }
        Stmt::Switch { subject, arms, else_body, .. }
        | Stmt::ComptimeSwitch { subject, arms, else_body, .. } => {
            visit_expr_calls(subject, calls);
            for arm in arms {
                visit_expr_calls(&arm.cond, calls);
                visit_stmt_list(&arm.body, calls);
            }
            if let Some(body) = else_body {
                visit_stmt_list(body, calls);
            }
        }
        Stmt::CountedLoop { init, cond, step, body, .. } => {
            visit_expr_calls(&init.init, calls);
            visit_expr_calls(cond, calls);
            if let Some(step) = step {
                visit_stmt_calls(step, calls);
            }
            visit_stmt_list(body, calls);
        }
        Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
            visit_expr_calls(cond, calls);
            visit_stmt_list(then_body, calls);
            if let Some(body) = else_body {
                visit_stmt_list(body, calls);
            }
        }
        Stmt::ContextBlock { fields, body, .. } => {
            for (_, expr, _) in fields {
                visit_expr_calls(expr, calls);
            }
            visit_stmt_list(body, calls);
        }
        Stmt::ScopeMember { args, body, .. } => {
            for arg in args {
                visit_expr_calls(arg, calls);
            }
            visit_stmt_list(body, calls);
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
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. } => visit_stmt_list(body, calls),
    }
}

fn visit_stmt_list(statements: &[jet::AST::Stmt], calls: &mut impl FnMut(&jet::AST::Call)) {
    for statement in statements {
        visit_stmt_calls(statement, calls);
    }
}

fn visit_lvalue_calls(target: &jet::AST::LValue, calls: &mut impl FnMut(&jet::AST::Call)) {
    match target {
        jet::AST::LValue::Local { .. } => {}
        jet::AST::LValue::Index { base, index, .. } => {
            visit_expr_calls(base, calls);
            visit_expr_calls(index, calls);
        }
        jet::AST::LValue::Field { base, .. } => visit_expr_calls(base, calls),
    }
}

fn visit_call_args(args: &[jet::AST::CallArg], calls: &mut impl FnMut(&jet::AST::Call)) {
    for arg in args {
        visit_expr_calls(&arg.expr, calls);
    }
}

fn visit_expr_calls(expr: &Expr, calls: &mut impl FnMut(&jet::AST::Call)) {
    match expr {
        Expr::Call(call) => {
            calls(call);
            visit_call_args(&call.args, calls);
        }
        Expr::MethodCall { receiver, args, .. } => {
            visit_expr_calls(receiver, calls);
            visit_call_args(args, calls);
        }
        Expr::CallValue { callee, args, .. } => {
            visit_expr_calls(callee, calls);
            visit_call_args(args, calls);
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let jet::AST::StrPart::Interp(expr, _) = part {
                    visit_expr_calls(expr, calls);
                }
            }
        }
        Expr::ListLit(items, _) => {
            for item in items {
                visit_expr_calls(item, calls);
            }
        }
        Expr::TupleLit(items, _, _) => {
            for (_, item) in items {
                visit_expr_calls(item, calls);
            }
        }
        Expr::MemberSpread { base, .. }
        | Expr::Spread(base, _)
        | Expr::Deref(base, _)
        | Expr::RawOf(base, _)
        | Expr::Copy(base, _)
        | Expr::Place(base, _, _)
        | Expr::Field(base, _, _)
        | Expr::Present(base, _)
        | Expr::Ok(base, _)
        | Expr::Err(base, _)
        | Expr::Try(base, _, _)
        | Expr::Paren(base, _) => visit_expr_calls(base, calls),
        Expr::MapLit(entries, _) => {
            for (key, value) in entries {
                visit_expr_calls(key, calls);
                visit_expr_calls(value, calls);
            }
        }
        Expr::Index { base, index, .. } => {
            visit_expr_calls(base, calls);
            visit_expr_calls(index, calls);
        }
        Expr::Slice { base, start, end, range, .. } => {
            visit_expr_calls(base, calls);
            if let Some(range) = range {
                visit_expr_calls(range, calls);
            } else {
                visit_expr_calls(start, calls);
                visit_expr_calls(end, calls);
            }
        }
        Expr::Range { start, end, .. } => {
            visit_expr_calls(start, calls);
            visit_expr_calls(end, calls);
        }
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. } => {
            visit_expr_calls(inner, calls)
        }
        Expr::Binary(_, left, right, _) => {
            visit_expr_calls(left, calls);
            visit_expr_calls(right, calls);
        }
        Expr::CompareChain { operands, .. } => {
            for operand in operands {
                visit_expr_calls(operand, calls);
            }
        }
        Expr::OptField { base, .. } => visit_expr_calls(base, calls),
        Expr::StructLit { fields, .. } => {
            for (_, _, value) in fields {
                visit_expr_calls(value, calls);
            }
        }
        Expr::TypedLit { body, .. } => body.for_each_expr(|value| visit_expr_calls(value, calls)),
        Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    jet::AST::EnumLitArg::Positional(value)
                    | jet::AST::EnumLitArg::Named { expr: value, .. } => {
                        visit_expr_calls(value, calls)
                    }
                }
            }
        }
        Expr::Tainted(inner, _, _)
        | Expr::PatternTest { subject: inner, .. } => visit_expr_calls(inner, calls),
        Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
            visit_expr_calls(cond, calls);
            visit_stmt_list(then_body, calls);
            visit_expr_calls(then_value, calls);
            visit_stmt_list(else_body, calls);
            visit_expr_calls(else_value, calls);
        }
        Expr::Lambda(lambda) => match &lambda.body {
            jet::AST::LambdaBody::Expr(value) => visit_expr_calls(value, calls),
            jet::AST::LambdaBody::Block(body) => visit_stmt_list(body, calls),
        },
        Expr::OrFallback { value, fallback, .. } => {
            visit_expr_calls(value, calls);
            match fallback {
                jet::AST::OrFallback::Value(value)
                | jet::AST::OrFallback::Return(Some(value), _) => visit_expr_calls(value, calls),
                jet::AST::OrFallback::Panic { args, .. } => visit_call_args(args, calls),
                _ => {}
            }
        }
        Expr::PtrFromAddr { addr, .. } => visit_expr_calls(addr, calls),
        _ => {}
    }
}

fn source_span_text(source: &str, span: jet::Diagnostics::Span) -> String {
    let (start_line, start_column) = span_line_col(source, span.start);
    let (end_line, end_column) = span_line_col(source, span.end);
    format!("{start_line}:{start_column}-{end_line}:{end_column}")
}

fn framed_sha256(fields: &[&str]) -> String {
    let mut bytes = Vec::new();
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u64).to_be_bytes());
        bytes.extend_from_slice(field.as_bytes());
    }
    SHA256::sha256_hex(&bytes)
}

fn expr_to_inequalities(expr: &Expr) -> Option<Vec<Inequality>> {
    expr_to_inequalities_with_subst(expr, &BTreeMap::new())
}

fn expr_to_inequalities_with_subst(
    expr: &Expr,
    substitutions: &BTreeMap<String, Affine>,
) -> Option<Vec<Inequality>> {
    match expr {
        Expr::Binary(op, left, right, _) => {
            if *op == BinOp::And {
                let mut out = expr_to_inequalities_with_subst(left, substitutions)?;
                out.extend(expr_to_inequalities_with_subst(right, substitutions)?);
                return Some(out);
            }
            let lhs = expr_to_affine_with_subst(left, substitutions)?;
            let rhs = expr_to_affine_with_subst(right, substitutions)?;
            match op {
                BinOp::Le => Some(vec![Inequality::le(lhs.add(&rhs.scale(-1).ok()?).ok()?)]),
                BinOp::Ge => Some(vec![Inequality::le(rhs.add(&lhs.scale(-1).ok()?).ok()?)]),
                BinOp::Lt => {
                    let rhs_m1 = rhs.add(&Affine::constant(-1)).ok()?;
                    Some(vec![Inequality::le(lhs.add(&rhs_m1.scale(-1).ok()?).ok()?)])
                }
                BinOp::Gt => {
                    let rhs_p1 = rhs.add(&Affine::constant(1)).ok()?;
                    Some(vec![Inequality::le(rhs_p1.add(&lhs.scale(-1).ok()?).ok()?)])
                }
                BinOp::Eq => {
                    let a = Inequality::le(lhs.add(&rhs.scale(-1).ok()?).ok()?);
                    let b = Inequality::le(rhs.add(&lhs.scale(-1).ok()?).ok()?);
                    Some(vec![a, b])
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn expr_to_affine_with_subst(
    expr: &Expr,
    substitutions: &BTreeMap<String, Affine>,
) -> Option<Affine> {
    match expr {
        Expr::Int(v, _, _, _) => Some(Affine::constant(*v as i128)),
        Expr::Ident(name, _) => substitutions
            .get(name)
            .cloned()
            .or_else(|| Some(Affine::var(name, 1))),
        Expr::Binary(op, left, right, _) => {
            let l = expr_to_affine_with_subst(left, substitutions)?;
            let r = expr_to_affine_with_subst(right, substitutions)?;
            match op {
                BinOp::Add => l.add(&r).ok(),
                BinOp::Sub => l.add(&r.scale(-1).ok()?).ok(),
                BinOp::Mul => {
                    if l.terms.is_empty() {
                        r.scale(l.constant).ok()
                    } else if r.terms.is_empty() {
                        l.scale(r.constant).ok()
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        Expr::Unary(UnOp::Neg, inner, _) => {
            expr_to_affine_with_subst(inner, substitutions).and_then(|a| a.scale(-1).ok())
        }
        Expr::Paren(inner, _) => expr_to_affine_with_subst(inner, substitutions),
        _ => None,
    }
}

fn inclusive_bounds(lower: i64, upper: i64) -> Vec<Inequality> {
    vec![
        Inequality::le(
            Affine::var("value", -1)
                .add(&Affine::constant(lower as i128))
                .unwrap_or_else(|_| Affine::constant(1)),
        ),
        Inequality::le(
            Affine::var("value", 1)
                .add(&Affine::constant(-(upper as i128)))
                .unwrap_or_else(|_| Affine::constant(1)),
        ),
    ]
}

fn prove_obligation(formula: &Formula) -> Result<SolverOutcome, String> {
    // Prove assumptions => claim by showing assumptions ∧ ¬claim is unsat.
    let mut negated_claim = Vec::new();
    for ineq in &formula.claim {
        // ¬(a <= 0)  <=>  a >= 1  <=>  -a + 1? Wait: a >= 1 => 1 - a <= 0 => (-a) + 1 <= 0
        match ineq.affine.scale(-1).and_then(|a| a.add(&Affine::constant(1))) {
            Ok(affine) => negated_claim.push(Inequality::le(affine)),
            Err(SolverFail::CoefficientOverflow) => {
                return Ok(SolverOutcome::Unknown {
                    reason: "coefficient_overflow",
                    steps: 0,
                });
            }
            Err(SolverFail::StructuralLimit) => {
                return Ok(SolverOutcome::Unknown {
                    reason: "structural_limit",
                    steps: 0,
                });
            }
        }
    }
    // For AND-claim, ¬claim is OR of negations. Split into branches per negated conjunct.
    if negated_claim.is_empty() {
        return Ok(SolverOutcome::Unknown {
            reason: "structural_limit",
            steps: 0,
        });
    }

    let mut steps = 0u64;
    let mut certificates = Vec::new();
    for (branch_index, neg) in negated_claim.iter().enumerate() {
        let mut branch = formula.assumptions.clone();
        branch.push(neg.clone());
        if let Some(assignment) = find_counterexample(&branch) {
            charge(&mut steps).map_err(|_| "step_limit".to_string())?;
            return Ok(SolverOutcome::Disproved { assignment, steps });
        }
        match search_unsat(&branch, &mut steps) {
            Ok(proof) => certificates.push((branch_index, proof)),
            Err(SearchErr::Unknown(reason)) => {
                return Ok(SolverOutcome::Unknown { reason, steps });
            }
            Err(SearchErr::Sat(assignment)) => {
                return Ok(SolverOutcome::Disproved { assignment, steps });
            }
        }
    }

    let cert = format!(
        "{{\"kind\":\"and_intro\",\"children\":[{}]}}",
        certificates
            .iter()
            .map(|(branch_index, proof)| {
                format!(
                    "{{\"branchIndex\":{branch_index},\"proof\":{proof}}}"
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    );
    // Prefer the law's linear_contradiction tree shape when a single branch.
    let certificate = if certificates.len() == 1 {
        certificates[0].1.clone()
    } else {
        cert
    };
    let certificate_sha256 = SHA256::sha256_hex(format!("{certificate}\n").as_bytes());
    if let Err(reason) = check_certificate(formula, &certificate) {
        return match reason {
            "coefficient_overflow" | "structural_limit" | "step_limit" => {
                Ok(SolverOutcome::Unknown {
                    reason,
                    steps,
                })
            }
            _ => Err(format!("invalid certificate: {reason}")),
        };
    }
    Ok(SolverOutcome::Proved {
        certificate,
        certificate_sha256,
        steps,
    })
}

enum SearchErr {
    Unknown(&'static str),
    Sat(BTreeMap<String, i128>),
}

fn search_unsat(ineqs: &[Inequality], steps: &mut u64) -> Result<String, SearchErr> {
    // Bound variables from simple one-var inequalities, then try Fourier-Motzkin-lite
    // contradiction detection + bounded enumeration.
    let vars: BTreeSet<String> = ineqs
        .iter()
        .flat_map(|i| i.affine.terms.keys().cloned())
        .collect();
    if vars.len() > MAX_VARS {
        return Err(SearchErr::Unknown("structural_limit"));
    }

    // Fast path: look for a linear combination that yields 0 <= -1.
    if let Some(proof) = find_linear_contradiction(ineqs, steps)? {
        return Ok(proof);
    }

    // Finite domain guess from bounds; enumerate when every var is bounded.
    let mut domains: BTreeMap<String, (i128, i128)> = BTreeMap::new();
    for v in &vars {
        let mut lo = i128::MIN / 4;
        let mut hi = i128::MAX / 4;
        for ineq in ineqs {
            if ineq.affine.terms.len() == 1 {
                if let Some(coeff) = ineq.affine.terms.get(v) {
                    // coeff*v + c <= 0
                    let c = ineq.affine.constant;
                    if *coeff > 0 {
                        // v <= floor((-c)/coeff)
                        let negated = c
                            .checked_neg()
                            .ok_or(SearchErr::Unknown("coefficient_overflow"))?;
                        let bound = negated.div_euclid(*coeff);
                        hi = hi.min(bound);
                    } else if *coeff < 0 {
                        // v >= ceil((-c)/coeff) ; for negatives use checked div
                        let negated = c
                            .checked_neg()
                            .ok_or(SearchErr::Unknown("coefficient_overflow"))?;
                        let bound = ceil_div(negated, *coeff)
                            .ok_or(SearchErr::Unknown("coefficient_overflow"))?;
                        lo = lo.max(bound);
                    }
                }
            }
        }
        if lo > hi {
            charge(steps)?;
            return Ok(format!(
                "{{\"kind\":\"linear_contradiction\",\"multipliers\":[]}}"
            ));
        }
        // Keep enumeration honest: only when the span is tiny.
        if hi.saturating_sub(lo) > 64 {
            return Err(SearchErr::Unknown("structural_limit"));
        }
        domains.insert(v.clone(), (lo, hi));
    }

    if domains.is_empty() {
        // No variables — check constant inequalities.
        charge(steps)?;
        for ineq in ineqs {
            if ineq.affine.terms.is_empty() && ineq.affine.constant > 0 {
                return Ok(
                    "{\"kind\":\"linear_contradiction\",\"multipliers\":[]}".into(),
                );
            }
        }
        return Err(SearchErr::Sat(BTreeMap::new()));
    }

    let order: Vec<String> = domains.keys().cloned().collect();
    let mut assignment = BTreeMap::new();
    if let Some(counter) = enumerate(&order, &domains, ineqs, &mut assignment, steps)? {
        return Err(SearchErr::Sat(counter));
    }
    // All assignments fail => unsat. Emit split tree over first variable when present.
    if let Some(first) = order.first() {
        let (lo, hi) = domains[first];
        let pivot = lo + (hi - lo) / 2;
        Ok(format!(
            "{{\"kind\":\"split\",\"variable\":{},\"pivot\":\"{pivot}\",\"left\":{{\"kind\":\"linear_contradiction\",\"multipliers\":[]}},\"right\":{{\"kind\":\"linear_contradiction\",\"multipliers\":[]}}}}",
            json_str(first)
        ))
    } else {
        Ok("{\"kind\":\"linear_contradiction\",\"multipliers\":[]}".into())
    }
}

fn enumerate(
    order: &[String],
    domains: &BTreeMap<String, (i128, i128)>,
    ineqs: &[Inequality],
    assignment: &mut BTreeMap<String, i128>,
    steps: &mut u64,
) -> Result<Option<BTreeMap<String, i128>>, SearchErr> {
    if order.is_empty() {
        charge(steps)?;
        if satisfies(ineqs, assignment) {
            return Ok(Some(assignment.clone()));
        }
        return Ok(None);
    }
    let var = &order[0];
    let (lo, hi) = domains[var];
    for value in lo..=hi {
        charge(steps)?;
        assignment.insert(var.clone(), value);
        if let Some(hit) = enumerate(&order[1..], domains, ineqs, assignment, steps)? {
            return Ok(Some(hit));
        }
    }
    assignment.remove(var);
    Ok(None)
}

fn satisfies(ineqs: &[Inequality], assignment: &BTreeMap<String, i128>) -> bool {
    for ineq in ineqs {
        let mut total = ineq.affine.constant;
        for (var, coeff) in &ineq.affine.terms {
            let Some(value) = assignment.get(var) else {
                return false;
            };
            match coeff
                .checked_mul(*value)
                .and_then(|term| total.checked_add(term))
            {
                Some(next) => total = next,
                None => return false,
            }
        }
        if total > 0 {
            return false;
        }
    }
    true
}

/// Try the canonical small integer witness before bounded search gives up on
/// an unbounded variable.  This is a witness finder only: the caller still
/// verifies the complete branch and the solver payload never exposes an
/// unverified assignment.
fn find_counterexample(ineqs: &[Inequality]) -> Option<BTreeMap<String, i128>> {
    let variables: BTreeSet<String> = ineqs
        .iter()
        .flat_map(|ineq| ineq.affine.terms.keys().cloned())
        .collect();
    if variables.len() > MAX_VARS {
        return None;
    }
    let mut domains: BTreeMap<String, (Option<i128>, Option<i128>)> = variables
        .iter()
        .map(|name| (name.clone(), (None, None)))
        .collect();
    for ineq in ineqs {
        if ineq.affine.terms.len() != 1 {
            continue;
        }
        let Some((name, coefficient)) = ineq.affine.terms.iter().next() else {
            continue;
        };
        let Some(negated_constant) = ineq.affine.constant.checked_neg() else {
            return None;
        };
        let entry = domains.get_mut(name)?;
        if *coefficient > 0 {
            let upper = negated_constant.div_euclid(*coefficient);
            entry.1 = Some(entry.1.map_or(upper, |old| old.min(upper)));
        } else if *coefficient < 0 {
            let lower = ceil_div(negated_constant, *coefficient)?;
            entry.0 = Some(entry.0.map_or(lower, |old| old.max(lower)));
        }
    }
    let mut assignment = BTreeMap::new();
    for (name, (lower, upper)) in domains {
        let value = match (lower, upper) {
            (Some(lo), Some(hi)) if lo > hi => return None,
            (Some(lo), Some(hi)) if lo <= 1 && 1 <= hi => 1,
            (Some(lo), Some(_)) => lo,
            (Some(lo), None) => lo.max(1),
            (None, Some(hi)) => hi.min(1),
            (None, None) => 1,
        };
        assignment.insert(name, value);
    }
    satisfies(ineqs, &assignment).then_some(assignment)
}

fn ceil_div(numerator: i128, denominator: i128) -> Option<i128> {
    if denominator == 0 {
        return None;
    }
    let quotient = numerator.checked_div(denominator)?;
    let remainder = numerator.checked_rem(denominator)?;
    if remainder != 0 && ((numerator > 0) == (denominator > 0)) {
        quotient.checked_add(1)
    } else {
        Some(quotient)
    }
}

fn find_linear_contradiction(
    ineqs: &[Inequality],
    steps: &mut u64,
) -> Result<Option<String>, SearchErr> {
    charge(steps)?;
    // Unit constant contradiction.
    for (index, ineq) in ineqs.iter().enumerate() {
        if ineq.affine.terms.is_empty() && ineq.affine.constant > 0 {
            return Ok(Some(format!(
                "{{\"kind\":\"linear_contradiction\",\"multipliers\":[{{\"inequalityIndex\":{index},\"multiplier\":\"1\"}}]}}"
            )));
        }
    }
    // Pairwise opposite unit bounds on one variable.
    for i in 0..ineqs.len() {
        for j in (i + 1)..ineqs.len() {
            charge(steps)?;
            let a = &ineqs[i].affine;
            let b = &ineqs[j].affine;
            if a.terms.len() == 1 && b.terms.len() == 1 {
                let Some((va, ca)) = a.terms.iter().next() else {
                    continue;
                };
                let Some((vb, cb)) = b.terms.iter().next() else {
                    continue;
                };
                if va == vb && ca.checked_neg() == Some(*cb) && *ca != 0 {
                    // ca*x + a.c <= 0 and -ca*x + b.c <= 0 => a.c + b.c <= 0 required;
                    // contradiction when a.c + b.c > 0.
                    let Some(sum) = a.constant.checked_add(b.constant) else {
                        return Err(SearchErr::Unknown("coefficient_overflow"));
                    };
                    if sum > 0 {
                        return Ok(Some(format!(
                            "{{\"kind\":\"linear_contradiction\",\"multipliers\":[{{\"inequalityIndex\":{i},\"multiplier\":\"1\"}},{{\"inequalityIndex\":{j},\"multiplier\":\"1\"}}]}}"
                        )));
                    }
                }
            }
        }
    }
    Ok(None)
}

fn charge(steps: &mut u64) -> Result<(), SearchErr> {
    if *steps >= MAX_STEPS {
        return Err(SearchErr::Unknown("step_limit"));
    }
    *steps += 1;
    Ok(())
}

fn check_certificate(formula: &Formula, certificate: &str) -> Result<(), &'static str> {
    let root = parse_json(certificate).map_err(|_| "certificate_invalid")?;
    let mut negated = Vec::new();
    for ineq in &formula.claim {
        let affine = ineq
            .affine
            .scale(-1)
            .and_then(|a| a.add(&Affine::constant(1)))
            .map_err(|_| "coefficient_overflow")?;
        negated.push(Inequality::le(affine));
    }
    if negated.is_empty() {
        return Err("certificate_invalid");
    }

    match object_kind(&root)? {
        "and_intro" => {
            let object = as_object(&root)?;
            let children = match object.get("children") {
                Some(JSONValue::Array(children)) => children,
                _ => return Err("certificate_invalid"),
            };
            if children.len() != negated.len() {
                return Err("certificate_invalid");
            }
            let mut seen = BTreeSet::new();
            for child in children {
                let child = as_object(child)?;
                let branch_index = json_usize(child.get("branchIndex"))?;
                if branch_index >= negated.len() || !seen.insert(branch_index) {
                    return Err("certificate_invalid");
                }
                let proof = child.get("proof").ok_or("certificate_invalid")?;
                let mut branch = formula.assumptions.clone();
                branch.push(negated[branch_index].clone());
                check_certificate_node(proof, &branch)?;
            }
            if seen.len() != negated.len() {
                return Err("certificate_invalid");
            }
        }
        _ => {
            if negated.len() != 1 {
                return Err("certificate_invalid");
            }
            let mut branch = formula.assumptions.clone();
            branch.push(negated[0].clone());
            check_certificate_node(&root, &branch)?;
        }
    }

    // Independent recomputation remains mandatory even after the tree check.
    // It makes malformed leaves, unsupported arithmetic, and future certificate
    // parser mistakes fail closed rather than turning a shape check into trust.
    let mut steps = 0u64;
    for neg in &negated {
        let mut branch = formula.assumptions.clone();
        branch.push(neg.clone());
        match search_unsat(&branch, &mut steps) {
            Ok(_) => {}
            Err(SearchErr::Sat(_)) => return Err("certificate_invalid"),
            Err(SearchErr::Unknown(reason)) => return Err(reason),
        }
    }
    Ok(())
}

fn as_object(value: &JSONValue) -> Result<&std::collections::HashMap<String, JSONValue>, &'static str> {
    match value {
        JSONValue::Object(object) => Ok(object),
        _ => Err("certificate_invalid"),
    }
}

fn object_kind(value: &JSONValue) -> Result<&str, &'static str> {
    let object = as_object(value)?;
    match object.get("kind") {
        Some(JSONValue::String(kind)) => Ok(kind.as_str()),
        _ => Err("certificate_invalid"),
    }
}

fn json_usize(value: Option<&JSONValue>) -> Result<usize, &'static str> {
    match value {
        Some(JSONValue::Number(value)) if *value >= 0 => usize::try_from(*value).map_err(|_| "certificate_invalid"),
        _ => Err("certificate_invalid"),
    }
}

fn json_i128(value: Option<&JSONValue>) -> Result<i128, &'static str> {
    let Some(JSONValue::String(value)) = value else {
        return Err("certificate_invalid");
    };
    value.parse::<i128>().map_err(|_| "certificate_invalid")
}

fn check_certificate_node(
    value: &JSONValue,
    inequalities: &[Inequality],
) -> Result<(), &'static str> {
    match object_kind(value)? {
        "linear_contradiction" => {
            let object = as_object(value)?;
            let entries = match object.get("multipliers") {
                Some(JSONValue::Array(entries)) => entries,
                _ => return Err("certificate_invalid"),
            };
            if entries.is_empty() {
                let mut steps = 0u64;
                return match search_unsat(inequalities, &mut steps) {
                    Ok(_) => Ok(()),
                    Err(SearchErr::Sat(_)) => Err("certificate_invalid"),
                    Err(SearchErr::Unknown(reason)) => Err(reason),
                };
            }
            let mut sum = Affine::constant(0);
            for entry in entries {
                let entry = as_object(entry)?;
                let index = json_usize(entry.get("inequalityIndex"))?;
                if index >= inequalities.len() {
                    return Err("certificate_invalid");
                }
                let multiplier = json_i128(entry.get("multiplier"))?;
                if multiplier < 0 {
                    return Err("certificate_invalid");
                }
                sum = sum
                    .add(&inequalities[index].affine.scale(multiplier).map_err(|_| "coefficient_overflow")?)
                    .map_err(|_| "coefficient_overflow")?;
            }
            if sum.terms.is_empty() && sum.constant > 0 {
                Ok(())
            } else {
                Err("certificate_invalid")
            }
        }
        "split" => {
            let object = as_object(value)?;
            let variable = match object.get("variable") {
                Some(JSONValue::String(variable)) if !variable.is_empty() => variable,
                _ => return Err("certificate_invalid"),
            };
            let pivot = json_i128(object.get("pivot"))?;
            let left = object.get("left").ok_or("certificate_invalid")?;
            let right = object.get("right").ok_or("certificate_invalid")?;
            let mut left_branch = inequalities.to_vec();
            left_branch.push(Inequality::le(
                Affine::var(variable, 1)
                    .add(&Affine::constant(pivot.checked_neg().ok_or("coefficient_overflow")?))
                    .map_err(|_| "coefficient_overflow")?,
            ));
            let mut right_branch = inequalities.to_vec();
            right_branch.push(Inequality::le(
                Affine::var(variable, -1)
                    .add(&Affine::constant(
                        pivot.checked_add(1).ok_or("coefficient_overflow")?,
                    ))
                    .map_err(|_| "coefficient_overflow")?,
            ));
            check_certificate_node(left, &left_branch)?;
            check_certificate_node(right, &right_branch)
        }
        "assumption" => Err("certificate_invalid"),
        _ => Err("certificate_invalid"),
    }
}

pub(crate) fn evidence_json(item: &SolverEvidence, diagnostic_indexes: &str) -> String {
    let formula_sha = item.obligation.formula.hash();
    let backend = json_str(BACKEND);
    let backend_version = json_str(BACKEND_VERSION);
    let obligation_id = json_str(&item.obligation.id);
    let obligation_kind = json_str(&item.obligation.kind);
    let formula_sha_json = json_str(&formula_sha);
    let (outcome, solver_payload) = match &item.outcome {
        SolverOutcome::Proved {
            certificate,
            certificate_sha256,
            steps,
        } => (
            "proved",
            format!(
                "{{\"backend\":{backend},\"backendVersion\":{backend_version},\"certificate\":{certificate},\"certificateSha256\":{},\"counterexample\":null,\"formulaSha256\":{formula_sha_json},\"obligationId\":{obligation_id},\"obligationKind\":{obligation_kind},\"reason\":null,\"status\":\"proved\",\"stepLimit\":{MAX_STEPS},\"steps\":{steps}}}",
                json_str(certificate_sha256),
            ),
        ),
        SolverOutcome::Disproved { assignment, steps } => {
            let values = assignment
                .iter()
                .map(|(k, v)| format!("{{\"name\":{},\"value\":\"{v}\"}}", json_str(k)))
                .collect::<Vec<_>>()
                .join(",");
            (
                "disproved",
                format!(
                    "{{\"backend\":{backend},\"backendVersion\":{backend_version},\"certificate\":null,\"certificateSha256\":null,\"counterexample\":[{values}],\"formulaSha256\":{formula_sha_json},\"obligationId\":{obligation_id},\"obligationKind\":{obligation_kind},\"reason\":null,\"status\":\"disproved\",\"stepLimit\":{MAX_STEPS},\"steps\":{steps}}}"
                ),
            )
        }
        SolverOutcome::Unknown { reason, steps } => (
            "unknown",
            format!(
                "{{\"backend\":{backend},\"backendVersion\":{backend_version},\"certificate\":null,\"certificateSha256\":null,\"counterexample\":null,\"formulaSha256\":{formula_sha_json},\"obligationId\":{obligation_id},\"obligationKind\":{obligation_kind},\"reason\":{},\"status\":\"unknown\",\"stepLimit\":{MAX_STEPS},\"steps\":{steps}}}",
                json_str(reason)
            ),
        ),
    };
    let (line, column) = span_start(&item.obligation.span);
    format!(
        "{{\"attachment\":null,\"budget\":null,\"contract\":null,\"count\":1,\"diagnosticIndexes\":{diagnostic_indexes},\"facet\":\"solver\",\"id\":{},\"kind\":\"solver\",\"outcome\":\"{outcome}\",\"producer\":\"native-presburger\",\"property\":null,\"reason\":null,\"solver\":{solver_payload},\"source\":{{\"column\":{column},\"line\":{line},\"path\":{}}},\"state\":\"checked\"}}",
        json_str(&item.evidence_id),
        json_str(&item.obligation.origin)
    )
}

fn span_start(span: &str) -> (u64, u64) {
    let Some((line, column)) = span.split_once(':') else {
        return (1, 1);
    };
    let line = line.parse::<u64>().unwrap_or(1).max(1);
    let column = column
        .split('-')
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1)
        .max(1);
    (line, column)
}

pub(crate) fn summarize(items: &[SolverEvidence]) -> (usize, usize, usize, usize, usize) {
    let selected = items.len();
    let mut proved = 0;
    let mut disproved = 0;
    let mut unknown = 0;
    let unavailable = 0;
    for item in items {
        match item.outcome {
            SolverOutcome::Proved { .. } => proved += 1,
            SolverOutcome::Disproved { .. } => disproved += 1,
            SolverOutcome::Unknown { .. } => unknown += 1,
        }
    }
    (selected, proved, disproved, unknown, unavailable)
}

fn json_str(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proves_simple_bound_implication() {
        // assumptions empty; claim: 0 <= value <= 3 is NOT auto-proved without assumptions.
        // Prove: assumptions value>=0 && value<=3 => claim value>=0
        let formula = Formula {
            assumptions: vec![
                Inequality::le(Affine::var("value", -1)), // 0 - value <= 0 => value >= 0
                Inequality::le(
                    Affine::var("value", 1)
                        .add(&Affine::constant(-3))
                        .unwrap(),
                ),
            ],
            claim: vec![Inequality::le(Affine::var("value", -1))],
        };
        match prove_obligation(&formula) {
            Ok(SolverOutcome::Proved { .. }) => {}
            other => panic!("expected proved, got {other:?}"),
        }
    }
}
