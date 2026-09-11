//! D-SCHEDULE1 (ratified 2026-07-11, card #505): checked `#Every(…)`
//! schedule arguments, plus D-JPK-TASKRUN1 reserved-name law on `#Job fn`.
//!
//! The parser only records the raw shape it saw (`EveryArg` — a duration
//! literal or a quoted string) and, at parse time, whether `#Every(…)` is
//! paired with `#Job` on the same function (E0925, pushed directly by
//! `jet-parser` — a placement question, not a value question). This module
//! owns the one thing left for schedules: is the VALUE a real schedule?
//! This module resolves the raw marker through the registered unit-plane
//! facts and writes one checked `EverySchedule` projection onto the marker.
//! Runtime consumers read that projection; they never parse source suffixes.
//!
//! D-JPK-TASKRUN1 / D-CMD-OVERRIDE1=C also lives here: a `#Job fn` must not
//! reuse the reserved lifecycle verbs `run`/`dev`/`build`/`test` (E0928).
//!
//! I3: this module only decides; codegen never reads `Func::every` at all —
//! a `#Job`/`#Every` function generates as an ordinary fn.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{
    EveryArg, EverySchedule, EveryScheduleError, Func, Item, JobScope, LoadedModule,
    ProgramBundle,
};
use std::collections::{HashMap, HashSet};

fn resolve_every_arg(
    arg: &EveryArg,
    registry: &super::TypeRegistry,
) -> Result<EverySchedule, EveryScheduleError> {
    match arg {
        EveryArg::Duration {
            int, float, suffix, ..
        } => {
            let Some(unit) = registry.unit_literal("Time", suffix) else {
                return Err(EveryScheduleError::UnknownDurationUnit);
            };
            if int.iter().any(|value| *value <= 0)
                || float
                    .iter()
                    .any(|value| !value.is_finite() || *value <= 0.0)
            {
                return Err(EveryScheduleError::NonPositiveDuration);
            }
            let nanos = if let Some(value) = int {
                let value = *value;
                let scale = unit
                    .scale
                    .num
                    .to_string()
                    .parse::<i64>()
                    .map_err(|_| EveryScheduleError::DurationOutOfRange)?;
                let numerator = value
                    .checked_mul(scale)
                    .ok_or(EveryScheduleError::DurationOutOfRange)?;
                let denominator = unit
                    .scale
                    .den
                    .to_string()
                    .parse::<i64>()
                    .map_err(|_| EveryScheduleError::DurationOutOfRange)?;
                numerator
                    .checked_div(denominator)
                    .ok_or(EveryScheduleError::DurationOutOfRange)?
            } else {
                let value = float.as_ref().copied().unwrap_or(0.0);
                let scale_num = unit
                    .scale
                    .num
                    .to_string()
                    .parse::<f64>()
                    .map_err(|_| EveryScheduleError::DurationOutOfRange)?;
                let scale_den = unit
                    .scale
                    .den
                    .to_string()
                    .parse::<f64>()
                    .map_err(|_| EveryScheduleError::DurationOutOfRange)?;
                let nanoseconds = value * scale_num / scale_den;
                if !nanoseconds.is_finite()
                    || nanoseconds < i64::MIN as f64
                    || nanoseconds >= 9_223_372_036_854_775_808.0
                {
                    return Err(EveryScheduleError::DurationOutOfRange);
                }
                nanoseconds.trunc() as i64
            };
            if nanos == 0 {
                return Err(EveryScheduleError::NonPositiveDuration);
            }
            if nanos < 0 {
                return Err(EveryScheduleError::NonPositiveDuration);
            }
            Ok(EverySchedule::Duration { nanos })
        }
        EveryArg::WallClock { text, .. } => {
            let bytes = text.as_bytes();
            let digits_ok = bytes.len() == 5
                && bytes[2] == b':'
                && bytes[0].is_ascii_digit()
                && bytes[1].is_ascii_digit()
                && bytes[3].is_ascii_digit()
                && bytes[4].is_ascii_digit();
            if !digits_ok {
                return Err(EveryScheduleError::BadWallClockFormat);
            }
            let hour: u32 = text[0..2].parse().unwrap_or(99);
            let minute: u32 = text[3..5].parse().unwrap_or(99);
            if hour > 23 {
                return Err(EveryScheduleError::HourOutOfRange);
            }
            if minute > 59 {
                return Err(EveryScheduleError::MinuteOutOfRange);
            }
            Ok(EverySchedule::WallClockTime {
                hour: hour as u8,
                minute: minute as u8,
            })
        }
        EveryArg::Expression(_) => Err(EveryScheduleError::DynamicValue),
    }
}

