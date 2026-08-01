//! Type inference: binary operators and overflow/op-mismatch checks.
//!
//! Split out of the original `CheckerInfer.rs`; behavior unchanged.

use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{substitute_type, COMPARABLE};
use crate::AST::{BinOp, Dimension, Expr, Type};
use std::collections::HashMap;

impl<'a> Checker<'a> {
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

    /// D-NUMLIT-PEER1=A / D-INTLIT-WIDTH1=F: a bare numeral adopts a *fixed-width*
    /// peer that contains its value. With no sized peer (or an `Int` peer), it
    /// stays `Int` — peerless `1000000 * 1000000` and `0 - 17` must not invent a
    /// U32/U8 and trap. Destination width still arrives through `expected_type`
    /// on `Expr::Int` (e.g. `take_u8(1 + 2)`).
    fn minimal_integer_literal_type(expr: &mut Expr, peer: Option<&Type>) -> Option<Type> {
        match expr {
            Expr::Paren(inner, _) => Self::minimal_integer_literal_type(inner, peer),
            Expr::Unary(crate::AST::UnOp::Neg, inner, _) => {
                let Expr::Int(value, _, width, _) = inner.as_mut() else {
                    return None;
                };
                if width.is_some() {
                    return None;
                }
                let negated = -(*value as i128);
                match peer {
                    Some(Type::IntN { signed, bits }) => {
                        let (lower, upper) = crate::AST::int_range(*signed, *bits);
                        if negated >= lower && negated <= upper {
                            *width = Some((*signed, *bits));
                            Some(Type::IntN {
                                signed: *signed,
                                bits: *bits,
                            })
                        } else {
                            None
                        }
                    }
                    Some(Type::Int) | None => {
                        *width = None;
                        Some(Type::Int)
                    }
                    Some(_) => {
                        *width = None;
                        Some(Type::Int)
                    }
                }
            }
            Expr::Int(value, _, width, _) if *value >= 0 && width.is_none() => {
                let value = *value as i128;
                match peer {
                    Some(Type::IntN { signed, bits }) => {
                        let (lower, upper) = crate::AST::int_range(*signed, *bits);
                        if value >= lower && value <= upper {
                            *width = Some((*signed, *bits));
                            Some(Type::IntN {
                                signed: *signed,
                                bits: *bits,
                            })
                        } else {
                            None
                        }
                    }
                    Some(Type::Int) | None => {
                        *width = None;
                        Some(Type::Int)
                    }
                    Some(_) => {
                        *width = None;
                        Some(Type::Int)
                    }
                }
            }
            _ => None,
        }
    }

    fn take_numeric_approx_operand(expr: &mut Expr, span: Span) -> Option<Expr> {
        match expr {
            Expr::Paren(inner, _) => Self::take_numeric_approx_operand(inner, span),
            Expr::Call(call)
                if call.name == Type::APPROX_NUMERIC_WIDEN_MARKER
                    && call.args.len() == 1 =>
            {
                Some(std::mem::replace(
                    &mut call.args[0].expr,
                    Expr::Absent(span),
                ))
            }
            _ => None,
        }
    }

