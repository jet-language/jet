//! D-BIGINT1 / D-DECIMAL1: arbitrary-precision `BigInt` and base-10 `Decimal`.
//! Shared name/method tables for sema and codegen.

use crate::AST::{Expr, Marker, Type};
use crate::Syntax;

pub const MONEY_LINT_NAMES: &[&str] =
    &["price", "cost", "amount", "total", "fee", "balance", "tax"];

pub fn is_bigint_type_name(name: &str) -> bool {
    name == Syntax::TYPE_BIGINT
}

pub fn is_decimal_type_name(name: &str) -> bool {
    name == Syntax::TYPE_DECIMAL
}

pub fn is_precise_numeric_type_name(name: &str) -> bool {
    is_bigint_type_name(name) || is_decimal_type_name(name)
}

pub fn type_is_bigint(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if is_bigint_type_name(n))
}

pub fn type_is_decimal(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if is_decimal_type_name(n))
}

pub fn is_money_like_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    MONEY_LINT_NAMES
        .iter()
        .any(|m| lower.contains(m))
}

/// D-DECIMAL1: `#[allow(float_money)]` suppresses the default-on money lint.
pub fn allows_float_money(markers: &[Marker]) -> bool {
    markers.iter().any(|m| {
        m.name == "allow"
            && m.args.iter().any(|a| {
                matches!(a, Expr::Ident(s, _) if s == "float_money")
            })
    })
}

pub fn bigint_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    let bigint = || Type::Named(Syntax::TYPE_BIGINT.to_string());
    match (method, nargs) {
        ("add" | "sub" | "mul", 1) => Some(Some(bigint())),
        ("neg", 0) => Some(Some(bigint())),
        ("to_string", 0) => Some(Some(Type::String)),
        _ => None,
    }
}

pub fn decimal_method_return(method: &str, nargs: usize) -> Option<Option<Type>> {
    let decimal = || Type::Named(Syntax::TYPE_DECIMAL.to_string());
    match (method, nargs) {
        ("add" | "sub" | "mul", 1) => Some(Some(decimal())),
        ("to_string", 0) => Some(Some(Type::String)),
        _ => None,
    }
}
