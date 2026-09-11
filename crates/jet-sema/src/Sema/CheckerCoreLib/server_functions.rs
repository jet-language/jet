//! D-DX-SERVERFN1=A: front-end checks for the one server-function wire
//! boundary. App discovery supplies the function identity; serde_diags owns
//! the serializability truth so this module never creates a second type table.

use crate::AST::{FailureContract, Func, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Traits::TraitRegistry;

fn wire_type_ok(ty: &Type, reg: &TraitRegistry) -> bool {
    match ty {
        Type::Result { ok, err } => {
            super::is_encodable_ty(ok, reg)
                && super::is_decodable_ty(ok, reg)
                && super::is_encodable_ty(err, reg)
                && super::is_decodable_ty(err, reg)
        }
        _ => super::is_encodable_ty(ty, reg) && super::is_decodable_ty(ty, reg),
    }
}

fn wire_error(
    function: &Func,
    direction: &str,
    ty: &Type,
    span: Span,
) -> Diagnostic {
    Diagnostic::error(
        "E2411",
        format!(
            "server function `{}` {} type `{}` is not serializable",
            function.name,
            direction,
            ty.show()
        ),
        "server functions cross a checked JSON boundary for both scripted calls and native forms".to_string(),
        "derive Encode and Decode for the type, or use a supported wire type".to_string(),
        Some(span),
    )
}

/// Validate every user-visible side of a server-function signature.
///
/// A parameter is both encoded by the client and decoded by the server. The
/// success and declared failure values are both encoded by the server and
/// decoded by the client. The shared serde helpers remain authoritative for
/// the recursive type rules and local derive requirements.
pub(crate) fn validate_server_function_boundary(
    function: &Func,
    reg: &TraitRegistry,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for parameter in function
        .params
        .iter()
        .filter(|parameter| parameter.name != crate::Syntax::KW_SELF)
    {
        if !wire_type_ok(&parameter.ty, reg) {
            diagnostics.push(wire_error(function, "input", &parameter.ty, parameter.ty_span));
        }
    }

    match function.failure_contract() {
        FailureContract::Default { success, error }
        | FailureContract::Explicit { success, error } => {
            if !wire_type_ok(&success, reg) {
                diagnostics.push(wire_error(
                    function,
                    "output",
                    &success,
                    function.return_type_span.unwrap_or(function.name_span),
                ));
            }
            if !wire_type_ok(&error, reg) {
                diagnostics.push(wire_error(
                    function,
                    "error",
                    &error,
                    function.return_type_span.unwrap_or(function.name_span),
                ));
            }
        }
        FailureContract::Optional { success }
        | FailureContract::ProvenUnreachable { success }
        | FailureContract::Converted { success, .. } => {
            if !wire_type_ok(&success, reg) {
                diagnostics.push(wire_error(
                    function,
                    "output",
                    &success,
                    function.return_type_span.unwrap_or(function.name_span),
                ));
            }
        }
        FailureContract::DeclaredNever => {}
    }
    diagnostics
}
