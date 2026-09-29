//! Type inference: S71 / D-SUGAR6 optional method chaining.
//!
//! The parser keeps `receiver?.method(args)` as the postfix call of an
//! optional member (`CallValue` over `OptField`). Sema rewrites it to the
//! value-`if` every tier already lowers:
//! `if receiver == Val(tmp) -> Val(tmp.method(args)) else -> None`, flattening
//! when the method already returns `T?`. A named receiver is copied first, so
//! the pattern test never consumes the caller's binding.

use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{Expr, Pattern, Type};

/// `receiver?.method(args)` as parsed: a call whose callee is `receiver?.method`.
pub(crate) fn is_optional_method_call(expr: &Expr) -> bool {
    matches!(expr, Expr::CallValue { callee, .. } if matches!(callee.as_ref(), Expr::OptField { .. }))
}

fn receiver_is_place(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(..) => true,
        Expr::Field(base, _, _)
        | Expr::OptField { base, .. }
        | Expr::Index { base, .. }
        | Expr::Paren(base, _) => receiver_is_place(base),
        _ => false,
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn infer_optional_method_call(&mut self, e: &mut Expr) -> Option<Type> {
        let placeholder = e.span();
        let Expr::CallValue {
            callee,
            args,
            span: call_span,
        } = std::mem::replace(e, Expr::Absent(placeholder))
        else {
            unreachable!("an optional method call is a call through `?.`");
        };
        let Expr::OptField {
            base,
            member,
            member_span,
            ..
        } = *callee
        else {
            unreachable!("an optional method call's callee is `?.member`");
        };
        let base_span = base.span();
        let full = Span::new(base_span.start, call_span.end);
        let binding = format!("__jet_optional_receiver_{}", member_span.start);
        let mut subject = if receiver_is_place(&base) {
            Expr::Copy(base, base_span)
        } else {
            *base
        };
        let mut call = Expr::MethodCall {
            receiver: Box::new(Expr::Ident(binding.clone(), base_span)),
            method: member,
            method_span: member_span,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args,
            recv_type: None,
            resolved_ret: None,
            operator_rhs: None,
            checked_widen: false,
        };

        // Probe the receiver and the call once to learn the lift. The probe's
        // diagnostics and flow facts are discarded; the real check below
        // reports every diagnostic exactly once.
        let saved_expected = self.expected_type.take();
        let diagnostics_start = self.diags.len();
        let flow = self.flow.clone();
        let mut probe_subject = subject.clone();
        let subject_ty = self.infer(&mut probe_subject);
        let payload_ty = match subject_ty.as_ref() {
            Some(Type::Option(inner)) => Some((**inner).clone()),
            _ => None,
        };
        let call_ty = payload_ty.as_ref().and_then(|payload| {
            self.push_scope();
            let _ = self.declare_condition_binding(&binding, base_span, payload.clone());
            let mut probe_call = call.clone();
            let call_ty = self.infer(&mut probe_call);
            self.pop_scope();
            call_ty
        });
        self.diags.truncate(diagnostics_start);
        self.flow = flow;
        self.expected_type = saved_expected;

        let Some(payload_ty) = payload_ty else {
            if let Some(other) = self.infer(&mut subject) {
                self.diags.push(Diagnostic::error(
                    "E0047",
                    format!(
                        "`?.` needs an optional on the left, but this is `{}`",
                        other.show()
                    ),
                    "optional chaining short-circuits a `T?` to absent on a missing link"
                        .to_string(),
                    "use plain `.` here, or make the value optional first".to_string(),
                    Some(member_span),
                ));
            }
            *e = subject;
            return None;
        };
        let Some(call_ty) = call_ty else {
            self.push_scope();
            let _ = self.declare_condition_binding(&binding, base_span, payload_ty);
            let _ = self.infer(&mut call);
            self.pop_scope();
            *e = subject;
            return None;
        };

        let flatten = matches!(call_ty, Type::Option(_));
        let result_ty = if flatten {
            call_ty
        } else {
            Type::Option(Box::new(call_ty))
        };
        let then_value = if flatten {
            call
        } else {
            Expr::Present(Box::new(call), full)
        };
        *e = Expr::If {
            cond: Box::new(Expr::PatternTest {
                subject: Box::new(subject),
                pattern: Pattern::Present {
                    binding,
                    binding_span: base_span,
                    span: base_span,
                },
                span: base_span,
            }),
            then_body: Vec::new(),
            then_value: Box::new(then_value),
            else_body: Vec::new(),
            else_value: Box::new(Expr::Absent(full)),
            span: full,
        };
        let saved_expected = self.expected_type.replace(result_ty);
        let checked = self.infer(e);
        self.expected_type = saved_expected;
        checked
    }
}
