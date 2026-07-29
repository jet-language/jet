//! D-SCHEDULE1 (ratified 2026-07-11, card #505): checked `#Every(…)`
//! schedule arguments, plus D-JPK-TASKRUN1 reserved-name law on `#Job fn`.
//!
//! The parser only records the raw shape it saw (`EveryArg` — a duration
//! literal or a quoted string) and, at parse time, whether `#Every(…)` is
//! paired with `#Job` on the same function (E0925, pushed directly by
//! `jet-parser` — a placement question, not a value question). This module
//! owns the one thing left for schedules: is the VALUE a real schedule?
//! `EveryArg::resolve` (`crates/jet-foundation/src/AST/items.rs`) is the
//! single source of truth for that arithmetic/range check — `jet dev`, the
//! service runtime, and a jetos timer projection all call the same function
//! later to get the same answer, so this checker and every runtime consumer
//! can never disagree.
//!
//! D-JPK-TASKRUN1 also lives here: a `#Job fn` must not reuse the reserved
//! lifecycle verbs `run`/`dev`/`build`/`test` (E0928).
//!
//! I3: this module only decides; codegen never reads `Func::every` at all —
//! a `#Job`/`#Every` function generates as an ordinary fn.

use crate::AST::{EveryScheduleError, Func};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

/// E0926: `#Every(…)`'s value isn't a real schedule — a bad duration unit,
/// a non-positive duration, or a malformed/out-of-range `"HH:MM"`.
fn e0926_bad_schedule_value(reason: EveryScheduleError, span: Span) -> Diagnostic {
    let (what, why, fix) = match reason {
        EveryScheduleError::UnknownDurationUnit => (
            "this duration's unit isn't a recognized schedule cadence",
            "a schedule's repeat interval is one of a closed set of time units — \
             `ns`/`us`/`ms`/`s`/`min` — not an arbitrary `#UnitFamily` member.",
            "use `ns`, `us`, `ms`, `s`, or `min` (e.g. `#Every(5min)`).",
        ),
        EveryScheduleError::NonPositiveDuration => (
            "a schedule interval must be a positive duration",
            "`#Every(0ms)` or a negative duration never becomes due — it isn't a real cadence.",
            "write a duration greater than zero, e.g. `#Every(5min)`.",
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
            "a task schedule must be known at compile time",
            "the marker argument has the right type, but task discovery needs one fixed cadence.",
            "write a duration or wall-clock literal, e.g. `#Every(5min)` or `#Every(\"03:00\")`.",
        ),
    };
    Diagnostic::error("E0926", what.to_string(), why.to_string(), fix.to_string(), Some(span))
}

/// E0928: `#Job fn` reused a reserved lifecycle verb (D-JPK-TASKRUN1).
fn e0928_reserved_task_name(name: &str, span: Span) -> Diagnostic {
    let reserved = Syntax::TASK_RESERVED_LIFECYCLE.join(", ");
    Diagnostic::error(
        "E0928",
        format!("`{name}` is a built-in lifecycle verb, not a task name"),
        format!(
            "`run`, `dev`, `build`, and `test` already name Jet's built-in entry points — \
             a `#Job fn` picks a user-chosen verb beside them (D-JPK-TASKRUN1)."
        ),
        format!(
            "rename it, e.g. `#Job fn {name}_assets()`, or drop `#Job` if this is the lifecycle entry."
        ),
        Some(span),
    )
    .with_detail(format!("reserved: {reserved}\n"))
}

/// D-JPK-TASKRUN1: reject `#Job fn run|dev|build|test`. Called alongside the
/// `#Every` value check during registration.
pub(crate) fn check_task_marker(f: &Func) -> Vec<Diagnostic> {
    if !f.is_task {
        return Vec::new();
    }
    if Syntax::TASK_RESERVED_LIFECYCLE.contains(&f.name.as_str()) {
        let span = f.task_span.unwrap_or(f.name_span);
        return vec![e0928_reserved_task_name(&f.name, span)];
    }
    Vec::new()
}

/// D-SCHEDULE1: validate `f`'s `#Every(…)` argument, if it has one. Called
/// once per function during registration (mirrors `check_inline_always_fn`'s
/// call sites in `Registration.rs`/`Bundle.rs`) — E0925 placement is already
/// handled by the parser, so this is the value check alone. Also runs the
/// D-JPK-TASKRUN1 reserved-name check for `#Job`.
pub(crate) fn check_every_marker(f: &Func) -> Vec<Diagnostic> {
    let mut diags = check_task_marker(f);
    let Some(every) = &f.every else {
        return diags;
    };
    match every.arg.resolve() {
        Ok(_) => diags,
        Err(reason) => {
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
