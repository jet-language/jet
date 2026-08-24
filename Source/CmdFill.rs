//! Typed-goal reports and checked candidate proposals for jet check / jet fill.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use jet::Diagnostics::Span;
use jet::AST::{Expr, Func, Item, ProgramBundle};
use jet_foundation::Report::render_status_json;
use jet_foundation::JSON::json_escape;

use crate::{CmdInspect::CheckProjection, OutputMode};

#[derive(Clone)]
struct FunctionInfo {
    name: String,
    return_type: String,
}

struct GoalSite {
    file: String,
    source: String,
    line: usize,
    function: String,
    span: Span,
    expected_type: String,
    required_effects: String,
    params: Vec<(String, String)>,
    functions: Vec<FunctionInfo>,
}

/// Run the checked-candidate command. An optional :line suffix narrows the
/// report to one goal site; the checker still validates the whole file first.
pub(crate) fn run_fill(target: &str, mode: OutputMode) {
    let (file, line) = split_target(target);
    let path = Path::new(file);
    let mut checked =
        match crate::CmdInspect::check_projection_for_effects(path, "dev", &BTreeMap::new()) {
            Ok(checked) => checked,
            Err(diagnostics) => {
                crate::CmdInspect::render_check_failure(
                    path,
                    &diagnostics,
                    mode.json,
                    mode.color_stderr(),
                );
            }
        };

    let report = render_goal_report(&mut checked, Some(path), line, true, mode.json);
    match report {
        Some(report) => println!("{report}"),
        None if mode.json => println!(
            "{}",
            render_status_json("ok", true, "fill", ",\"fill\":{\"goals\":[]}")
        ),
        None => println!("no goals found in '{file}'"),
    }
}

/// Render the checked goal facts already present in a projection. fill asks
/// for candidates; check asks only for the typed goal card.
pub(crate) fn render_goal_report(
    checked: &mut CheckProjection,
    file_filter: Option<&Path>,
    line_filter: Option<usize>,
    include_candidates: bool,
    json: bool,
) -> Option<String> {
    let mut goals = collect_goals(&mut checked.bundle, &checked.facts);
    if let Some(path) = file_filter {
        goals.retain(|goal| same_file(&goal.file, path));
    }
    if let Some(line) = line_filter {
        goals.retain(|goal| goal.line == line);
    }
    if goals.is_empty() {
        return None;
    }

    let mut out = String::new();
    if json {
        out.push_str("{\"goals\":[");
        for (index, goal) in goals.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let candidates = checked_candidates(goal, include_candidates);
            write!(
                out,
                "{{\"file\":\"{}\",\"line\":{},\"function\":\"{}\",\"expected_type\":\"{}\",\"required_effects\":\"{}\",\"candidates\":[{}]}}",
                json_escape(&goal.file),
                goal.line,
                json_escape(&goal.function),
                json_escape(&goal.expected_type),
                json_escape(&goal.required_effects),
                candidates
                    .iter()
                    .map(|candidate| format!("\"{}\"", json_escape(candidate)))
                    .collect::<Vec<_>>()
                    .join(","),
            )
            .expect("writing to a String cannot fail");
        }
        out.push_str("]}");
        return Some(render_status_json(
            "ok",
            true,
            "fill",
            &format!(",\"fill\":{out}"),
        ));
    }

    for goal in &goals {
        writeln!(out, "goal: {}:{} ({})", goal.file, goal.line, goal.function)
            .expect("writing to a String cannot fail");
        writeln!(out, "  expected type: {}", goal.expected_type)
            .expect("writing to a String cannot fail");
        writeln!(out, "  required effects: {}", goal.required_effects)
            .expect("writing to a String cannot fail");
        if include_candidates {
            let candidates = checked_candidates(goal, true);
            if candidates.is_empty() {
                out.push_str("  candidates: none (no local expression passed the checker)\n");
            } else {
                out.push_str("  candidates:\n");
                for (index, candidate) in candidates.iter().enumerate() {
                    writeln!(out, "    {}. {candidate} (checked)", index + 1)
                        .expect("writing to a String cannot fail");
                }
            }
        }
    }
    Some(out)
}

