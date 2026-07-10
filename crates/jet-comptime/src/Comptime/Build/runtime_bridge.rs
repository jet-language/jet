use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::CtValue;
use std::cell::RefCell;

#[derive(Debug)]
struct ProgramBuildSession {
    context: BuildContext,
    package: String,
    diagnostics: Vec<Diagnostic>,
}

thread_local! {
    static PROGRAM_BUILD_SESSIONS: RefCell<BTreeMap<u64, ProgramBuildSession>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// Start one selected-root build evaluation. Session is thread-local so
/// parallel compiler invocations cannot share graph mutation.
pub fn begin_program_build(package: impl Into<String>, program: CtValue) -> CtValue {
    let context = BuildContext::new();
    let id = context.context;
    PROGRAM_BUILD_SESSIONS.with(|sessions| {
        sessions.borrow_mut().insert(
            id,
            ProgramBuildSession {
                context,
                package: package.into(),
                diagnostics: Vec::new(),
            },
        );
    });
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_BUILD_CONTEXT.to_string(),
        fields: vec![
            ("__session".to_string(), CtValue::Int(id as i64)),
            ("program".to_string(), program),
        ],
    }
}

pub fn abort_program_build(context: &CtValue) {
    if let Some(id) = build_session_id(context) {
        PROGRAM_BUILD_SESSIONS.with(|sessions| {
            sessions.borrow_mut().remove(&id);
        });
    }
}

pub fn finish_program_build(context: &CtValue, returned: &CtValue) -> Result<(BuildPlan, Vec<Diagnostic>), Diagnostic> {
    let id = build_session_id(context).ok_or_else(|| build_diag("lost build context", Span::new(0, 0)))?;
    let default = returned_handle(returned, crate::Syntax::TYPE_BUILD_PLAN)
        .and_then(|(_, target)| (target >= 0).then_some(target as usize));
    let session = PROGRAM_BUILD_SESSIONS.with(|sessions| sessions.borrow_mut().remove(&id));
    let session = session.ok_or_else(|| build_diag("build context expired", Span::new(0, 0)))?;
    let plan = match default {
        Some(target) => session
            .context
            .plan_with_default(TargetRef {
                id: TargetId(target),
                context: id,
            })
            .map_err(|e| build_error_diag(&e, Span::new(0, 0))),
        None => session
            .context
            .plan()
            .map_err(|e| build_error_diag(&e, Span::new(0, 0))),
    }?;
    Ok((plan, session.diagnostics))
}

pub(crate) fn eval_program_build_method(
    receiver: &CtValue,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    in_impure_gate: bool,
) -> Option<Result<CtValue, Diagnostic>> {
    let id = build_session_id(receiver)?;
    Some(PROGRAM_BUILD_SESSIONS.with(|sessions| {
        let mut sessions = sessions.borrow_mut();
        let session = sessions
            .get_mut(&id)
            .ok_or_else(|| build_diag("build context expired", span))?;
        eval_session_method(id, session, method, args, span, in_impure_gate)
    }))
}

