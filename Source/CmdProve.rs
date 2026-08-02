//! D-PROVE-SEM1: first real `jet prove` producer tranche.
//!
//! This owns target discovery, front-end evidence, runtime producers, and the
//! canonical ProofReport/artifact boundary.

use std::fs;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::process::Command;
use std::process::Stdio;
use std::time::{Duration, Instant};

use jet::AST::{Expr, Func, Item, Stmt};
use jet::Diagnostics::{span_line_col, Diagnostic, Severity};
use jet::ExitCodes;

#[derive(Clone)]
struct Member {
    path: String,
    bytes: Vec<u8>,
    sha256: String,
}

struct Target {
    kind: &'static str,
    root: String,
    members: Vec<Member>,
    identity_members: Vec<(String, String)>,
    input_sha256: String,
}

struct FrontEndItem {
    id: String,
    path: String,
    diagnostic: Option<Diagnostic>,
    source: String,
    facet: &'static str,
    line: usize,
    column: usize,
}

struct ContractDeclaration {
    id: String,
    path: String,
    line: usize,
    column: usize,
    marker: &'static str,
}

struct TestItem {
    id: String,
    path: String,
    state: u8,
    kind: u8,
    message: String,
    line: u32,
    name: String,
    seed: String,
}

struct ProducerRecord {
    kind: u8,
    state: u8,
    name: String,
    message: String,
    file: String,
    line: u32,
}

enum ChildOutcome {
    Exited(Option<i32>),
    TimedOut,
    LaunchFailed,
}

pub(crate) fn run_prove(args: &[String], json: bool) {
    let mut positional = Vec::new();
    let mut lenses = Vec::new();
    let mut capture: Option<crate::ProveReplay::CaptureOpts> = None;
    let mut replay: Option<String> = None;
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--json" {
            i += 1;
            continue;
        }
        if let Some(opts) = crate::ProveReplay::parse_capture_flag(arg) {
            if capture.is_some() || replay.is_some() {
                eprintln!("error: `jet prove` accepts at most one of `--capture` / `--replay`");
                exit(ExitCodes::USAGE);
            }
            capture = Some(opts);
            i += 1;
            continue;
        }
        if let Some(parsed) = crate::ProveReplay::parse_replay_flag(arg, args.get(i + 1).map(String::as_str)) {
            if capture.is_some() || replay.is_some() {
                eprintln!("error: `jet prove` accepts at most one of `--capture` / `--replay`");
                exit(ExitCodes::USAGE);
            }
            match parsed {
                Ok(path) => {
                    replay = Some(path);
                    i += if arg == "--replay" { 2 } else { 1 };
                }
                Err(message) => {
                    eprintln!("error: {message}");
                    exit(ExitCodes::USAGE);
                }
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--lens=") {
            validate_lens(value, positional.first().copied().unwrap_or("TARGET"), json);
            lenses.push(value.to_string());
            i += 1;
            continue;
        }
        if arg == "--lens" {
            let value = args.get(i + 1).map(String::as_str).unwrap_or("");
            validate_lens(value, positional.first().copied().unwrap_or("TARGET"), json);
            lenses.push(value.to_string());
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            eprintln!("error: unknown `jet prove` flag `{arg}`");
            exit(ExitCodes::USAGE);
        }
        positional.push(arg);
        i += 1;
    }
    if positional.len() != 1 {
        eprintln!("error: `jet prove` needs exactly one file, package, or workspace target");
        eprintln!(" Fix: jet prove path/to/program.jet");
        exit(ExitCodes::USAGE);
    }
    let target = match resolve_target(positional[0]) {
        Ok(target) => target,
        Err(message) => {
            eprintln!("error: {message}");
            exit(ExitCodes::USER_ERROR);
        }
    };

    let identity = crate::ProveReplay::ReplayIdentity {
        entry: target.root.clone(),
        source_digest: target.input_sha256.clone(),
        execution_adapter: "dev-tir-v1".to_string(),
        target_triple: host_target_triple(),
    };
    if let Some(opts) = capture.as_ref() {
        if let Err(status) = preflight_capture_target(&target, opts, json) {
            exit(status);
        }
    }
    let capture_authority = if let Some(opts) = capture {
        match crate::ProveReplay::prepare_safe_capture(&opts, json) {
            Ok(authority) => Some(authority),
            Err(status) => exit(status),
        }
    } else {
        None
    };
    let replay_authority = if let Some(path) = replay {
        match crate::ProveReplay::prepare_replay(&identity, &path) {
            Ok(authority) => {
                std::env::set_var("JET_PROVE_REPLAY_TIME_MS", authority.time_ms.to_string());
                eprintln!("ambient authority opened: Time; {} exact", identity.execution_adapter);
                Some(authority)
            }
            Err((code, why)) => {
                eprintln!("Error [{code}]: {why}");
                exit(ExitCodes::USER_ERROR);
            }
        }
    } else {
        None
    };
    // Only a validated replay artifact may add replay evidence to the report.
    // The environment variable is an execution-tier adapter, not proof that a
    // replay was requested; accepting it here would let ambient process state
    // forge a replay claim.
    let replay_time_ms = replay_authority.as_ref().map(|authority| authority.time_ms);

    let mut items = Vec::new();
    let mut declarations = Vec::new();
    for member in &target.members {
        let source = String::from_utf8_lossy(&member.bytes).into_owned();
        let (semantic_items, member_declarations) = semantic_front_end_items(&target, &member.path, &source);
        declarations.extend(member_declarations);
        let diagnostics = jet::check_with_path(&member.path);
        if diagnostics.is_empty() {
            items.extend(semantic_items.into_iter().map(|mut item| {
                item.source = source.clone();
                item
            }));
        } else {
            for diagnostic in diagnostics {
                let span = diagnostic_span(&source, &diagnostic);
                let (line, column) = diagnostic.span
                    .map(|span| span_line_col(&source, span.start))
                    .unwrap_or((1, 1));
                let claim = format!("{}:{}", diagnostic.code, diagnostic.what);
                items.push(FrontEndItem {
                    id: evidence_id(&target, "front_end", &member.path, &span, &claim),
                    path: member.path.clone(),
                    diagnostic: Some(diagnostic),
                    facet: "all",
                    source: source.clone(),
                    line,
                    column,
                });
            }
        }
    }

    let failed = items.iter().filter(|item| item.diagnostic.is_some()).count();
    let proved = items.len() - failed;
    let (tests, producer_exit) = if failed == 0 {
        run_test_producers(&target)
    } else {
        (Vec::new(), ExitCodes::OK)
    };
    let test_failed = tests.iter().filter(|item| (item.kind == 0 || item.kind == 3 || item.kind == 4) && item.state == 1).count();
    let budgets = budget_projection(&target);
    let budget_failed = budgets.facts.iter().any(|fact| fact.outcome == "fail");
    let enable_solver = lenses.iter().any(|lens| lens == "solver");
    let solver_members: Vec<(String, String)> = target
        .members
        .iter()
        .map(|member| {
            (
                member.path.clone(),
                String::from_utf8_lossy(&member.bytes).into_owned(),
            )
        })
        .collect();
    // The solver is a projection of the checked front end. Do not let it parse
    // and discharge a member after sema has rejected another member in the
    // target; that would turn a failed source check into partial proof output.
    let solver = if failed == 0 {
        match crate::ProveSolver::run_solver_producer(
            &solver_members,
            &target.input_sha256,
            enable_solver,
        ) {
            Ok(items) => items,
            Err(message) => {
                eprintln!("error: solver producer failed: {message}");
                exit(ExitCodes::ICE);
            }
        }
    } else {
        Vec::new()
    };
    let solver_disproved = solver
        .iter()
        .any(|item| matches!(item.outcome, crate::ProveSolver::SolverOutcome::Disproved { .. }));
    let exit_code = if producer_exit == ExitCodes::ICE {
        ExitCodes::ICE
    } else if producer_exit == ExitCodes::RUNTIME_PANIC {
        ExitCodes::RUNTIME_PANIC
    } else if failed > 0 || test_failed > 0 || budget_failed || solver_disproved {
        ExitCodes::USER_ERROR
    } else {
        ExitCodes::OK
    };
    if let Some(authority) = replay_authority.as_ref() {
        let outcome = if exit_code == ExitCodes::RUNTIME_PANIC {
            "panic"
        } else {
            "exit"
        };
        if authority.expected_status != exit_code || authority.expected_outcome != outcome {
            eprintln!(
                "Error [E3623]: replay diverged: captured outcome={} status={}, current outcome={} status={}",
                authority.expected_outcome, authority.expected_status, outcome, exit_code
            );
            eprintln!(" Fix: recapture with `--capture` so the normal producer result is authoritative");
            exit(ExitCodes::USER_ERROR);
        }
    }
    let report = render_report(
        &target,
        &items,
        &tests,
        &budgets,
        &solver,
        &lenses,
        &declarations,
        replay_time_ms,
        proved,
        failed,
        exit_code,
    );
    // D-JPROOF1=A (#1127): persist the exact ProofReport under the canonical
    // `.jetproof` envelope. Usage errors and internal compiler errors do not
    // produce evidence artifacts; `--json` still prints only the ProofReport
    // object for every producer outcome that has a valid report.
    if exit_code != ExitCodes::ICE {
        if let Err(message) = write_jetproof(&target, &report) {
            eprintln!("error: failed to write .jetproof: {message}");
            exit(ExitCodes::ICE);
        }
    }
    if failed == 0 && producer_exit != ExitCodes::ICE {
        if let Some(authority) = capture_authority.as_ref() {
            if let Err(status) = crate::ProveReplay::finalize_safe_capture(
                &identity,
                authority,
                exit_code,
                json,
            ) {
                exit(status);
            }
        }
    }
    if json {
        println!("{report}");
    } else {
        let show = |facet: &str| lens_shows(&lenses, facet);
        if !lenses.is_empty() {
            let mut uniq = lenses.clone();
            uniq.sort();
            uniq.dedup();
            println!("LENSES   {}", uniq.join(", "));
        }
        if show("refinements") || show("all") || show("effects") || show("taint") || lenses.is_empty() {
            println!("CHECKED  front end: {proved} proved, {failed} failed");
        }
        for item in &items {
            if let Some(diagnostic) = &item.diagnostic {
                eprintln!("{}", diagnostic.render(&item.path, &item.source));
            }
        }
        for row in outside_selected_lenses(&lenses, &items, &tests, &budgets, &declarations) {
            println!("{row}");
        }
        if show("tests") || lenses.is_empty() {
            if !tests.is_empty() {
                let passed = tests.iter().filter(|item| item.kind == 0 && item.state == 0).count();
                let skipped = tests.iter().filter(|item| item.kind == 0 && item.state == 2).count();
                println!("TESTS    unit: {passed} passed, {test_failed} failed, {skipped} skipped");
            }
        }
        if show("budgets") || lenses.is_empty() {
            if !budgets.facts.is_empty() {
                let met = budgets.facts.iter().filter(|fact| fact.outcome == "pass").count();
                let failed = budgets.facts.iter().filter(|fact| fact.outcome == "fail").count();
                let warned = budgets.facts.len() - met - failed;
                println!("BUDGETS  {met} met, {failed} failed, {warned} warned · verified canonical reports");
            }
        }
        if replay_time_ms.is_some() && (show("replay") || lenses.is_empty()) {
            println!("REPLAY   exact Time authority; execution adapter matched");
        }
        if enable_solver {
            let (selected, proved_s, disproved_s, unknown_s, _) =
                crate::ProveSolver::summarize(&solver);
            println!(
                "SOLVER   {selected} selected, {proved_s} proved, {disproved_s} disproved, {unknown_s} unknown"
            );
            for item in &solver {
                if let crate::ProveSolver::SolverOutcome::Disproved { assignment, .. } = &item.outcome
                {
                    let values = assignment
                        .iter()
                        .map(|(k, v)| format!("{k} = {v}"))
                        .collect::<Vec<_>>()
                        .join(" and ");
                    eprintln!(
                        "Error [E2950]: solver found a counterexample to {}",
                        item.obligation.kind
                    );
                    eprintln!(" Why: {values} satisfy the assumptions but make the claim false");
                    eprintln!(
                        " Fix: change {} so every return satisfies the claim, or correct the contract",
                        item.obligation.origin
                    );
                }
            }
        }
        let unavailable = tests.iter().filter(|item| item.state == 3).count();
        println!(
            "RESULT   {}",
            if failed > 0 || test_failed > 0 || budget_failed || solver_disproved {
                "fail"
            } else if unavailable > 0
                || solver.iter().any(|item| {
                    matches!(item.outcome, crate::ProveSolver::SolverOutcome::Unknown { .. })
                })
            {
                "pass_incomplete"
            } else {
                "pass"
            }
        );
    }
    exit(exit_code);
}