/// E0926: `#Every(…)`'s value isn't a real schedule — a bad duration unit,
/// a non-positive/out-of-range duration, or a malformed/out-of-range `"HH:MM"`.
fn e0926_bad_schedule_value(reason: EveryScheduleError, span: Span) -> Diagnostic {
    let (what, why, fix) = match reason {
        EveryScheduleError::UnknownDurationUnit => (
            "this duration's unit isn't a recognized schedule cadence",
            "a schedule's repeat interval uses the canonical Time family — \
             `ns`/`us`/`ms`/`s`/`min`/`h`/`d` — not an arbitrary `#UnitFamily` member.",
            "use a canonical Time unit such as `min`, `h`, or `d` (e.g. `#Every(2h)`).",
        ),
        EveryScheduleError::NonPositiveDuration => (
            "a schedule interval must be a positive duration",
            "`#Every(0ms)` or a negative duration never becomes due — it isn't a real cadence.",
            "write a duration greater than zero, e.g. `#Every(5min)`.",
        ),
        EveryScheduleError::DurationOutOfRange => (
            "this schedule duration is outside the supported range",
            "schedule intervals use the same fixed i64 nanosecond carrier as Duration.",
            "write a smaller positive duration, e.g. `#Every(5min)`.",
        ),
        EveryScheduleError::BadWallClockFormat => (
            "this daily schedule isn't a plain `\"HH:MM\"` time",
            "a wall-clock trigger is exactly two digits, a colon, and two digits — 24h time, \
             no seconds, no timezone.",
            "write a fixed daily time like `#Every(\"03:00\")`.",
        ),
        EveryScheduleError::HourOutOfRange => (
            "this daily schedule's hour is out of range",
            "24h wall-clock hours run `00`..=`23`.",
            "write an hour between `00` and `23`, e.g. `#Every(\"03:00\")`.",
        ),
        EveryScheduleError::MinuteOutOfRange => (
            "this daily schedule's minute is out of range",
            "wall-clock minutes run `00`..=`59`.",
            "write a minute between `00` and `59`, e.g. `#Every(\"03:00\")`.",
        ),
        EveryScheduleError::DynamicValue => (
            "a job schedule must be known at compile time",
            "the marker argument has the right type, but job discovery needs one fixed cadence.",
            "write a duration or wall-clock literal, e.g. `#Every(5min)` or `#Every(\"03:00\")`.",
        ),
    };
    Diagnostic::error(
        "E0926",
        what.to_string(),
        why.to_string(),
        fix.to_string(),
        Some(span),
    )
}

/// E0928: `#Job fn` reused a reserved lifecycle verb (D-JPK-TASKRUN1/D-CMD-OVERRIDE1=C).
fn e0928_reserved_job_name(name: &str, span: Span) -> Diagnostic {
    let reserved = Syntax::JOB_RESERVED_LIFECYCLE.join(", ");
    Diagnostic::error(
        "E0928",
        format!("`{name}` is a built-in lifecycle verb, not a job name"),
        format!(
            "`run`, `dev`, `build`, and `test` already name Jet's built-in entry points — \
             a `#Job fn` picks a user-chosen verb beside them (D-JPK-TASKRUN1/D-CMD-OVERRIDE1=C)."
        ),
        format!(
            "rename it, e.g. `#Job fn {name}_assets()`, or drop `#Job` if this is the lifecycle entry."
        ),
        Some(span),
    )
    .with_detail(format!("reserved: {reserved}\n"))
}

fn e0928_job_collision(name: &str, scope: JobScope, span: Span) -> Diagnostic {
    let scope = match scope {
        JobScope::Dev => "Dev",
        JobScope::Ship => "Ship",
        JobScope::Internal => "Internal",
    };
    Diagnostic::error(
        "E0928",
        format!("job `{name}` is declared more than once in scope .{scope}"),
        "one argv subcommand cannot select two functions at the same job scope".to_string(),
        "rename the job, or give the declarations different scopes".to_string(),
        Some(span),
    )
}