    fn contextualize_numeric_literal(
        &mut self,
        expr: &mut Expr,
        target: &Type,
    ) -> Option<Type> {
        match expr {
            Expr::Paren(inner, _) => self.contextualize_numeric_literal(inner, target),
            Expr::Unary(crate::AST::UnOp::Neg, inner, _) => match (inner.as_mut(), target) {
                (Expr::Int(value, span, width, _), Type::IntN { signed: true, bits }) => {
                    let negated = -(*value as i128);
                    let (lower, upper) = crate::AST::int_range(true, *bits);
                    if negated < lower || negated > upper {
                        self.diags
                            .push(crate::Sema::int_range_error(true, *bits, *span));
                    }
                    *width = Some((true, *bits));
                    Some(target.clone())
                }
                (_, Type::IntN { signed: false, .. }) => None,
                _ => self.contextualize_numeric_literal(inner, target),
            },
            Expr::Int(value, span, width, _) => match target {
                Type::Int => {
                    *width = None;
                    Some(Type::Int)
                }
                Type::IntN { signed, bits } => {
                    let (lower, upper) = crate::AST::int_range(*signed, *bits);
                    if (*value as i128) < lower || (*value as i128) > upper {
                        self.diags
                            .push(crate::Sema::int_range_error(*signed, *bits, *span));
                    }
                    *width = Some((*signed, *bits));
                    Some(target.clone())
                }
                Type::Float | Type::Float32 => {
                    let exact = if *target == Type::Float32 {
                        (*value as f32) as i128 == *value as i128
                    } else {
                        (*value as f64) as i128 == *value as i128
                    };
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
                                limit,
                                limit
                            ),
                            Some(*span),
                        ));
                    }
                    *expr = Expr::Float(*value as f64, *span, *target == Type::Float32);
                    Some(target.clone())
                }
                _ => None,
            },
            Expr::Float(_, _, is_f32) => match target {
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
    pub(crate) fn widen_numeric_expr(
        &mut self,
        expr: &mut Expr,
        source: &Type,
        target: &Type,
    ) {
        if source == target {
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
        let checked = approximate.is_none();

        if checked {
            if let Expr::Float(_, _, is_f32) = expr {
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
            type_args: Vec::new(),
            args: vec![crate::AST::CallArg {
                convention: crate::AST::AccessConvention::Read,
                expr: old,
                span,
                flags: crate::AST::CallArgFlags::default(),
                label: None,
                spread: false,
            }],
            recv_type: (widening && checked)
                .then(|| Type::CHECKED_NUMERIC_WIDEN_MARKER.to_string()),
            resolved_ret: Some(target.clone()),
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

    fn unit_raw(expr: Expr, type_name: &str, span: Span) -> Expr {
        Expr::MethodCall {
            receiver: Box::new(expr),
            method: "raw".to_string(),
            method_span: span,
            type_args: Vec::new(),
            args: Vec::new(),
            recv_type: Some(type_name.to_string()),
            resolved_ret: Some(Type::Float),
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
            Box::new(Self::unit_raw(left, left_name, span)),
            Box::new(Self::unit_raw(right, right_name, span)),
            span,
        );
        Expr::MethodCall {
            receiver: Box::new(Expr::Ident(destination_name.to_string(), span)),
            method: Syntax::numeric_conversion_method("Float")
                .expect("Float conversion is registered")
                .to_string(),
            method_span: span,
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
                let (name, subst) = match owner {
                    Type::Named(name) => (name, HashMap::new()),
                    Type::Apply { name, args } => {
                        let params = self.trait_reg.struct_params.get(&name)?;
                        let subst = params
                            .iter()
                            .zip(args)
                            .map(|(param, arg)| (param.name.clone(), arg))
                            .collect();
                        (name, subst)
                    }
                    _ => return None,
                };
                self.registry
                    .struct_fields(&name)?
                    .iter()
                    .find(|(candidate, _, _, _)| candidate == field)
                    .map(|(_, _, ty, _)| substitute_type(ty, &subst))
            }
            _ => None,
        }
    }

    fn operator_operand_needs_borrow(&self, expr: &Expr, op: BinOp) -> bool {
        if !matches!(expr, Expr::Field(..) | Expr::Index { .. }) {
            return false;
        }
        let trait_name = match op {
            BinOp::Add => crate::Syntax::TRAIT_ADD,
            BinOp::Sub => crate::Syntax::TRAIT_SUB,
            BinOp::Mul => crate::Syntax::TRAIT_MUL,
            BinOp::Div => crate::Syntax::TRAIT_DIV,
            BinOp::Eq | BinOp::Ne => crate::Syntax::TRAIT_EQUATABLE,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => crate::Syntax::TRAIT_COMPARABLE,
            _ => return false,
        };
        self.operator_expr_type(expr)
            .is_some_and(|ty| match &ty {
                Type::Named(name) => {
                    !self.registry.is_distinct(name)
                        && (self.trait_reg
                            .trait_impls
                            .contains(&(name.clone(), trait_name.to_string()))
                            || self.type_param_has_bound(&ty, trait_name))
                }
                _ => false,
            })
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
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
        ) {
            self.expected_type = lt
                .as_ref()
                .filter(|_| expr_wants_expected_type(rhs))
                .cloned();
        }
        let rt = self.infer(rhs);
        self.expected_type = saved_expected;
        let (mut lt, mut rt) = (lt?, rt?);

        // D-INTLIT-WIDTH1=F / D-NUMLIT-PEER1=A: an unowned whole literal adopts a
        // fixed-width peer that contains its singleton value; without a sized
        // peer it stays `Int`. The ordinary value-set law then decides whether
        // one operand widens to the other.
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
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
        ) || (matches!(op, BinOp::Rem | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor)
            && lt.is_integer()
            && rt.is_integer());
        if joins_numeric && lt != rt {
            if let Some(join) = lt.numeric_join(&rt) {
                self.widen_numeric_expr(lhs, &lt, &join);
                self.widen_numeric_expr(rhs, &rt, &join);
                lt = join.clone();
                rt = join;
            }
        }

        // D-OPDEF1=A: user operators are ordinary trait-method calls after sema
        // proves one exact impl and the fixed same-type law.
        if lt == rt {
            if let Type::Named(type_name) = &lt {
                let hook = match op {
                    BinOp::Add => Some((crate::Syntax::TRAIT_ADD, "add", lt.clone())),
                    BinOp::Sub => Some((crate::Syntax::TRAIT_SUB, "sub", lt.clone())),
                    BinOp::Mul => Some((crate::Syntax::TRAIT_MUL, "mul", lt.clone())),
                    BinOp::Div => Some((crate::Syntax::TRAIT_DIV, "div", lt.clone())),
                    BinOp::Eq | BinOp::Ne => {
                        Some((crate::Syntax::TRAIT_EQUATABLE, "equal", Type::Bool))
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Some((
                        crate::Syntax::TRAIT_COMPARABLE,
                        "compare",
                        Type::Named(crate::Syntax::TYPE_ORDERING.to_string()),
                    )),
                    _ => None,
                };
                if let Some((trait_name, method, ret)) = hook {
                    if !self.registry.is_distinct(type_name)
                        && (self
                            .trait_reg
                            .trait_impls
                            .contains(&(type_name.clone(), trait_name.to_string()))
                            || self.type_param_has_bound(&lt, trait_name))
                    {
                        if self.fn_name == method
                            && self
                                .lookup(crate::Syntax::KW_SELF)
                                .is_some_and(|info| info.ty == lt)
                        {
                            self.diags.push(Diagnostic::error(
                                "E0361",
                                format!("`{}` calls itself through `{}`", method, op.spell()),
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
                            type_args: Vec::new(),
                            args: vec![crate::AST::CallArg {
                                convention: crate::AST::AccessConvention::Read,
                                expr: *right,
                                span,
                                flags: crate::AST::CallArgFlags::default(),
                                label: None,
                                spread: false,
                            }],
                            recv_type: Some(type_name.clone()),
                            resolved_ret: Some(ret),
                        };
                        *replacement = Some(match op {
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
                                        args: Vec::new(),
                                        span,
                                    }),
                                    span,
                                )
                            }
                            _ => call,
                        });
                        return Some(if op.is_comparison() {
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
                            self.convert_unit_operand(lhs, lname, lfact, dest_name, dest_fact, span);
                            self.convert_unit_operand(rhs, rname, rfact, dest_name, dest_fact, span);
                            return Some(Type::Named(dest_name.clone()));
                        }
                        (BinOp::Add, Point, Delta) | (BinOp::Sub, Point, Delta) => {
                            let Some(delta_name) = self.counterpart_unit_type(lname, lfact, Delta) else {
                                return None;
                            };
                            let (_, delta_fact) = self
                                .unit_fact_for_type(&Type::Named(delta_name.clone()))
                                .expect("counterpart unit fact");
                            if self.reject_implicit_unit_conversion(
                                &delta_name,
                                rname,
                                rhs,
                                span,
                            ) {
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
                                op, left, lname, right, &delta_name, lname, span,
                            ));
                            return Some(Type::Named(lname.clone()));
                        }
                        (BinOp::Add, Delta, Point) => {
                            let Some(delta_name) = self.counterpart_unit_type(rname, rfact, Delta) else {
                                return None;
                            };
                            let (_, delta_fact) = self
                                .unit_fact_for_type(&Type::Named(delta_name.clone()))
                                .expect("counterpart unit fact");
                            if self.reject_implicit_unit_conversion(
                                &delta_name,
                                lname,
                                lhs,
                                span,
                            ) {
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
                                BinOp::Add, right, rname, left, &delta_name, rname, span,
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
                            let Some(delta_name) = self.counterpart_unit_type(point_name, point_fact, Delta) else {
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
                                format!("operator `{}` is not available between `{}` and `{}`", op.spell(), lname, rname),
                                "affine points are positions; only point + delta, point - delta, point - point, and delta + delta are defined".to_string(),
                                "subtract two points for a delta, or add a matching Delta to a point".to_string(),
                                Some(span),
                            ));
                            return None;
                        }
                        (BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge, left_kind, right_kind)
                            if left_kind == right_kind =>
                        {
                            let left_wins = lfact.scale.abs() <= rfact.scale.abs();
                            let (dest_name, dest_fact) = if left_wins { (lname, lfact) } else { (rname, rfact) };
                            let source_name = if left_wins { rname } else { lname };
                            let source_expr = if left_wins { &**rhs } else { &**lhs };
                            if self.reject_implicit_unit_conversion(
                                dest_name,
                                source_name,
                                source_expr,
                                span,
                            ) {
                                return Some(Type::Bool);
                            }
                            self.convert_unit_operand(lhs, lname, lfact, dest_name, dest_fact, span);
                            self.convert_unit_operand(rhs, rname, rfact, dest_name, dest_fact, span);
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
                if self.registry.distinct_range(distinct_name).is_some() {
                    return self.registry.distinct_base(distinct_name).cloned();
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
                // D-VERDICT-1304-1: the widening join above rewrites both
                // operands to one numeric type. Arithmetic keeps that type.
                if lt == rt && lt.is_numeric() {
                    Some(lt)
                } else if let Type::Named(type_name) = &lt {
                    if self.registry.contains(type_name) {
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
                                "no `{}` operator is defined for `{}`",
                                op.spell(),
                                type_name
                            ),
                            format!(
                                concat!(
                                    "user arithmetic dispatches only through one ",
                                    "`impl {}.{}` hook"
                                ),
                                type_name,
                                trait_name
                            ),
                            format!(
                                "implement `{type_name}.{trait_name}`, or call a named method"
                            ),
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
                    if !crate::Sema::Diagnostics::is_equatable(
                        &lt,
                        self.registry,
                        self.trait_reg,
                    ) {
                        if crate::Sema::Diagnostics::is_secret_bearing_crypto_type(&lt) {
                            self.diags.push(Diagnostic::error(
                                "E0312",
                                format!("secret-bearing `{}` values cannot use `{}`", lt.name(), op.spell()),
                                "ordinary equality may leak secret information through timing and secret-bearing crypto types never implement comparison".to_string(),
                                "use `core.crypto.constant_time_equal` for `Secret` values; compare public keys through their canonical `.bytes()` values".to_string(),
                                Some(span),
                            ));
                        } else if let Some(field) = incomparable_field(&lt, self.registry) {
                            self.diags.push(Diagnostic::error(
                                "E0312",
                                format!("`{}` can't be compared with `{}` because field `{}` doesn't support `{}`", lt.name(), rt.name(), field, op.spell()),
                                "value equality needs every field to support the comparison".to_string(),
                                "compare individual fields instead".to_string(),
                                Some(span),
                            ));
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0312",
                                format!("`{}` doesn't support `{}`", lt.name(), op.spell()),
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
        } else if lt == rt
            && (types_comparable(lt, self.registry) || self.type_param_has_bound(lt, COMPARABLE))
        {
            Some(Type::Bool)
        } else if lt == rt && *lt == Type::String {
            self.diags.push(Diagnostic::error(
                "E0109",
                format!("text isn't ordered with `{}`", op.spell()),
                "comparing text for order isn't supported yet".to_string(),
                "compare with `==` or `!=`, or compare lengths/numbers instead".to_string(),
                Some(span),
            ));
            None
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
                                !self.registry.is_distinct(type_name)
                                    && (self.trait_reg.trait_impls.contains(&(
                                        type_name.clone(),
                                        crate::Syntax::TRAIT_COMPARABLE.to_string(),
                                    )) || self.type_param_has_bound(
                                        lt,
                                        crate::Syntax::TRAIT_COMPARABLE,
                                    ))
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
                            format!("`compare` calls itself through `{}`", op.spell()),
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
        } else {
            (
                "the two sides of an operator must be the same type".to_string(),
                "make both sides the same type".to_string(),
            )
        };
        self.diags.push(Diagnostic::error(
            "E0109",
            format!(
                "`{}` can't compare or combine {} and {}",
                op.spell(),
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
        base(left) == Type::Float && base(right) == Type::Float
    }

    fn dimension_mismatch(
        &mut self,
        op: BinOp,
        left: Dimension,
        right: Dimension,
        span: Span,
    ) {
        self.diags.push(Diagnostic::error(
            "E0359",
            format!(
                "`{}` can't combine {} with {}",
                op.spell(),
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
            format!("`{}` makes the physical dimension too large", op.spell()),
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