fn lens_shows(lenses: &[String], facet: &str) -> bool {
    if lenses.is_empty() {
        return true;
    }
    if lenses.iter().any(|lens| lens == "all") {
        return facet != "solver";
    }
    lenses.iter().any(|lens| lens == facet)
}

fn outside_selected_lenses(
    lenses: &[String],
    items: &[FrontEndItem],
    tests: &[TestItem],
    budgets: &jet::BudgetView::BudgetProjection,
    declarations: &[ContractDeclaration],
) -> Vec<String> {
    if lenses.is_empty() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for item in items.iter().filter(|item| item.diagnostic.is_some()) {
        rows.push(format!("  front end failed at {}:{}", item.path, item.line));
    }
    for item in tests.iter().filter(|item| item.state >= 1) {
        let kind = match item.kind {
            1 => "contract",
            2 => "runtime",
            3 => "property",
            4 => "doctest",
            _ => "unit",
        };
        if !lens_shows(lenses, if item.kind == 1 { "contracts" } else { "tests" }) {
            rows.push(format!(
                "  {kind} {}: {}",
                item.name,
                item.message.replace('\n', " ").replace('\r', " ")
            ));
        }
    }
    for declaration in declarations {
        let observed = tests.iter().any(|item| {
            item.kind == 1
                && item.name == declaration.marker
                && item.line as usize == declaration.line
                && source_paths_match(&item.path, &declaration.path)
                && item.state <= 1
        });
        if !observed && !lens_shows(lenses, "contracts") {
            rows.push(format!(
                "  contract {} at {}:{}: not observed",
                declaration.marker, declaration.path, declaration.line
            ));
        }
    }
    for fact in &budgets.facts {
        let unavailable = fact.evidence == "unavailable";
        let failed = fact.outcome != "pass";
        if (unavailable || failed) && !lens_shows(lenses, "budgets") {
            rows.push(format!(
                "  budget {}: {} ({})",
                fact.budget_id, fact.outcome, fact.evidence
            ));
        }
    }
    if rows.is_empty() {
        return vec!["OUTSIDE SELECTED LENSES", "  none failed or unavailable"]
            .into_iter()
            .map(str::to_string)
            .collect();
    }
    let mut output = vec!["OUTSIDE SELECTED LENSES".to_string()];
    output.extend(rows);
    output
}

#[derive(Clone)]
struct CaptureSite {
    operation: String,
    effect: Option<jet::Sema::Effect>,
}

/// Capture authority is checked against the parsed program before the child
/// producer starts. Safe capture can record only Time; sensitive capture may
/// authorize Rand/IO/Net later, but it still cannot model native, task, or
/// provider boundaries. This is deliberately conservative: an unknown call
/// shape fails closed instead of discovering an ambient effect after it ran.
fn preflight_capture_target(
    target: &Target,
    opts: &crate::ProveReplay::CaptureOpts,
    json_mode: bool,
) -> Result<(), i32> {
    let mut sites = Vec::new();
    for member in &target.members {
        let source = String::from_utf8_lossy(&member.bytes);
        let (tokens, lex_diagnostics) = jet::Lexer::lex(&source);
        if !lex_diagnostics.is_empty() {
            continue;
        }
        let Ok(program) = jet::Parser::parse(&tokens) else {
            continue;
        };
        let aliases = program
            .imports
            .iter()
            .filter_map(|import| {
                let module = import.core_module_path()?;
                let alias = import.import_alias();
                Some((alias, module))
            })
            .fold(HashMap::new(), |mut aliases, (alias, module)| {
                aliases.insert(alias.clone(), module.clone());
                if let Some(short) = alias.rsplit('.').next() {
                    aliases.entry(short.to_string()).or_insert(module);
                }
                aliases
            });
        collect_capture_items(&program.items, &aliases, &mut sites);
    }

    for site in sites {
        let Some(effect) = site.effect else {
            return capture_preflight_error(
                "E3625",
                &site.operation,
                "the reachable call shape is opaque to the replay authority",
                "route the operation through a supported deterministic capability, or remove it from the captured target",
                json_mode,
            );
        };
        match effect {
            jet::Sema::Effect::Time => {}
            jet::Sema::Effect::Rand | jet::Sema::Effect::IO | jet::Sema::Effect::Net
                if !opts.sensitive =>
            {
                return capture_preflight_error(
                    "E3627",
                    &site.operation,
                    &format!(
                        "safe capture reached raw {}; untainted values still require sensitive-data consent",
                        effect.name()
                    ),
                    "inject deterministic data, or explicitly use --capture-sensitive",
                    json_mode,
                );
            }
            jet::Sema::Effect::Rand | jet::Sema::Effect::IO | jet::Sema::Effect::Net => {}
            other => {
                return capture_preflight_error(
                    "E3625",
                    &site.operation,
                    &format!("replay capture has no authority adapter for the {} effect", other.name()),
                    "route the operation through a supported deterministic capability, or change the capture target",
                    json_mode,
                );
            }
        }
    }
    Ok(())
}

fn capture_preflight_error(
    code: &str,
    operation: &str,
    why: &str,
    fix: &str,
    json_mode: bool,
) -> Result<(), i32> {
    let what = if code == "E3627" {
        "replay capture refused sensitive data".to_string()
    } else {
        format!("replay capture cannot model {operation}")
    };
    crate::ProveReplay::emit_diag(code, &what, why, fix, json_mode);
    Err(jet::ExitCodes::USER_ERROR)
}

