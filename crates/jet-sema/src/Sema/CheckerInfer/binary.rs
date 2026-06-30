//! Type inference: binary operators and overflow/op-mismatch checks.
//!
//! Split out of the original `CheckerInfer.rs`; behavior unchanged.

use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::COMPARABLE;
use crate::AST::{BinOp, Expr, Pattern, Type};

impl<'a> Checker<'a> {
    /// Binary operators and type checking.
    pub(crate) fn infer_binary(
        &mut self,
        op: BinOp,
        lhs: &mut Box<Expr>,
        rhs: &mut Box<Expr>,
        span: Span,
    ) -> Option<Type> {
        if matches!(op, BinOp::And | BinOp::Or) {
            let lt = self.infer(lhs);
            if let Some(lt) = lt {
                if lt != Type::Bool {
                    self.diags.push(Diagnostic::error(
                        "E0110",
                        format!(
                            "the left side of `{}` must be {}, but this is {}",
                            op.spell(),
                            Type::Bool.show(),
                            lt.show()
                        ),
                        "logic joins yes/no values".to_string(),
                        "compare the value to something first".to_string(),
                        Some(lhs.span()),
                    ));
                }
            }
            let rt = self.infer(rhs);
            if let Some(rt) = rt {
                if rt != Type::Bool {
                    // D-S25-RETIRE1=A: comparison distribution via `||`/`&&` is gone.
                    // Each side of `||`/`&&` must be a Bool expression.
                    self.diags.push(Diagnostic::error(
                        "E0110",
                        format!(
                            "the right side of `{}` must be {}, but this is {}",
                            op.spell(),
                            Type::Bool.show(),
                            rt.show()
                        ),
                        "each side of a logic operator must be a yes/no comparison".to_string(),
                        "write the full comparison, e.g. `x == 2`".to_string(),
                        Some(rhs.span()),
                    ));
                }
            }
            return Some(Type::Bool);
        }

        let lt = self.infer(lhs);

        // S31: pattern-shaped `==` before RHS name lookup.
        if op == BinOp::Eq {
            let subj_name = match lhs.as_ref() {
                Expr::Ident(n, _) => Some(n.as_str()),
                _ => None,
            };
            if let Some(lt) = &lt {
                if let Some(pattern) = self.eq_unit_variant_pattern(lhs, rhs, subj_name, lt) {
                    self.validate_pattern(lt, &pattern, span);
                    return Some(Type::Bool);
                }
                if let Expr::Ident(name, rhs_span) = rhs.as_ref() {
                    if self.lookup(name).is_none() && !self.consts.contains_key(name) {
                        if matches!(lt, Type::Option(_) | Type::Named(_)) {
                            let pattern = Pattern::Variant {
                                variant: name.clone(),
                                bindings: Vec::new(),
                                span: *rhs_span,
                            };
                            self.validate_pattern(lt, &pattern, span);
                            return Some(Type::Bool);
                        }
                    }
                }
            }
        }

        let rt = self.infer(rhs);
        let (lt, rt) = (lt?, rt?);

        // D-DIST3 (ratified 2026-06-20): distinct type arithmetic rules (E0127/E0128).
        {
            let lt_is_distinct = if let Type::Named(n) = &lt {
                self.registry.is_distinct(n)
            } else {
                false
            };
            let rt_is_distinct = if let Type::Named(n) = &rt {
                self.registry.is_distinct(n)
            } else {
                false
            };
            let is_arith = matches!(
                op,
                BinOp::Add
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::Div
                    | BinOp::Rem
                    | BinOp::BitAnd
                    | BinOp::BitOr
                    | BinOp::BitXor
                    | BinOp::Shl
                    | BinOp::Shr
            );
            let is_eq = matches!(op, BinOp::Eq | BinOp::Ne);
            if (lt_is_distinct || rt_is_distinct) && is_arith {
                let distinct_name = if lt_is_distinct {
                    if let Type::Named(n) = &lt {
                        n.as_str()
                    } else {
                        ""
                    }
                } else {
                    if let Type::Named(n) = &rt {
                        n.as_str()
                    } else {
                        ""
                    }
                };
                if lt != rt {
                    // Arithmetic between different types (or distinct + base) — E0127/E0128.
                    // Primary error is E0127 when the distinct type isn't numeric.
                    if !self.registry.distinct_is_numeric(distinct_name) {
                        self.diags.push(Diagnostic::error(
                            "E0127",
                            format!("operator `{}` is not available on `{}`", op.spell(), distinct_name),
                            format!("`{}` is a distinct type but isn't marked `#Numeric`, so it doesn't inherit arithmetic operators — only `==` is available", distinct_name),
                            format!("add `#Numeric` before the declaration if this is a quantity, or use `.raw()` to work on the underlying value"),
                            Some(span),
                        ));
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0127",
                            format!("operator `{}` is not available between `{}` and `{}`", op.spell(), lt.name(), rt.name()),
                            format!("`#Numeric` distinct types only support arithmetic between the same type"),
                            format!("both sides must be `{}`", distinct_name),
                            Some(span),
                        ));
                    }
                    return None;
                }
                // Same distinct type — check numeric marker.
                if !self.registry.distinct_is_numeric(distinct_name) {
                    self.diags.push(Diagnostic::error(
                        "E0127",
                        format!("operator `{}` is not available on `{}`", op.spell(), distinct_name),
                        format!("`{}` is a distinct type but isn't marked `#Numeric`, so it doesn't inherit arithmetic operators — only `==` is available", distinct_name),
                        "add `#Numeric` before the declaration if this is a quantity, or use `.raw()` to work on the underlying value".to_string(),
                        Some(span),
                    ));
                    return None;
                }
                // Same #Numeric distinct type — arithmetic is allowed, returns the same type.
                return Some(lt);
            }
            if (lt_is_distinct || rt_is_distinct) && is_eq {
                // Equality between two values of the same distinct type: allowed.
                // Equality between distinct and different type: E0128.
                if lt != rt {
                    let dt_name = if lt_is_distinct {
                        if let Type::Named(n) = &lt {
                            n.clone()
                        } else {
                            lt.name()
                        }
                    } else {
                        if let Type::Named(n) = &rt {
                            n.clone()
                        } else {
                            rt.name()
                        }
                    };
                    self.diags.push(Diagnostic::error(
                        "E0128",
                        format!("a `{}` can't be compared with a `{}`", lt.name(), rt.name()),
                        format!(
                            "`{}` is a distinct type; it only compares equal to another `{}`",
                            dt_name, dt_name
                        ),
                        format!(
                            "use `.raw()` to compare the underlying values, or construct a `{}`",
                            dt_name
                        ),
                        Some(span),
                    ));
                    return None;
                }
                return Some(Type::Bool);
            }
            // Implicit coercion check (non-arithmetic, non-eq): handled at assignment.
        }

        // D-SIMD2 / D-LINALG1: operator overloading on the closed built-in math
        // family (lane + linalg types). Element-wise `+`/`-`/`*`/`/`, scalar-free
        // matrix×vector, and `==`/`!=`. Runs before the builtin numeric match (a
        // math type isn't `is_numeric`, so the builtin path would reject it).
        {
            // A user struct sharing a math name isn't part of the closed family;
            // skip the math-operator path so it gets the normal operator handling.
            let lname = match &lt {
                Type::Named(n) if !self.registry.contains(n) => n.clone(),
                _ => String::new(),
            };
            let rname = match &rt {
                Type::Named(n) if !self.registry.contains(n) => n.clone(),
                _ => String::new(),
            };
            if is_math_type(&lname) || is_math_type(&rname) {
                if let Some(result) = math_binop_result(op, &lname, &rname) {
                    return Some(result);
                }
                // A math operand with an unsupported operator/operand pairing — a
                // teaching diagnostic (the closed family has fixed operators).
                self.diags.push(Diagnostic::error(
                    "E2511",
                    format!("operator `{}` isn't defined between `{}` and `{}`", op.spell(), lt.name(), rt.name()),
                    "the built-in math types support element-wise `+`/`-` (and `/` for lanes), `*` (element-wise, or matrix×vector), and `==`".to_string(),
                    "check the operands are the same lane/vector type, or use a method like `.dot()`/`.matmul()`".to_string(),
                    Some(span),
                ));
                return None;
            }
        }

        // D-BIGINT1 / D-DECIMAL1: precise numeric operators (closed family).
        {
            let lname = match &lt {
                Type::Named(n) if !self.registry.contains(n) => n.clone(),
                _ => String::new(),
            };
            let rname = match &rt {
                Type::Named(n) if !self.registry.contains(n) => n.clone(),
                _ => String::new(),
            };
            if crate::Numeric::is_precise_numeric_type_name(&lname)
                || crate::Numeric::is_precise_numeric_type_name(&rname)
            {
                if let Some((code, what, fix)) = precise_mix_error(&lt, &rt) {
                    self.diags.push(Diagnostic::error(
                        code,
                        what,
                        "precise numeric types never mix with fixed-width integers or floats without an explicit constructor".to_string(),
                        fix,
                        Some(span),
                    ));
                    return None;
                }
                if let Some(result) = precise_binop_result(op, &lname, &rname) {
                    return Some(result);
                }
                self.diags.push(Diagnostic::error(
                    "E0133",
                    format!(
                        "operator `{}` isn't defined between `{}` and `{}`",
                        op.spell(),
                        lt.name(),
                        rt.name()
                    ),
                    "`BigInt` and `Decimal` support `+`, `-`, `*`, and `==`/`!=` on matching types only".to_string(),
                    "use a method like `.add(other)` or make both operands the same precise type".to_string(),
                    Some(span),
                ));
                return None;
            }
        }

        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                // D-SG9: arithmetic stays within one numeric type (any width) and
                // keeps that width; widths never mix implicitly.
                if lt == rt && lt.is_numeric() {
                    Some(lt)
                } else if lt == Type::String && op == BinOp::Add {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        "text isn't joined with `+`".to_string(),
                        "there's one way to build text: interpolation (S8)".to_string(),
                        "write the pieces inside one string: \"{a}{b}\"".to_string(),
                        Some(span),
                    ));
                    None
                } else if (lt == Type::Int && rt == Type::Float)
                    || (lt == Type::Float && rt == Type::Int)
                {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        format!(
                            "`{}` can't mix {} and {}",
                            op.spell(),
                            lt.show(),
                            rt.show()
                        ),
                        "Jet never converts numbers silently; the two sides must match"
                            .to_string(),
                        "make both sides the same kind of number (write `2.0` instead of `2`, or drop the `.0`)".to_string(),
                        Some(span),
                    ));
                    None
                } else {
                    self.op_mismatch(op, &lt, &rt, span);
                    None
                }
            }
            BinOp::Shl | BinOp::Shr => {
                // D-NUMOPS1: a shift's left side carries the value/width; the right
                // side is a bit-count, so it may be any integer width and does NOT
                // have to match. The result keeps the left side's type. (Same rule
                // as Rust/C/Swift — `U8 << 1` shifts a `U8` by an `Int` count.)
                if lt.is_integer() && rt.is_integer() {
                    Some(lt)
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        format!(
                            "`{}` works on {} only, but this has {} and {}",
                            op.spell(),
                            Type::Int.show(),
                            lt.show(),
                            rt.show()
                        ),
                        compound_why(op),
                        "use whole numbers here".to_string(),
                        Some(span),
                    ));
                    None
                }
            }
            BinOp::Rem | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                // D-SG9: remainder and the bitwise ops work on any integer width,
                // both sides the same width, and keep it.
                if lt == rt && lt.is_integer() {
                    Some(lt)
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        format!(
                            "`{}` works on {} only, but this has {} and {}",
                            op.spell(),
                            Type::Int.show(),
                            lt.show(),
                            rt.show()
                        ),
                        compound_why(op),
                        "use whole numbers here".to_string(),
                        Some(span),
                    ));
                    None
                }
            }
            BinOp::Eq | BinOp::Ne => {
                if lt == rt {
                    if !types_comparable(&lt, self.registry) {
                        if let Some(field) = incomparable_field(&lt, self.registry) {
                            self.diags.push(Diagnostic::error(
                                "E0312",
                                format!("`{}` can't be compared with `{}` because field `{}` doesn't support `{}`", lt.name(), rt.name(), field, op.spell()),
                                "value equality needs every field to support the comparison".to_string(),
                                "compare individual fields instead".to_string(),
                                Some(span),
                            ));
                        } else {
                            self.op_mismatch(op, &lt, &rt, span);
                        }
                        return None;
                    }
                    // D-SMELLLINT1 / L0502: float `==` is almost always a bug.
                    if matches!(lt, Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::lint(
                            "L0502",
                            format!("comparing floats with `{}` is unreliable", op.spell()),
                            "floating-point arithmetic is inexact; two values computed differently may not be bit-identical even when mathematically equal".to_string(),
                            "compare within a tolerance: `(a - b).abs() < 1e-9`".to_string(),
                            Some(span),
                        ));
                    }
                    Some(Type::Bool)
                } else {
                    self.op_mismatch(op, &lt, &rt, span);
                    None
                }
            }
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                if lt == rt && matches!(lt, Type::Int | Type::Float) {
                    Some(Type::Bool)
                } else if lt == rt
                    && (types_comparable(&lt, self.registry)
                        || self.type_param_has_bound(&lt, COMPARABLE))
                {
                    Some(Type::Bool)
                } else if lt == rt && lt == Type::String {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        format!("text isn't ordered with `{}`", op.spell()),
                        "comparing text for order isn't supported yet".to_string(),
                        "compare with `==` or `!=`, or compare lengths/numbers instead".to_string(),
                        Some(span),
                    ));
                    None
                } else {
                    self.op_mismatch(op, &lt, &rt, span);
                    None
                }
            }
            BinOp::And | BinOp::Or => unreachable!(),
        }
    }

    pub(crate) fn op_mismatch(&mut self, op: BinOp, lt: &Type, rt: &Type, span: Span) {
        self.diags.push(Diagnostic::error(
            "E0109",
            format!(
                "`{}` can't compare or combine {} and {}",
                op.spell(),
                lt.show(),
                rt.show()
            ),
            "the two sides of an operator must be the same type".to_string(),
            "make both sides the same type".to_string(),
            Some(span),
        ));
    }

    // --- calls -----------------------------------------------------------
}