/// D-JPK-TASKRUN1/D-CMD-OVERRIDE1=C: reject `#Job fn run|dev|build|test`. Called alongside the
/// `#Every` value check during registration.
pub(crate) fn check_job_marker(f: &Func) -> Vec<Diagnostic> {
    if !f.is_job {
        return Vec::new();
    }
    if Syntax::JOB_RESERVED_LIFECYCLE.contains(&f.name.as_str()) {
        let span = f.job_span.unwrap_or(f.name_span);
        return vec![e0928_reserved_job_name(&f.name, span)];
    }
    if Syntax::JOB_RESERVED_CLI.contains(&f.name.as_str()) {
        let span = f.job_span.unwrap_or(f.name_span);
        return vec![Diagnostic::error(
            "E0928",
            format!("`{}` is reserved by Jet's command line", f.name),
            "job dispatch reserves built-in command and flag names before ordinary CLI parsing"
                .to_string(),
            "rename the job, or choose a name outside Jet's command and flag vocabulary"
                .to_string(),
            Some(span),
        )];
    }
    Vec::new()
}

/// D-JOB-SUBCMD1=C: names may repeat across scopes, but not within one.
pub(crate) fn check_job_collisions(modules: &[LoadedModule]) -> Vec<Diagnostic> {
    let mut seen: Vec<(String, JobScope)> = Vec::new();
    let mut diags = Vec::new();
    for module in modules {
        for item in &module.items {
            let Item::Func(function) = item else { continue };
            if !function.is_job {
                continue;
            }
            let scope = function
                .job_metadata
                .as_ref()
                .map(|metadata| metadata.scope)
                .unwrap_or_default();
            if !seen
                .iter()
                .any(|(name, previous)| name == &function.name && *previous == scope)
            {
                seen.push((function.name.clone(), scope));
            } else {
                diags.push(e0928_job_collision(
                    &function.name,
                    scope,
                    function.name_span,
                ));
            }
        }
    }
    diags
}

fn job_graph_error(
    code: &str,
    what: String,
    why: &str,
    fix: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::error(
        code,
        what,
        why.to_string(),
        fix.to_string(),
        Some(span),
    )
}

