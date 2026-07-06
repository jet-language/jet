//! U12: field-check one dev-supervised `Service` record (an entry in an
//! `env.<name>` role-module's `services: { … }` map) and capture it as a
//! `DevServicePlan`.
//!
//! Mirrors `System::evaluate_service` (same ratified `Service` grammar — open
//! record, required `enable: Bool`, E0975 reused verbatim) but produces a
//! distinct type: `Jetpack::Services` (the dev-runtime tier, std::process-based)
//! is the only consumer, and it owns 100% of a dev service's semantics, unlike
//! the jetos capture (`ServicePlan`), which is inert until Phase D realization.
//! So the *recognized* fields (`ports`/`init`/`shutdown`/`data_dir`/`ready`) are
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
use super::Types::DevServicePlan;

/// U12: evaluate one `name: { … }` entry from an `env.<name>` role-module's
/// `services:` map into a `DevServicePlan`.
pub(super) fn evaluate_dev_service(
    entry: &ServiceEntry,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<DevServicePlan, Diagnostic> {
    let mut plan = DevServicePlan {
        name: entry.name.clone(),
        ..Default::default()
    };
    let mut enable = None;
    for (name, span, value) in &entry.fields {
        check_build_io(value)?;
        let v = Comptime::evaluate(value, funcs, &HashSet::new(), base_dir, &HashMap::new())?;
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
        } else if name == Syntax::DEV_SERVICE_FIELD_INIT {
            set_or_extra(&mut plan.init, name, &v, &mut plan.extra);
        } else if name == Syntax::DEV_SERVICE_FIELD_SHUTDOWN {
            set_or_extra(&mut plan.shutdown, name, &v, &mut plan.extra);
        } else if name == Syntax::DEV_SERVICE_FIELD_DATA_DIR {
            set_or_extra(&mut plan.data_dir, name, &v, &mut plan.extra);
        } else if name == Syntax::DEV_SERVICE_FIELD_READY {
            set_or_extra(&mut plan.ready, name, &v, &mut plan.extra);
        } else {
            plan.extra.push((name.clone(), v.jet_show()));
        }
    }
    plan.enable = enable.ok_or_else(|| service_missing_enable(&entry.name, entry.span))?;
    Ok(plan)
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
