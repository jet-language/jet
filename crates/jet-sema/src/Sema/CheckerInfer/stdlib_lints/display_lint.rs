use crate::AST::{Expr, Type};
use crate::Diagnostics::{Span, TextEdit};
use crate::Generics;
use crate::Sema::Diagnostics::is_core_shown_type;
use crate::Sema::Checker;

/// D-DISPLAY-SHAPE: only user auto-printable structs with a Debug projection
/// participate in the temporary bare-interpolation migration. Core display
/// projections and generated checker bodies have no user-owned edit site.
pub(crate) fn is_display_migration_candidate(checker: &Checker<'_>, ty: &Type) -> bool {
    matches!(
        ty,
        Type::Named(name)
            if checker.registry.is_user_struct(name)
                && !is_core_shown_type(name)
                && checker.trait_reg.auto_printable.contains(name)
                && checker.trait_reg.implements_trait(name, Generics::DEBUG)
                && !checker.trait_reg.implements_trait(name, Generics::DISPLAY)
    )
}

/// Return a source span that belongs to an authored expression, not a
/// compiler-generated body or an empty recovery node.
fn authored_expr_span(checker: &Checker<'_>, expr: &Expr) -> Option<Span> {
    if checker.compiler_generated {
        return None;
    }
    let span = expr.span();
    if span.start >= span.end {
        return None;
    }
    checker
        .source
        .get(span.start..span.end)
        .filter(|source| !source.trim().is_empty())
        .map(|_| span)
}

/// Prefix an authored expression with Jet's canonical explicit-copy sigil.
pub(crate) fn explicit_copy_edit(checker: &Checker<'_>, expr: &Expr) -> Option<TextEdit> {
    let span = authored_expr_span(checker, expr)?;
    Some(TextEdit {
        span: Span::new(span.start, span.start),
        new_text: crate::Syntax::SIGIL_COPY.to_string(),
    })
}

/// Replace a bare authored interpolation expression with its explicit Debug
/// selector. The edit is only offered when the source expression is real.
pub(crate) fn debug_interpolation_edit(
    checker: &Checker<'_>,
    expr: &Expr,
    selector: &str,
) -> Option<TextEdit> {
    let span = authored_expr_span(checker, expr)?;
    let source = checker.source.get(span.start..span.end)?;
    Some(TextEdit {
        span,
        new_text: format!("{source}:{selector}"),
    })
}
