//! D-APILABEL1=A — the one call binder.
//!
//! Every call form (free function, method, constructor, generic, variadic,
//! function value) resolves its arguments here. A written label binds by
//! **name**, so a caller may skip a default and may write the labelled
//! arguments in any order. `/` and `*` decide whether a label is forbidden,
//! optional, or required.
//!
//! The binder rewrites `args` into declaration order and appends the defaults
//! for parameters the caller left unbound. It returns the **source evaluation
//! order** so lowering can keep the ratified rule: supplied expressions run
//! left to right in the order they were written, and unbound defaults run
//! afterwards in declaration order.

use crate::AST::{CallArg, ParamZone};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Diagnostics::{
    binder_ambiguous_call, binder_ambiguous_positional, binder_label_forbidden,
    binder_label_required, binder_missing_argument, binder_repeated_label,
    binder_unknown_label,
};

/// One parameter's public call contract, as the binder needs to see it.
pub(crate) struct BindParam<'a> {
    /// The label a caller writes (`Param::call_label`).
    pub label: &'a str,
    /// The declaration-local name a default expression may reference. The
    /// binder rewrites that reference to a private slot temp; it never copies
    /// a supplied argument AST into the default.
    pub name: &'a str,
    pub zone: ParamZone,
    /// D-NARG-D2: the `= expr` default, already registered on the signature.
    pub default: Option<&'a crate::AST::Expr>,
    pub convention: crate::AST::AccessConvention,
    /// Declaration type used to type compiler-private default references.
    pub ty: Option<&'a crate::AST::Type>,
    /// D-VARIADIC1: a rest parameter. It collects the trailing arguments, so
    /// it is never "missing" and never carries a default.
    pub variadic: bool,
    /// D-APILABEL1=A: a Core library parameter's declared default. Core
    /// signatures are a table rather than Jet source, so the default is given
    /// as a shape the binder builds rather than as a parsed expression.
    pub core_default: Option<crate::Sema::CheckerCoreLib::CoreDefault>,
}

impl BindParam<'_> {
    /// True when a call may leave this parameter unbound.
    fn optional(&self) -> bool {
        self.default.is_some() || self.core_default.is_some() || self.variadic
    }

    /// The expression that fills this parameter when a call skips it.
    fn default_expr(&self, span: Span) -> Option<crate::AST::Expr> {
        if let Some(expr) = self.default {
            return Some(expr.clone());
        }
        self.core_default.map(|default| default.build(span))
    }
}

/// What the binder decided for one argument slot, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgSource {
    /// The caller wrote this argument; the payload is its index in the
    /// original source-order argument list.
    Written(usize),
    /// The caller omitted it, so the parameter's default fills the slot.
    Default,
}

/// The binding for one call.
#[derive(Debug, Clone)]
pub(crate) struct Binding {
    /// Parallel to the rewritten `args`: where each slot came from.
    pub sources: Vec<ArgSource>,
}

/// A callable body supplied by an interop resolver. Jet user definitions stay
/// unique (D-CAP10); this seam is for imported overload sets such as C++.
// Dead outside tests until the C++ overload consumer lands (card #2042). Jet's
// own callables are unique by D-CAP10, so nothing in-tree calls this yet; the
// seam exists for imported overload sets and is exercised by the unit tests
// below. Delete it, not this attribute, if that card is dropped.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct CallableCandidate<'a> {
    pub params: &'a [BindParam<'a>],
    pub signature: &'a str,
}

impl Binding {
    /// True when evaluating the rewritten list left to right already matches
    /// the ratified order, so lowering needs no temporaries.
    ///
    /// Two things can break it. The written arguments can be out of the order
    /// the caller wrote them; and a filled default can sit *before* a written
    /// argument, because the rule is that every supplied expression runs first
    /// and the unbound defaults run afterwards in declaration order.
    pub fn is_source_ordered(&self) -> bool {
        let mut last = 0usize;
        let mut seen_default = false;
        for source in &self.sources {
            match source {
                ArgSource::Written(index) => {
                    if seen_default || *index < last {
                        return false;
                    }
                    last = *index;
                }
                ArgSource::Default => seen_default = true,
            }
        }
        true
    }

}