/// D-DX-JOBGRAPH1=A: validate the checked job graph before any execution tier
/// can admit a job. Names are resolved in one deterministic package namespace;
/// package metadata is also checked against the package authority projection.
pub(crate) fn check_job_graph(bundle: &ProgramBundle) -> Vec<Diagnostic> {
    let mut jobs = Vec::new();
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            let Item::Func(function) = item else { continue };
            if !function.is_job {
                continue;
            }
            let index = jobs.len();
            by_name
                .entry(function.name.clone())
                .or_default()
                .push(index);
            jobs.push((
                function.name.clone(),
                function
                    .job_metadata
                    .as_ref()
                    .map(|metadata| metadata.after.clone())
                    .unwrap_or_default(),
                function
                    .job_metadata
                    .as_ref()
                    .map(|metadata| metadata.packages.clone())
                    .unwrap_or_default(),
                function.job_span.unwrap_or(function.name_span),
            ));
        }
    }

    let mut diagnostics = Vec::new();
    let mut reported = HashSet::new();
    for (name, after, packages, span) in &jobs {
        for dependency in after {
            if dependency == name {
                if reported.insert(("self", name.clone(), dependency.clone())) {
                    diagnostics.push(job_graph_error(
                        "E1331",
                        format!("job `{name}` depends on itself"),
                        "a job graph must be acyclic so every prerequisite has a finite completion point",
                        "remove the self-reference from `after`",
                        *span,
                    ));
                }
                continue;
            }
            match by_name.get(dependency) {
                None => {
                    if reported.insert(("unknown", name.clone(), dependency.clone())) {
                        diagnostics.push(job_graph_error(
                            "E1331",
                            format!("job `{name}` depends on unknown job `{dependency}`"),
                            "every `after` name is resolved against the checked #Job declarations before effects",
                            "declare the prerequisite with `#Job`, or correct the name",
                            *span,
                        ));
                    }
                }
                Some(candidates) if candidates.len() > 1 => {
                    if reported.insert(("ambiguous", name.clone(), dependency.clone())) {
                        diagnostics.push(job_graph_error(
                            "E1331",
                            format!("job `{name}` has an ambiguous prerequisite `{dependency}`"),
                            "the graph uses one source identity per job name and cannot choose between scopes",
                            "rename one declaration or make the prerequisite name unique",
                            *span,
                        ));
                    }
                }
                Some(_) => {}
            }
        }
        for package in packages {
            let declared = bundle.package_guarantees.dependency_names.contains(package)
                || bundle.dep_roots.contains_key(package);
            let allowed = bundle
                .package_guarantees
                .deps
                .as_ref()
                .is_none_or(|allowlist| allowlist.iter().any(|value| value == package));
            if !declared || !allowed {
                diagnostics.push(job_graph_error(
                    "E1332",
                    format!("job `{name}` names unauthorized package `{package}`"),
                    "job package capabilities must be declared by the checked package authority",
                    "declare the dependency in package.jet authority, or remove it from `packages`",
                    *span,
                ));
            }
        }
    }

    let mut state = vec![0u8; jobs.len()];
    let mut stack = Vec::new();
    fn visit(
        index: usize,
        jobs: &[(String, Vec<String>, Vec<String>, Span)],
        by_name: &HashMap<String, Vec<usize>>,
        state: &mut [u8],
        stack: &mut Vec<usize>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if state[index] == 2 {
            return;
        }
        if state[index] == 1 {
            let cycle_start = stack.iter().position(|candidate| *candidate == index).unwrap_or(0);
            let cycle = stack[cycle_start..]
                .iter()
                .map(|candidate| jobs[*candidate].0.as_str())
                .chain(std::iter::once(jobs[index].0.as_str()))
                .collect::<Vec<_>>()
                .join(" -> ");
            diagnostics.push(job_graph_error(
                "E1331",
                format!("job graph contains a cycle: {cycle}"),
                "a prerequisite graph must reach a completed root before scheduling can start",
                "remove one `after` edge from the cycle",
                jobs[index].3,
            ));
            return;
        }
        state[index] = 1;
        stack.push(index);
        for dependency in &jobs[index].1 {
            let Some(candidates) = by_name.get(dependency) else { continue };
            if candidates.len() == 1 {
                visit(candidates[0], jobs, by_name, state, stack, diagnostics);
            }
        }
        stack.pop();
        state[index] = 2;
    }
    for index in 0..jobs.len() {
        visit(index, &jobs, &by_name, &mut state, &mut stack, &mut diagnostics);
    }
    diagnostics
}

/// D-SCHEDULE1: validate `f`'s `#Every(…)` argument, if it has one. Called
/// once per function during registration (mirrors `check_inline_always_fn`'s
/// call sites in `Registration.rs`/`Bundle.rs`) — E0925 placement is already
/// handled by the parser, so this is the value check alone. Also runs the
/// D-JPK-TASKRUN1 reserved-name check for `#Job`.
pub(crate) fn check_every_marker(f: &mut Func, registry: &super::TypeRegistry) -> Vec<Diagnostic> {
    let mut diags = check_job_marker(f);
    let Some(every) = &mut f.every else {
        return diags;
    };
    match resolve_every_arg(&every.arg, registry) {
        Ok(schedule) => {
            every.resolved = Some(schedule);
            diags
        }
        Err(reason) => {
            every.resolved = None;
            let span = match &every.arg {
                crate::AST::EveryArg::Duration { suffix_span, .. } => *suffix_span,
                crate::AST::EveryArg::WallClock { text_span, .. } => *text_span,
                crate::AST::EveryArg::Expression(expression) => expression.span(),
            };
            diags.push(e0926_bad_schedule_value(reason, span));
            diags
        }
    }
}
/// Project the sema-checked bundle into the shared named-job registry.
///
/// `Func::every.resolved` is populated by `check_every_marker` before this
/// projection is consumed. The registry only carries the stable presentation
/// shape; runtime schedule consumers continue reading the checked AST fact.
pub fn checked_job_registry(
    bundle: &crate::AST::ProgramBundle,
) -> jet_foundation::CLISchema::JobRegistry {
    jet_foundation::CLISchema::JobRegistry::from_bundle(bundle)
}
