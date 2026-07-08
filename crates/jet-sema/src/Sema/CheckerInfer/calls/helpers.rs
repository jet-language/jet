/// D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): E1313 — a trait-bounded variadic
/// call-site argument doesn't implement one of the bound trait(s).
fn e1313(
    arg_ty_name: &str,
    trait_name: &str,
    param_name: &str,
    fn_name: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::error(
        "E1313",
        format!("`{arg_ty_name}` doesn't implement `{trait_name}`"),
        format!(
            "`{param_name}: ...{trait_name}` checks every argument against `{trait_name}` — \
             that's how `{fn_name}` accepts a mix of types safely"
        ),
        format!(
            "implement `{trait_name}` for `{arg_ty_name}`'s type, or drop the value from this call"
        ),
        Some(span),
    )
}