fn eval_session_method(
    id: u64,
    session: &mut ProgramBuildSession,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    in_impure_gate: bool,
) -> Result<CtValue, Diagnostic> {
    let action_has_effects = method == "action"
        && matches!(args.get(4), Some(CtValue::List(values)) if !values.is_empty());
    if (method == "probe" || action_has_effects) && !in_impure_gate {
        return Err(Diagnostic::error(
            "E3502",
            format!("`b.{method}` touches the ambient world and must be inside `#Impure(\"reason\")`"),
            "build effects must be declared exactly where an auditor can see them".to_string(),
            format!("wrap the `b.{method}` call in `#Impure(\"why it is needed\")`"),
            Some(span),
        ));
    }
    let result = match method {
        "generate" => {
            let name = string_arg(&args, 0, span)?;
            let source = string_arg(&args, 1, span)?;
            let path = format!(".jet/generated/{}/{}.jet", session.package, safe_name(&name));
            session.context.generate(name, path, source).map(|_| CtValue::Unit)
        }
        "action" => {
            let name = string_arg(&args, 0, span)?;
            let inputs = string_list_arg(&args, 1, span)?;
            let outputs = string_list_arg(&args, 2, span)?;
            let argv = string_list_arg(&args, 3, span)?;
            let caps = string_list_arg(&args, 4, span)?;
            let mut spec = ActionSpec::cached(argv).with_inputs(inputs).with_outputs(outputs);
            for cap in caps {
                spec = spec.with_cap(parse_capability(&cap, span)?);
            }
            if args.len() == 7 {
                let toolchain = handle_arg(
                    &args,
                    5,
                    crate::Syntax::TYPE_BUILD_TOOLCHAIN,
                    id,
                    span,
                )?;
                spec = spec.with_toolchain(ToolchainHandle {
                    id: ToolchainId(toolchain),
                    context: id,
                });
                for probe in handle_list_arg(
                    &args,
                    6,
                    crate::Syntax::TYPE_BUILD_PROBE,
                    id,
                    span,
                )? {
                    spec = spec.with_probe(ProbeHandle {
                        id: ProbeId(probe),
                        context: id,
                    });
                }
            }
            session
                .context
                .action(name, spec)
                .map(|handle| handle_value(crate::Syntax::TYPE_BUILD_ACTION, id, handle.id.0))
        }
        "add_executable" | "add_library" | "add_test" | "add_bench" | "add_asset_bundle"
        | "add_doc" | "add_install" | "add_package" | "add_publish" => {
            let name = string_arg(&args, 0, span)?;
            let sources = string_list_arg(&args, 1, span)?;
            let actions = handle_list_arg(&args, 2, crate::Syntax::TYPE_BUILD_ACTION, id, span)?;
            let mut spec = TargetSpec::new();
            for source in sources {
                spec = spec.with_source(source);
            }
            for action in actions {
                spec = spec.with_action(ActionHandle {
                    id: ActionId(action),
                    context: id,
                });
            }
            let target = match method {
                "add_executable" => session.context.add_executable(name, spec).map(|h| h.id.0),
                "add_library" => session.context.add_library(name, spec).map(|h| h.id.0),
                "add_test" => session.context.add_test(name, spec).map(|h| h.id.0),
                "add_bench" => session.context.add_bench(name, spec).map(|h| h.id.0),
                "add_asset_bundle" => session.context.add_asset_bundle(name, spec).map(|h| h.id.0),
                "add_doc" => session.context.add_doc(name, spec).map(|h| h.id.0),
                "add_install" => session.context.add_install(name, spec).map(|h| h.id.0),
                "add_package" => session.context.add_package(name, spec).map(|h| h.id.0),
                "add_publish" => session.context.add_publish(name, spec).map(|h| h.id.0),
                _ => unreachable!(),
            };
            target.map(|target| handle_value(crate::Syntax::TYPE_BUILD_TARGET, id, target))
        }
        "toolchain" => {
            let name = string_arg(&args, 0, span)?;
            let triple = string_arg(&args, 1, span)?;
            session
                .context
                .toolchain(
                    name,
                    ToolchainSpec::target(triple, BuildProvenance::user_declared("fn build", None)),
                )
                .map(|h| handle_value(crate::Syntax::TYPE_BUILD_TOOLCHAIN, id, h.id.0))
        }
        "probe" => {
            let name = string_arg(&args, 0, span)?;
            let kind = string_arg(&args, 1, span)?;
            let value = string_arg(&args, 2, span)?;
            let spec = match kind.as_str() {
                "find_program" => ProbeSpec::find_program(value),
                "pkg_config" => ProbeSpec::pkg_config(value),
                "header" => ProbeSpec::header_check(value),
                _ => return Err(build_diag(&format!("unknown typed probe kind `{kind}`"), span)),
            };
            session
                .context
                .probe(name, spec)
                .map(|h| handle_value(crate::Syntax::TYPE_BUILD_PROBE, id, h.id.0))
        }
        "error" => {
            let diagnostic_span = source_span_arg(&args, 0, span)?;
            let code = string_arg(&args, 1, span)?;
            let what = string_arg(&args, 2, span)?;
            let why = string_arg(&args, 3, span)?;
            let fix = string_arg(&args, 4, span)?;
            let reserved = code.len() > 1
                && matches!(code.as_bytes()[0], b'E' | b'W')
                && code.as_bytes()[1..].iter().all(u8::is_ascii_digit);
            if reserved {
                session.diagnostics.push(Diagnostic::error(
                    "E3530",
                    format!("build rule code `{code}` is reserved"),
                    "codes beginning with E or W followed only by digits belong to the compiler".to_string(),
                    "use a project prefix such as `ORG01`".to_string(),
                    Some(diagnostic_span),
                ));
            } else {
                session.diagnostics.push(Diagnostic::error(code, what, why, fix, Some(diagnostic_span)));
            }
            return Ok(CtValue::Unit);
        }
        "plan" => {
            let default = if args.is_empty() {
                usize::MAX
            } else {
                handle_arg(&args, 0, crate::Syntax::TYPE_BUILD_TARGET, id, span)?
            };
            return Ok(CtValue::ResOk(Box::new(handle_value(
                crate::Syntax::TYPE_BUILD_PLAN,
                id,
                default,
            ))));
        }
        _ => return None.ok_or_else(|| build_diag(&format!("unknown build method `{method}`"), span)),
    };
    Ok(match result {
        Ok(value) => CtValue::ResOk(Box::new(value)),
        Err(error) => CtValue::ResErr(Box::new(CtValue::Str(build_error_text(&error)))),
    })
}

fn build_session_id(value: &CtValue) -> Option<u64> {
    let CtValue::Struct { type_name, fields } = value else { return None };
    if type_name != crate::Syntax::TYPE_BUILD_CONTEXT { return None; }
    fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("__session", CtValue::Int(id)) if *id >= 0 => Some(*id as u64),
        _ => None,
    })
}