fn collect_capture_items(
    items: &[Item],
    aliases: &HashMap<String, String>,
    sites: &mut Vec<CaptureSite>,
) {
    for item in items {
        match item {
            Item::Func(func) => collect_capture_func(func, aliases, sites),
            Item::Impl(implementation) => {
                for func in &implementation.methods {
                    collect_capture_func(func, aliases, sites);
                }
            }
            Item::Struct(definition) => {
                for func in &definition.methods {
                    collect_capture_func(func, aliases, sites);
                }
            }
            Item::Enum(definition) => {
                for func in &definition.methods {
                    collect_capture_func(func, aliases, sites);
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_capture_items(body, aliases, sites);
                }
            }
            _ => {}
        }
    }
}

fn collect_capture_func(
    func: &Func,
    aliases: &HashMap<String, String>,
    sites: &mut Vec<CaptureSite>,
) {
    collect_capture_statements(&func.body, aliases, sites);
}

fn collect_capture_statements(
    statements: &[Stmt],
    aliases: &HashMap<String, String>,
    sites: &mut Vec<CaptureSite>,
) {
    for statement in statements {
        match statement {
            Stmt::Expr(expr) | Stmt::Yield(expr, _) => collect_capture_expr(expr, aliases, sites),
            Stmt::Val(binding) => collect_capture_expr(&binding.init, aliases, sites),
            Stmt::Assign { target, value, .. } => {
                collect_capture_lvalue(target, aliases, sites);
                collect_capture_expr(value, aliases, sites);
            }
            Stmt::Return(Some(expr), _)
            | Stmt::BreakValue(expr, _)
            | Stmt::BreakLabelValue(_, _, expr, _) => collect_capture_expr(expr, aliases, sites),
            Stmt::While { cond, body, .. } => {
                collect_capture_expr(cond, aliases, sites);
                collect_capture_statements(body, aliases, sites);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    jet::AST::ForKind::Range { start, end, step, .. } => {
                        collect_capture_expr(start, aliases, sites);
                        collect_capture_expr(end, aliases, sites);
                        if let Some(step) = step {
                            collect_capture_expr(step, aliases, sites);
                        }
                    }
                    jet::AST::ForKind::In { collection, step } => {
                        collect_capture_expr(collection, aliases, sites);
                        if let Some(step) = step {
                            collect_capture_expr(step, aliases, sites);
                        }
                    }
                }
                collect_capture_statements(body, aliases, sites);
            }
            Stmt::Switch { subject, arms, else_body, .. }
            | Stmt::ComptimeSwitch { subject, arms, else_body, .. } => {
                collect_capture_expr(subject, aliases, sites);
                for arm in arms {
                    collect_capture_expr(&arm.cond, aliases, sites);
                    collect_capture_statements(&arm.body, aliases, sites);
                }
                if let Some(body) = else_body {
                    collect_capture_statements(body, aliases, sites);
                }
            }
            Stmt::CountedLoop { init, cond, step, body, .. } => {
                collect_capture_expr(&init.init, aliases, sites);
                collect_capture_expr(cond, aliases, sites);
                if let Some(step) = step {
                    collect_capture_statements(std::slice::from_ref(step), aliases, sites);
                }
                collect_capture_statements(body, aliases, sites);
            }
            Stmt::Unsafe { body, .. } => {
                sites.push(CaptureSite {
                    operation: "#Unsafe raw boundary".to_string(),
                    effect: None,
                });
                collect_capture_statements(body, aliases, sites);
            }
            Stmt::TaskGroup { body, .. } | Stmt::Reactive { body, .. } => {
                sites.push(CaptureSite {
                    operation: "task/concurrency boundary".to_string(),
                    effect: None,
                });
                collect_capture_statements(body, aliases, sites);
            }
            Stmt::Loop { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Off { body, .. }
            | Stmt::DebugOnly { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Policy { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::Transact { body, .. } => collect_capture_statements(body, aliases, sites),
            Stmt::Return(None, _)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::BreakLabel(..)
            | Stmt::ContinueLabel(..) => {}
            _ => {}
        }
    }
}

fn collect_capture_lvalue(
    target: &jet::AST::LValue,
    aliases: &HashMap<String, String>,
    sites: &mut Vec<CaptureSite>,
) {
    match target {
        jet::AST::LValue::Local { .. } => {}
        jet::AST::LValue::Index { base, index, .. } => {
            collect_capture_expr(base, aliases, sites);
            collect_capture_expr(index, aliases, sites);
        }
        jet::AST::LValue::Field { base, .. } => collect_capture_expr(base, aliases, sites),
    }
}

fn collect_capture_expr(
    expr: &Expr,
    aliases: &HashMap<String, String>,
    sites: &mut Vec<CaptureSite>,
) {
    match expr {
        Expr::Call(call) => {
            if let Some(effect) = jet::Sema::builtin_effect(&call.name) {
                sites.push(CaptureSite {
                    operation: call.name.clone(),
                    effect: Some(effect),
                });
            } else if let Some((module, method)) = call.name.rsplit_once('.') {
                if let Some(effect) = jet::Sema::core_effect(module, method) {
                    sites.push(CaptureSite {
                        operation: call.name.clone(),
                        effect: Some(effect),
                    });
                }
            }
            for arg in &call.args {
                collect_capture_expr(&arg.expr, aliases, sites);
            }
        }
        Expr::MethodCall { receiver, method, args, .. } => {
            if let Some(path) = capture_receiver_path(receiver) {
                let mut parts = path.split('.');
                if let Some(alias) = parts.next() {
                    if let Some(module) = aliases.get(alias) {
                        let suffix = parts.collect::<Vec<_>>().join(".");
                        let module = if suffix.is_empty() {
                            module.clone()
                        } else {
                            format!("{module}.{suffix}")
                        };
                        if let Some(effect) = jet::Sema::core_effect(&module, method) {
                            sites.push(CaptureSite {
                                operation: format!("{module}.{method}"),
                                effect: Some(effect),
                            });
                        }
                    }
                }
            }
            collect_capture_expr(receiver, aliases, sites);
            for arg in args {
                collect_capture_expr(&arg.expr, aliases, sites);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            sites.push(CaptureSite {
                operation: "opaque callable".to_string(),
                effect: None,
            });
            collect_capture_expr(callee, aliases, sites);
            for arg in args {
                collect_capture_expr(&arg.expr, aliases, sites);
            }
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let jet::AST::StrPart::Interp(value, _) = part {
                    collect_capture_expr(value, aliases, sites);
                }
            }
        }
        Expr::ListLit(items, _) => {
            for item in items {
                collect_capture_expr(item, aliases, sites);
            }
        }
        Expr::TupleLit(items, _, _) => {
            for (_, item) in items {
                collect_capture_expr(item, aliases, sites);
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
        | Expr::Paren(base, _) => collect_capture_expr(base, aliases, sites),
        Expr::MapLit(entries, _) => {
            for (key, value) in entries {
                collect_capture_expr(key, aliases, sites);
                collect_capture_expr(value, aliases, sites);
            }
        }
        Expr::Index { base, index, .. } => {
            collect_capture_expr(base, aliases, sites);
            collect_capture_expr(index, aliases, sites);
        }
        Expr::Slice { base, start, end, range, .. } => {
            collect_capture_expr(base, aliases, sites);
            if let Some(range) = range {
                collect_capture_expr(range, aliases, sites);
            } else {
                collect_capture_expr(start, aliases, sites);
                collect_capture_expr(end, aliases, sites);
            }
        }
        Expr::Range { start, end, .. } => {
            collect_capture_expr(start, aliases, sites);
            collect_capture_expr(end, aliases, sites);
        }
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. } => {
            collect_capture_expr(inner, aliases, sites)
        }
        Expr::Binary(_, left, right, _) => {
            collect_capture_expr(left, aliases, sites);
            collect_capture_expr(right, aliases, sites);
        }
        Expr::CompareChain { operands, .. } => {
            for operand in operands {
                collect_capture_expr(operand, aliases, sites);
            }
        }
        Expr::OptField { base, .. } => collect_capture_expr(base, aliases, sites),
        Expr::StructLit { fields, .. } => {
            for (_, _, value) in fields {
                collect_capture_expr(value, aliases, sites);
            }
        }
        Expr::TypedLit { body, .. } => {
            body.for_each_expr(|value| collect_capture_expr(value, aliases, sites))
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    jet::AST::EnumLitArg::Positional(value)
                    | jet::AST::EnumLitArg::Named { expr: value, .. } => {
                        collect_capture_expr(value, aliases, sites)
                    }
                }
            }
        }
        Expr::Tainted(inner, _, _) | Expr::PatternTest { subject: inner, .. } => {
            collect_capture_expr(inner, aliases, sites)
        }
        Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
            collect_capture_expr(cond, aliases, sites);
            collect_capture_statements(then_body, aliases, sites);
            collect_capture_expr(then_value, aliases, sites);
            collect_capture_statements(else_body, aliases, sites);
            collect_capture_expr(else_value, aliases, sites);
        }
        Expr::Lambda(lambda) => match &lambda.body {
            jet::AST::LambdaBody::Expr(value) => collect_capture_expr(value, aliases, sites),
            jet::AST::LambdaBody::Block(body) => collect_capture_statements(body, aliases, sites),
        },
        Expr::OrFallback { value, fallback, .. } => {
            collect_capture_expr(value, aliases, sites);
            match fallback {
                jet::AST::OrFallback::Value(value)
                | jet::AST::OrFallback::Return(Some(value), _) => {
                    collect_capture_expr(value, aliases, sites)
                }
                jet::AST::OrFallback::Panic { args, .. } => {
                    for arg in args {
                        collect_capture_expr(&arg.expr, aliases, sites);
                    }
                }
                _ => {}
            }
        }
        Expr::PtrFromAddr { addr, .. } => {
            sites.push(CaptureSite {
                operation: "native pointer boundary".to_string(),
                effect: None,
            });
            collect_capture_expr(addr, aliases, sites);
        }
        _ => {}
    }
}

