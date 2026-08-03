//! U12: field-check one dev-supervised `Service` record (an entry in an
//! `env.<name>` role-module's `services: { … }` map) and capture it as a
//! `DevServicePlan`.
//!
//! Mirrors `System::evaluate_service` (same ratified `Service` grammar — open
//! record, required `enable: Bool`, E0975 reused verbatim) but produces a
//! distinct type: `Jetpack::Services` (the dev-runtime tier, std::process-based)
//! is the only consumer, and it owns 100% of a dev service's semantics, unlike
//! the jetos capture (`ServicePlan`), which is inert until Phase D realization.
//! So the *recognized* fields (`ports`/`run`/`shutdown`/`data_dir`/`ready`) are
//! extracted to typed struct fields here, while anything else still lands in
//! `extra` (the record stays open, per U12) for `Jetpack::Services` to flag as
//! E1262 (`unknown-service-field`) at supervision time — never here, so a
//! forward-compatible extra key is never a hard parse/sema error.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::Comptime::{self, CtValue};
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::AST::{Func, ServiceEntry};

use super::Diagnostics::{service_enable_not_bool, service_missing_enable};
use super::Eval::check_build_io;
use super::Types::{DevServicePlan, ReadyProbe, RestartPolicy, ShutdownPolicy};

/// U12: evaluate one `name: { … }` entry from an `env.<name>` role-module's
/// `services:` map into a `DevServicePlan`.
pub(super) fn evaluate_dev_service(
    entry: &ServiceEntry,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
) -> Result<DevServicePlan, Diagnostic> {
    let mut plan = DevServicePlan {
        name: entry.name.clone(),
        ..Default::default()
    };
    let mut enable = None;
    for (name, span, value) in &entry.fields {
        check_build_io(value)?;
        if name == Syntax::DEV_SERVICE_FIELD_READY {
            if let Some(probe) = ready_probe_from(value, base_dir, funcs, globals)? {
                plan.ready_probe = Some(probe);
                continue;
            }
        }
        let v = Comptime::evaluate(value, funcs, &HashSet::new(), base_dir, globals)?;
        if name == Syntax::SERVICE_FIELD_ENABLE {
            match v {
                CtValue::Bool(b) => enable = Some(b),
                _ => return Err(service_enable_not_bool(*span)),
            }
        } else if name == Syntax::DEV_SERVICE_FIELD_PORTS {
            match ports_from(&v) {
                Some(ports) => plan.ports = ports,
                // Recognized name, wrong shape (not `[Int]`) — captured in
                // `extra` rather than silently dropped, so `jet services`
                // still surfaces it (E1262) instead of ignoring a typo'd value.
                None => plan.extra.push((name.clone(), v.jet_show())),
            }
        } else if name == Syntax::DEV_SERVICE_FIELD_RUN {
            match command_from(&v) {
                Some(command) => plan.run = Some(command),
                None => plan.extra.push((name.clone(), v.jet_show())),
            }
        } else if name == Syntax::DEV_SERVICE_FIELD_SHUTDOWN {
            match shutdown_from(&v) {
                Some(shutdown) => plan.shutdown = Some(shutdown),
                None => plan.extra.push((name.clone(), v.jet_show())),
            }
        } else if name == Syntax::DEV_SERVICE_FIELD_DATA_DIR {
            set_or_extra(&mut plan.data_dir, name, &v, &mut plan.extra);
        } else if name == Syntax::DEV_SERVICE_FIELD_READY {
            set_or_extra(&mut plan.ready, name, &v, &mut plan.extra);
        } else if name == Syntax::DEV_SERVICE_FIELD_RESTART {
            match restart_from(&v) {
                Some(restart) => plan.restart = Some(restart),
                None => plan.extra.push((name.clone(), v.jet_show())),
            }
        } else if name == Syntax::DEV_SERVICE_FIELD_WATCH {
            match strings_from(&v) {
                Some(paths) => plan.watch = paths,
                None => plan.extra.push((name.clone(), v.jet_show())),
            }
        } else if name == Syntax::DEV_SERVICE_FIELD_AFTER {
            match strings_from(&v) {
                Some(names) => plan.after = names,
                None => plan.extra.push((name.clone(), v.jet_show())),
            }
        } else if name == Syntax::DEV_SERVICE_FIELD_DEPENDS_ON {
            match strings_from(&v) {
                Some(names) => plan.depends_on = names,
                None => plan.extra.push((name.clone(), v.jet_show())),
            }
        } else if name == Syntax::DEV_SERVICE_FIELD_BEFORE_START {
            match strings_from(&v) {
                Some(names) => plan.before_start = names,
                None => plan.extra.push((name.clone(), v.jet_show())),
            }
        } else if name == Syntax::DEV_SERVICE_FIELD_SOCKETS {
            match strings_from(&v) {
                Some(names) => plan.sockets = names,
                None => plan.extra.push((name.clone(), v.jet_show())),
            }
        } else {
            plan.extra.push((name.clone(), v.jet_show()));
        }
    }
    plan.enable = match (enable, plan.run.is_some()) {
        (Some(value), _) => value,
        (None, true) => true,
        (None, false) => return Err(service_missing_enable(&entry.name, entry.span)),
    };
    Ok(plan)
}