fn returned_handle(value: &CtValue, type_name: &str) -> Option<(u64, i64)> {
    let value = match value { CtValue::ResOk(value) => value.as_ref(), other => other };
    let CtValue::Struct { type_name: actual, fields } = value else { return None };
    if actual != type_name { return None; }
    let session = fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
        ("session", CtValue::Int(id)) => Some(*id as u64), _ => None
    })?;
    let id = fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
        ("id", CtValue::Int(id)) => Some(*id), _ => None
    })?;
    Some((session, id))
}

fn handle_value(type_name: &str, session: u64, id: usize) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![
            ("session".to_string(), CtValue::Int(session as i64)),
            ("id".to_string(), CtValue::Int(id as i64)),
        ],
    }
}

fn handle_arg(args: &[CtValue], index: usize, ty: &str, session: u64, span: Span) -> Result<usize, Diagnostic> {
    match args.get(index).and_then(|v| returned_handle(v, ty)) {
        Some((actual, id)) if actual == session && id >= 0 => Ok(id as usize),
        _ => Err(build_diag(&format!("argument {} must be a `{ty}` from this build", index + 1), span)),
    }
}

fn handle_list_arg(args: &[CtValue], index: usize, ty: &str, session: u64, span: Span) -> Result<Vec<usize>, Diagnostic> {
    let Some(CtValue::List(values)) = args.get(index) else {
        return Err(build_diag(&format!("argument {} must be a list", index + 1), span));
    };
    values.iter().map(|value| handle_arg(std::slice::from_ref(value), 0, ty, session, span)).collect()
}

fn string_arg(args: &[CtValue], index: usize, span: Span) -> Result<String, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(build_diag(&format!("argument {} must be String", index + 1), span)),
    }
}

fn source_span_arg(args: &[CtValue], index: usize, span: Span) -> Result<Span, Diagnostic> {
    let Some(CtValue::Struct { type_name, fields }) = args.get(index) else {
        return Err(build_diag("custom build diagnostics require a source span", span));
    };
    if type_name != crate::Syntax::TYPE_SOURCE_SPAN {
        return Err(build_diag("custom build diagnostics require a source span", span));
    }
    let value = |name: &str| {
        fields.iter().find_map(|(field, value)| match (field.as_str(), value) {
            (actual, CtValue::Int(value)) if actual == name && *value >= 0 => Some(*value as usize),
            _ => None,
        })
    };
    match (value("start"), value("end")) {
        (Some(start), Some(end)) if start <= end => Ok(Span::new(start, end)),
        _ => Err(build_diag("custom build diagnostics require a valid source span", span)),
    }
}

fn string_list_arg(args: &[CtValue], index: usize, span: Span) -> Result<Vec<String>, Diagnostic> {
    let Some(CtValue::List(values)) = args.get(index) else {
        return Err(build_diag(&format!("argument {} must be [String]", index + 1), span));
    };
    values.iter().map(|value| match value {
        CtValue::Str(value) => Ok(value.clone()),
        _ => Err(build_diag(&format!("argument {} must be [String]", index + 1), span)),
    }).collect()
}

fn parse_capability(name: &str, span: Span) -> Result<BuildCapability, Diagnostic> {
    BuildCapability::parse(name).ok_or_else(|| {
        build_diag(
            &format!("unknown build effect `{name}`; use one of the ten declared effects"),
            span,
        )
    })
}

fn safe_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn build_error_diag(error: &BuildError, span: Span) -> Diagnostic {
    build_diag(&build_error_text(error), span)
}

fn build_error_text(error: &BuildError) -> String {
    match error {
        BuildError::DuplicateTargetName(name) => format!("target name `{name}` is registered twice"),
        BuildError::DuplicateActionName(name) => format!("action name `{name}` is registered twice"),
        BuildError::DuplicateToolchainName(name) => format!("toolchain name `{name}` is registered twice"),
        BuildError::DuplicateProbeName(name) => format!("probe name `{name}` is registered twice"),
        BuildError::CachedActionWithoutOutputs(name) => format!("cached action `{name}` declares no outputs"),
        BuildError::PhonyActionWithoutCaps(name) => format!("uncached action `{name}` declares no capabilities"),
        BuildError::DuplicateBuildOutput { output, first_action, second_action } => format!(
            "output `{output}` is owned by both `{first_action}` and `{second_action}`"
        ),
        BuildError::TargetDependencyCycle => "target dependency graph contains a cycle".to_string(),
        BuildError::ActionDependencyCycle => "action dependency graph contains a cycle".to_string(),
        other => format!("{other:?}"),
    }
}

fn build_diag(detail: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3502",
        format!("build plan is invalid: {detail}"),
        "`fn build` must return one deterministic typed graph whose handles all belong to that build".to_string(),
        "fix the named target/action/toolchain/probe and run `jet explain-build` to inspect the graph".to_string(),
        Some(span),
    )
}