fn capture_receiver_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(base, field, _) => Some(format!("{}.{field}", capture_receiver_path(base)?)),
        _ => None,
    }
}

fn semantic_front_end_items(
    target: &Target,
    path: &str,
    source: &str,
) -> (Vec<FrontEndItem>, Vec<ContractDeclaration>) {
    let (tokens, lex_diagnostics) = jet::Lexer::lex(source);
    let program = if lex_diagnostics.is_empty() {
        jet::Parser::parse(&tokens).ok()
    } else {
        None
    };
    let Some(program) = program else {
        return (
            vec![generic_front_end_item(target, path, source)],
            Vec::new(),
        );
    };

    let mut facts = Vec::new();
    let mut declarations = Vec::new();
    collect_semantic_items(
        target,
        path,
        source,
        &program.items,
        &mut facts,
        &mut declarations,
    );
    if facts.is_empty() {
        facts.push(generic_front_end_item(target, path, source));
    }
    (facts, declarations)
}

fn generic_front_end_item(target: &Target, path: &str, source: &str) -> FrontEndItem {
    FrontEndItem {
        id: evidence_id(target, "front_end", path, "1:1-1:1", "program checked"),
        path: path.to_string(),
        diagnostic: None,
        source: source.to_string(),
        facet: "all",
        line: 1,
        column: 1,
    }
}

fn collect_semantic_items(
    target: &Target,
    path: &str,
    source: &str,
    items: &[Item],
    facts: &mut Vec<FrontEndItem>,
    declarations: &mut Vec<ContractDeclaration>,
) {
    for item in items {
        match item {
            Item::Func(func) => collect_func_semantics(target, path, source, func, facts, declarations),
            Item::Impl(implementation) => {
                for func in &implementation.methods {
                    collect_func_semantics(target, path, source, func, facts, declarations);
                }
            }
            Item::Struct(definition) => {
                for func in &definition.methods {
                    collect_func_semantics(target, path, source, func, facts, declarations);
                }
            }
            Item::Enum(definition) => {
                for func in &definition.methods {
                    collect_func_semantics(target, path, source, func, facts, declarations);
                }
            }
            Item::Distinct(definition) => {
                if let Some((lower, upper, span)) = definition.range {
                    push_front_end_fact(
                        target,
                        path,
                        source,
                        facts,
                        "refinements",
                        "refinement",
                        span,
                        format!("{} range [{lower}, {upper}]", definition.name),
                    );
                }
                if let Some((invariant, span)) = &definition.invariant {
                    push_front_end_fact(
                        target,
                        path,
                        source,
                        facts,
                        "refinements",
                        "refinement",
                        *span,
                        format!("{} invariant {invariant}", definition.name),
                    );
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_semantic_items(target, path, source, body, facts, declarations);
                }
            }
            _ => {}
        }
    }
}

fn collect_func_semantics(
    target: &Target,
    path: &str,
    source: &str,
    func: &Func,
    facts: &mut Vec<FrontEndItem>,
    declarations: &mut Vec<ContractDeclaration>,
) {
    for (marker, clauses) in [("Pre", &func.pre), ("Post", &func.post)] {
        for clause in clauses {
            let span = clause.cond.span();
            let (line, column) = span_line_col(source, span.start);
            let span_text = span_text(source, span);
            let claim = normalized_claim(source, span);
            declarations.push(ContractDeclaration {
                id: evidence_id(target, "contract", path, &span_text, &claim),
                path: path.to_string(),
                line,
                column,
                marker,
            });
        }
    }
    if let Some(effects) = &func.declared_effects {
        let span = effects.first().map(|(_, span)| *span).unwrap_or(func.span);
        let names = effects.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(",");
        push_front_end_fact(
            target,
            path,
            source,
            facts,
            "effects",
            "effect",
            span,
            format!("{} effect bound [{names}]", func.name),
        );
    }
    if let Some((callback, span)) = &func.effect_via {
        push_front_end_fact(
            target,
            path,
            source,
            facts,
            "effects",
            "effect",
            *span,
            format!("{} effects via {callback}", func.name),
        );
    }
    if let Some(tag) = &func.scrub_tag {
        push_front_end_fact(
            target,
            path,
            source,
            facts,
            "taint",
            "taint",
            func.span,
            format!("{} scrubs {tag}", func.name),
        );
    }
    if func.is_replayable {
        push_front_end_fact(
            target,
            path,
            source,
            facts,
            "replay",
            "replayability",
            func.replayable_span.unwrap_or(func.span),
            format!("{} is replayable", func.name),
        );
    }
}

fn push_front_end_fact(
    target: &Target,
    path: &str,
    source: &str,
    facts: &mut Vec<FrontEndItem>,
    facet: &'static str,
    id_kind: &str,
    span: jet::Diagnostics::Span,
    claim: String,
) {
    let span_text = span_text(source, span);
    let (line, column) = span_line_col(source, span.start);
    facts.push(FrontEndItem {
        id: evidence_id(target, id_kind, path, &span_text, &claim),
        path: path.to_string(),
        diagnostic: None,
        source: source.to_string(),
        facet,
        line,
        column,
    });
}

fn span_text(source: &str, span: jet::Diagnostics::Span) -> String {
    let (start_line, start_column) = span_line_col(source, span.start);
    let (end_line, end_column) = span_line_col(source, span.end);
    format!("{start_line}:{start_column}-{end_line}:{end_column}")
}

fn normalized_claim(source: &str, span: jet::Diagnostics::Span) -> String {
    source
        .get(span.start..span.end)
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn host_target_triple() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

const PROOF_LENSES: &[&str] = &[
    "all", "refinements", "effects", "taint", "contracts", "tests", "budgets", "replay",
    "solver",
];

fn validate_lens(value: &str, target: &str, json_mode: bool) {
    if PROOF_LENSES.contains(&value) {
        return;
    }
    let what = format!("unknown proof lens `{value}`");
    let why = "`jet prove` accepts all, refinements, effects, taint, contracts, tests, budgets, replay, solver";
    let fix = format!("try `jet prove {target} --lens tests`");
    if json_mode {
        println!(
            "{{\"schema_version\":1,\"code\":\"E2941\",\"severity\":\"error\",\"message\":{},\"why\":{},\"fix\":{},\"detail\":null,\"file\":null,\"line\":null,\"col\":null,\"span\":null,\"edit\":null}}",
            json(&what), json(why), json(&fix)
        );
    } else {
        eprintln!("Error [E2941]: {what}");
        eprintln!(" Why: {why}");
        eprintln!(" Fix: {fix}");
    }
    exit(ExitCodes::USAGE);
}

fn run_test_producers(target: &Target) -> (Vec<TestItem>, i32) {
    let mut items = Vec::new();
    let mut highest_exit = ExitCodes::OK;
    for member in &target.members {
        if !jet::has_test_blocks(&member.path) && jet::Doctest::discover(&String::from_utf8_lossy(&member.bytes)).is_empty() {
            continue;
        }
        let report_path = std::env::temp_dir().join(format!(
            "jet_prove_test_{}_{}.bin",
            std::process::id(),
            &member.sha256[..16]
        ));
        let _ = fs::remove_file(&report_path);
        let mut command = Command::new(std::env::current_exe().unwrap_or_else(|_| PathBuf::from("jet")));
        command
            .args(["test", &member.path, "--serial"])
            .env("JET_TEST_PROOF_REPORT", &report_path)
            .env("JET_PROVE_FRESH_TEST", "1");
        match supervise_child(&mut command, Duration::from_secs(120)) {
            ChildOutcome::Exited(Some(code)) => {
                if code == ExitCodes::ICE {
                    highest_exit = ExitCodes::ICE;
                } else if code == ExitCodes::RUNTIME_PANIC && highest_exit != ExitCodes::ICE {
                    highest_exit = ExitCodes::RUNTIME_PANIC;
                }
                match read_test_report(&report_path) {
                    Ok(records) => {
                        if records.iter().any(|record| record.kind == 2 || (record.kind == 1 && record.state == 1))
                            && highest_exit != ExitCodes::ICE
                        {
                            highest_exit = ExitCodes::RUNTIME_PANIC;
                        }
                        for record in records {
                            let claim = format!("{}:{}:{}", record.kind, record.name, record.message);
                            items.push(TestItem {
                                id: evidence_id(target, if record.kind == 1 { "contract" } else { "unit" }, &member.path, "0:0-0:0", &claim),
                                path: if record.kind == 3 || record.file.is_empty() { member.path.clone() } else { record.file.clone() },
                                state: record.state,
                                kind: record.kind,
                                message: record.message,
                                line: record.line,
                                name: record.name,
                                seed: if record.kind == 3 { record.file } else { String::new() },
                            });
                        }
                    }
                    Err(_) => highest_exit = ExitCodes::ICE,
                }
            }
            ChildOutcome::Exited(None) => {
                highest_exit = ExitCodes::ICE;
            }
            ChildOutcome::TimedOut | ChildOutcome::LaunchFailed => {
                items.push(TestItem {
                    id: evidence_id(target, "unit", &member.path, "0:0-0:0", "producer unavailable"),
                    path: member.path.clone(),
                    state: 3,
                    kind: 0,
                    message: String::new(),
                    line: 0,
                    name: String::new(),
                    seed: String::new(),
                });
            }
        }
        let _ = fs::remove_file(report_path);
    }
    (items, highest_exit)
}

fn supervise_child(command: &mut Command, deadline: Duration) -> ChildOutcome {
    // Keep the producer's stdout/stderr out of the user's proof stdout, but
    // drain both pipes concurrently so a noisy child cannot deadlock on a full
    // OS pipe. The privacy policy intentionally does not persist these
    // transcripts in ProofReport; they are bounded diagnostic material for the
    // supervising process only.
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return ChildOutcome::LaunchFailed,
    };
    let stdout = child.stdout.take().map(|mut stream| {
        std::thread::spawn(move || capture_child_stream(&mut stream))
    });
    let stderr = child.stderr.take().map(|mut stream| {
        std::thread::spawn(move || capture_child_stream(&mut stream))
    });
    let finish = |outcome| {
        if let Some(thread) = stdout {
            let _ = thread.join();
        }
        if let Some(thread) = stderr {
            let _ = thread.join();
        }
        outcome
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return finish(ChildOutcome::Exited(status.code())),
            Ok(None) if started.elapsed() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return finish(ChildOutcome::TimedOut);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return finish(ChildOutcome::Exited(None));
            }
        }
    }
}