fn split_target(target: &str) -> (&str, Option<usize>) {
    target
        .rsplit_once(':')
        .and_then(|(file, line)| line.parse::<usize>().ok().map(|line| (file, Some(line))))
        .unwrap_or((target, None))
}

fn same_file(display: &str, path: &Path) -> bool {
    if display == path.to_string_lossy().as_ref() {
        return true;
    }
    match (std::fs::canonicalize(display), std::fs::canonicalize(path)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn collect_goals(
    bundle: &mut ProgramBundle,
    facts: &jet::Sema::SemIndexEffectFacts,
) -> Vec<GoalSite> {
    let mut goals = Vec::new();
    for module in &mut bundle.modules {
        let mut functions = Vec::new();
        collect_function_infos(&module.items, &mut functions);
        let file = module.display.clone();
        let source = module.source.clone();
        let alias = module.alias.clone();
        for item in &mut module.items {
            collect_item_goals(
                item, None, &file, &source, &alias, facts, &functions, &mut goals,
            );
        }
    }
    goals
}

fn collect_function_infos(items: &[Item], out: &mut Vec<FunctionInfo>) {
    for item in items {
        match item {
            Item::Func(function) => add_function_info(function, out),
            Item::Struct(structure) => {
                for function in &structure.methods {
                    add_function_info(function, out);
                }
                for block in &structure.trait_impls {
                    for function in &block.methods {
                        add_function_info(function, out);
                    }
                }
            }
            Item::Enum(enumeration) => {
                for function in &enumeration.methods {
                    add_function_info(function, out);
                }
                for block in &enumeration.trait_impls {
                    for function in &block.methods {
                        add_function_info(function, out);
                    }
                }
            }
            Item::Impl(implementation) => {
                for function in &implementation.methods {
                    add_function_info(function, out);
                }
            }
            _ => {}
        }
    }
}

fn add_function_info(function: &Func, out: &mut Vec<FunctionInfo>) {
    if function.params.is_empty() {
        if let Some(return_type) = &function.return_type {
            out.push(FunctionInfo {
                name: function.name.clone(),
                return_type: return_type.name(),
            });
        }
    }
}

fn collect_item_goals(
    item: &mut Item,
    owner: Option<&str>,
    file: &str,
    source: &str,
    alias: &str,
    facts: &jet::Sema::SemIndexEffectFacts,
    functions: &[FunctionInfo],
    out: &mut Vec<GoalSite>,
) {
    match item {
        Item::Func(function) => {
            collect_function_goals(function, owner, file, source, alias, facts, functions, out)
        }
        Item::Impl(implementation) => {
            let owner = implementation.type_name.clone();
            for function in &mut implementation.methods {
                collect_function_goals(
                    function,
                    Some(&owner),
                    file,
                    source,
                    alias,
                    facts,
                    functions,
                    out,
                );
            }
        }
        Item::Struct(structure) => {
            let owner = structure.name.clone();
            for function in &mut structure.methods {
                collect_function_goals(
                    function,
                    Some(&owner),
                    file,
                    source,
                    alias,
                    facts,
                    functions,
                    out,
                );
            }
            for block in &mut structure.trait_impls {
                for function in &mut block.methods {
                    collect_function_goals(
                        function,
                        Some(&owner),
                        file,
                        source,
                        alias,
                        facts,
                        functions,
                        out,
                    );
                }
            }
        }
        Item::Enum(enumeration) => {
            let owner = enumeration.name.clone();
            for function in &mut enumeration.methods {
                collect_function_goals(
                    function,
                    Some(&owner),
                    file,
                    source,
                    alias,
                    facts,
                    functions,
                    out,
                );
            }
            for block in &mut enumeration.trait_impls {
                for function in &mut block.methods {
                    collect_function_goals(
                        function,
                        Some(&owner),
                        file,
                        source,
                        alias,
                        facts,
                        functions,
                        out,
                    );
                }
            }
        }
        _ => {}
    }
}

fn collect_function_goals(
    function: &mut Func,
    owner: Option<&str>,
    file: &str,
    source: &str,
    alias: &str,
    facts: &jet::Sema::SemIndexEffectFacts,
    functions: &[FunctionInfo],
    out: &mut Vec<GoalSite>,
) {
    let function_name = owner
        .map(|owner| format!("{owner}.{}", function.name))
        .unwrap_or_else(|| function.name.clone());
    let params = function
        .params
        .iter()
        .map(|param| (param.name.clone(), param.ty.name()))
        .collect::<Vec<_>>();
    let required_effects = required_effects(facts, alias, owner, &function.name);
    for statement in &mut function.body {
        statement.for_each_expr_mut(|expr| {
            let Expr::Todo {
                span,
                expected_type,
            } = expr
            else {
                return;
            };
            let offset = (*span).start.min(source.len());
            out.push(GoalSite {
                file: file.to_string(),
                source: source.to_string(),
                line: source[..offset]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1,
                function: function_name.clone(),
                span: *span,
                expected_type: expected_type
                    .clone()
                    .unwrap_or_else(|| "(unknown)".to_string()),
                required_effects: required_effects.clone(),
                params: params.clone(),
                functions: functions.to_vec(),
            });
        });
    }
}

fn required_effects(
    facts: &jet::Sema::SemIndexEffectFacts,
    alias: &str,
    owner: Option<&str>,
    name: &str,
) -> String {
    let local_key = owner
        .map(|owner| format!("{owner}::{name}"))
        .unwrap_or_else(|| name.to_string());
    let keys = [format!("{alias}::{local_key}"), local_key, name.to_string()];
    for key in keys {
        if let Some(effects) = facts.solved.get(&key) {
            return if effects.is_empty() {
                "none".to_string()
            } else {
                format!("[{}]", jet::Sema::show_set(effects))
            };
        }
    }
    // The shared reachability projection omits pure rows from its solved
    // effects table. A collected function is still a checked summary, so a
    // missing solved row means the function is pure rather than unknown.
    "none".to_string()
}

fn checked_candidates(goal: &GoalSite, include_candidates: bool) -> Vec<String> {
    if !include_candidates {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    for (name, ty) in &goal.params {
        if ty == &goal.expected_type {
            candidates.push(name.clone());
        }
    }
    let current_name = goal.function.rsplit('.').next().unwrap_or(&goal.function);
    for function in &goal.functions {
        if function.name != current_name && function.return_type == goal.expected_type {
            candidates.push(format!("{}()", function.name));
        }
    }
    match goal.expected_type.as_str() {
        "Int" => candidates.push("0".to_string()),
        "Float" | "F32" => candidates.push("0.0".to_string()),
        "Bool" => candidates.push("false".to_string()),
        "String" => candidates.push("\"\"".to_string()),
        "Char" => candidates.push("'a'".to_string()),
        "Unit" | "()" => candidates.push("()".to_string()),
        expected if expected.ends_with('?') => candidates.push("null".to_string()),
        _ => {}
    }

    let mut checked = Vec::new();
    for candidate in candidates {
        if checked.contains(&candidate) {
            continue;
        }
        if candidate_checks(goal, &candidate) {
            checked.push(candidate);
        }
    }
    checked
}

fn candidate_checks(goal: &GoalSite, candidate: &str) -> bool {
    if goal.span.end > goal.source.len() || goal.span.start > goal.span.end {
        return false;
    }
    let mut source = goal.source.clone();
    source.replace_range(goal.span.start..goal.span.end, candidate);
    jet::Driver::check_eval(&source, &goal.file).is_empty()
}