fn ready_probe_from(
    value: &crate::AST::Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
) -> Result<Option<ReadyProbe>, Diagnostic> {
    let crate::AST::Expr::Call(call) = value else {
        return Ok(None);
    };
    let name = call.name.rsplit('.').next().unwrap_or(call.name.as_str());
    if !matches!(name, "exec" | "http" | "notify" | "tcp") {
        return Ok(None);
    }
    let mut values = Vec::new();
    for arg in &call.args {
        check_build_io(&arg.expr)?;
        values.push(Comptime::evaluate(
            &arg.expr,
            funcs,
            &HashSet::new(),
            base_dir,
            globals,
        )?);
    }
    let string_at = |index: usize| match values.get(index) {
        Some(CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    };
    let int_at = |index: usize| match values.get(index) {
        Some(CtValue::Int(value)) => u16::try_from(*value).ok().filter(|value| *value != 0),
        _ => None,
    };
    let probe = match name {
        "exec" => ReadyProbe::Exec(string_at(0).ok_or_else(|| {
            Diagnostic::error(
                "E1328",
                "typed exec readiness needs one command".to_string(),
                "ServiceProbe.exec carries one shell command string".to_string(),
                "write ready: .exec(\"...\")".to_string(),
                Some(call.name_span),
            )
        })?),
        "http" => ReadyProbe::Http {
            url: string_at(0).ok_or_else(|| {
                Diagnostic::error(
                    "E1328",
                    "typed HTTP readiness needs a URL".to_string(),
                    "ServiceProbe.http carries a URL and an optional status".to_string(),
                    "write ready: .http(\"http://127.0.0.1:8080/health\", 200)".to_string(),
                    Some(call.name_span),
                )
            })?,
            status: match values.get(1) {
                None => None,
                Some(_) => Some(int_at(1).ok_or_else(|| {
                    Diagnostic::error(
                        "E1328",
                        "typed HTTP readiness status must be an integer".to_string(),
                        "ServiceProbe.http accepts an optional HTTP status code".to_string(),
                        "write ready: .http(\"http://127.0.0.1:8080/health\", 200)".to_string(),
                        Some(call.name_span),
                    )
                })?),
            },
        },
        "notify" => ReadyProbe::Notify {
            path: {
                let path = string_at(0).ok_or_else(|| {
                    Diagnostic::error(
                        "E1328",
                        "typed notify readiness needs a path".to_string(),
                        "ServiceProbe.notify observes one project-relative readiness file".to_string(),
                        "write ready: .notify(\".jet/ready/api\")".to_string(),
                        Some(call.name_span),
                    )
                })?;
                let path_ref = Path::new(&path);
                if path_ref.is_absolute()
                    || path_ref
                        .components()
                        .any(|component| component == std::path::Component::ParentDir)
                {
                    return Err(Diagnostic::error(
                        "E1328",
                        "typed notify readiness path must stay inside the project".to_string(),
                        "ServiceProbe.notify observes a project-relative readiness file".to_string(),
                        "write a relative path without `..` or an absolute prefix".to_string(),
                        Some(call.name_span),
                    ));
                }
                path
            },
        },
        "tcp" => ReadyProbe::Tcp {
            host: string_at(0).unwrap_or_else(|| "127.0.0.1".to_string()),
            port: int_at(if values.len() > 1 { 1 } else { 0 }).ok_or_else(|| {
                Diagnostic::error(
                    "E1328",
                    "typed TCP readiness needs a port".to_string(),
                    "ServiceProbe.tcp carries a host and port, or only a port".to_string(),
                    "write ready: .tcp(5432) or .tcp(\"127.0.0.1\", 5432)".to_string(),
                    Some(call.name_span),
                )
            })?,
        },
        _ => return Ok(None),
    };
    let valid_arity = match name {
        "exec" | "notify" => values.len() == 1,
        "http" => (1..=2).contains(&values.len()),
        "tcp" => (1..=2).contains(&values.len()),
        _ => false,
    };
    if !valid_arity {
        return Err(Diagnostic::error(
            "E1328",
            format!("typed {name} readiness has the wrong number of arguments"),
            "each readiness probe has a closed argument shape".to_string(),
            "use exec(command), http(url[, status]), notify(path), or tcp(port|host, port)".to_string(),
            Some(call.name_span),
        ));
    }
    Ok(Some(probe))
}

/// `ports: [Int]` — `None` when `v` isn't a list of `Int` (caller falls back
/// to capturing it in `extra`).
fn ports_from(v: &CtValue) -> Option<Vec<i64>> {
    let CtValue::List(xs) = v else {
        return None;
    };
    xs.iter()
        .map(|x| match x {
            CtValue::Int(n) => Some(*n),
            _ => None,
        })
        .collect()
}

fn string_from(v: &CtValue) -> Option<String> {
    match v {
        CtValue::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn command_from(v: &CtValue) -> Option<Vec<String>> {
    let CtValue::List(values) = v else {
        return None;
    };
    let values = values
        .iter()
        .map(string_from)
        .collect::<Option<Vec<_>>>()?;
    (!values.is_empty()).then_some(values)
}

fn strings_from(v: &CtValue) -> Option<Vec<String>> {
    let CtValue::List(values) = v else { return None };
    values.iter().map(string_from).collect()
}

fn restart_from(v: &CtValue) -> Option<RestartPolicy> {
    match v {
        CtValue::Str(value) if value.eq_ignore_ascii_case("never") => Some(RestartPolicy::Never),
        CtValue::Str(value) if value.eq_ignore_ascii_case("on_failure") => {
            Some(RestartPolicy::OnFailure {
                max: 3,
                backoff_ms: 250,
                exponential: false,
            })
        }
        CtValue::Str(value) if value.eq_ignore_ascii_case("always") => {
            Some(RestartPolicy::Always {
                max: 3,
                backoff_ms: 250,
                exponential: false,
            })
        }
        CtValue::Enum { variant, args, .. } => {
            let (max, backoff_ms, exponential) = restart_args(args)?;
            match variant.rsplit('.').next() {
                Some("Never") if args.is_empty() => Some(RestartPolicy::Never),
                Some("OnFailure") => Some(RestartPolicy::OnFailure {
                    max,
                    backoff_ms,
                    exponential,
                }),
                Some("Always") => Some(RestartPolicy::Always {
                    max,
                    backoff_ms,
                    exponential,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn shutdown_from(v: &CtValue) -> Option<ShutdownPolicy> {
    let CtValue::Enum { variant, args, .. } = v else {
        return None;
    };
    let variant = variant.rsplit('.').next()?;
    match variant {
        "Kill" if args.is_empty() => Some(ShutdownPolicy::Kill),
        "Term" | "Graceful" => shutdown_term_args(args),
        _ => None,
    }
}

fn restart_args(args: &[(Option<String>, CtValue)]) -> Option<(u32, u64, bool)> {
    let mut max = 3;
    let mut backoff_ms = 250;
    let mut exponential = false;
    let mut saw_max = false;
    let mut saw_backoff = false;
    for (name, value) in args {
        match named_payload_field(name.as_deref()) {
            Some("max") if !saw_max => {
                max = nonnegative_u32(value)?;
                saw_max = true;
            }
            Some("backoff") | Some("backoff_ms") if !saw_backoff => {
                (backoff_ms, exponential) = backoff_value(value)?;
                saw_backoff = true;
            }
            None if !saw_max => {
                max = nonnegative_u32(value)?;
                saw_max = true;
            }
            None if !saw_backoff => {
                (backoff_ms, exponential) = backoff_value(value)?;
                saw_backoff = true;
            }
            _ => return None,
        }
    }
    Some((max, backoff_ms, exponential))
}

fn nonnegative_u32(value: &CtValue) -> Option<u32> {
    match value {
        CtValue::Int(value) if *value >= 0 => u32::try_from(*value).ok(),
        _ => None,
    }
}

fn backoff_value(value: &CtValue) -> Option<(u64, bool)> {
    match value {
        CtValue::Int(value) if *value >= 0 => Some((u64::try_from(*value).ok()?, false)),
        CtValue::Str(value) if value.eq_ignore_ascii_case("exponential") => Some((250, true)),
        CtValue::Enum { variant, args, .. }
            if variant.rsplit('.').next() == Some("Exponential") && args.is_empty() =>
        {
            Some((250, true))
        }
        CtValue::Enum { variant, args, .. }
            if variant.rsplit('.').next() == Some("Fixed") && args.len() == 1 => {
            Some((nonnegative_u64(&args[0].1)?, false))
        }
        _ => None,
    }
}

fn nonnegative_u64(value: &CtValue) -> Option<u64> {
    match value {
        CtValue::Int(value) if *value >= 0 => u64::try_from(*value).ok(),
        _ => None,
    }
}

fn shutdown_term_args(args: &[(Option<String>, CtValue)]) -> Option<ShutdownPolicy> {
    if args.len() > 1 {
        return None;
    }
    let grace_ms = match args.first() {
        None => 3_000,
        Some((name, value)) if named_payload_field(name.as_deref()).is_none() || matches!(named_payload_field(name.as_deref()), Some("grace" | "grace_ms")) => {
            nonnegative_u64(value)?
        }
        Some(_) => return None,
    };
    Some(ShutdownPolicy::Term { grace_ms })
}

fn named_payload_field(name: Option<&str>) -> Option<&str> {
    name.map(|name| name.strip_prefix("user_").unwrap_or(name))
}

/// Set `slot` from `v` if it's a `Str`; otherwise capture it in `extra` (same
/// "recognized name, wrong shape" fallback as `ports`).
fn set_or_extra(
    slot: &mut Option<String>,
    name: &str,
    v: &CtValue,
    extra: &mut Vec<(String, String)>,
) {
    match string_from(v) {
        Some(s) => *slot = Some(s),
        None => extra.push((name.to_string(), v.jet_show())),
    }
}