fn capture_child_stream(stream: &mut impl std::io::Read) -> Vec<u8> {
    const MAX_TRANSCRIPT_BYTES: usize = 64 * 1024;
    let mut captured = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let remaining = MAX_TRANSCRIPT_BYTES.saturating_sub(captured.len());
                captured.extend_from_slice(&buffer[..count.min(remaining)]);
            }
            Err(_) => break,
        }
    }
    captured
}

fn read_test_report(path: &Path) -> Result<Vec<ProducerRecord>, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("oversized test producer report".into());
    }
    read_test_report_bytes(&bytes)
}

fn read_test_report_bytes(bytes: &[u8]) -> Result<Vec<ProducerRecord>, String> {
    if bytes.get(..8) != Some(b"JETTEST2") {
        return Err("invalid test producer report".into());
    }
    let mut at = 8usize;
    let mut records = Vec::new();
    while at < bytes.len() {
        if records.len() == 10_000 { return Err("too many test producer records".into()); }
        let kind = *bytes.get(at).ok_or("truncated test producer report")?;
        let state = *bytes.get(at + 1).ok_or("truncated test producer report")?;
        at += 2;
        let line = read_u64(&bytes, &mut at)? as u32;
        let name = read_string(&bytes, &mut at)?;
        let message = read_string(&bytes, &mut at)?;
        let file = read_string(&bytes, &mut at)?;
        records.push(ProducerRecord { kind, state, name, message, file, line });
    }
    Ok(records)
}

fn read_u64(bytes: &[u8], at: &mut usize) -> Result<u64, String> {
    let end = (*at)
        .checked_add(8)
        .ok_or("test producer report offset overflow")?;
    let raw: [u8; 8] = bytes
        .get(*at..end)
        .ok_or("truncated test producer report")?
        .try_into()
        .map_err(|_| "invalid test producer integer")?;
    *at = end;
    Ok(u64::from_be_bytes(raw))
}

fn read_string(bytes: &[u8], at: &mut usize) -> Result<String, String> {
    let len = usize::try_from(read_u64(bytes, at)?)
        .map_err(|_| "test producer field length is too large")?;
    if len > 1024 * 1024 {
        return Err("oversized test producer field".into());
    }
    let end = (*at)
        .checked_add(len)
        .ok_or("test producer report offset overflow")?;
    let raw = bytes
        .get(*at..end)
        .ok_or("truncated test producer report")?;
    *at = end;
    String::from_utf8(raw.to_vec()).map_err(|_| "non-UTF-8 test producer report".into())
}

#[cfg(test)]
mod protocol_tests {
    use super::read_test_report_bytes;

    #[test]
    fn hostile_protocol_shapes_fail_closed() {
        assert!(read_test_report_bytes(b"BADMAGIC").is_err());
        assert!(read_test_report_bytes(b"JETTEST2\0").is_err());

        let mut oversized = b"JETTEST2".to_vec();
        oversized.extend_from_slice(&[0, 0]);
        oversized.extend_from_slice(&0u64.to_be_bytes());
        oversized.extend_from_slice(&(1024u64 * 1024 + 1).to_be_bytes());
        assert!(read_test_report_bytes(&oversized).is_err());

        let mut trailing = b"JETTEST2".to_vec();
        trailing.push(0);
        assert!(read_test_report_bytes(&trailing).is_err());
    }
}

#[cfg(test)]
mod supervision_tests {
    use super::{supervise_child, ChildOutcome};
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Duration;

    #[test]
    #[ignore]
    fn child_helper() {
        match std::env::var("JET_PROVE_CHILD_MODE").as_deref() {
            Ok("exit70") => std::process::exit(70),
            Ok("crash") => std::process::abort(),
            Ok("timeout") => {
                std::thread::sleep(Duration::from_millis(300));
                if let Some(path) = std::env::var_os("JET_PROVE_CHILD_MARKER") {
                    fs::write(path, b"orphan survived").unwrap();
                }
            }
            _ => {}
        }
    }

    fn helper(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "CmdProve::supervision_tests::child_helper",
                "--ignored",
            ])
            .env("JET_PROVE_CHILD_MODE", mode);
        command
    }

    #[test]
    fn classifies_exit_crash_timeout_and_unavailable_without_orphan() {
        assert!(matches!(
            supervise_child(&mut helper("exit70"), Duration::from_secs(2)),
            ChildOutcome::Exited(Some(70))
        ));

        assert!(matches!(
            supervise_child(&mut helper("crash"), Duration::from_secs(2)),
            ChildOutcome::Exited(None) | ChildOutcome::Exited(Some(_))
        ));

        let marker = PathBuf::from(format!(
            "/tmp/jet-prove-supervision-{}-marker",
            std::process::id()
        ));
        let _ = fs::remove_file(&marker);
        let mut timeout = helper("timeout");
        timeout.env("JET_PROVE_CHILD_MARKER", &marker);
        assert!(matches!(
            supervise_child(&mut timeout, Duration::from_millis(20)),
            ChildOutcome::TimedOut
        ));
        std::thread::sleep(Duration::from_millis(350));
        assert!(!marker.exists(), "timed-out child survived kill+wait");

        let mut missing = Command::new(format!(
            "/tmp/jet-prove-missing-executable-{}",
            std::process::id()
        ));
        assert!(matches!(
            supervise_child(&mut missing, Duration::from_millis(20)),
            ChildOutcome::LaunchFailed
        ));
    }
}

fn resolve_target(raw: &str) -> Result<Target, String> {
    let path = Path::new(raw);
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("can't find proof target `{raw}`")
        } else {
            format!("can't inspect proof target `{raw}`: {error}")
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!("proof target `{raw}` must not be a symlink"));
    }
    let (kind, mut paths) = if metadata.is_file() {
        if path.extension().and_then(|ext| ext.to_str()) != Some(jet::Syntax::FILE_EXT) {
            return Err(format!("proof target `{raw}` is not a .jet file"));
        }
        ("file", vec![path.to_path_buf()])
    } else {
        if !metadata.is_dir() {
            return Err(format!("proof target `{raw}` is not a file or directory"));
        }
        let kind = if has_proof_manifest(path, jet::Syntax::PACKAGE_FILE)?
            || has_proof_manifest(path, jet::Syntax::PAYLOAD_FILE)?
        {
            "package"
        } else {
            "workspace"
        };
        let mut found = Vec::new();
        collect_jet_files(path, &mut found)?;
        if found.is_empty() {
            return Err(format!("proof target `{raw}` contains no .jet files"));
        }
        (kind, found)
    };
    paths.sort_by(|a, b| normalized(a).as_bytes().cmp(normalized(b).as_bytes()));
    let mut members = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(&path).map_err(|e| format!("couldn't read `{}`: {e}", path.display()))?;
        members.push(Member {
            path: normalized(&path),
            sha256: jet::SHA256::sha256_hex(&bytes),
            bytes,
        });
    }
    let mut identity_members = members
        .iter()
        .map(|member| (member.path.clone(), member.sha256.clone()))
        .collect::<Vec<_>>();
    if metadata.is_dir() {
        let mut closure_paths = Vec::new();
        collect_identity_files(path, &mut closure_paths)?;
        for closure_path in closure_paths {
            let bytes = fs::read(&closure_path)
                .map_err(|e| format!("couldn't read `{}`: {e}", closure_path.display()))?;
            let closure_path = normalized(&closure_path);
            if !identity_members.iter().any(|(member_path, _)| member_path == &closure_path) {
                identity_members.push((closure_path, jet::SHA256::sha256_hex(&bytes)));
            }
        }
    }
    identity_members.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let mut identity = Vec::new();
    for (path, sha256) in &identity_members {
        identity.extend_from_slice(
            format!("{{\"path\":{},\"sha256\":{}}}\n", json(path), json(sha256)).as_bytes(),
        );
    }
    Ok(Target {
        kind,
        root: normalized(path),
        identity_members,
        input_sha256: jet::SHA256::sha256_hex(&identity),
        members,
    })
}

