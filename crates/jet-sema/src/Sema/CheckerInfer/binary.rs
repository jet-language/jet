//! Type inference: binary operators and overflow/op-mismatch checks.
//!
//! Split out of the original `CheckerInfer.rs`; behavior unchanged.

use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{e0905, substitute_type, COMPARABLE};
use crate::Sema::{KnowledgeGate, KnowledgePlane};
use crate::AST::{BinOp, CtValue, Dimension, Expr, Type};
use std::collections::HashMap;

/// D-EXPSEM1=A: a written-out negative exponent, such as the `-1` in `2 ^ -1`.
/// Only a spelled numeral counts. An exponent whose sign the checker cannot
/// see keeps the whole-number result type, and the Prelude traps if the value
/// turns out negative at run time.
fn is_written_negative_int(expr: &Expr) -> bool {
    match expr {
        Expr::Paren(inner, _) => is_written_negative_int(inner),
        Expr::Int(value, _, _, raw) => {
            super::exact_integer_literal(*value, raw.as_deref()).negative
        }
        Expr::Unary(crate::AST::UnOp::Neg, inner, _) => match inner.as_ref() {
            Expr::Int(value, _, _, raw) => {
                let value = super::exact_integer_literal(*value, raw.as_deref());
                !value.is_zero() && !value.negative
            }
            _ => false,
        },
        _ => false,
    }
}

fn known_comptime_value(expr: &Expr) -> Option<CtValue> {
    match expr {
        Expr::ComptimeName {
            value: Some(value), ..
        } => Some(value.clone()),
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } if args.is_empty() && !type_name.is_empty() => Some(CtValue::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: Vec::new(),
        }),
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => known_comptime_value(inner),
        _ => None,
    }
}

fn is_folded_fact_expr(expr: &Expr) -> bool {
    match expr {
        Expr::ComptimeName { value: Some(_), .. } => true,
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => is_folded_fact_expr(inner),
        _ => false,
    }
}

impl<'a> Checker<'a> {
    fn is_complex_type(ty: &Type) -> bool {
        matches!(ty, Type::Named(name) if name == crate::Syntax::TYPE_COMPLEX)
    }

