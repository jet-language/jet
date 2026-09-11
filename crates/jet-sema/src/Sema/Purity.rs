use crate::Diagnostics::Diagnostic;

/// `call_name` is the callee; `path` is the chain of calls that led here
/// (for the trace message).
pub fn e3401(
    pure_fn_name: &str,
    call_name: &str,
    path: &[String],
    span: crate::Diagnostics::Span,
) -> Diagnostic {
    let why = if path.is_empty() {
        format!(
            "`{}` is impure, but `{}` declares `-[]>`",
            call_name, pure_fn_name
        )
    } else {
        format!(
            "{} calls `{}`, which is impure — the whole call chain must be pure inside `{}`",
            path.join(" → "),
            call_name,
            pure_fn_name
        )
    };
    let mut call_chain = Vec::with_capacity(path.len() + 2);
    call_chain.push(pure_fn_name.to_string());
    for component in path {
        if call_chain.last().map(String::as_str) != Some(component.as_str()) {
            call_chain.push(component.clone());
        }
    }
    if call_chain.last().map(String::as_str) != Some(call_name) {
        call_chain.push(call_name.to_string());
    }
    Diagnostic::error(
        "E3401",
        format!(
            "`{}` calls the impure function `{}`",
            pure_fn_name, call_name
        ),
        why,
        format!(
            "give `{}` an explicit `-[]>` bound, or remove the call from `{}`",
            call_name, pure_fn_name
        ),
        Some(span),
    )
    .with_rights_chain("pure", call_chain, std::iter::empty::<String>(), None)
}

/// E3402: ambient I/O or network access attempted during a sandboxed package build.
pub fn e3402(call_name: &str, span: Option<crate::Diagnostics::Span>) -> Diagnostic {
    Diagnostic::error(
        "E3402",
        format!(
            "`{}` is not allowed during a sandboxed package build",
            call_name
        ),
        "package builds run with ambient I/O and network access disabled (D-PURE2)".to_string(),
        "compute this value at compile time or pass it in as a parameter".to_string(),
        span,
    )
}

/// E3403: keep the sema-facing constructor while the registered diagnostic
/// wording lives with the foundation diagnostic row.
pub fn e3403(what: &str, span: Option<crate::Diagnostics::Span>) -> Diagnostic {
    Diagnostic::e3403(what, span)
}