fn has_proof_manifest(dir: &Path, name: &str) -> Result<bool, String> {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("proof manifest `{}` is a symlink", path.display()))
        }
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("couldn't inspect proof manifest `{}`: {error}", path.display())),
    }
}

fn collect_jet_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("couldn't read `{}`: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("couldn't inspect `{}`: {e}", dir.display()))?.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| format!("couldn't inspect `{}`: {e}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("proof target contains symlink `{}`", path.display()));
        }
        if metadata.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "build" || name.starts_with('.') {
                continue;
            }
            collect_jet_files(&path, out)?;
        } else if metadata.is_file()
            && path.extension().and_then(|ext| ext.to_str()) == Some(jet::Syntax::FILE_EXT)
        {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
            if !matches!(name, "package.jet" | "pkg.jet" | "build.jet") {
                out.push(path);
            }
        }
    }
    Ok(())
}

fn collect_identity_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("couldn't read `{}`: {e}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("couldn't inspect `{}`: {e}", dir.display()))?
            .path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| format!("couldn't inspect `{}`: {e}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("proof target contains symlink `{}`", path.display()));
        }
        if metadata.is_dir() {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
            if name == "build" || name.starts_with('.') {
                continue;
            }
            collect_identity_files(&path, out)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if matches!(
            name,
            "package.jet" | "pkg.jet" | "jet.lock" | "jet.lock.json" | "build.jet"
        ) {
            out.push(path);
        }
    }
    Ok(())
}