    fn is_complex_scalar(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Int | Type::IntN { .. } | Type::Float | Type::Float32
        )
    }

    fn complexize_operand(&mut self, expr: &mut Box<Expr>, span: Span) -> Option<Type> {
        let original = std::mem::replace(expr, Box::new(Expr::Absent(span)));
        *expr = Box::new(Expr::Call(crate::AST::Call {
            name: crate::Syntax::TYPE_COMPLEX.to_string(),
            name_span: span,
            type_args: Vec::new(),
            args: vec![
                crate::AST::CallArg {
                    convention: crate::AST::AccessConvention::Read,
                    expr: *original,
                    span,
                    flags: crate::AST::CallArgFlags::default(),
                    label: None,
                    spread: false,
                },
                crate::AST::CallArg {
                    convention: crate::AST::AccessConvention::Read,
                    expr: Expr::Float(0.0, span, false, None),
                    span,
                    flags: crate::AST::CallArgFlags::default(),
                    label: None,
                    spread: false,
                },
            ],
            resolved_ret: None,
            range_checked: false,
            widen_approx: false,
        }));
        self.infer(expr.as_mut())
    }

    fn is_bare_integer_literal(expr: &Expr) -> bool {
        match expr {
            Expr::Paren(inner, _) => Self::is_bare_integer_literal(inner),
            Expr::Unary(crate::AST::UnOp::Neg, inner, _) => {
                matches!(inner.as_ref(), Expr::Int(_, _, None, _))
            }
            Expr::Int(_, _, None, _) => true,
            _ => false,
        }
    }

    fn decimalize_integer_literal(&mut self, expr: &mut Box<Expr>, span: Span) -> Option<Type> {
        let value = match expr.as_ref() {
            Expr::Int(value, _, None, raw) => {
                super::exact_integer_literal(*value, raw.as_deref()).to_string_rep()
            }
            Expr::Paren(inner, _) if Self::is_bare_integer_literal(inner) => {
                let Expr::Int(value, _, None, raw) = inner.as_ref() else {
                    return None;
                };
                super::exact_integer_literal(*value, raw.as_deref()).to_string_rep()
            }
            _ => return None,
        };
        *expr = Box::new(super::exact_decimal_literal(value, span));
        self.infer(expr.as_mut())
    }

    /// D-NUMLIT-PEER1=A / D-INTLIT-WIDTH1=F: the width a bare numeral takes
    /// next to a sized peer. The peer chooses the signedness family; the
    /// numeral chooses the *narrowest* width of that family that holds it. A
    /// numeral with no sized peer, or one that outgrows every fixed width,
    /// stays `Int` — peerless `1000000 * 1000000` and `0 - 17` must not invent
    /// a U32/U8 and trap, and `byte + -1` must not invent a signed peer.
    fn minimal_literal_width(value: &crate::Numeric::CtBigInt, peer: Option<&Type>) -> Type {
        let Some(Type::IntN { signed, .. }) = peer else {
            return Type::Int;
        };
        [8u8, 16, 32, 64]
            .into_iter()
            .find_map(|bits| {
                let interval = super::integer_width_interval(*signed, bits);
                super::exact_integer_fits(value, interval.lo, interval.hi).then_some(Type::IntN {
                    signed: *signed,
                    bits,
                })
            })
            .unwrap_or(Type::Int)
    }

    /// D-NUMLIT-PEER1=A / D-INTLIT-WIDTH1=F: a bare numeral takes its own
    /// minimal width *before* the operator join, then the ordinary value-set
    /// law decides the join. The peer's width is never pushed onto the numeral,
    /// so `byte + 256` joins `U8` with `U16` instead of rejecting `256` against
    /// `U8`. Destination width still arrives through `expected_type` on
    /// `Expr::Int` (e.g. `take_u8(1 + 2)`), and a numeral that already carries a
    /// width is left alone.
    fn minimal_integer_literal_type(expr: &mut Expr, peer: Option<&Type>) -> Option<Type> {
        match expr {
            Expr::Paren(inner, _) => Self::minimal_integer_literal_type(inner, peer),
            Expr::Unary(crate::AST::UnOp::Neg, inner, _) => {
                let Expr::Int(value, _, width, raw) = inner.as_mut() else {
                    return None;
                };
                if width.is_some() {
                    return None;
                }
                let negated = super::exact_integer_literal(*value, raw.as_deref()).neg();
                let minimal = Self::minimal_literal_width(&negated, peer);
                *width = match &minimal {
                    Type::IntN { signed, bits } => Some((*signed, *bits)),
                    _ => None,
                };
                Some(minimal)
            }
            Expr::Int(value, _, width, raw)
                if width.is_none()
                    && super::exact_integer_literal(*value, raw.as_deref())
                        .compare(&crate::Numeric::CtBigInt::from_int(0))
                        != std::cmp::Ordering::Less =>
            {
                let exact = super::exact_integer_literal(*value, raw.as_deref());
                let minimal = Self::minimal_literal_width(&exact, peer);
                *width = match &minimal {
                    Type::IntN { signed, bits } => Some((*signed, *bits)),
                    _ => None,
                };
                Some(minimal)
            }
            _ => None,
        }
    }

    fn take_numeric_approx_operand(expr: &mut Expr, span: Span) -> Option<Expr> {
        match expr {
            Expr::Paren(inner, _) => Self::take_numeric_approx_operand(inner, span),
            Expr::Call(call) if call.widen_approx && call.args.len() == 1 => Some(
                std::mem::replace(&mut call.args[0].expr, Expr::Absent(span)),
            ),
            _ => None,
        }
    }

    fn contextualize_numeric_literal(&mut self, expr: &mut Expr, target: &Type) -> Option<Type> {
        match expr {
            Expr::Paren(inner, _) => self.contextualize_numeric_literal(inner, target),
            Expr::Unary(crate::AST::UnOp::Neg, inner, _) => match (inner.as_mut(), target) {
                (Expr::Int(value, span, width, raw), Type::InlineRange { base, lo, hi }) => {
                    let negated = super::exact_integer_literal(*value, raw.as_deref()).neg();
                    let interval = super::IntegerInterval::new(i128::from(*lo), i128::from(*hi));
                    if !super::exact_integer_fits(&negated, interval.lo, interval.hi) {
                        self.diags.push(
                            crate::Sema::Diagnostics::inline_range_literal_out_of_bounds(
                                &negated, *lo, *hi, *span,
                            ),
                        );
                    }
                    *width = None;
                    Some(Type::InlineRange {
                        base: base.clone(),
                        lo: *lo,
                        hi: *hi,
                    })
                }
                (Expr::Int(value, span, width, raw), Type::IntN { signed: true, bits }) => {
                    let negated = super::exact_integer_literal(*value, raw.as_deref()).neg();
                    let interval = super::integer_width_interval(true, *bits);
                    if !super::exact_integer_fits(&negated, interval.lo, interval.hi) {
                        self.diags
                            .push(crate::Sema::int_range_error(true, *bits, *span));
                    }
                    *width = Some((true, *bits));
                    Some(target.clone())
                }
                (_, Type::IntN { signed: false, .. }) => None,
                _ => self.contextualize_numeric_literal(inner, target),
            },
            Expr::Int(value, span, width, raw) => match target {
                Type::Int => {
                    *width = None;
                    Some(Type::Int)
                }
                Type::InlineRange { base, lo, hi } => {
                    let exact = super::exact_integer_literal(*value, raw.as_deref());
                    let interval = super::IntegerInterval::new(i128::from(*lo), i128::from(*hi));
                    if !super::exact_integer_fits(&exact, interval.lo, interval.hi) {
                        self.diags.push(
                            crate::Sema::Diagnostics::inline_range_literal_out_of_bounds(
                                &exact, *lo, *hi, *span,
                            ),
                        );
                    }
                    *width = None;
                    Some(Type::InlineRange {
                        base: base.clone(),
                        lo: *lo,
                        hi: *hi,
                    })
                }
                Type::IntN { signed, bits } => {
                    let interval = super::integer_width_interval(*signed, *bits);
                    let exact = super::exact_integer_literal(*value, raw.as_deref());
                    if !super::exact_integer_fits(&exact, interval.lo, interval.hi) {
                        self.diags
                            .push(crate::Sema::int_range_error(*signed, *bits, *span));
                    }
                    *width = Some((*signed, *bits));
                    Some(target.clone())
                }
                Type::Float | Type::Float32 => {
                    let exact_value = super::exact_integer_literal(*value, raw.as_deref());
                    let exact = exact_value.try_i64().is_some_and(|value| {
                        if *target == Type::Float32 {
                            (value as f32) as i64 == value
                        } else {
                            (value as f64) as i64 == value
                        }
                    });
                    if !exact {
                        let limit = if *target == Type::Float32 {
                            1i128 << 24
                        } else {
                            1i128 << 53
                        };
                        self.diags.push(Diagnostic::error(
                            "E1003",
                            format!("{} does not fit exactly in {}", value, target.name()),
                            format!(
                                "{} holds every whole number only from -{} through {}",
                                target.name(),
                                limit,
                                limit
                            ),
                            format!(
                                "use a whole-number type, or write a value from -{} through {}",
                                limit, limit
                            ),
                            Some(*span),
                        ));
                    }
                    *expr = Expr::Float(*value as f64, *span, *target == Type::Float32, None);
                    Some(target.clone())
                }
                _ => None,
            },
            Expr::Float(_, _, is_f32, _) => match target {
                Type::Float => {
                    *is_f32 = false;
                    Some(Type::Float)
                }
                Type::Float32 => {
                    *is_f32 = true;
                    Some(Type::Float32)
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Record sema's numeric widening decision on the expression itself.
    /// Lowering consumes the explicit conversion and checked-crossing marker.
    pub(crate) fn widen_numeric_expr(&mut self, expr: &mut Expr, source: &Type, target: &Type) {
        if source == target {
            return;
        }
        // An inline range is a proof on its carrier, not a runtime wrapper.
        // Strip that proof before selecting the ordinary numeric conversion;
        // otherwise `Int(0..10)` would manufacture a method name from the
        // display spelling instead of widening through `Int`.
        let erased = source.erased_inline_ranges();
        if &erased != source {
            self.require_knowledge_gate(
                KnowledgePlane::Range,
                KnowledgeGate::BoundedArithmetic,
                expr.span(),
            );
            if &erased == target {
                return;
            }
            self.widen_numeric_expr(expr, &erased, target);
            return;
        }
        if self.contextualize_numeric_literal(expr, target).is_some() {
            return;
        }
        let Some(widening) = source.numeric_widening_to(target) else {
            return;
        };
        let span = expr.span();
        let approximate = Self::take_numeric_approx_operand(expr, span);
        let checked = crate::Sema::knowledge_loss_requires_gate(
            KnowledgePlane::Exactness,
            approximate.as_ref().map(|_| KnowledgeGate::Approximation),
        );

        if checked {
            if let Expr::Float(_, _, is_f32, _) = expr {
                if *target == Type::Float32 {
                    *is_f32 = true;
                    return;
                }
            }
        }

        let old = approximate.unwrap_or_else(|| std::mem::replace(expr, Expr::Absent(span)));
        *expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident(target.name(), span)),
            method: Syntax::conversion_method_for_source(&source.name()),
            method_span: span,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args: vec![crate::AST::CallArg {
                convention: crate::AST::AccessConvention::Read,
                expr: old,
                span,
                flags: crate::AST::CallArgFlags::default(),
                label: None,
                spread: false,
            }],
            recv_type: None,
            resolved_ret: Some(target.clone()),
            checked_widen: widening && checked,
        };
    }

    pub(crate) fn widen_numeric_argument(
        &mut self,
        expr: &mut Expr,
        source: Type,
        target: &Type,
        convention: crate::AST::AccessConvention,
    ) -> Type {
        if convention != crate::AST::AccessConvention::Write
            && source != *target
            && source.numeric_widening_to(target).is_some()
        {
            self.widen_numeric_expr(expr, &source, target);
            target.clone()
        } else {
            source
        }
    }

    fn distinct_raw(expr: Expr, type_name: &str, base: Type, span: Span) -> Expr {
        Expr::MethodCall {
            receiver: Box::new(expr),
            method: "raw".to_string(),
            method_span: span,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args: Vec::new(),
            recv_type: Some(type_name.to_string()),
            resolved_ret: Some(base),
            checked_widen: false,
        }
    }

    pub(crate) fn unit_conversion_expr(
        &self,
        expr: Expr,
        source_name: &str,
        _source: &UnitFact,
        destination_name: &str,
        _destination: &UnitFact,
        span: Span,
    ) -> Expr {
        if source_name == destination_name {
            return expr;
        }
        let source_leaf = source_name.rsplit('.').next().unwrap_or(source_name);
        Expr::MethodCall {
            receiver: Box::new(Expr::Ident(destination_name.to_string(), span)),
            method: Syntax::conversion_method_for_source(source_leaf),
            method_span: span,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args: vec![crate::AST::CallArg {
                convention: crate::AST::AccessConvention::Read,
                expr,
                span,
                flags: crate::AST::CallArgFlags::default(),
                label: None,
                spread: false,
            }],
            recv_type: None,
            resolved_ret: Some(Type::Named(destination_name.to_string())),
            checked_widen: false,
        }
    }

    fn convert_unit_operand(
        &self,
        operand: &mut Box<Expr>,
        source_name: &str,
        source: &UnitFact,
        destination_name: &str,
        destination: &UnitFact,
        span: Span,
    ) {
        let old = std::mem::replace(operand, Box::new(Expr::Absent(span)));
        *operand = Box::new(self.unit_conversion_expr(
            *old,
            source_name,
            source,
            destination_name,
            destination,
            span,
        ));
    }

    fn unit_wrapped_binary(
        &self,
        op: BinOp,
        left: Expr,
        left_name: &str,
        right: Expr,
        right_name: &str,
        destination_name: &str,
        span: Span,
    ) -> Expr {
        let raw = Expr::Binary(
            op,
            Box::new(Self::distinct_raw(left, left_name, Type::Float, span)),
            Box::new(Self::distinct_raw(right, right_name, Type::Float, span)),
            span,
        );
        Expr::MethodCall {
            receiver: Box::new(Expr::Ident(destination_name.to_string(), span)),
            method: Syntax::numeric_conversion_method("Float")
                .expect("Float conversion is registered")
                .to_string(),
            method_span: span,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args: vec![crate::AST::CallArg {
                convention: crate::AST::AccessConvention::Read,
                expr: raw,
                span,
                flags: crate::AST::CallArgFlags::default(),
                label: None,
                spread: false,
            }],
            recv_type: None,
            resolved_ret: Some(Type::Named(destination_name.to_string())),
            checked_widen: false,
        }
    }

    fn operator_expr_type(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Ident(name, _) => self.lookup(name).map(|info| info.ty.clone()),
            Expr::Field(base, field, _) => {
                let mut owner = self.operator_expr_type(base)?;
                while let Type::Tagged { inner, .. } = owner {
                    owner = *inner;
                }
                let (type_name, type_args) = match owner {
                    Type::Named(name) => (name, None),
                    Type::Apply { name, args } => (name, Some(args)),
                    _ => return None,
                };
                let (import_ns, leaf) = Self::split_type_name(&type_name);
                let owner_mod = self.struct_owner_module(leaf, import_ns)?;
                let (registry, trait_reg) = if owner_mod == self.module_idx {
                    (self.registry, self.trait_reg)
                } else {
                    let module = self.modules.and_then(|modules| modules.get(owner_mod))?;
                    (&module.registry, &module.trait_reg)
                };
                let subst: HashMap<String, Type> = type_args
                    .and_then(|args| {
                        Some(
                            trait_reg
                                .struct_params
                                .get(leaf)?
                                .iter()
                                .zip(args)
                                .map(|(param, arg)| (param.name.clone(), arg))
                                .collect::<HashMap<String, Type>>(),
                        )
                    })
                    .unwrap_or_default();
                registry
                    .struct_fields(leaf)?
                    .iter()
                    .find(|(candidate, _, _)| candidate == field)
                    .map(|(_, _, ty)| substitute_type(ty, &subst))
            }
            _ => None,
        }
    }

    fn operator_operand_needs_borrow(&self, expr: &Expr, op: BinOp) -> bool {
        if !matches!(expr, Expr::Field(..) | Expr::Index { .. }) {
            return false;
        }
        let ty = self.operator_expr_type(expr);
        let trait_name = match op {
            BinOp::Add => crate::Syntax::TRAIT_ADD,
            BinOp::Sub => crate::Syntax::TRAIT_SUB,
            BinOp::Mul => crate::Syntax::TRAIT_MUL,
            BinOp::Div => crate::Syntax::TRAIT_DIV,
            BinOp::Eq | BinOp::Ne => {
                let has_comparable = ty.as_ref().is_some_and(|ty| match ty {
                    Type::Named(name) => {
                        (self.type_implements_trait_for_name(name, crate::Syntax::TRAIT_COMPARABLE)
                            || self.type_param_has_bound(ty, crate::Syntax::TRAIT_COMPARABLE))
                            && (self.type_implements_trait_for_name(
                                name,
                                crate::Syntax::TRAIT_EQUATABLE,
                            ) || self.type_param_has_bound(ty, crate::Syntax::TRAIT_EQUATABLE))
                    }
                    _ => false,
                });
                if has_comparable {
                    crate::Syntax::TRAIT_COMPARABLE
                } else {
                    crate::Syntax::TRAIT_EQUATABLE
                }
            }
            BinOp::Compare | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                crate::Syntax::TRAIT_COMPARABLE
            }
            _ => return false,
        };
        ty.is_some_and(|ty| match &ty {
            Type::Named(name) => {
                self.type_implements_trait_for_name(name, trait_name)
                    || self.type_param_has_bound(&ty, trait_name)
            }
            _ => false,
        })
    }

    fn is_measurement_type(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Apply { name, args }
                if name == crate::Syntax::TYPE_MEASUREMENT && args == &[Type::Float]
        )
    }

    fn is_exact_numeric_literal(expr: &Expr) -> bool {
        match expr {
            Expr::Int(..) | Expr::Float(..) => true,
            Expr::Paren(inner, _) | Expr::Unary(crate::AST::UnOp::Neg, inner, _) => {
                Self::is_exact_numeric_literal(inner)
            }
            _ => false,
        }
    }

    fn known_measurement_expr(&self, expr: &Expr) -> bool {
        matches!(
            expr,
            Expr::Call(call)
                if call.name == "measurement"
                    && self.funcs.get(&call.name).is_none()
                    && self.lookup(&call.name).is_none()
        ) || self
            .operator_expr_type(expr)
            .is_some_and(|ty| Self::is_measurement_type(&ty))
    }

    fn zero_uncertainty(expr: &mut Box<Expr>, span: Span) {
        let value = std::mem::replace(expr, Box::new(Expr::Absent(span)));
        *expr = Box::new(Expr::Call(crate::AST::Call {
            name: "measurement".to_string(),
            name_span: span,
            type_args: Vec::new(),
            args: vec![
                crate::AST::CallArg {
                    convention: crate::AST::AccessConvention::Read,
                    expr: *value,
                    span,
                    flags: crate::AST::CallArgFlags::default(),
                    label: None,
                    spread: false,
                },
                crate::AST::CallArg {
                    convention: crate::AST::AccessConvention::Read,
                    expr: Expr::Float(0.0, span, false, None),
                    span,
                    flags: crate::AST::CallArgFlags::default(),
                    label: Some(("uncertainty".to_string(), span)),
                    spread: false,
                },
            ],
            resolved_ret: Some(Type::Apply {
                name: crate::Syntax::TYPE_MEASUREMENT.to_string(),
                args: vec![Type::Float],
            }),
            range_checked: false,
            widen_approx: false,
        }));
    }

    /// Binary operators and type checking.
    pub(crate) fn infer_binary(
        &mut self,
        op: BinOp,
        lhs: &mut Box<Expr>,
        rhs: &mut Box<Expr>,
        span: Span,
        replacement: &mut Option<Expr>,
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

        // Only a synthetic read/read hook borrows a non-Copy field operand.
        // Ordinary expressions retain the canonical owning-read clone rule.
        self.borrow_ctx = self.operator_operand_needs_borrow(lhs, op);
        let saved_expected = self.expected_type.clone();
        if Self::is_exact_numeric_literal(lhs) && self.known_measurement_expr(rhs) {
            self.expected_type = Some(Type::Float);
        }
        let lt = self.infer(lhs);
        self.expected_type = saved_expected;

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
            }
        }

        self.borrow_ctx = self.operator_operand_needs_borrow(rhs, op);
        // Same-type ops: LHS feeds RHS `.{…}`. Clear inherited assign expected
        // otherwise so heterogeneous RHS (unit Point += Delta) can still infer.
        let saved_expected = self.expected_type.clone();
        if matches!(
            op,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::Compare
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
        ) {
            self.expected_type = if lt.as_ref().is_some_and(Self::is_measurement_type)
                && Self::is_exact_numeric_literal(rhs)
            {
                Some(Type::Float)
            } else {
                lt.as_ref()
                    .filter(|_| expr_wants_expected_type(rhs))
                    .cloned()
            };
        }
        let rt = self.infer(rhs);
        self.expected_type = saved_expected;
        let (mut lt, mut rt) = (lt?, rt?);

        // A folded fact compared with a closed enum literal is already a Bool.
        // Keep the comparison out of TIR: core enum literals intentionally stay
        // outside the resident subset, and no engine may rediscover a fact at
        // runtime.
        if matches!(op, BinOp::Eq | BinOp::Ne)
            && (is_folded_fact_expr(lhs) || is_folded_fact_expr(rhs))
            && matches!((&lt, &rt), (Type::Named(left), Type::Named(right)) if left == right
                && crate::Sema::CheckerCoreLib::core_type_known(left))
        {
            if let (Some(left), Some(right)) =
                (known_comptime_value(lhs), known_comptime_value(rhs))
            {
                if let Ok(CtValue::Bool(value)) =
                    crate::Comptime::Builtins::eval_binop(op, left, right, span)
                {
                    *replacement = Some(Expr::Bool(value, span));
                    return Some(Type::Bool);
                }
            }
        }

        // D-INTLIT-WIDTH1=F / D-NUMLIT-PEER1=A: an unowned whole literal takes
        // its own minimal width in a sized peer's signedness family; without a
        // sized peer it stays `Int`. The ordinary value-set law then decides
        // whether one operand widens to the other — the peer's width is never
        // pushed onto the numeral, so a numeral the peer cannot hold widens the
        // join instead of being rejected against the peer.
        let lhs_is_literal = Self::is_bare_integer_literal(lhs);
        let rhs_is_literal = Self::is_bare_integer_literal(rhs);
        if let Some(minimal) =
            Self::minimal_integer_literal_type(lhs, (!rhs_is_literal).then_some(&rt))
        {
            lt = minimal;
        }
        if let Some(minimal) =
            Self::minimal_integer_literal_type(rhs, (!lhs_is_literal).then_some(&lt))
        {
            rt = minimal;
        }

        // Decimal context still owns an untyped numeral's representation.
        if lt != rt && matches!(rt, Type::Float | Type::Float32) {
            if let Some(contextual) = self.contextualize_numeric_literal(lhs, &rt) {
                lt = contextual;
            }
        }
        if lt != rt && matches!(lt, Type::Float | Type::Float32) {
            if let Some(contextual) = self.contextualize_numeric_literal(rhs, &lt) {
                rt = contextual;
            }
        }

        // D-TYPE2-MEASURE1=A: Matrix shape composition is the measure
        // substrate's match/compose rule, not a compute-runtime heuristic.
        if op == BinOp::Mul {
            if let (Some([left_rows, left_inner]), Some([right_inner, right_cols])) = (
                lt.compute_shape_dimensions()
                    .and_then(|shape| <[u64; 2]>::try_from(shape).ok()),
                rt.compute_shape_dimensions()
                    .and_then(|shape| <[u64; 2]>::try_from(shape).ok()),
            ) {
                if matches!((&lt, &rt), (
                    Type::Apply { name: left, .. },
                    Type::Apply { name: right, .. }
                ) if left == "Matrix" && right == "Matrix")
                {
                    let left = crate::AST::Measure::literal("shape", left_inner);
                    let right = crate::AST::Measure::literal("shape", right_inner);
                    if left
                        .combine(&right, crate::AST::MeasureRule::Match)
                        .is_none()
                    {
                        self.diags.push(Diagnostic::error(
                            "E2512",
                            format!(
                                "matrix multiplication needs equal inner sides, but these are {left_inner} and {right_inner}"
                            ),
                            "the left column measure must match the right row measure".to_string(),
                            format!(
                                "use `Matrix<{left_rows}, {left_inner}> * Matrix<{left_inner}, {right_cols}>`"
                            ),
                            Some(span),
                        ));
                        return None;
                    }
                    return Some(Type::Result {
                        ok: Box::new(Type::compute_shape_type("Matrix", &[left_rows, right_cols])),
                        err: Box::new(Type::Named("ComputeError".to_string())),
                    });
                }
            }
        }

        // D-TYPE2-UNCERT1=A: entering the measured grade is contagious.
        // Exact numeric operands enter it through the canonical zero-uncertainty
        // constructor; widening to the Measurement<Float> carrier is implicit.
        if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
            && (Self::is_measurement_type(&lt) || Self::is_measurement_type(&rt))
        {
            if lt != Type::Float && lt.numeric_widening_to(&Type::Float).is_some() {
                self.widen_numeric_expr(lhs, &lt, &Type::Float);
                lt = Type::Float;
            }
            if rt != Type::Float && rt.numeric_widening_to(&Type::Float).is_some() {
                self.widen_numeric_expr(rhs, &rt, &Type::Float);
                rt = Type::Float;
            }
            if lt == Type::Float {
                Self::zero_uncertainty(lhs, span);
                lt = Type::Apply {
                    name: crate::Syntax::TYPE_MEASUREMENT.to_string(),
                    args: vec![Type::Float],
                };
            }
            if rt == Type::Float {
                Self::zero_uncertainty(rhs, span);
                rt = Type::Apply {
                    name: crate::Syntax::TYPE_MEASUREMENT.to_string(),
                    args: vec![Type::Float],
                };
            }
            if Self::is_measurement_type(&lt) && Self::is_measurement_type(&rt) {
                return Some(lt);
            }
            self.op_mismatch(op, &lt, &rt, span);
            return None;
        }

        if matches!(op, BinOp::Eq | BinOp::Ne)
            && (self.type_contains_observable_clock(&lt)
                || self.type_contains_observable_clock(&rt))
        {
            self.record_effect(crate::Sema::Effects::Effect::Time.name(), span);
            if self.in_pure && self.det_suppress == 0 {
                self.diags
                    .push(crate::Sema::e3403("Clock comparison", Some(span)));
            }
        }

        let joins_numeric = matches!(
            op,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                // D-FLOORDIV1=A: `/%` takes the same operands as `/`.
                | BinOp::FloorDiv
                // D-EXPSEM1=A: any Float operand makes the power a Float.
                | BinOp::Pow
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::Compare
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
        ) || (matches!(
            op,
            BinOp::Mod | BinOp::Rem | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor
        ) && lt.is_integer()
            && rt.is_integer());
        if joins_numeric && lt != rt {
            if let Some(join) = lt.numeric_join(&rt) {
                self.widen_numeric_expr(lhs, &lt, &join);
                self.widen_numeric_expr(rhs, &rt, &join);
                lt = join.clone();
                rt = join;
            }
        }

        // D-RANGETYPE1: arithmetic on a range refinement widens to the base
        // carrier. The result is not silently claimed to remain inside the
        // input interval (`10 + 10` is not in `0..10`).
        let arithmetic = matches!(
            op,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::FloorDiv
                | BinOp::Mod
                | BinOp::Rem
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr
                | BinOp::Pow
        );
        if arithmetic {
            let left_erased = lt.erased_inline_ranges();
            if left_erased != lt {
                self.widen_numeric_expr(lhs, &lt, &left_erased);
                lt = left_erased;
            }
            let right_erased = rt.erased_inline_ranges();
            if right_erased != rt {
                self.widen_numeric_expr(rhs, &rt, &right_erased);
                rt = right_erased;
            }
        }

        // D-OPDEF1=A: user operators are ordinary trait-method calls after sema
        // proves one exact impl and the fixed same-type law.
        // A dimensional unit is still represented by a nominal `#Numeric`
        // type, but `*` and `/` must derive the physical dimension before the
        // ordinary same-type trait hook can erase it back to that nominal type.
        // Otherwise `Meter * Meter` stays `Meter` forever and exponent
        // overflow never reaches the sema dimension check.
        let dimensional_multiplicative = matches!(op, BinOp::Mul | BinOp::Div)
            && (self.quantity_dimension(&lt).is_some() || self.quantity_dimension(&rt).is_some());
        let unit_operator =
            self.unit_fact_for_type(&lt).is_some() || self.unit_fact_for_type(&rt).is_some();
        if lt == rt && !dimensional_multiplicative && !unit_operator {
            if let Type::Named(type_name) = &lt {
                let comparable_hook = (self
                    .type_implements_trait_for_name(type_name, crate::Syntax::TRAIT_COMPARABLE)
                    || self.type_param_has_bound(&lt, crate::Syntax::TRAIT_COMPARABLE))
                    && (self
                        .type_implements_trait_for_name(type_name, crate::Syntax::TRAIT_EQUATABLE)
                        || self.type_param_has_bound(&lt, crate::Syntax::TRAIT_EQUATABLE));
                let hook = match op {
                    BinOp::Add => Some((crate::Syntax::TRAIT_ADD, "add", lt.clone())),
                    BinOp::Sub => Some((crate::Syntax::TRAIT_SUB, "sub", lt.clone())),
                    BinOp::Mul => Some((crate::Syntax::TRAIT_MUL, "mul", lt.clone())),
                    BinOp::Div => Some((crate::Syntax::TRAIT_DIV, "div", lt.clone())),
                    BinOp::Eq | BinOp::Ne if comparable_hook => Some((
                        crate::Syntax::TRAIT_COMPARABLE,
                        "compare",
                        Type::Named(crate::Syntax::TYPE_ORDERING.to_string()),
                    )),
                    BinOp::Eq | BinOp::Ne => {
                        Some((crate::Syntax::TRAIT_EQUATABLE, "equal", Type::Bool))
                    }
                    BinOp::Compare | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Some((
                        crate::Syntax::TRAIT_COMPARABLE,
                        "compare",
                        Type::Named(crate::Syntax::TYPE_ORDERING.to_string()),
                    )),
                    _ => None,
                };
                if let Some((trait_name, method, ret)) = hook {
                    if self.type_implements_trait_for_name(type_name, trait_name)
                        || self.type_param_has_bound(&lt, trait_name)
                    {
                        // `lt` and `rt` are the typed operands that selected this
                        // exact hook. Do not inspect source shape here: a field,
                        // call, literal, or other expression of the same type
                        // dispatches back to this hook just like an identifier.
                        if self.fn_name == method
                            && self
                                .lookup(crate::Syntax::KW_SELF)
                                .is_some_and(|info| info.ty == lt)
                        {
                            self.diags.push(Diagnostic::error(
                                "E0361",
                                format!("`{}` calls itself through {}", method, operator_label(op)),
                                concat!(
                                    "the operator symbol dispatches back to this same hook, ",
                                    "so evaluation would recurse forever"
                                )
                                .to_string(),
                                concat!(
                                    "combine the value's fields or call a different named helper ",
                                    "inside the hook"
                                )
                                .to_string(),
                                Some(span),
                            ));
                            return None;
                        }
                        let left = std::mem::replace(lhs, Box::new(Expr::Absent(span)));
                        let right = std::mem::replace(rhs, Box::new(Expr::Absent(span)));
                        let call = Expr::MethodCall {
                            receiver: left,
                            method: method.to_string(),
                            method_span: span,
                            owner_type_args: Vec::new(),
                            type_args: Vec::new(),
                            args: vec![crate::AST::CallArg {
                                convention: crate::AST::AccessConvention::Read,
                                expr: *right,
                                span,
                                flags: crate::AST::CallArgFlags::default(),
                                label: None,
                                spread: false,
                            }],
                            recv_type: Some(
                                if self
                                    .type_param_scope
                                    .iter()
                                    .any(|param| param.name == type_name.as_str())
                                {
                                    trait_name.to_string()
                                } else {
                                    Self::split_type_name(type_name).1.to_string()
                                },
                            ),
                            resolved_ret: Some(ret),
                            checked_widen: false,
                        };
                        *replacement = Some(match op {
                            BinOp::Eq | BinOp::Ne if comparable_hook => Expr::Binary(
                                if op == BinOp::Eq {
                                    BinOp::Eq
                                } else {
                                    BinOp::Ne
                                },
                                Box::new(call),
                                Box::new(Expr::EnumLit {
                                    type_name: crate::Syntax::TYPE_ORDERING.to_string(),
                                    variant: "Equal".to_string(),
                                    variant_span: None,
                                    args: Vec::new(),
                                    leading_dot: false,
                                    span,
                                }),
                                span,
                            ),
                            BinOp::Ne => Expr::Unary(crate::AST::UnOp::Not, Box::new(call), span),
                            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                                let (cmp, variant) = match op {
                                    BinOp::Lt => (BinOp::Eq, "Less"),
                                    BinOp::Le => (BinOp::Ne, "Greater"),
                                    BinOp::Gt => (BinOp::Eq, "Greater"),
                                    BinOp::Ge => (BinOp::Ne, "Less"),
                                    _ => unreachable!(),
                                };
                                Expr::Binary(
                                    cmp,
                                    Box::new(call),
                                    Box::new(Expr::EnumLit {
                                        type_name: crate::Syntax::TYPE_ORDERING.to_string(),
                                        variant: variant.to_string(),
                                        variant_span: None,
                                        args: Vec::new(),
                                        leading_dot: false,
                                        span,
                                    }),
                                    span,
                                )
                            }
                            BinOp::Compare => call,
                            _ => call,
                        });
                        return Some(if op == BinOp::Compare {
                            Type::Named(crate::Syntax::TYPE_ORDERING.to_string())
                        } else if op.is_comparison() {
                            Type::Bool
                        } else {
                            lt
                        });
                    }
                }
            }
        }

        // D-SHAPE-QUANTITY1=A: physical dimensions are compiler-known and
        // normalized before the ordinary nominal/numeric operator rules.
        // Unit-family declarations remain nominal for +/-, while */ derives a
        // package-independent erased quantity type.
        let lunit = self.unit_fact_for_type(&lt);
        let runit = self.unit_fact_for_type(&rt);
        let ldim = self.quantity_dimension(&lt);
        let rdim = self.quantity_dimension(&rt);
        // D-DIMENSION-OPEN1=D: a nominal family has no dimension, and still
        // owns unit conversion, affine points, and rounding policy.
        let any_dimensional = ldim.is_some() || rdim.is_some();
        if any_dimensional || lunit.is_some() || runit.is_some() {
            if let (Some((lname, lfact)), Some((rname, rfact))) = (&lunit, &runit) {
                if lfact.package == rfact.package
                    && lfact.dimension == rfact.dimension
                    && lfact.family == rfact.family
                {
                    use QuantityKind::{Delta, Linear, Point};
                    if lfact.family == "Time"
                        && lfact.package == std::path::PathBuf::from("core.units")
                    {
                        let result = match (op, lfact.kind, rfact.kind) {
                            (BinOp::Add | BinOp::Sub, Delta, Delta) => {
                                Some(Type::Named(crate::Syntax::DURATION_TYPE.to_string()))
                            }
                            (BinOp::Add, Point, Delta)
                            | (BinOp::Sub, Point, Delta)
                            | (BinOp::Add, Delta, Point) => {
                                Some(Type::Named(crate::Syntax::TYPE_INSTANT.to_string()))
                            }
                            (BinOp::Sub, Point, Point) => {
                                Some(Type::Named(crate::Syntax::DURATION_TYPE.to_string()))
                            }
                            (
                                BinOp::Eq
                                | BinOp::Ne
                                | BinOp::Lt
                                | BinOp::Gt
                                | BinOp::Le
                                | BinOp::Ge
                                | BinOp::Compare,
                                left_kind,
                                right_kind,
                            ) if left_kind == right_kind => Some(if op == BinOp::Compare {
                                Type::Named(crate::Syntax::TYPE_ORDERING.to_string())
                            } else {
                                Type::Bool
                            }),
                            (BinOp::Add, Point, Point) | (BinOp::Sub, Delta, Point) => {
                                self.diags.push(Diagnostic::error(
                                    "E0127",
                                    format!(
                                        "{} is not available between `{}` and `{}`",
                                        operator_label(op), lname, rname
                                    ),
                                    "Time points are positions; only point + delta, point - delta, point - point, and delta + delta are defined".to_string(),
                                    "subtract two Instants for a Duration, or add a Duration to an Instant".to_string(),
                                    Some(span),
                                ));
                                return None;
                            }
                            _ => None,
                        };
                        if result.is_some() {
                            return result;
                        }
                    }
                    match (op, lfact.kind, rfact.kind) {
                        (BinOp::Add | BinOp::Sub, Linear, Linear)
                        | (BinOp::Add | BinOp::Sub, Delta, Delta) => {
                            let left_wins = lfact.scale.abs() <= rfact.scale.abs();
                            let (dest_name, dest_fact) = if left_wins {
                                (lname, lfact)
                            } else {
                                (rname, rfact)
                            };
                            let source_name = if left_wins { rname } else { lname };
                            let source_expr = if left_wins { &**rhs } else { &**lhs };
                            if self.reject_implicit_unit_conversion(
                                dest_name,
                                source_name,
                                source_expr,
                                span,
                            ) {
                                return Some(Type::Named(dest_name.clone()));
                            }
                            self.convert_unit_operand(
                                lhs, lname, lfact, dest_name, dest_fact, span,
                            );
                            self.convert_unit_operand(
                                rhs, rname, rfact, dest_name, dest_fact, span,
                            );
                            return Some(Type::Named(dest_name.clone()));
                        }
                        (BinOp::Add, Point, Delta) | (BinOp::Sub, Point, Delta) => {
                            let Some(delta_name) = self.counterpart_unit_type(lname, lfact, Delta)
                            else {
                                return None;
                            };
                            let (_, delta_fact) = self
                                .unit_fact_for_type(&Type::Named(delta_name.clone()))
                                .expect("counterpart unit fact");
                            if self.reject_implicit_unit_conversion(&delta_name, rname, rhs, span) {
                                return Some(Type::Named(lname.clone()));
                            }
                            let left = *std::mem::replace(lhs, Box::new(Expr::Absent(span)));
                            let right = *std::mem::replace(rhs, Box::new(Expr::Absent(span)));
                            let right = self.unit_conversion_expr(
                                right,
                                rname,
                                rfact,
                                &delta_name,
                                &delta_fact,
                                span,
                            );
                            *replacement = Some(self.unit_wrapped_binary(
                                op,
                                left,
                                lname,
                                right,
                                &delta_name,
                                lname,
                                span,
                            ));
                            return Some(Type::Named(lname.clone()));
                        }
                        (BinOp::Add, Delta, Point) => {
                            let Some(delta_name) = self.counterpart_unit_type(rname, rfact, Delta)
                            else {
                                return None;
                            };
                            let (_, delta_fact) = self
                                .unit_fact_for_type(&Type::Named(delta_name.clone()))
                                .expect("counterpart unit fact");
                            if self.reject_implicit_unit_conversion(&delta_name, lname, lhs, span) {
                                return Some(Type::Named(rname.clone()));
                            }
                            let left = *std::mem::replace(lhs, Box::new(Expr::Absent(span)));
                            let right = *std::mem::replace(rhs, Box::new(Expr::Absent(span)));
                            let left = self.unit_conversion_expr(
                                left,
                                lname,
                                lfact,
                                &delta_name,
                                &delta_fact,
                                span,
                            );
                            *replacement = Some(self.unit_wrapped_binary(
                                BinOp::Add,
                                right,
                                rname,
                                left,
                                &delta_name,
                                rname,
                                span,
                            ));
                            return Some(Type::Named(rname.clone()));
                        }
                        (BinOp::Sub, Point, Point) => {
                            let left_wins = lfact.scale.abs() <= rfact.scale.abs();
                            let (point_name, point_fact) = if left_wins {
                                (lname, lfact)
                            } else {
                                (rname, rfact)
                            };
                            let Some(delta_name) =
                                self.counterpart_unit_type(point_name, point_fact, Delta)
                            else {
                                return None;
                            };
                            let source_name = if left_wins { rname } else { lname };
                            let source_expr = if left_wins { &**rhs } else { &**lhs };
                            if self.reject_implicit_unit_conversion(
                                point_name,
                                source_name,
                                source_expr,
                                span,
                            ) {
                                return Some(Type::Named(delta_name));
                            }
                            let left = *std::mem::replace(lhs, Box::new(Expr::Absent(span)));
                            let right = *std::mem::replace(rhs, Box::new(Expr::Absent(span)));
                            let left = self.unit_conversion_expr(
                                left, lname, lfact, point_name, point_fact, span,
                            );
                            let right = self.unit_conversion_expr(
                                right, rname, rfact, point_name, point_fact, span,
                            );
                            *replacement = Some(self.unit_wrapped_binary(
                                BinOp::Sub,
                                left,
                                point_name,
                                right,
                                point_name,
                                &delta_name,
                                span,
                            ));
                            return Some(Type::Named(delta_name));
                        }
                        (BinOp::Add, Point, Point) | (BinOp::Sub, Delta, Point) => {
                            self.diags.push(Diagnostic::error(
                                "E0127",
                                format!("{} is not available between `{}` and `{}`", operator_label(op), lname, rname),
                                "affine points are positions; only point + delta, point - delta, point - point, and delta + delta are defined".to_string(),
                                "subtract two points for a delta, or add a matching Delta to a point".to_string(),
                                Some(span),
                            ));
                            return None;
                        }
                        (
                            BinOp::Eq
                            | BinOp::Ne
                            | BinOp::Compare
                            | BinOp::Lt
                            | BinOp::Gt
                            | BinOp::Le
                            | BinOp::Ge,
                            left_kind,
                            right_kind,
                        ) if left_kind == right_kind => {
                            let left_wins = lfact.scale.abs() <= rfact.scale.abs();
                            let (dest_name, dest_fact) = if left_wins {
                                (lname, lfact)
                            } else {
                                (rname, rfact)
                            };
                            let source_name = if left_wins { rname } else { lname };
                            let source_expr = if left_wins { &**rhs } else { &**lhs };
                            if self.reject_implicit_unit_conversion(
                                dest_name,
                                source_name,
                                source_expr,
                                span,
                            ) {
                                return Some(if op == BinOp::Compare {
                                    Type::Named(crate::Syntax::TYPE_ORDERING.to_string())
                                } else {
                                    Type::Bool
                                });
                            }
                            self.convert_unit_operand(
                                lhs, lname, lfact, dest_name, dest_fact, span,
                            );
                            self.convert_unit_operand(
                                rhs, rname, rfact, dest_name, dest_fact, span,
                            );
                            return Some(Type::Bool);
                        }
                        _ => {}
                    }
                }
            }
            match op {
                BinOp::Add | BinOp::Sub => {
                    if let (Some(ldim), Some(rdim)) = (ldim.clone(), rdim.clone()) {
                        if ldim != rdim {
                            self.dimension_mismatch(op, ldim, rdim, span);
                            return None;
                        }
                        // Same concrete unit/derived quantity is safe. Different
                        // units of one dimension still need #603's conversions.
                        if lt == rt {
                            return Some(lt);
                        }
                    } else if any_dimensional {
                        self.dimension_mismatch(
                            op,
                            ldim.unwrap_or_else(Dimension::scalar),
                            rdim.unwrap_or_else(Dimension::scalar),
                            span,
                        );
                        return None;
                    } else if lt == rt {
                        return Some(lt);
                    }
                    // Two nominal families: the ordinary distinct-type rule
                    // below reports the mismatch.
                }
                BinOp::Mul | BinOp::Div if any_dimensional => {
                    if !self.quantity_base_is_compatible(&lt, &rt) {
                        self.op_mismatch(op, &lt, &rt, span);
                        return None;
                    }
                    let ldim = ldim.unwrap_or_else(Dimension::scalar);
                    let rdim = rdim.unwrap_or_else(Dimension::scalar);
                    let result = if op == BinOp::Mul {
                        ldim.multiply(&rdim)
                    } else {
                        ldim.divide(&rdim)
                    };
                    let Some(result) = result else {
                        self.dimension_overflow(op, span);
                        return None;
                    };
                    return if result == Dimension::scalar() {
                        Some(Type::Float)
                    } else {
                        Some(Type::quantity(Type::Float, result))
                    };
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    if lt == rt {
                        return Some(Type::Bool);
                    }
                    if any_dimensional && ldim != rdim {
                        self.dimension_mismatch(
                            op,
                            ldim.unwrap_or_else(Dimension::scalar),
                            rdim.unwrap_or_else(Dimension::scalar),
                            span,
                        );
                        return None;
                    }
                }
                _ => {}
            }
        }

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
                    | BinOp::FloorDiv
                    | BinOp::Mod
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
                            format!("{} is not available on `{}`", operator_label(op), distinct_name),
                            format!("`{}` is a distinct type but isn't marked `#Numeric`, so it doesn't inherit arithmetic operators — only `==` is available", distinct_name),
                            format!("add `#Numeric` before the declaration if this is a quantity, or use `.raw()` to work on the underlying value"),
                            Some(span),
                        ));
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0127",
                            format!("{} is not available between `{}` and `{}`", operator_label(op), lt.name(), rt.name()),
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
                        format!("{} is not available on `{}`", operator_label(op), distinct_name),
                        format!("`{}` is a distinct type but isn't marked `#Numeric`, so it doesn't inherit arithmetic operators — only `==` is available", distinct_name),
                        "add `#Numeric` before the declaration if this is a quantity, or use `.raw()` to work on the underlying value".to_string(),
                        Some(span),
                    ));
                    return None;
                }
                if self.registry.distinct_range(distinct_name).is_some() {
                    self.require_knowledge_gate(
                        KnowledgePlane::Range,
                        KnowledgeGate::BoundedArithmetic,
                        span,
                    );
                    let base = self
                        .registry
                        .distinct_base(distinct_name)
                        .cloned()
                        .expect("range-constrained distinct type has a base");
                    // Range loss changes the carrier, not just the result fact. Keep
                    // the nominal wrapper at the boundary, then expose its base to
                    // every backend through the existing raw projection seam.
                    let left = std::mem::replace(lhs, Box::new(Expr::Absent(span)));
                    let right = std::mem::replace(rhs, Box::new(Expr::Absent(span)));
                    *lhs = Box::new(Self::distinct_raw(*left, distinct_name, base.clone(), span));
                    *rhs = Box::new(Self::distinct_raw(
                        *right,
                        distinct_name,
                        base.clone(),
                        span,
                    ));
                    return Some(base);
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

        // D-LAYOUT1 / D-LAYOUT-GATES1: operator overloading on the closed
        // built-in layout family (`HVar`/`VVar`/`LengthVar`/`Constraint`).
        // GATE 1 is exactly this: `>=`/`<=`/`==` return `Constraint` instead
        // of `Bool` for this family — the ONLY place that blessing is wired.
        // A cross-axis combination (`HVar` with `VVar`, on `+`/`-` OR a
        // comparison) is E2932, `E-LAYOUT-AXIS-MISMATCH`.
        match layout_binop_result(op, &lt, &rt) {
            Some(Ok(result)) => return Some(result),
            Some(Err(())) => {
                self.diags.push(Diagnostic::error(
                    "E2932",
                    format!(
                        "layout constraint mixes a horizontal and vertical value (`{}` and `{}`)",
                        lt.name(),
                        rt.name()
                    ),
                    "`left`/`right`/`width` are horizontal (`HVar`); `top`/`bottom`/`height` are vertical (`VVar`) — combining or comparing across axes is caught at compile time instead of producing a nonsensical layout".to_string(),
                    "compare or combine values from the same axis (a `LengthVar`, or a plain number, fits either axis)".to_string(),
                    Some(span),
                ));
                return None;
            }
            None => {}
        }

        // D-TYPE2-IMAG1=A: Complex shares the precise-builtin path, while a
        // scalar operand is promoted through the same explicit constructor.
        let lhs_complex = Self::is_complex_type(&lt);
        let rhs_complex = Self::is_complex_type(&rt);
        if lhs_complex || rhs_complex {
            if lhs_complex && !rhs_complex && Self::is_complex_scalar(&rt) {
                rt = self.complexize_operand(rhs, span)?;
            } else if rhs_complex && !lhs_complex && Self::is_complex_scalar(&lt) {
                lt = self.complexize_operand(lhs, span)?;
            }
            if Self::is_complex_type(&lt) && Self::is_complex_type(&rt) {
                if let Some(result) = precise_binop_result(
                    op,
                    crate::Syntax::TYPE_COMPLEX,
                    crate::Syntax::TYPE_COMPLEX,
                ) {
                    return Some(result);
                }
            }
            self.op_mismatch(op, &lt, &rt, span);
            return None;
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
                    format!("{} isn't defined between `{}` and `{}`", operator_label(op), lt.name(), rt.name()),
                    "the built-in math types support element-wise `+`/`-` (and `/` for lanes), `*` (element-wise, or matrix×vector), and `==`".to_string(),
                    "check the operands are the same lane/vector type, or use a method like `.dot()`/`.matmul()`".to_string(),
                    Some(span),
                ));
                return None;
            }
        }

        // D-TYPE2-DEFAULT1 / D-NUMTYPE1: exact decimal and rational operators.
        {
            let lname = match &lt {
                Type::Named(n) if !self.registry.contains(n) => n.clone(),
                _ => String::new(),
            };
            let rname = match &rt {
                Type::Named(n) if !self.registry.contains(n) => n.clone(),
                _ => String::new(),
            };
            if crate::Numeric::is_decimal_type_name(&lname)
                || crate::Numeric::is_decimal_type_name(&rname)
                || lname == crate::Syntax::TYPE_FRACTION
                || rname == crate::Syntax::TYPE_FRACTION
            {
                // D-TYPE2-DEFAULT1: an exact decimal may absorb an untyped
                // integer literal. Keep the promotion at sema so TIR and all
                // engines consume the same Decimal Prelude operation.
                if crate::Numeric::type_is_decimal(&lt)
                    && rt == Type::Int
                    && Self::is_bare_integer_literal(rhs)
                {
                    if let Some(promoted) = self.decimalize_integer_literal(rhs, span) {
                        rt = promoted;
                    }
                } else if crate::Numeric::type_is_decimal(&rt)
                    && lt == Type::Int
                    && Self::is_bare_integer_literal(lhs)
                {
                    if let Some(promoted) = self.decimalize_integer_literal(lhs, span) {
                        lt = promoted;
                    }
                }
                let lname = match &lt {
                    Type::Named(n) if !self.registry.contains(n) => n.clone(),
                    _ => String::new(),
                };
                let rname = match &rt {
                    Type::Named(n) if !self.registry.contains(n) => n.clone(),
                    _ => String::new(),
                };
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
                let fraction_and_integer_literal = (lname == crate::Syntax::TYPE_FRACTION
                    && rt == Type::Int
                    && Self::is_bare_integer_literal(rhs))
                    || (rname == crate::Syntax::TYPE_FRACTION
                        && lt == Type::Int
                        && Self::is_bare_integer_literal(lhs));
                if fraction_and_integer_literal {
                    return match op {
                        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                            Some(Type::Named(crate::Syntax::TYPE_FRACTION.to_string()))
                        }
                        BinOp::Eq | BinOp::Ne => Some(Type::Bool),
                        _ => None,
                    };
                }
                if let Some(result) = precise_binop_result(op, &lname, &rname) {
                    return Some(result);
                }
                self.diags.push(Diagnostic::error(
                    "E0133",
                    format!(
                        "{} isn't defined between `{}` and `{}`",
                        operator_label(op),
                        lt.name(),
                        rt.name()
                    ),
                    "`Decimal` supports `+`, `-`, `*`, and `==`/`!=` on matching values only"
                        .to_string(),
                    "use a method like `.add(other)` or make both operands the same precise type"
                        .to_string(),
                    Some(span),
                ));
                return None;
            }
        }

        match op {
            // D-FLOORDIV1=A: `/%` divides and rounds down. It keeps the operand
            // type — whole numbers stay whole, floats stay floats — so it is the
            // operator that gives a whole-number quotient.
            BinOp::FloorDiv => {
                if lt == rt && lt.is_numeric() {
                    Some(lt)
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        format!(
                            "{} needs both sides to be the same number type, but this has {} and {}",
                            operator_label(op),
                            lt.show(),
                            rt.show()
                        ),
                        compound_why(op),
                        "make both sides the same number type".to_string(),
                        Some(span),
                    ));
                    None
                }
            }
            // D-EXPSEM1=A / D-EXPNEG1=A: a whole-number base with a
            // written negative exponent leaves the exact whole-number world
            // for the exact Fraction carrier. A dynamic exponent remains an
            // Int and the shared Prelude reports a negative-power stop.
            BinOp::Pow => {
                if lt == Type::Int && rt == Type::Int && is_written_negative_int(rhs) {
                    return Some(Type::Named(crate::Syntax::TYPE_FRACTION.to_string()));
                }
                if lt == rt && lt.is_numeric() {
                    Some(lt)
                } else {
                    self.op_mismatch(op, &lt, &rt, span);
                    None
                }
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                // D-TYPE2-DEFAULT1 amends D-INTDIV1: `/` on exact whole numbers
                // answers an exact Fraction. `/%` remains the whole-number
                // floor path. Sized widths keep the D-SG9 same-width rule and
                // are not touched here.
                if op == BinOp::Div && lt == Type::Int && rt == Type::Int {
                    return Some(Type::Named(crate::Syntax::TYPE_FRACTION.to_string()));
                }
                // D-VERDICT-1304-1: the widening join above rewrites both
                // operands to one numeric type. Arithmetic keeps that type.
                if lt == rt && lt.is_numeric() {
                    Some(lt)
                } else if let Type::Named(type_name) = &lt {
                    let (import_ns, leaf) = Self::split_type_name(type_name);
                    if self.struct_owner_module(leaf, import_ns).is_some() {
                        let trait_name = match op {
                            BinOp::Add => crate::Syntax::TRAIT_ADD,
                            BinOp::Sub => crate::Syntax::TRAIT_SUB,
                            BinOp::Mul => crate::Syntax::TRAIT_MUL,
                            BinOp::Div => crate::Syntax::TRAIT_DIV,
                            _ => unreachable!(),
                        };
                        self.diags.push(Diagnostic::error(
                            "E0360",
                            format!(
                                "no {} operator is defined for `{}`",
                                operator_label(op),
                                type_name
                            ),
                            format!(
                                concat!(
                                    "user arithmetic dispatches only through one ",
                                    "`impl {}.{}` hook"
                                ),
                                type_name, trait_name
                            ),
                            format!("implement `{type_name}.{trait_name}`, or call a named method"),
                            Some(span),
                        ));
                        None
                    } else {
                        self.op_mismatch(op, &lt, &rt, span);
                        None
                    }
                } else if lt == Type::String && op == BinOp::Add {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        "text isn't joined with `+`".to_string(),
                        "there's one way to build text: interpolation (S8)".to_string(),
                        "write the pieces inside one string: \"{a}{b}\"".to_string(),
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
                            "{} works on {} only, but this has {} and {}",
                            operator_label(op),
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
            BinOp::Mod | BinOp::Rem | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                // D-SG9/D-MODSEM1: both remainders and the bitwise ops work on any
                // integer width,
                // both sides the same width, and keep it.
                if lt == rt && lt.is_integer() {
                    Some(lt)
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        format!(
                            "{} works on {} only, but this has {} and {}",
                            operator_label(op),
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
                    if !self.is_equatable_type(&lt) {
                        if crate::Sema::Diagnostics::is_secret_bearing_crypto_type(&lt) {
                            self.diags.push(Diagnostic::error(
                                "E0312",
                                format!("secret-bearing `{}` values cannot use {}", lt.name(), operator_label(op)),
                                "ordinary equality may leak secret information through timing and secret-bearing crypto types never implement comparison".to_string(),
                                "use `core.crypto.constant_time_equal` for `Secret` values; compare public keys through their canonical `.bytes()` values".to_string(),
                                Some(span),
                            ));
                        } else if let Some(field) = self.incomparable_field_type(&lt) {
                            self.diags.push(Diagnostic::error(
                                "E0312",
                                format!("`{}` can't be compared with `{}` because field `{}` doesn't support {}", lt.name(), rt.name(), field, operator_label(op)),
                                "value equality needs every field to support the comparison".to_string(),
                                "compare individual fields instead".to_string(),
                                Some(span),
                            ));
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0312",
                                format!("`{}` doesn't support {}", lt.name(), operator_label(op)),
                                "value equality requires the Equatable trait".to_string(),
                                "add `#Equatable` before the type, implement `Equatable` by hand, or compare individual fields".to_string(),
                                Some(span),
                            ));
                        }
                        return None;
                    }
                    // D-SMELLLINT1 / L0502: float `==` is almost always a bug.
                    if matches!(lt, Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::lint(
                            "L0502",
                            format!("comparing floats with {} is unreliable", operator_label(op)),
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
            BinOp::Compare => {
                if lt == rt
                    && (self.types_comparable_type(&lt)
                        || self.type_param_has_bound(&lt, COMPARABLE))
                {
                    Some(Type::Named(crate::Syntax::TYPE_ORDERING.to_string()))
                } else {
                    self.op_mismatch(op, &lt, &rt, span);
                    None
                }
            }
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                self.check_relational(op, &lt, &rt, span)
            }
            BinOp::And | BinOp::Or => unreachable!(),
        }
    }

    /// D-CHAINCMP1: the `</<=/>/>=` type-check, shared by a plain `Binary`
    /// pair and each adjacent pair of a `CompareChain`. No new *type* rule —
    /// every pair type-checks exactly as a standalone relational comparison.
    pub(crate) fn check_relational(
        &mut self,
        op: BinOp,
        lt: &Type,
        rt: &Type,
        span: Span,
    ) -> Option<Type> {
        if lt == rt && matches!(lt, Type::Int | Type::Float) {
            Some(Type::Bool)
        } else if lt == rt {
            let (normalized, registry, trait_reg) = self.capability_type_context(lt);
            if let Type::Named(name) = &normalized {
                if registry.is_distinct(name) {
                    if trait_reg.implements_trait(name, COMPARABLE) {
                        return Some(Type::Bool);
                    }
                    self.diags.push(e0905(name, COMPARABLE, span, false));
                    return None;
                }
            }
            if self.types_comparable_type(lt) || self.type_param_has_bound(lt, COMPARABLE) {
                Some(Type::Bool)
            } else if *lt == Type::String {
                self.diags.push(Diagnostic::error(
                    "E0109",
                    format!("text isn't ordered with {}", operator_label(op)),
                    "comparing text for order isn't supported yet".to_string(),
                    "compare with `==` or `!=`, or compare lengths/numbers instead".to_string(),
                    Some(span),
                ));
                None
            } else {
                self.op_mismatch(op, lt, rt, span);
                None
            }
        } else {
            self.op_mismatch(op, lt, rt, span);
            None
        }
    }

    /// D-CHAINCMP1: `0 <= sev < 10` — infer each operand once, type-check
    /// every adjacent pair via `check_relational`. The chain's own type is
    /// `Bool`; a bad pair pushes its own diagnostic and the overall result
    /// is `None` (mirrors a plain mismatched `Binary` comparison).
    pub(crate) fn infer_compare_chain(
        &mut self,
        operands: &mut [Expr],
        ops: &[BinOp],
        hooks: &mut Vec<bool>,
        span: Span,
    ) -> Option<Type> {
        let types: Vec<Option<Type>> = operands
            .iter_mut()
            .map(|e| {
                self.borrow_ctx = self.operator_operand_needs_borrow(e, BinOp::Lt);
                self.infer(e)
            })
            .collect();
        hooks.clear();
        let mut ok = true;
        for (i, op) in ops.iter().enumerate() {
            match (&types[i], &types[i + 1]) {
                (Some(lt), Some(rt)) => {
                    let uses_hook = lt == rt
                        && match lt {
                            Type::Named(type_name) => {
                                self.type_implements_trait_for_name(
                                    type_name,
                                    crate::Syntax::TRAIT_COMPARABLE,
                                ) || self.type_param_has_bound(lt, crate::Syntax::TRAIT_COMPARABLE)
                            }
                            _ => false,
                        };
                    hooks.push(uses_hook);
                    if uses_hook
                        && self.fn_name == "compare"
                        && self
                            .lookup(crate::Syntax::KW_SELF)
                            .is_some_and(|info| info.ty == *lt)
                    {
                        self.diags.push(Diagnostic::error(
                            "E0361",
                            format!("`compare` calls itself through {}", operator_label(*op)),
                            "the operator symbol dispatches back to this same hook, so evaluation would recurse forever".to_string(),
                            "compare the value's fields or call a different named helper inside the hook".to_string(),
                            Some(span),
                        ));
                        ok = false;
                        continue;
                    }
                    if self.check_relational(*op, lt, rt, span).is_none() {
                        ok = false;
                    }
                }
                _ => {
                    hooks.push(false);
                    ok = false;
                }
            }
        }
        if ok {
            Some(Type::Bool)
        } else {
            None
        }
    }

    pub(crate) fn op_mismatch(&mut self, op: BinOp, lt: &Type, rt: &Type, span: Span) {
        let (why, fix) = if lt.is_numeric() && rt.is_numeric() {
            (
                format!(
                    "neither {} contains every value of {}, nor {} every value of {}",
                    lt.name(),
                    rt.name(),
                    rt.name(),
                    lt.name()
                ),
                "choose the result type and convert the other side into it".to_string(),
            )
        } else if lt == rt {
            (
                format!("{} is not defined for {}", operator_label(op), lt.show()),
                format!(
                    "use an operation supported by {}, or call a named method",
                    lt.show()
                ),
            )
        } else {
            (
                "the two sides of an operator must be the same type".to_string(),
                "make both sides the same type".to_string(),
            )
        };
        self.diags.push(Diagnostic::error(
            "E0109",
            format!(
                "{} can't compare or combine {} and {}",
                operator_label(op),
                lt.show(),
                rt.show()
            ),
            why,
            fix,
            Some(span),
        ));
    }

    fn quantity_dimension(&self, ty: &Type) -> Option<Dimension> {
        if let Some((_, dimension)) = ty.quantity_parts() {
            return Some(dimension);
        }
        let Type::Named(name) = ty else {
            return None;
        };
        if let Some(dimension) = self.registry.unit_dimension(name) {
            return Some(dimension);
        }
        if let Some((module, leaf)) = name.split_once('.') {
            return self.modules.and_then(|modules| {
                modules
                    .iter()
                    .find(|candidate| candidate.module_alias == module)
                    .and_then(|candidate| candidate.registry.unit_dimension(leaf))
            });
        }
        // A local nominal shadows imported names; do not borrow an unrelated
        // module's dimension merely because its member has the same spelling.
        if self.registry.contains(name) {
            return None;
        }
        None
    }

    fn quantity_base_is_compatible(&self, left: &Type, right: &Type) -> bool {
        let base = |ty: &Type| {
            if let Some((base, _)) = ty.quantity_parts() {
                base.clone()
            } else if self.quantity_dimension(ty).is_some() {
                Type::Float
            } else {
                ty.clone()
            }
        };
        base(left) == Type::Float
            && base(right) == Type::Float
            && !matches!(
                (left, right),
                (Type::Named(name), _) if name == crate::Syntax::TYPE_INSTANT
            )
            && !matches!(
                (left, right),
                (_, Type::Named(name)) if name == crate::Syntax::TYPE_INSTANT
            )
    }

    fn dimension_mismatch(&mut self, op: BinOp, left: Dimension, right: Dimension, span: Span) {
        self.diags.push(Diagnostic::error(
            "E0359",
            format!(
                "{} can't combine {} with {}",
                operator_label(op),
                left.display_name(),
                right.display_name()
            ),
            "physical quantities must have compatible dimensions before they can be added, subtracted, or compared".to_string(),
            "use matching dimensions, or use `*` or `/` to derive a new dimension".to_string(),
            Some(span),
        ));
    }

    fn dimension_overflow(&mut self, op: BinOp, span: Span) {
        self.diags.push(Diagnostic::error(
            "E0359",
            format!(
                "{} makes the physical dimension too large",
                operator_label(op)
            ),
            "physical dimension exponents are checked compiler facts and cannot overflow"
                .to_string(),
            "simplify the repeated physical multiplication or division".to_string(),
            Some(span),
        ));
    }

    // --- calls -----------------------------------------------------------
}

/// RHS forms that need a surrounding expected type (`.{…}`, `.Variant`).
fn expr_wants_expected_type(expr: &Expr) -> bool {
    match expr {
        Expr::Paren(inner, _) | Expr::Unary(crate::AST::UnOp::Neg, inner, _) => {
            expr_wants_expected_type(inner)
        }
        Expr::Float(..) => true,
        Expr::StructLit { inferred: true, .. } => true,
        Expr::TypedLit { head: None, .. } => true,
        Expr::EnumLit { type_name, .. } if type_name.is_empty() => true,
        _ => false,
    }
}
