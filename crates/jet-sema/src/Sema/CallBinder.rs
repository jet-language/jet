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

/// One parameter's public call contract, as the binder needs to see it.
pub(crate) struct BindParam<'a> {
    /// The label a caller writes (`Param::call_label`).
    pub label: &'a str,
    /// The local name the body reads. A later parameter's default expression
    /// may reference it, so substitution keys off this and not `label`.
    pub name: &'a str,
    pub zone: ParamZone,
    /// D-NARG-D2: the `= expr` default, already registered on the signature.
    pub default: Option<&'a crate::AST::Expr>,
    pub convention: crate::AST::AccessConvention,
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
pub(crate) struct Binding {
    /// Parallel to the rewritten `args`: where each slot came from.
    pub sources: Vec<ArgSource>,
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
                        diags.push(ambiguous_positional(callee, arg.span));
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
                        diags.push(label_required(callee, param.label, arg.span));
                        ok = false;
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
                    diags.push(unknown_label(callee, label, params, *label_span));
                    ok = false;
                    continue;
                };
                if params[position].zone == ParamZone::PositionalOnly {
                    diags.push(label_forbidden(callee, label, *label_span));
                    ok = false;
                    continue;
                }
                if !slots[position].is_empty() {
                    diags.push(repeated_label(callee, label, *label_span));
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
        diags.push(missing_argument(callee, params[position].label, call_span));
        ok = false;
    }
    if !ok {
        return None;
    }

    Some(rewrite(params, args, &slots, call_span))
}

/// Reorder `args` into declaration order and fill every unbound parameter with
/// its default. Defaults run after the supplied arguments, in declaration
/// order, and may reference an earlier parameter — so each one is substituted
/// against the arguments already placed to its left (D-NARG-D2).
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
        let earlier: Vec<String> = params
            .iter()
            .take(position)
            .map(|p| p.name.to_string())
            .collect();
        args.push(CallArg {
            convention: params[position].convention,
            expr: super::substitute_param_refs(default, &earlier, args),
            span: call_span,
            flags: Default::default(),
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
    if binding.is_source_ordered() {
        // Nothing moved, so lowering needs no temporaries. Clearing the marks
        // keeps the ordinary call shape identical to before the binder ran.
        for arg in args.iter_mut() {
            arg.flags.source_index = None;
        }
    }
    binding
}

// ── diagnostics (D-APILABEL1=A) ──────────────────────────────────────────────

fn unknown_label(
    callee: &str,
    label: &str,
    params: &[BindParam<'_>],
    span: Span,
) -> Diagnostic {
    let callable: Vec<&str> = params
        .iter()
        .filter(|p| p.zone != ParamZone::PositionalOnly)
        .map(|p| p.label)
        .collect();
    Diagnostic::error(
        "E0764",
        format!("`{callee}` has no parameter labelled `{label}`"),
        "a label binds an argument to the parameter of that name".to_string(),
        if callable.is_empty() {
            format!("`{callee}` takes no labelled arguments")
        } else {
            format!("`{callee}` accepts `{}`", callable.join("`, `"))
        },
        Some(span),
    )
}

fn repeated_label(callee: &str, label: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0765",
        format!("`{label}:` is written twice in this call to `{callee}`"),
        "each parameter takes exactly one argument".to_string(),
        format!("remove one of the `{label}:` arguments"),
        Some(span),
    )
}

fn missing_argument(callee: &str, label: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0766",
        format!("this call to `{callee}` is missing `{label}`"),
        "`{label}` has no default, so every call has to supply it".replace("{label}", label),
        format!("add `{label}: …` to the call"),
        Some(span),
    )
}

fn label_forbidden(callee: &str, label: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0767",
        format!("`{label}` is a positional-only parameter of `{callee}`"),
        "the `/` in the declaration keeps these parameters positional, so their names stay free to change"
            .to_string(),
        format!("drop the `{label}:` label and pass the value by position"),
        Some(span),
    )
}

fn ambiguous_positional(callee: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0768",
        format!("this argument to `{callee}` follows a labelled one without a label"),
        "labels bind by name, so a bare argument after one has no parameter to fill".to_string(),
        "label this argument, or move it before the labelled ones".to_string(),
        Some(span),
    )
}

fn label_required(callee: &str, label: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0769",
        format!("`{label}` is a label-only parameter of `{callee}`"),
        "the `*` in the declaration requires the label, so the call says what the value means"
            .to_string(),
        format!("write `{label}: …` for this argument"),
        Some(span),
    )
}