/// Read the public call contract off a registered free-function signature.
pub(crate) fn bind_params_from_sig(sig: &crate::AST::FuncSig) -> Vec<BindParam<'_>> {
    (0..sig.param_info.len())
        .map(|index| BindParam {
            label: sig
                .param_call
                .get(index)
                .map(|(label, _)| label.as_str())
                .unwrap_or(sig.param_info[index].0.as_str()),
            name: sig.param_info[index].0.as_str(),
            zone: sig
                .param_call
                .get(index)
                .map(|(_, zone)| *zone)
                .unwrap_or(ParamZone::Either),
            default: sig.defaults.get(index).and_then(|d| d.as_ref()),
            convention: sig
                .params
                .get(index)
                .map(|(convention, _)| *convention)
                .unwrap_or(crate::AST::AccessConvention::Read),
            ty: sig.params.get(index).map(|(_, ty)| ty),
            variadic: sig.param_variadic.get(index).copied().unwrap_or(false),
            core_default: None,
        })
        .collect()
}

/// Bind `args` to `params`, rewriting `args` into declaration order.
///
/// `callee` names the function in diagnostics. Returns `None` when the call
/// could not be bound at all (a diagnostic was already reported and `args` is
/// left as written, so the caller can still infer each argument's type).
///
/// Arity is **not** checked here — the existing per-call arity diagnostics keep
/// their wording and spans. The binder only reports label-contract failures and
/// unbound required parameters.
pub(crate) fn bind_call_args(
    callee: &str,
    params: &[BindParam<'_>],
    args: &mut Vec<CallArg>,
    call_span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Binding> {
    // A variadic parameter is one declaration slot but may receive many
    // written arguments. Keep every source index instead of collapsing the
    // tail to its first argument; the variadic normalizer packs the tail only
    // after the call has been bound.
    let mut slots: Vec<Vec<usize>> = vec![Vec::new(); params.len()];
    let mut first_label: Option<Span> = None;
    let mut next_positional = 0usize;
    let mut ok = true;

    for (index, arg) in args.iter().enumerate() {
        match &arg.label {
            None => {
                // D-APILABEL1=A: once a label appears, every later argument
                // must carry one — otherwise which parameter it fills depends
                // on how many labels came before it.
                if first_label.is_some() {
                    // One report per call: every later bare argument has the
                    // same cause, and repeating it just buries the fix.
                    if ok {
                        diags.push(binder_ambiguous_positional(callee, arg.span));
                    }
                    ok = false;
                    continue;
                }
                // Positional arguments fill declaration order from the left.
                // Once the fixed parameters are full, a final variadic slot
                // owns every remaining bare argument.
                while next_positional < params.len()
                    && !params[next_positional].variadic
                    && !slots[next_positional].is_empty()
                {
                    next_positional += 1;
                }
                match params.get(next_positional) {
                    Some(param) if param.zone == ParamZone::LabelOnly => {
                        diags.push(binder_label_required(callee, param.label, arg.span));
                        ok = false;
                        // Keep recovery aligned with the written argument
                        // position so consecutive label-only omissions report
                        // each parameter's registered label.
                        if !param.variadic {
                            next_positional += 1;
                        }
                    }
                    Some(param) => {
                        slots[next_positional].push(index);
                        if !param.variadic {
                            next_positional += 1;
                        }
                    }
                    // Arity is reported by the caller's own check.
                    None => {}
                }
            }
            Some((label, label_span)) => {
                if first_label.is_none() {
                    first_label = Some(*label_span);
                }
                let Some(position) = params.iter().position(|p| p.label == label) else {
                    let callable: Vec<&str> = params
                        .iter()
                        .filter(|param| param.zone != ParamZone::PositionalOnly)
                        .map(|param| param.label)
                        .collect();
                    diags.push(binder_unknown_label(callee, label, &callable, *label_span));
                    ok = false;
                    continue;
                };
                if params[position].zone == ParamZone::PositionalOnly {
                    diags.push(binder_label_forbidden(callee, label, *label_span));
                    ok = false;
                    continue;
                }
                if !slots[position].is_empty() {
                    diags.push(binder_repeated_label(callee, label, *label_span));
                    ok = false;
                    continue;
                }
                slots[position].push(index);
            }
        }
    }

    if !ok {
        return None;
    }

    let missing: Vec<usize> = slots
        .iter()
        .enumerate()
        .filter(|(position, slot)| slot.is_empty() && !params[*position].optional())
        .map(|(position, _)| position)
        .collect();

    // A short *positional* call is an arity problem, and the caller's
    // count-based diagnostic says it better than a per-parameter one. Leave the
    // arguments exactly as written so that count is the count the user typed,
    // and fill no defaults — a default belongs to its own parameter, and
    // placing one while an earlier slot is empty would silently shift it into
    // the wrong position.
    let labelled = args.iter().any(|arg| arg.label.is_some());
    if !missing.is_empty()
        && !labelled
        && missing
            .iter()
            .all(|position| params[*position].zone != ParamZone::LabelOnly)
    {
        return Some(Binding {
            sources: (0..args.len()).map(ArgSource::Written).collect(),
        });
    }

    for position in missing {
        diags.push(binder_missing_argument(callee, params[position].label, call_span));
        ok = false;
    }
    if !ok {
        return None;
    }

    Some(rewrite(params, args, &slots, call_span))
}

/// Bind a call against every candidate body, then keep exactly one successful
/// binding. The resolver supplies the type check after binding; the binder
/// never changes a positional slot to make a type fit.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn bind_call_candidate_set<F>(
    callee: &str,
    candidates: &[CallableCandidate<'_>],
    args: &mut Vec<CallArg>,
    call_span: Span,
    diags: &mut Vec<Diagnostic>,
    mut candidate_types_match: F,
) -> Option<Binding>
where
    F: FnMut(usize, &[CallArg]) -> bool,
{
    let mut successes = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let mut trial_args = args.clone();
        let mut trial_diags = Vec::new();
        let Some(binding) = bind_call_args(
            callee,
            candidate.params,
            &mut trial_args,
            call_span,
            &mut trial_diags,
        ) else {
            continue;
        };
        if !candidate_arity_matches(candidate.params, &binding)
            || !candidate_types_match(index, &trial_args)
        {
            continue;
        }
        successes.push((index, binding, trial_args));
    }

    match successes.len() {
        0 => {
            // Preserve today's binder diagnostics when no candidate binds.
            if let Some(candidate) = candidates.first() {
                let _ = bind_call_args(
                    callee,
                    candidate.params,
                    args,
                    call_span,
                    diags,
                );
            }
            None
        }
        1 => {
            let (_, binding, trial_args) = successes.pop().expect("one candidate succeeded");
            *args = trial_args;
            Some(binding)
        }
        _ => {
            let signatures: Vec<&str> = successes
                .iter()
                .map(|(index, _, _)| candidates[*index].signature)
                .collect();
            let (_, first_binding, first_args) = &successes[0];
            let rewrite = labeled_rewrite(first_args, first_binding);
            diags.push(binder_ambiguous_call(
                callee,
                &signatures,
                &rewrite,
                call_span,
            ));
            None
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn candidate_arity_matches(params: &[BindParam<'_>], binding: &Binding) -> bool {
    if params.last().is_some_and(|param| param.variadic) {
        binding.sources.len() >= params.len().saturating_sub(1)
    } else {
        binding.sources.len() == params.len()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn labeled_rewrite(args: &[CallArg], binding: &Binding) -> String {
    args.iter()
        .zip(&binding.sources)
        .filter_map(|(arg, source)| match source {
            ArgSource::Written(_) => arg
                .flags
                .binder_label
                .as_deref()
                .map(|label| format!("{label}: …")),
            ArgSource::Default => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reorder `args` into declaration order and fill every unbound parameter with
/// its default. Defaults run after the supplied arguments, in declaration
/// order. A default reference names a private declaration-slot temp, so a
/// supplied side-effect expression is evaluated once by source-order lowering
/// and is never cloned into the default (D-NARG-D2).
fn rewrite(
    params: &[BindParam<'_>],
    args: &mut Vec<CallArg>,
    slots: &[Vec<usize>],
    call_span: Span,
) -> Binding {
    // Arguments the binder never placed (an arity error the caller reports)
    // keep their written order after the bound prefix.
    let mut taken: Vec<Option<CallArg>> = args.drain(..).map(Some).collect();
    let mut sources = Vec::with_capacity(params.len());

    for (position, indices) in slots.iter().enumerate() {
        if !indices.is_empty() {
            for index in indices {
                let mut arg = taken[*index]
                    .take()
                    .expect("each argument binds to at most one parameter");
                // Lowering reads this back to keep the ratified evaluation
                // order across the reorder the binder just performed.
                arg.flags.source_index = Some(*index);
                arg.flags.binder_slot = Some(position);
                arg.flags.binder_site = Some(call_span.start as u32);
                arg.flags.binder_label = Some(params[position].label.to_string());
                args.push(arg);
                sources.push(ArgSource::Written(*index));
            }
            continue;
        }
        let Some(default) = params[position].default_expr(call_span) else {
            // A variadic rest parameter, or a slot a diagnostic already
            // covered. Nothing to place.
            continue;
        };
        let earlier: Vec<(String, String)> = params
            .iter()
            .take(position)
            .enumerate()
            .map(|(slot, p)| {
                (
                    p.name.to_string(),
                    jet_foundation::Names::mangle_generated(&format!(
                        "binder_ref_{}_{}",
                        call_span.start,
                        slot
                    )),
                )
            })
            .collect();
        let ref_pairs: Vec<(&str, String)> = earlier
            .iter()
            .map(|(name, replacement)| (name.as_str(), replacement.clone()))
            .collect();
        let binder_refs = earlier
            .iter()
            .enumerate()
            .filter_map(|(slot, (_, replacement))| {
                params
                    .get(slot)
                    .and_then(|param| param.ty)
                    .map(|ty| (replacement.clone(), slot, ty.clone()))
            })
            .collect();
        args.push(CallArg {
            convention: params[position].convention,
            expr: super::substitute_param_refs(default, &ref_pairs),
            span: call_span,
            flags: crate::AST::CallArgFlags {
                binder_slot: Some(position),
                binder_refs,
                binder_site: Some(call_span.start as u32),
                ..Default::default()
            },
            label: None,
            spread: false,
        });
        sources.push(ArgSource::Default);
    }
    for (index, arg) in taken.into_iter().enumerate() {
        if let Some(arg) = arg {
            args.push(arg);
            // This is a written argument that had no declaration slot. Keep
            // its source identity so the normal arity diagnostic does not
            // turn it into a fake default and source-order lowering stays
            // truthful.
            sources.push(ArgSource::Written(index));
        }
    }
    let binding = Binding { sources };
    let has_inserted_slot = args
        .iter()
        .any(|arg| arg.flags.binder_slot.is_some() && arg.flags.source_index.is_none());
    if binding.is_source_ordered() && !has_inserted_slot {
        // Nothing moved, so lowering needs no temporaries. Clearing the marks
        // keeps the ordinary call shape identical to before the binder ran.
        for arg in args.iter_mut() {
            arg.flags.source_index = None;
        }
    }
    binding
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::{AccessConvention, Expr};

    fn param(label: &'static str) -> BindParam<'static> {
        BindParam {
            label,
            name: label,
            zone: ParamZone::Either,
            default: None,
            convention: AccessConvention::Read,
            ty: None,
            variadic: false,
            core_default: None,
        }
    }

    fn arg(start: usize) -> CallArg {
        let span = Span::new(start, start + 1);
        CallArg {
            convention: AccessConvention::Read,
            expr: Expr::Ident(format!("value_{start}"), span),
            span,
            flags: Default::default(),
            label: None,
            spread: false,
        }
    }

    #[test]
    fn candidate_set_reports_ambiguous_bare_call_with_labeled_fix() {
        let first = vec![param("name"), param("text")];
        let second = vec![param("key"), param("id")];
        let candidates = [
            CallableCandidate {
                params: &first,
                signature: "put(name: String, text: String)",
            },
            CallableCandidate {
                params: &second,
                signature: "put(key: String, id: String)",
            },
        ];
        let mut args = vec![arg(4), arg(12)];
        let mut diags = Vec::new();

        assert!(bind_call_candidate_set(
            "put",
            &candidates,
            &mut args,
            Span::new(0, 16),
            &mut diags,
            |_, _| true,
        )
        .is_none());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "E0772");
        assert!(diags[0].why.contains("put(name: String, text: String)"));
        assert!(diags[0].why.contains("put(key: String, id: String)"));
        assert!(diags[0].fix.contains("put(name: …, text: …)"));
    }

    #[test]
    fn candidate_type_check_runs_after_binding_and_selects_one_body() {
        let first = vec![param("name"), param("text")];
        let second = vec![param("key"), param("id")];
        let candidates = [
            CallableCandidate {
                params: &first,
                signature: "put(name: String, text: String)",
            },
            CallableCandidate {
                params: &second,
                signature: "put(key: String, id: Int)",
            },
        ];
        let mut args = vec![arg(4), arg(12)];
        let mut diags = Vec::new();

        assert!(bind_call_candidate_set(
            "put",
            &candidates,
            &mut args,
            Span::new(0, 16),
            &mut diags,
            |index, bound_args| index == 1 && bound_args.len() == 2,
        )
        .is_some());
        assert!(diags.is_empty());
        assert_eq!(args[0].flags.binder_label.as_deref(), Some("key"));
        assert_eq!(args[1].flags.binder_label.as_deref(), Some("id"));
    }
}