fn normalized(path: &Path) -> String {
    let path = if path.is_absolute() {
        path.strip_prefix(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
            .unwrap_or(path)
    } else {
        path
    };
    path.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn budget_projection(target: &Target) -> jet::BudgetView::BudgetProjection {
    let target_path = Path::new(&target.root);
    let search = if target_path.is_file() { target_path.parent().unwrap_or(Path::new(".")) } else { target_path };
    let root = jet::Loader::find_manifest_root(search).unwrap_or_else(|| search.to_path_buf());
    let sources = target.members.iter().map(|member| {
        let path = Path::new(&member.path);
        let path = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        (path.trim_start_matches("./").to_string(), member.sha256.clone())
    }).collect::<Vec<_>>();
    jet::BudgetView::read_compatible(&root, &sources)
}

fn diagnostic_span(source: &str, diagnostic: &Diagnostic) -> String {
    let Some(span) = diagnostic.span else { return "0:0-0:0".into() };
    let (sl, sc) = span_line_col(source, span.start);
    let (el, ec) = span_line_col(source, span.end);
    format!("{sl}:{sc}-{el}:{ec}")
}

fn evidence_id(target: &Target, kind: &str, origin: &str, span: &str, claim: &str) -> String {
    let claim_sha = jet::SHA256::sha256_hex(claim.as_bytes());
    let mut preimage = Vec::new();
    for field in [&target.input_sha256, kind, origin, span, &claim_sha] {
        preimage.extend_from_slice(&(field.len() as u64).to_be_bytes());
        preimage.extend_from_slice(field.as_bytes());
    }
    jet::SHA256::sha256_hex(&preimage)
}

fn render_report(
    target: &Target,
    items: &[FrontEndItem],
    tests: &[TestItem],
    budgets: &jet::BudgetView::BudgetProjection,
    solver: &[crate::ProveSolver::SolverEvidence],
    _lenses: &[String],
    declarations: &[ContractDeclaration],
    replay_time_ms: Option<i64>,
    proved: usize,
    failed: usize,
    exit_code: i32,
) -> String {
    let members = target
        .identity_members
        .iter()
        .map(|(path, sha256)| format!("{{\"path\":{},\"sha256\":{}}}", json(path), json(sha256)))
        .collect::<Vec<_>>()
        .join(",");
    let mut diagnostics = items.iter().filter_map(|item| item.diagnostic.as_ref().map(|d| diagnostic_json(&item.path, &item.source, d))).collect::<Vec<_>>();
    let mut diagnostic_index = 0usize;
    let mut evidence_rows = items.iter().map(|item| {
        let (outcome, indexes) = if item.diagnostic.is_some() { let i = diagnostic_index; diagnostic_index += 1; ("failed", format!("[{i}]")) } else { ("proved", "[]".into()) };
        format!("{{\"attachment\":null,\"budget\":null,\"contract\":null,\"count\":1,\"diagnosticIndexes\":{indexes},\"facet\":\"{}\",\"id\":{},\"kind\":\"front_end\",\"outcome\":\"{outcome}\",\"producer\":\"jet-sema\",\"property\":null,\"reason\":null,\"solver\":null,\"source\":{{\"column\":{},\"line\":{},\"path\":{}}},\"state\":\"checked\"}}", item.facet, json(&item.id), item.column, item.line, json(&item.path))
    }).collect::<Vec<_>>();

    let mut used_contract_records = BTreeSet::new();
    for declaration in declarations {
        let matching = tests.iter().enumerate().find(|(index, item)| {
            item.kind == 1
                && !used_contract_records.contains(index)
                && item.name == declaration.marker
                && item.line as usize == declaration.line
                && source_paths_match(&item.path, &declaration.path)
        });
        let (id, attachment, state, outcome, reason, observation, diagnostic_indexes) =
            if let Some((index, item)) = matching {
                used_contract_records.insert(index);
                let (state, outcome, reason, observation) = match item.state {
                    0 => ("executed", "passed", "null", "reached_pass"),
                    1 => ("executed", "failed", "null", "reached_fail"),
                    2 => ("skipped", "not_run", "\"fail_fast_policy\"", "not_reached"),
                    _ => ("unavailable", "unavailable", "\"producer_start_failed\"", "not_reached"),
                };
                let diagnostic_indexes = if item.state == 1 {
                    let index = diagnostics.len();
                    diagnostics.push(runtime_contract_diagnostic(&declaration.path, item));
                    format!("[{index}]")
                } else {
                    "[]".to_string()
                };
                (
                    declaration.id.clone(),
                    if item.state == 0 || item.state == 1 {
                        format!("{{\"testEvidenceId\":{}}}", json(&item.id))
                    } else {
                        "null".to_string()
                    },
                    state,
                    outcome,
                    reason,
                    observation,
                    diagnostic_indexes,
                )
            } else {
                (
                    declaration.id.clone(),
                    "null".to_string(),
                    "declared",
                    "not_observed",
                    "null",
                    "not_reached",
                    "[]".to_string(),
                )
            };
        let contract = format!(
            "{{\"marker\":{},\"observation\":\"{observation}\",\"site\":{{\"column\":{},\"line\":{},\"path\":{}}}}}",
            json(declaration.marker),
            declaration.column,
            declaration.line,
            json(&declaration.path)
        );
        evidence_rows.push(format!(
            "{{\"attachment\":{attachment},\"budget\":null,\"contract\":{contract},\"count\":1,\"diagnosticIndexes\":{diagnostic_indexes},\"facet\":\"contracts\",\"id\":{},\"kind\":\"contract\",\"outcome\":\"{outcome}\",\"producer\":\"jet-runtime\",\"property\":null,\"reason\":{reason},\"solver\":null,\"source\":{{\"column\":{},\"line\":{},\"path\":{}}},\"state\":\"{state}\"}}",
            json(&id),
            declaration.column,
            declaration.line,
            json(&declaration.path)
        ));
    }
    for item in tests {
        if item.kind == 1 {
            continue;
        }
        if item.kind == 2 {
            // Runtime panics are diagnostics, not a fifth evidence kind. The
            // existing producer record still controls exit precedence.
            let diagnostic_index = diagnostics.len();
            diagnostics.push(runtime_item_diagnostic(item));
            let _ = diagnostic_index;
            continue;
        }
        let (kind, facet, producer) = match item.kind {
            3 => ("property", "tests", "jet-property"),
            4 => ("doctest", "tests", "jet-doctest"),
            _ => ("unit", "tests", "jet-test"),
        };
        let (state, outcome, reason) = match item.state { 0 => ("executed", "passed", "null"), 1 => ("executed", "failed", "null"), 2 => ("skipped", "not_run", "\"fail_fast_policy\""), _ => ("unavailable", "unavailable", "\"producer_start_failed\"") };
        let diagnostic_indexes = if item.state == 1 {
            let index = diagnostics.len();
            diagnostics.push(runtime_item_diagnostic(item));
            format!("[{index}]")
        } else { "[]".into() };
        let property = if item.kind == 3 { format!("{{\"caseIndex\":{},\"effectiveSeed\":{},\"generatedCases\":{},\"shrinkTrace\":{},\"source\":{{\"column\":1,\"line\":1,\"path\":{}}},\"toolchain\":{{\"jet\":{},\"targetTriple\":{}}}}}", item.line.saturating_sub(1), item.seed.parse::<u64>().unwrap_or(0), item.line, if item.message.is_empty() { "[]".into() } else { format!("[{{\"name\":\"minimized_inputs\",\"value\":{}}}]", json(&item.message)) }, json(&item.path), json(env!("CARGO_PKG_VERSION")), json(&host_target_triple())) } else { "null".into() };
        evidence_rows.push(format!("{{\"attachment\":null,\"budget\":null,\"contract\":null,\"count\":1,\"diagnosticIndexes\":{diagnostic_indexes},\"facet\":\"{facet}\",\"id\":{},\"kind\":\"{kind}\",\"outcome\":\"{outcome}\",\"producer\":\"{producer}\",\"property\":{property},\"reason\":{reason},\"solver\":null,\"source\":{{\"column\":1,\"line\":{},\"path\":{}}},\"state\":\"{state}\"}}", json(&item.id), item.line.max(1), json(&item.path)));
    }
    for fact in &budgets.facts {
        let kind = if fact.statistical { "statistical_budget" } else { "deterministic_budget" };
        let facet = "budgets";
        let outcome = match fact.outcome.as_str() { "pass" => "met", "warn" => "warning", _ => "failed" };
        let budget = format!("{{\"budgetId\":{},\"enforcement\":{},\"evidenceId\":{},\"reportId\":{},\"statistical\":{}}}", json(&fact.budget_id), json(&fact.enforcement), json(&fact.evidence_id), json(&fact.report_id), fact.statistical);
        evidence_rows.push(format!("{{\"attachment\":null,\"budget\":{budget},\"contract\":null,\"count\":1,\"diagnosticIndexes\":[],\"facet\":\"{facet}\",\"id\":{},\"kind\":\"{kind}\",\"outcome\":\"{outcome}\",\"producer\":\"jet-budget\",\"property\":null,\"reason\":null,\"solver\":null,\"source\":{{\"column\":1,\"line\":1,\"path\":{}}},\"state\":\"checked\"}}", json(&fact.evidence_id), json(&target.root)));
    }
    for item in solver {
        let diagnostic_indexes = if let crate::ProveSolver::SolverOutcome::Disproved {
            assignment,
            ..
        } = &item.outcome
        {
            let index = diagnostics.len();
            let values = assignment
                .iter()
                .map(|(name, value)| format!("{name} = {value}"))
                .collect::<Vec<_>>()
                .join(", ");
            diagnostics.push(format!(
                "{{\"caret\":null,\"code\":\"E2950\",\"context\":[],\"frames\":[],\"message\":{},\"notes\":[{},{}],\"origin\":{{\"producer\":\"native-presburger\",\"stage\":\"solver\"}},\"safeLocals\":[],\"severity\":\"error\",\"span\":{{\"endColumn\":1,\"endLine\":1,\"path\":{},\"sourceLine\":null,\"startColumn\":1,\"startLine\":1}},\"type\":\"producer\"}}",
                json(&format!(
                    "solver found a counterexample to {}",
                    item.obligation.kind
                )),
                json(&format!(
                    "Why: these values satisfy every assumption but make the claim false: {values}"
                )),
                json(&format!(
                    "Fix: change {} so every admitted input satisfies the claim, or correct the contract",
                    item.obligation.origin
                )),
                json(&item.obligation.origin),
            ));
            format!("[{index}]")
        } else {
            "[]".to_string()
        };
        evidence_rows.push(crate::ProveSolver::evidence_json(item, &diagnostic_indexes));
    }
    if let Some(time_ms) = replay_time_ms {
        let claim = format!("replayed Time authority at {time_ms} ms");
        let id = evidence_id(target, "replay", &target.root, "1:1-1:1", &claim);
        evidence_rows.push(format!(
            "{{\"attachment\":null,\"budget\":null,\"contract\":null,\"count\":1,\"diagnosticIndexes\":[],\"facet\":\"replay\",\"id\":{},\"kind\":\"front_end\",\"outcome\":\"observed\",\"producer\":\"jet-replay\",\"property\":null,\"reason\":null,\"solver\":null,\"source\":{{\"column\":1,\"line\":1,\"path\":{}}},\"state\":\"executed\"}}",
            json(&id),
            json(&target.root)
        ));
    }
    let evidence = evidence_rows.join(",");
    let (solver_selected, solver_proved, solver_disproved, solver_unknown, solver_unavailable) =
        crate::ProveSolver::summarize(solver);
    let unit_passed = tests.iter().filter(|item| item.kind == 0 && item.state == 0).count();
    let unit_failed = tests.iter().filter(|item| item.kind == 0 && item.state == 1).count();
    let unit_skipped = tests.iter().filter(|item| item.kind == 0 && item.state >= 2).count();
    let unit_unavailable = tests.iter().filter(|item| item.kind == 0 && item.state == 3).count();
    let contract_selected = declarations.len();
    let (contract_passed, contract_failed, contract_not_observed, contract_skipped) =
        contract_summary(declarations, tests);
    let contract_observed = contract_passed + contract_failed;
    let property_passed = tests.iter().filter(|item| item.kind == 3 && item.state == 0).count();
    let property_failed = tests.iter().filter(|item| item.kind == 3 && item.state == 1).count();
    let property_cases: u32 = tests.iter().filter(|item| item.kind == 3).map(|item| item.line).sum();
    let property_shrunk_failures = tests
        .iter()
        .filter(|item| item.kind == 3 && item.state == 1 && !item.message.is_empty())
        .count();
    let doctest_passed = tests.iter().filter(|item| item.kind == 4 && item.state == 0).count();
    let doctest_failed = tests.iter().filter(|item| item.kind == 4 && item.state == 1).count();
    let doctest_skipped = tests.iter().filter(|item| item.kind == 4 && item.state >= 2).count();
    let doctest_selected = doctest_passed + doctest_failed + doctest_skipped;
    let unit_selected = unit_passed + unit_failed + unit_skipped;
    let deterministic_selected = budgets.facts.iter().filter(|fact| !fact.statistical).count();
    let deterministic_failed = budgets.facts.iter().filter(|fact| !fact.statistical && fact.outcome == "fail").count();
    let deterministic_met = budgets.facts.iter().filter(|fact| !fact.statistical && fact.outcome == "pass").count();
    let deterministic_unavailable = budgets.facts.iter().filter(|fact| !fact.statistical && fact.evidence == "unavailable").count();
    let statistical_selected = budgets.facts.iter().filter(|fact| fact.statistical).count();
    let statistical_failed = budgets.facts.iter().filter(|fact| fact.statistical && fact.outcome == "fail").count();
    let statistical_met = budgets.facts.iter().filter(|fact| fact.statistical && fact.outcome == "pass").count();
    let statistical_unavailable = budgets.facts.iter().filter(|fact| fact.statistical && fact.evidence == "unavailable").count();
    let front_end_selected = proved + failed;
    let property_skipped = tests.iter().filter(|item| item.kind == 3 && item.state >= 2).count();
    let property_selected = property_passed + property_failed + property_skipped;
    let result = if exit_code != ExitCodes::OK {
        "fail"
    } else if unit_unavailable > 0
        || deterministic_unavailable > 0
        || statistical_unavailable > 0
        || solver_unknown > 0
    {
        "pass_incomplete"
    } else {
        "pass"
    };
    format!("{{\"diagnostics\":[{}],\"evidence\":[{evidence}],\"evidencePolicy\":\"allow_incomplete\",\"exitCode\":{exit_code},\"result\":\"{result}\",\"schemaVersion\":1,\"summaries\":{{\"contract\":{{\"declared\":{contract_selected},\"failed\":{contract_failed},\"notObserved\":{contract_not_observed},\"observed\":{contract_observed},\"passed\":{contract_passed},\"selected\":{contract_selected},\"skipped\":{contract_skipped}}},\"deterministicBudget\":{{\"failed\":{deterministic_failed},\"met\":{deterministic_met},\"selected\":{deterministic_selected},\"skipped\":0,\"unavailable\":{deterministic_unavailable}}},\"doctest\":{{\"failed\":{doctest_failed},\"passed\":{doctest_passed},\"selected\":{doctest_selected},\"skipped\":{doctest_skipped}}},\"frontEnd\":{{\"failed\":{failed},\"proved\":{proved},\"selected\":{front_end_selected},\"skipped\":0}},\"property\":{{\"failed\":{property_failed},\"generatedCases\":{property_cases},\"passed\":{property_passed},\"selected\":{property_selected},\"shrunkFailures\":{property_shrunk_failures},\"skipped\":{property_skipped}}},\"solver\":{{\"disproved\":{solver_disproved},\"proved\":{solver_proved},\"selected\":{solver_selected},\"unavailable\":{solver_unavailable},\"unknown\":{solver_unknown}}},\"statisticalBudget\":{{\"failed\":{statistical_failed},\"met\":{statistical_met},\"selected\":{statistical_selected},\"skipped\":0,\"unavailable\":{statistical_unavailable}}},\"unit\":{{\"failed\":{unit_failed},\"passed\":{unit_passed},\"selected\":{unit_selected},\"skipped\":{unit_skipped}}}}},\"target\":{{\"inputSha256\":{},\"kind\":\"{}\",\"members\":[{members}],\"root\":{}}},\"tool\":{{\"jet\":{},\"proofProducer\":\"jet-prove\",\"targetTriple\":{}}}}}", diagnostics.join(","), json(&target.input_sha256), target.kind, json(&target.root), json(env!("CARGO_PKG_VERSION")), json(&host_target_triple()))
}

fn contract_summary(
    declarations: &[ContractDeclaration],
    tests: &[TestItem],
) -> (usize, usize, usize, usize) {
    let mut used = BTreeSet::new();
    let mut passed = 0;
    let mut failed = 0;
    let mut not_observed = 0;
    let mut skipped = 0;
    for declaration in declarations {
        let matching = tests.iter().enumerate().find(|(index, item)| {
            item.kind == 1
                && !used.contains(index)
                && item.name == declaration.marker
                && item.line as usize == declaration.line
                && source_paths_match(&item.path, &declaration.path)
        });
        let Some((index, item)) = matching else {
            not_observed += 1;
            continue;
        };
        used.insert(index);
        match item.state {
            0 => passed += 1,
            1 => failed += 1,
            _ => skipped += 1,
        }
    }
    (passed, failed, not_observed, skipped)
}

fn diagnostic_json(path: &str, source: &str, d: &Diagnostic) -> String {
    let severity = if matches!(d.severity, Severity::Error) { "error" } else { "warning" };
    let span = d.span.map(|s| { let (sl, sc) = span_line_col(source, s.start); let (el, ec) = span_line_col(source, s.end); format!("{{\"endColumn\":{ec},\"endLine\":{el},\"path\":{},\"sourceLine\":null,\"startColumn\":{sc},\"startLine\":{sl}}}", json(path)) }).unwrap_or_else(|| "null".into());
    format!("{{\"caret\":null,\"code\":{},\"context\":[],\"frames\":[],\"message\":{},\"notes\":[{},{}],\"origin\":{{\"producer\":\"jet-sema\",\"stage\":\"front_end\"}},\"safeLocals\":[],\"severity\":\"{severity}\",\"span\":{span},\"type\":\"front_end\"}}", json(&d.code), json(&d.what), json(&format!("Why: {}", d.why)), json(&format!("Fix: {}", d.fix)))
}

fn source_paths_match(left: &str, right: &str) -> bool {
    let normalize = |path: &str| path.trim_start_matches("./").replace('\\', "/");
    let left = normalize(left);
    let right = normalize(right);
    left == right || left.ends_with(&format!("/{right}")) || right.ends_with(&format!("/{left}"))
}

fn runtime_contract_diagnostic(path: &str, item: &TestItem) -> String {
    let message = format!("#{} contract failed: {}", item.name, item.message);
    let why = "Why: A `#Pre` (argument claim, checked at entry) or `#Post` (`result` claim, checked before return) condition evaluated false at runtime. The clause's own message string is included. Checked in every build (not a debug/release split).";
    let fix = "Fix: Fix the caller (a failed `#Pre` means an argument violated the function's stated contract) or the function body (a failed `#Post` means it broke its own promise about the result).";
    let source_line = source_line_for(path, item.line);
    let source_line_json = source_line.as_deref().map(json).unwrap_or_else(|| "null".to_string());
    let width = source_line.as_deref().map_or(1, |line| line.chars().count().max(1));
    format!(
        "{{\"caret\":{{\"startColumn\":1,\"width\":{width}}},\"code\":\"E3005\",\"context\":[],\"frames\":[],\"message\":{},\"notes\":[{},{}],\"origin\":{{\"producer\":\"jet-runtime\",\"stage\":\"runtime\"}},\"safeLocals\":[],\"severity\":\"error\",\"span\":{{\"endColumn\":{},\"endLine\":{},\"path\":{},\"sourceLine\":{source_line_json},\"startColumn\":1,\"startLine\":{}}},\"type\":\"runtime\"}}",
        json(&message),
        json(why),
        json(fix),
        width + 1,
        item.line,
        json(path),
        item.line
    )
}

fn source_line_for(path: &str, line: u32) -> Option<String> {
    let line = usize::try_from(line).ok()?.checked_sub(1)?;
    fs::read_to_string(path)
        .ok()?
        .lines()
        .nth(line)
        .map(str::to_string)
}

fn runtime_item_diagnostic(item: &TestItem) -> String {
    let code = if item.kind == 1 { "E3005" } else { "E3001" };
    let source_line = source_line_for(&item.path, item.line);
    let source_line_json = source_line.as_deref().map(json).unwrap_or_else(|| "null".to_string());
    let width = source_line.as_deref().map_or(1, |line| line.chars().count().max(1));
    let (why, fix) = if item.kind == 2 {
        (
            "the proof child terminated with a panic instead of producing a checked evidence record",
            "inspect the captured runtime message and fix the panic before relying on this proof",
        )
    } else {
        (
            "the runtime producer reported a failed checked property or test",
            "fix the reported property or test, then rerun `jet prove`",
        )
    };
    format!(
        "{{\"caret\":{{\"startColumn\":1,\"width\":{width}}},\"code\":{},\"context\":[],\"frames\":[],\"message\":{},\"notes\":[{},{}],\"origin\":{{\"producer\":\"jet-runtime\",\"stage\":\"runtime\"}},\"safeLocals\":[],\"severity\":\"error\",\"span\":{{\"endColumn\":{},\"endLine\":{},\"path\":{},\"sourceLine\":{source_line_json},\"startColumn\":1,\"startLine\":{}}},\"type\":\"runtime\"}}",
        json(code),
        json(&item.message),
        json(&format!("Why: {why}")),
        json(&format!("Fix: {fix}")),
        width + 1,
        item.line,
        json(&item.path),
        item.line
    )
}

fn json(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch { '\\' => out.push_str("\\\\"), '"' => out.push_str("\\\""), '\n' => out.push_str("\\n"), '\r' => out.push_str("\\r"), '\t' => out.push_str("\\t"), c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)), c => out.push(c) }
    }
    out.push('"');
    out
}

/// D-JPROOF1=A: write `.jet/proofs/<kind>/<name>/<first-16-report_id>.jetproof`.
/// Identical existing bytes are left unchanged; differing bytes refuse.
fn write_jetproof(target: &Target, proof_report: &str) -> Result<(), String> {
    let report_bytes = format!("{proof_report}\n");
    let report_id = jet::SHA256::sha256_hex(report_bytes.as_bytes());
    let kind = target.kind;
    let name = {
        let root = Path::new(&target.root);
        let stem = root
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(kind)
            .replace('.', "_");
        if stem.is_empty() { kind.to_string() } else { stem }
    };
    let rel = format!(
        ".jet/proofs/{kind}/{name}/{}.jetproof",
        &report_id[..16.min(report_id.len())]
    );
    let path = PathBuf::from(&rel);
    let envelope = format!(
        "{{\"artifact\":{{\"path\":{}}},\"privacy\":{{\"absolute_paths\":\"omitted\",\"argv\":\"omitted\",\"environment\":\"omitted\",\"full_source\":\"omitted\",\"producer_transcripts\":\"omitted\",\"safe_locals\":\"redacted_by_D-OBS2\"}},\"proofReport\":{proof_report},\"report_id\":{},\"schema\":\"jet.jproof\",\"version\":1}}\n",
        json(&rel),
        json(&report_id)
    );
    ensure_jetproof_parent(&path)?;
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if !metadata.file_type().is_file() {
            return Err(format!("final .jetproof path is not a regular file: {rel}"));
        }
        let existing = fs::read(&path).map_err(|e| e.to_string())?;
        if existing == envelope.as_bytes() {
            return Ok(());
        }
        return Err(format!(
            "refusing to overwrite differing .jetproof at {rel}"
        ));
    }
    let tmp = path.with_extension(format!(
        "jetproof.tmp.{}.{}",
        std::process::id(),
        &report_id[..8]
    ));
    let write_result = (|| -> Result<(), String> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        use std::io::Write;
        file.write_all(envelope.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::hard_link(&tmp, &path).map_err(|e| e.to_string())?;
        fs::remove_file(&tmp).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            fs::File::open(parent)
                .map_err(|e| e.to_string())?
                .sync_all()
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}

fn ensure_jetproof_parent(path: &Path) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut current = PathBuf::from(".");
    for component in parent.components() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(".jetproof parent is a symlink: {}", current.display()));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!(".jetproof parent is not a directory: {}", current.display()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|e| e.to_string())?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&current, fs::Permissions::from_mode(0o700))
                        .map_err(|e| e.to_string())?;
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod target_tests {
    use super::resolve_target;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn canonical_package_marker_selects_package_and_enters_identity() {
        let root = PathBuf::from(format!(
            "/tmp/jet-prove-target-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("package.jet"), "name: \"demo\"\n").unwrap();
        fs::write(root.join("main.jet"), "fn run() {}\n").unwrap();

        let target = resolve_target(&root.to_string_lossy()).unwrap();
        assert_eq!(target.kind, "package");
        assert!(target
            .identity_members
            .iter()
            .any(|(path, _)| path.ends_with("/package.jet")));

        fs::remove_dir_all(root).unwrap();
    }
}
