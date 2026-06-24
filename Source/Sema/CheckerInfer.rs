use super::*;
use crate::AST::{
    AccessConvention, BinOp, Call, EnumLitArg,
    Expr, IndexKind, Lambda, LambdaBody,
    OrFallback, Pattern, Stmt, StrPart, TryConvert, Type,
    UnOp,
};
use crate::Collections::{self, is_map_key_type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{
    e0901, e0904, COMPARABLE,
};
use crate::Syntax;
use std::collections::{HashMap, HashSet};

/// D-ITER1: returns true when `ty` or an immediate inner layer is `Type::Tuple`.
/// Used to decide whether to store `resolved_ret` on a `MethodCall` node so that
/// `Tuples.rs` can collect the JetTup_ shape for `enumerate`/`zip`/`partition`.
fn contains_tuple_type(ty: &Type) -> bool {
    match ty {
        Type::Tuple(_) => true,
        Type::List(inner) => matches!(inner.as_ref(), Type::Tuple(_)),
        _ => false,
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn infer_name_or(&mut self, e: &mut Expr, fallback: &str) -> String {
        self.infer(e)
            .map(|t| t.name())
            .unwrap_or_else(|| fallback.to_string())
    }

    /// Infer and check an expression. Returns None when a problem was
    /// already reported (avoids error cascades).
    ///
    /// This wrapper owns two rules that depend on *where* the expression
    /// appears (`borrow_ctx`):
    ///  - a struct-field read in owning position is rewritten to `.clone()`
    ///    so the generated Rust never moves a field out of its struct;
    ///  - a `-> view` call result may only be read in place (borrow
    ///    positions); storing or giving it away is E0206.
    pub(crate) fn infer(&mut self, e: &mut Expr) -> Option<Type> {
        let borrowed = std::mem::take(&mut self.borrow_ctx);
        let ty = self.infer_inner(e);
        if !borrowed {
            if self.is_view_call(e) {
                self.diags.push(Diagnostic::error(
                    "E0206",
                    "this borrowed view can only be read where it is".to_string(),
                    "a `view` result points into someone else's value, so it can't be stored or given away".to_string(),
                    "read it in place (print it, compare a field, call a method on it), or call a function that returns an owned value".to_string(),
                    Some(e.span()),
                ));
                return None;
            }
            if let Some(t) = &ty {
                if !type_is_copy(t) && field_read_to_clone(e, self.registry, self.imports) {
                    let span = e.span();
                    let old = std::mem::replace(e, Expr::Absent(span));
                    *e = Expr::MethodCall {
                        receiver: Box::new(old),
                        method: "clone".to_string(),
                        method_span: span,
                        type_args: Vec::new(),
                        args: Vec::new(),
                        recv_type: None,
                            resolved_ret: None,
                    };
                }
            }
        }
        ty
    }

    /// Whether `e` is a call to something declared `-> view T` (its Rust
    /// value is a reference).
    pub(crate) fn is_view_call(&self, e: &Expr) -> bool {
        match e {
            Expr::Call(c) => self.funcs.get(&c.name).is_some_and(|s| s.is_view_return),
            Expr::MethodCall {
                recv_type: Some(t),
                method,
                ..
            } => self
                .registry
                .method(t, method)
                .is_some_and(|m| m.is_view_return),
            Expr::MethodCall {
                receiver, method, ..
            } => {
                // Cross-file call through an import alias.
                if let Expr::Ident(alias, _) = receiver.as_ref() {
                    if let (Some(&idx), Some(mods)) = (self.imports.get(alias), self.modules) {
                        return mods[idx]
                            .funcs
                            .get(method)
                            .is_some_and(|s| s.is_view_return);
                    }
                }
                false
            }
            _ => false,
        }
    }

    pub(crate) fn infer_inner(&mut self, e: &mut Expr) -> Option<Type> {
        match e {
            // S68 (D-SG2): `if` in expression position. Condition is Bool; each
            // branch's trailing expression is its value, and both must agree.
            Expr::If {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                span,
            } => {
                let span = *span;
                let before = self.moved.clone();
                let mut after = before.clone();
                self.require_bool(cond, "an `if` used as a value");
                self.push_scope();
                self.check_block(then_body, false);
                let then_ty = self.infer(then_value);
                self.pop_scope();
                for (k, v) in self.moved.drain() {
                    after.entry(k).or_insert(v);
                }
                self.moved = before.clone();
                self.push_scope();
                self.check_block(else_body, false);
                let else_ty = self.infer(else_value);
                self.pop_scope();
                for (k, v) in self.moved.drain() {
                    after.entry(k).or_insert(v);
                }
                self.moved = after;
                match (then_ty, else_ty) {
                    (Some(a), Some(b)) => {
                        // D-TOOL2: `todo` is diverging; if one branch is a
                        // typed hole, the other branch's type wins.
                        let then_is_todo = matches!(then_value.as_ref(), Expr::Todo { .. });
                        let else_is_todo = matches!(else_value.as_ref(), Expr::Todo { .. });
                        if a == b || else_is_todo {
                            // Update the todo's expected_type to match what we know.
                            if else_is_todo {
                                if let Expr::Todo { expected_type, .. } = else_value.as_mut() {
                                    *expected_type = Some(a.name());
                                }
                            }
                            Some(a)
                        } else if then_is_todo {
                            if let Expr::Todo { expected_type, .. } = then_value.as_mut() {
                                *expected_type = Some(b.name());
                            }
                            Some(b)
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0124",
                                format!(
                                    "this `if`'s branches produce different types: {} and {}",
                                    a.show(),
                                    b.show()
                                ),
                                "an `if` used as a value must give the same type on every path (S68)"
                                    .to_string(),
                                format!(
                                    "make both branches produce {} (or the same type)",
                                    a.show()
                                ),
                                Some(span),
                            ));
                            Some(a)
                        }
                    }
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                }
            }
            // D-SG9: an untyped integer literal is `Int` by default, but adopts
            // a fixed-width integer type when one is expected here (binding/param/
            // return annotation, sized arithmetic). A literal that doesn't fit the
            // width is rejected (E1003) — there is no silent truncation.
            Expr::Int(n, span, width) => {
                let (n, span) = (*n, *span);
                if let Some(Type::IntN { signed, bits }) = self.expected_type.clone() {
                    let (lo, hi) = crate::AST::int_range(signed, bits);
                    if (n as i128) < lo || (n as i128) > hi {
                        self.diags.push(int_range_error(signed, bits, span));
                    }
                    *width = Some((signed, bits));
                    Some(Type::IntN { signed, bits })
                } else {
                    *width = None;
                    Some(Type::Int)
                }
            }
            // D-SG9/D-FLOATW1: a float literal is `Float` (f64) by default, but
            // adopts `F32` when that width is expected here. Write the resolution
            // back onto the AST so TIR lowering can read the width from the node.
            Expr::Float(_, _, is_f32) => {
                if matches!(self.expected_type, Some(Type::Float32)) {
                    *is_f32 = true;
                    Some(Type::Float32)
                } else {
                    Some(Type::Float)
                }
            }
            Expr::Bool(_, _) => Some(Type::Bool),
            Expr::Str(parts, _) => {
                for p in parts.iter_mut() {
                    if let StrPart::Interp(inner) = p {
                        // Interpolation borrows (`.jet_show()`); never moves.
                        self.borrow_ctx = true;
                        let t = self.infer(inner);
                        if let Some(t) = t {
                            if !is_printable(&t, self.registry) {
                                self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!("{} can't be put into text yet", t.show()),
                                    "interpolation shows printable values".to_string(),
                                    "show one of its parts instead".to_string(),
                                    Some(inner.span()),
                                ));
                            }
                        }
                    }
                }
                Some(Type::String)
            }
            Expr::Ident(name, span) => {
                if let Some(moved_at) = self.moved.get(name).copied() {
                    let (line_note, _) = (moved_at, ());
                    let _ = line_note;
                    self.diags.push(Diagnostic::error(
                        "E0121",
                        format!(
                            "`{}` was given away earlier, so it can't be used here",
                            name
                        ),
                        "after a value moves somewhere else, the old name no longer holds it"
                            .to_string(),
                        format!(
                            "give away a copy instead (`{}.clone()`) where it moved",
                            name
                        ),
                        Some(*span),
                    ));
                    self.moved.remove(name); // report once
                    return None;
                }
                // D-UNINIT1: reading a `#Uninit` binding before it is written.
                if self.uninit.contains_key(name) {
                    self.diags.push(Diagnostic::error(
                        "E0420",
                        format!("`{}` may be read before it is given a value", name),
                        format!(
                            "`{}` was declared `#Uninit`, so it holds no value until you write to it — this read could see garbage",
                            name
                        ),
                        format!(
                            "write to `{}` on every path before reading it (e.g. fill it via `mut {}`)",
                            name, name
                        ),
                        Some(*span),
                    ));
                    self.uninit.remove(name); // report once, then resolve its type below
                }
                // D-ALLOC2: E0632 — reading an arena `view` whose backing arena
                // was already `reset`/`free`d. (`alloc` and `reset`/`free` go
                // through the method-call path below, so reaching here means a
                // plain read of the view's value.)
                self.check_view_use(name, *span);
                if let Some(info) = self.lookup(name) {
                    return Some(info.ty.clone());
                }
                if let Some(t) = self.consts.get(name) {
                    return Some(t.clone());
                }
                if let Some(sig) = self.funcs.get(name) {
                    return Some(func_sig_to_fn_type(sig));
                }
                self.unknown_name(name, *span);
                None
            }
            Expr::Char(_, _) => Some(Type::Char),
            Expr::ListLit(elems, span) => self.infer_list_lit(elems, *span),
            Expr::TupleLit(fields, span, ty_slot) => {
                let t = self.infer_tuple_lit(fields, *span);
                *ty_slot = t.clone();
                t
            }
            Expr::MapLit(entries, span) => self.infer_map_lit(entries, *span),
            Expr::Index {
                base,
                index,
                span,
                kind,
            } => self.infer_index(base, index, span, kind),
            Expr::Slice {
                base,
                start,
                end,
                span,
            } => self.infer_slice(base, start, end, *span),
            Expr::Call(call) => {
                let span = call.name_span;
                // D-UNINIT1: a `mut` arg is the fill site, not a read.
                self.clear_uninit_mut_args(&call.args);
                match self.check_call(call, true) {
                    Some(Some(t)) => Some(t),
                    Some(None) => {
                        self.diags.push(Diagnostic::error(
                            "E0116",
                            format!("`{}` doesn't hand back a value", call.name),
                            "only calls that declare `-> Type` can be used as a value".to_string(),
                            format!(
                                "call `{}` on its own line, or give it a return type",
                                call.name
                            ),
                            Some(span),
                        ));
                        None
                    }
                    None => None,
                }
            }
            Expr::Unary(op, inner, span) => {
                // D-SG9: fold `-<intliteral>` *before* inferring the operand, so the
                // literal is range-checked at its negated value (`-128` fits `I8`)
                // and the operand's own positive range check doesn't fire spuriously.
                if let UnOp::Neg = op {
                    if let (Expr::Int(n, ispan, width), Some(Type::IntN { signed: true, bits })) =
                        (inner.as_mut(), self.expected_type.clone())
                    {
                        let v = -(*n as i128);
                        let (lo, hi) = crate::AST::int_range(true, bits);
                        if v < lo || v > hi {
                            self.diags.push(int_range_error(true, bits, *ispan));
                        }
                        *width = Some((true, bits));
                        return Some(Type::IntN { signed: true, bits });
                    }
                }
                let t = self.infer(inner)?;
                match op {
                    UnOp::Neg => {
                        if t.is_float()
                            || matches!(t, Type::Int | Type::IntN { signed: true, .. })
                        {
                            Some(t)
                        } else if let Type::IntN { bits, .. } = t {
                            // D-SG9: unsigned widths have no negatives.
                            self.diags.push(Diagnostic::error(
                                "E0109",
                                format!("`-` can't negate {}, which is unsigned", t.show()),
                                "unsigned numbers have no negative values".to_string(),
                                format!("use a signed type like `I{bits}` if you need negatives"),
                                Some(*span),
                            ));
                            None
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0109",
                                format!("`-` needs a number, but this is {}", t.show()),
                                "only Int and Float values can be negated".to_string(),
                                "use a number here".to_string(),
                                Some(*span),
                            ));
                            None
                        }
                    }
                    UnOp::Not => {
                        if t == Type::Bool {
                            Some(Type::Bool)
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0109",
                                format!(
                                    "`!` needs {}, but this is {}",
                                    Type::Bool.show(),
                                    t.show()
                                ),
                                "`!` flips a yes to a no and back".to_string(),
                                "compare the value to something first, e.g. `!(x > 0)`".to_string(),
                                Some(*span),
                            ));
                            None
                        }
                    }
                }
            }
            Expr::Binary(op, lhs, rhs, span) => {
                let (op, span) = (*op, *span);
                self.infer_binary(op, lhs, rhs, span)
            }
            Expr::Deref(inner, span) => {
                if !self.in_unsafe {
                    self.diags.push(Diagnostic::error(
                        "E0208",
                        "raw memory access requires unsafe".to_string(),
                        "`*` is a raw access operation; it is only valid inside a `#Unsafe { ... }` block"
                            .to_string(),
                        "remove `*`, or wrap this code in `#Unsafe { ... }`".to_string(),
                        Some(*span),
                    ));
                }
                self.infer(inner)
            }
            Expr::PtrFromAddr {
                alias,
                alias_span,
                elem,
                addr,
                span,
            } => self.infer_ptr_from_addr(alias, *alias_span, elem, addr, *span),
            Expr::Field(inner, member, span) => self.infer_field(inner, member, *span),
            Expr::OptField {
                base,
                member,
                member_span,
                flatten,
                ..
            } => {
                let bt = self.infer(base)?;
                let inner_t = match bt {
                    Type::Option(inner) => *inner,
                    other => {
                        self.diags.push(Diagnostic::error(
                            "E0047",
                            format!("`?.` needs an optional on the left, but this is `{}`", other.show()),
                            "optional chaining short-circuits a `T?` to absent on a missing link"
                                .to_string(),
                            "use plain `.` here, or make the value optional first".to_string(),
                            Some(*member_span),
                        ));
                        return None;
                    }
                };
                let fty = self.field_type(&inner_t, member, *member_span)?;
                match fty {
                    Type::Option(x) => {
                        *flatten = true;
                        Some(Type::Option(x))
                    }
                    t => Some(Type::Option(Box::new(t))),
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                method_span,
                type_args,
                args,
                recv_type,
                resolved_ret,
            } => {
                // D-UNINIT1: a `mut` arg is the fill site, not a read.
                self.clear_uninit_mut_args(args);
                self.infer_method_call(
                    receiver, method, *method_span, type_args, args, recv_type, resolved_ret,
                )
            }
            Expr::StructLit {
                type_name,
                type_args,
                import_ns,
                fields,
                span,
                ..
            } => Some(self.check_struct_lit(
                type_name,
                type_args,
                import_ns.as_deref(),
                fields,
                *span,
            )),
            Expr::EnumLit {
                type_name,
                variant,
                args,
                span,
            } => Some(self.check_enum_lit(type_name, variant, args, *span)),
            Expr::Present(inner, _span) => {
                let t = self.infer(inner)?;
                Some(Type::Option(Box::new(t)))
            }
            Expr::Absent(span) => {
                if let Some(expected) = self.expected_type.clone() {
                    if expected.unwrap_option().is_some() {
                        Some(expected)
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0308",
                            "bare `null` needs a known optional type here".to_string(),
                            format!(
                                "`{}` only fits where a `T?` is expected (S32)",
                                Syntax::LIT_NULL
                            ),
                            "add a type annotation, or use `null` where the type is already known"
                                .to_string(),
                            Some(*span),
                        ));
                        None
                    }
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0308",
                        "bare `null` needs a known optional type here".to_string(),
                        format!(
                            "`{}` only fits where a `T?` is expected (S32)",
                            Syntax::LIT_NULL
                        ),
                        "add a type annotation, or use `null` where the type is already known"
                            .to_string(),
                        Some(*span),
                    ));
                    None
                }
            }
            Expr::PatternTest {
                subject,
                pattern,
                span,
            } => {
                self.check_pattern_test(subject, pattern, *span);
                Some(Type::Bool)
            }
            Expr::Todo { expected_type, .. } => {
                // D-TOOL2 (E2-M11): `todo` is a typed hole — valid in any
                // position. Fill the expected-type field so codegen can print
                // it in the panic message, then return that type (or the
                // fallback Unit so callers that require Some(…) are satisfied).
                let ty = self.expected_type.clone();
                if let Some(ref t) = ty {
                    *expected_type = Some(t.name());
                } else {
                    *expected_type = Some("(unknown)".to_string());
                }
                // Return the expected type so the surrounding expression sees
                // a consistent type. If no expected type is known, return Unit.
                // todo is diverging — it never returns, so any type is OK.
                // When the expected type is unknown, return Int as a placeholder
                // so callers that need Some(…) don't see a false type error.
                Some(ty.unwrap_or(Type::Int))
            }
            Expr::Ok(inner, span) => self.infer_ok(inner, *span),
            Expr::Err(inner, span) => self.infer_err(inner, *span),
            Expr::Try(inner, span, convert) => self.infer_try(inner, *span, convert),
            Expr::OrFallback {
                value,
                fallback,
                span,
                is_option,
            } => self.infer_or_fallback(value, fallback, *span, is_option),
            Expr::Lambda(lam) => {
                let expected = self.expected_type.clone();
                self.check_lambda(lam, expected.as_ref())
            }
            Expr::CallValue { callee, args, span } => self.infer_call_value(callee, args, *span),
            Expr::FanOut { callee, items, span } => {
                self.infer_fan_out(callee, items, *span)
            }
        }
    }

    pub(crate) fn infer_ok(&mut self, inner: &mut Box<Expr>, span: Span) -> Option<Type> {
        let payload = self.infer(inner)?;
        if let Some(expected) = self.expected_type.clone() {
            if let Some((ok_ty, err_ty)) = expected.unwrap_result() {
                if payload != *ok_ty {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "this `{}` holds {}, but {} was expected",
                            Syntax::LIT_OK,
                            payload.show(),
                            ok_ty.show()
                        ),
                        "the success value must match the result's value type".to_string(),
                        type_fix_hint(ok_ty, &payload),
                        Some(span),
                    ));
                }
                return Some(Type::Result {
                    ok: Box::new(ok_ty.clone()),
                    err: Box::new(err_ty.clone()),
                });
            }
        }
        self.diags.push(Diagnostic::error(
            "E0404",
            format!("`{}(...)` only fits where a fallible result is expected", Syntax::LIT_OK),
            format!(
                "`{}` builds the success side of a `T ? E` result",
                Syntax::LIT_OK
            ),
            "use it in a `T ? E` return type, a `T ? E` binding annotation, or a call that expects one"
                .to_string(),
            Some(span),
        ));
        None
    }

    pub(crate) fn infer_err(&mut self, inner: &mut Box<Expr>, span: Span) -> Option<Type> {
        let payload = self.infer(inner)?;
        if let Some(expected) = self.expected_type.clone() {
            if let Some((ok_ty, err_ty)) = expected.unwrap_result() {
                if payload != *err_ty && !(is_default_error(err_ty) && payload == Type::String) {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "this `{}` holds {}, but {} was expected",
                            Syntax::LIT_ERR,
                            payload.show(),
                            err_ty.show()
                        ),
                        "the failure value must match the result's error type".to_string(),
                        type_fix_hint(err_ty, &payload),
                        Some(span),
                    ));
                }
                return Some(Type::Result {
                    ok: Box::new(ok_ty.clone()),
                    err: Box::new(err_ty.clone()),
                });
            }
        }
        self.diags.push(Diagnostic::error(
            "E0404",
            format!("`{}(...)` only fits where a fallible result is expected", Syntax::LIT_ERR),
            format!(
                "`{}` builds the failure side of a `T ? E` result",
                Syntax::LIT_ERR
            ),
            "use it in a `T ? E` return type, a `T ? E` binding annotation, or a call that expects one"
                .to_string(),
            Some(span),
        ));
        None
    }

    pub(crate) fn infer_try(&mut self, inner: &mut Box<Expr>, span: Span, convert: &mut TryConvert) -> Option<Type> {
        let inner_ty = self.infer(inner)?;
        match inner_ty {
            Type::Result { ok, err } => {
                let ret = self.ret.clone().unwrap_or(Type::Int);
                match &ret {
                    // E2-M7: error types match — propagate and unwrap the Ok value.
                    // The Ok types (`ret_ok` and `ok`) do NOT need to be equal: `?`
                    // only propagates the error; the unwrapped Ok value may have any
                    // type (it is bound by the caller, not returned unchanged).
                    Type::Result {
                        err: ret_err,
                        ..
                    } if *ret_err == err
                        || (is_default_error(ret_err)
                            && matches!(err.as_ref(), Type::String)) =>
                    {
                        Some((*ok).clone())
                    }
                    Type::Result { err: ret_err, .. } => {
                        let err_type_name = err.name();
                        let ret_err_name = ret_err.name();

                        // D-ERR-CONV: check if a declared `impl Source -> Target` conversion exists.
                        if self.trait_reg.has_error_conv(&err_type_name, &ret_err_name) {
                            let fn_name = error_conv_fn_name(&err_type_name, &ret_err_name);
                            *convert = TryConvert::Typed(fn_name);
                            return Some((*ok).clone());
                        }

                        // S80/D-LIB3: check if the error type implements `Fallible`
                        // and the return error is the default `Error`.
                        if is_default_error(ret_err) {
                            if self.trait_reg.implements_trait(&err_type_name, Syntax::TRAIT_FALLIBLE) {
                                // Mark the Try node for Fallible conversion in codegen.
                                *convert = TryConvert::Fallible;
                                return Some((*ok).clone());
                            }
                            // E2402: return is `Error` but the error type has no Fallible impl.
                            let err_name = err.name();
                            self.diags.push(Diagnostic::error(
                                "E2402",
                                format!(
                                    "`?` can't convert `{}` into `{}`",
                                    err_name,
                                    Syntax::TYPE_ERROR
                                ),
                                format!(
                                    "`{}` has no path to `{}`; implement `impl {}: {}` to enable conversion",
                                    err_name,
                                    Syntax::TYPE_ERROR,
                                    err_name,
                                    Syntax::TRAIT_FALLIBLE
                                ),
                                format!(
                                    "add `impl {}: {} {{ fn to_error(self) -> {} {{ … }} }}`, or change the return type",
                                    err_name,
                                    Syntax::TRAIT_FALLIBLE,
                                    Syntax::TYPE_ERROR
                                ),
                                Some(span),
                            ));
                            return None;
                        }
                        // E2404: no declared conversion between these two typed error types.
                        self.diags.push(Diagnostic::error(
                            "E2404",
                            format!(
                                "`?` can't turn a `{}` into a `{}` here",
                                err_type_name, ret_err_name
                            ),
                            format!(
                                "`?` only changes an error's type when you've declared how; \
                                 there's no declared way to turn `{}` into `{}`",
                                err_type_name, ret_err_name
                            ),
                            format!(
                                "add `impl {} -> {} {{ … }}` before this function",
                                err_type_name, ret_err_name
                            ),
                            Some(span),
                        ));
                        None
                    }
                    _ => {
                        self.diags.push(Diagnostic::error(
                            "E0403",
                            format!(
                                "`{}` only works inside a function that returns a fallible result",
                                Syntax::OP_TRY_SUFFIX
                            ),
                            "propagation early-returns the failure to the caller".to_string(),
                            format!(
                                "add `-> ... ? {}` to this function, or handle the result with `{}`",
                                err.name(),
                                Syntax::OP_FALLBACK
                            ),
                            Some(span),
                        ));
                        None
                    }
                }
            }
            Type::Option(ref inner) => {
                let ret = self.ret.clone().unwrap_or(Type::Int);
                if let Type::Option(ret_inner) = &ret {
                    if **ret_inner == **inner {
                        return Some((**inner).clone());
                    }
                }
                self.diags.push(Diagnostic::error(
                    "E0403",
                    format!(
                        "`{}` on `{}` needs a function that returns the same optional type",
                        Syntax::OP_TRY_SUFFIX,
                        inner_ty.name()
                    ),
                    "propagation passes `null` back to the caller".to_string(),
                    format!(
                        "add `-> {}` to this function, or handle it with `{}`",
                        inner_ty.name(),
                        Syntax::OP_FALLBACK
                    ),
                    Some(span),
                ));
                None
            }
            other => {
                self.diags.push(Diagnostic::error(
                    "E0403",
                    format!(
                        "`{}` only works on a fallible value, not {}",
                        Syntax::OP_TRY_SUFFIX,
                        other.show()
                    ),
                    "postfix `?` unwraps success or returns early with the failure".to_string(),
                    format!(
                        "call something that returns `T ? E` or an optional value, or remove `{}`",
                        Syntax::OP_TRY_SUFFIX
                    ),
                    Some(span),
                ));
                None
            }
        }
    }

    pub(crate) fn infer_or_fallback(
        &mut self,
        value: &mut Box<Expr>,
        fallback: &mut OrFallback,
        span: Span,
        is_option: &mut bool,
    ) -> Option<Type> {
        let val_ty = self.infer(value)?;
        *is_option = matches!(val_ty, Type::Option(_));
        let payload = match &val_ty {
            Type::Result { ok, .. } if !*is_option => (**ok).clone(),
            Type::Option(inner) if *is_option => (**inner).clone(),
            other => {
                self.diags.push(Diagnostic::error(
                    "E0405",
                    format!(
                        "`{}` only works on a fallible value, not {}",
                        Syntax::OP_FALLBACK,
                        other.show()
                    ),
                    "the left side must be a `Result` or optional value".to_string(),
                    format!(
                        "call something that can fail, then write `... {} fallback`",
                        Syntax::OP_FALLBACK
                    ),
                    Some(span),
                ));
                return None;
            }
        };
        match fallback {
            OrFallback::Value(e) => {
                // Infer in place: sema rewrites inside the fallback (index
                // kinds, S25 distribution, field clones) must reach codegen.
                // D-SG9: the fallback shares the success type, so a fixed-width
                // literal fallback (`x ?? 0` where `x` is `U8?`) elaborates to it.
                let saved = self.expected_type.clone();
                self.expected_type = Some(payload.clone());
                let ft = self.infer(e);
                self.expected_type = saved;
                let ft = ft?;
                if ft != payload {
                    self.diags.push(Diagnostic::error(
                        "E0405",
                        format!(
                            "the fallback is {}, but the success value is {}",
                            ft.show(),
                            payload.show()
                        ),
                        format!(
                            "both sides of `{}` must be the same type",
                            Syntax::OP_FALLBACK
                        ),
                        type_fix_hint(&payload, &ft),
                        Some(e.span()),
                    ));
                }
                Some(payload)
            }
            OrFallback::Return(ret_expr, ret_span) => {
                let ret = self.ret.clone();
                match (&ret, ret_expr) {
                    // `?? return value` in a value-returning fn — the value must match.
                    (Some(rt), Some(e)) => {
                        let saved = self.expected_type.clone();
                        self.expected_type = Some(rt.clone());
                        let et = self.infer(e);
                        self.expected_type = saved;
                        if let Some(et) = et {
                            let espan = e.span();
                            self.check_type_assignable(rt, &et, espan);
                        }
                    }
                    // Bare `?? return` in a value-returning fn — rustc would reject the
                    // emitted `return;` (E0069). Reject cleanly: the fn owes a value.
                    (Some(rt), None) => {
                        self.diags.push(Diagnostic::error(
                            "E0405",
                            format!(
                                "`{} return` here needs a value",
                                Syntax::OP_FALLBACK
                            ),
                            format!(
                                "a bare `return` needs a value here because the function returns {}",
                                rt.show()
                            ),
                            format!(
                                "give a fallback value: `{} return <value>`",
                                Syntax::OP_FALLBACK
                            ),
                            Some(*ret_span),
                        ));
                    }
                    // `?? return value` in a unit fn — there's nothing to return.
                    (None, Some(e)) => {
                        self.diags.push(Diagnostic::error(
                            "E0405",
                            format!(
                                "`{} return` can't return a value here",
                                Syntax::OP_FALLBACK
                            ),
                            "this function returns nothing, so `return` can't carry a value"
                                .to_string(),
                            "drop the value, or add `-> Type` to the function".to_string(),
                            Some(e.span()),
                        ));
                    }
                    // Bare `?? return` in a unit fn — rustc accepts the emitted `return;`.
                    (None, None) => {}
                }
                Some(payload)
            }
            OrFallback::Panic { name_span, args } => {
                let mut call = Call {
                    name: Syntax::BUILTIN_PANIC.to_string(),
                    name_span: *name_span,
                    args: std::mem::take(args),
                };
                self.check_panic_call(&mut call);
                *args = call.args;
                Some(payload)
            }
        }
    }

    pub(crate) fn infer_fallible_stmt(&mut self, expr: &mut Expr) -> Option<Type> {
        match expr {
            Expr::Call(call) => match self.check_call(call, false) {
                Some(Some(t)) => Some(t),
                _ => None,
            },
            Expr::MethodCall { .. } => self.infer(expr),
            _ => self.infer(expr),
        }
    }

    pub(crate) fn infer_call_value(
        &mut self,
        callee: &mut Box<Expr>,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let callee_ty = self.infer(callee)?;
        let Type::Fn { params, ret } = callee_ty.clone() else {
            self.diags.push(Diagnostic::error(
                "E0803",
                format!("this is {}, not a function", callee_ty.show()),
                "only a function value can be called with `(…)`".to_string(),
                "call a defined `fn` by name, or store a lambda in a binding first".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        if args.len() != params.len() {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "this function wants {} argument{}, got {}",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                "every argument must match a parameter".to_string(),
                "check how many values this function expects".to_string(),
                Some(span),
            ));
        }
        for (i, arg) in args.iter_mut().enumerate() {
            if let Some(param_ty) = params.get(i) {
                let saved = self.expected_type.clone();
                self.expected_type = Some(param_ty.clone());
                let got = self.infer(&mut arg.expr);
                self.expected_type = saved;
                if let Some(got) = got {
                    if got != *param_ty {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "argument {} should be {}, not {}",
                                i + 1,
                                param_ty.show(),
                                got.show()
                            ),
                            "every argument must match its parameter's type".to_string(),
                            type_fix_hint(param_ty, &got),
                            Some(arg.expr.span()),
                        ));
                    }
                }
            } else {
                self.infer(&mut arg.expr);
            }
        }
        ret.map(|r| *r)
    }

    pub(crate) fn check_lambda(&mut self, lam: &mut Lambda, expected: Option<&Type>) -> Option<Type> {
        let (exp_params, exp_ret) = match expected {
            Some(Type::Fn { params, ret }) => (Some(params.as_slice()), ret.as_ref()),
            _ => (None, None),
        };

        if let Some(ep) = exp_params {
            if lam.params.len() != ep.len() {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!(
                        "this lambda has {} parameter{}, but {} {} expected",
                        lam.params.len(),
                        if lam.params.len() == 1 { "" } else { "s" },
                        ep.len(),
                        if ep.len() == 1 { "was" } else { "were" }
                    ),
                    "parameter count must match the function type at this spot".to_string(),
                    "add or remove parameters, or fix the surrounding type".to_string(),
                    Some(lam.span),
                ));
            }
        }

        let mut param_types = Vec::new();
        for (i, p) in lam.params.iter_mut().enumerate() {
            let pty = if let Some(ty) = &p.ty {
                self.check_declared_type(ty, p.ty_span.unwrap_or(p.name_span));
                ty.clone()
            } else if let Some(ep) = exp_params.and_then(|ps| ps.get(i)) {
                ep.clone()
            } else {
                self.diags.push(Diagnostic::error(
                    "E0801",
                    format!("tell me the type of `{}`", p.name),
                    "this lambda parameter has no type to go on".to_string(),
                    format!("write `({}: Int) => …` (or whatever type fits)", p.name),
                    Some(p.name_span),
                ));
                Type::Int
            };
            param_types.push(pty);
        }

        if let Some(binding) = &self.lambda_binding {
            if lambda_body_refs_name(&lam.body, binding) {
                self.diags.push(Diagnostic::error(
                    "E0804",
                    format!("a lambda can't call itself as `{}`", binding),
                    "short functions stored in a binding can't recurse in v1".to_string(),
                    format!(
                        "write a named `{}` instead of assigning the lambda to `{}`",
                        Syntax::KW_FN,
                        binding
                    ),
                    Some(lam.span),
                ));
            }
        }

        let escapes = self.lambda_escapes;
        lam.meta.escapes = escapes;

        let param_names: HashSet<String> = lam.params.iter().map(|p| p.name.clone()).collect();
        let take_set: HashSet<String> = lam.take_names.iter().map(|(n, _)| n.clone()).collect();

        let mut read_caps = HashSet::new();
        let mut mut_caps = HashSet::new();
        lambda_collect_captures(&lam.body, &param_names, &mut read_caps, &mut mut_caps);

        for name in read_caps.iter().chain(mut_caps.iter()) {
            if take_set.contains(name) || param_names.contains(name) {
                continue;
            }
            // Module aliases (imports and core_imports) are always in scope in
            // lambdas — they're not local variables but they're valid references.
            // Don't report them as unknown names; the body check validates calls.
            if self.imports.contains_key(name) || self.core_imports.contains_key(name) {
                continue;
            }
            if self.lookup(name).is_none() && !self.consts.contains_key(name) {
                self.unknown_name(name, lam.span);
            }
        }

        for name in &mut_caps {
            if param_names.contains(name) || take_set.contains(name) {
                continue;
            }
            if let Some(info) = self.lookup(name) {
                if !info.mutable {
                    self.diags.push(Diagnostic::error(
                        "E0111",
                        format!("`{}` can't be changed inside this lambda", name),
                        "changing a value inside a short function requires a `var` binding"
                            .to_string(),
                        format!("declare `var {}: …` instead of `val`", name),
                        Some(lam.span),
                    ));
                }
            }
        }

        lam.meta.needs_fn_mut = !mut_caps.is_empty();
        lam.meta.mut_captures = mut_caps
            .iter()
            .filter(|n| !take_set.contains(*n) && !param_names.contains(*n))
            .cloned()
            .collect();

        if escapes {
            let mut seen_caps: HashSet<String> = HashSet::new();
            for name in read_caps.iter().chain(mut_caps.iter()) {
                if !seen_caps.insert(name.clone()) {
                    continue; // already processed this capture
                }
                if param_names.contains(name) {
                    continue;
                }
                let cap = self
                    .lookup(name)
                    .map(|i| (i.ty.clone(), i.sendable))
                    .or_else(|| self.consts.get(name).map(|t| (t.clone(), true)));
                let Some((cap_ty, cap_sendable)) = cap else {
                    continue;
                };
                let taken = take_set.contains(name);
                if self.is_task_spawn {
                    let problem = if !cap_sendable {
                        self.sendability_problem(&cap_ty, taken).or_else(|| {
                            Some(SendabilityProblem {
                                root: None,
                                path: Vec::new(),
                                kind: SendProblemKind::ClosureCaptures,
                            })
                        })
                    } else {
                        self.sendability_problem(&cap_ty, taken)
                    };
                    if let Some(problem) = problem {
                        self.report_unsendable(
                            name,
                            &cap_ty,
                            problem,
                            SendCrossing::TaskCapture,
                            lam.span,
                        );
                        continue;
                    }
                }
                if mut_caps.contains(name) && !taken {
                    if self.is_task_spawn {
                        self.diags.push(Diagnostic::error(
                            "E1101",
                            format!(
                                "`{}` is a mutable value — the new task might outlive this scope",
                                name
                            ),
                            "tasks run concurrently; a `var` binding can't be shared between tasks".to_string(),
                            format!(
                                "give the task its own copy (`{}.clone()`) or hand it over with `take({})`",
                                name, name
                            ),
                            Some(lam.span),
                        ));
                    }
                    continue; // taken by move into closure via mut borrow path
                }
                if mut_caps.contains(name) {
                    continue;
                }
                if !is_cloneable(&cap_ty, self.registry, self.structs) {
                    if !taken {
                        if self.is_task_spawn {
                            self.diags.push(Diagnostic::error(
                                "E1101",
                                format!(
                                    "`{}` can't be copied into a task — the task might outlive this scope",
                                    name
                                ),
                                "a spawned task must own everything it captures".to_string(),
                                format!(
                                    "use `take({})` on the lambda to move `{}` into the task",
                                    name, name
                                ),
                                Some(lam.span),
                            ));
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0802",
                                format!("`{}` can't be copied into a stored lambda", name),
                                "a lambda that outlives this line must own its captures"
                                    .to_string(),
                                format!(
                                    "prefix the lambda with `take({})` to move `{}` in",
                                    name, name
                                ),
                                Some(lam.span),
                            ));
                        }
                    }
                } else if !taken {
                    lam.meta.cloned_captures.push(name.clone());
                    self.diags.push(Diagnostic::lint(
                        "L0801",
                        format!(
                            "lambda stored a copy of `{}`; write `take({})` on the lambda to move it instead",
                            name, name
                        ),
                        "a stored lambda owns its captures — clonable values are copied silently"
                            .to_string(),
                        format!(
                            "use `take({}) (…) => …` to move `{}`, or `.clone()` at the call site to copy on purpose",
                            name, name
                        ),
                        Some(lam.span),
                    ));
                }
            }
        }

        self.push_scope();
        for (p, pty) in lam.params.iter().zip(param_types.iter()) {
            self.scopes.last_mut().unwrap().insert(
                p.name.clone(),
                LocalInfo {
                    ty: pty.clone(),
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                    sendable: true,
                    task_lint_span: None,
                    task_has_view_capture: false,
                },
            );
        }

        let body_ret = match &mut lam.body {
            LambdaBody::Expr(e) => {
                if self.is_task_spawn {
                    self.borrow_ctx = true;
                }
                self.infer(e)
            }
            LambdaBody::Block(stmts) => {
                self.check_block(stmts, false);
                let mut last_ret = None;
                for s in stmts.iter_mut().rev() {
                    match s {
                        Stmt::Return(Some(e), _) => {
                            last_ret = self.infer(e);
                            break;
                        }
                        Stmt::Expr(e) => {
                            last_ret = self.infer_fallible_stmt(e);
                            break;
                        }
                        _ => {}
                    }
                }
                last_ret
            }
        };

        self.pop_scope();

        if escapes {
            for (name, span) in &lam.take_names {
                if let Some(info) = self.lookup(name) {
                    if !info.ty.is_scalar() {
                        if matches!(
                            info.param_conv,
                            Some(AccessConvention::Read) | Some(AccessConvention::Write)
                        ) {
                            self.diags.push(Diagnostic::error(
                                "E0120",
                                format!(
                                    "`{}` was not moved here, so the lambda cannot take it (`^`)",
                                    name
                                ),
                                "this function has read access only and does not own the value".to_string(),
                                format!(
                                    "take ownership in this function with `{} {}: {}`",
                                    Syntax::KW_MOVE,
                                    name,
                                    info.ty.name()
                                ),
                                Some(*span),
                            ));
                        } else {
                            self.mark_moved(name.clone(), *span);
                        }
                    }
                }
            }
        }

        let ret_ty = if let Some(er) = exp_ret {
            if let Some(br) = &body_ret {
                if br != er.as_ref() {
                    self.diags.push(Diagnostic::error(
                        "E0113",
                        format!("this lambda should return {}, not {}", er.show(), br.show()),
                        "the lambda's return type must match what's expected here".to_string(),
                        type_fix_hint(er, br),
                        Some(lam.span),
                    ));
                }
            }
            Some((**er).clone())
        } else {
            body_ret
        };

        Some(Type::Fn {
            params: param_types,
            ret: ret_ty.map(Box::new),
        })
    }

    pub(crate) fn finish_builtin_method(
        &mut self,
        receiver: &Expr,
        method: &str,
        recv_ty: &Type,
        args: &mut [crate::AST::CallArg],
        span: Span,
        ret: Option<Type>,
    ) -> Option<Type> {
        if Collections::builtin_needs_mut_receiver(recv_ty, method) {
            if let Some(root) = expr_root_ident(receiver) {
                let root = root.to_string();
                let rspan = receiver.span();
                if self.iter_borrowed.contains(&root) {
                    self.diags.push(collection_changed_in_loop(&root, rspan));
                }
                if let Some(info) = self.lookup(&root) {
                    if !info.mutable {
                        let (what, fix) = if root == Syntax::KW_SELF {
                            (
                                format!(
                                    "`.{}()` edits `{}`, but this method has read access only",
                                    method,
                                    Syntax::KW_SELF
                                ),
                                format!(
                                    "declare the enclosing method with `{} {}`",
                                    Syntax::KW_MUTATE,
                                    Syntax::KW_SELF
                                ),
                            )
                        } else {
                            (
                                format!(
                                    "cannot write to `{}` — it does not have edit access (`~`); required before calling `.{}()`",
                                    root,
                                    method
                                ),
                                format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT),
                            )
                        };
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            what,
                            "this method edits the collection in place; write access (`~`) is required".to_string(),
                            fix,
                            Some(rspan),
                        ));
                    }
                }
            }
        }
        if let Type::Apply { name, .. } = recv_ty {
            match (name.as_str(), method) {
                ("Task", "join") => {
                    self.consume_builtin_receiver(receiver, method);
                    let _ = span;
                    return ret;
                }
                ("Task", "detach") => {
                    // D-DETACH1: consume the Task handle (marks it moved → L1101 won't fire).
                    // Two error cases:
                    //   E1106: task captured a `view` borrow — a detached task can outlive
                    //          the borrow; fix-it is to pass an owned `copy`/`share`.
                    //   E1103: task had a general sendability failure at spawn (E1102 already
                    //          fired); detaching an unsound task is doubly dangerous.
                    if let Expr::Ident(name, _) = receiver {
                        if self.view_borrow_escape_tasks.contains(name.as_str()) {
                            self.diags.push(Diagnostic::error(
                                "E1106",
                                format!(
                                    "can't detach task `{}` — it captured a `view` borrow that may not live long enough",
                                    name
                                ),
                                "a detached task runs unsupervised and may outlive the caller; a captured `view` would dangle".to_string(),
                                "pass an owned `copy` or `share` to the task instead of a `view`".to_string(),
                                Some(span),
                            ));
                        } else if self.view_capture_tasks.contains(name.as_str()) {
                            self.diags.push(Diagnostic::error(
                                "E1103",
                                format!(
                                    "can't detach task `{}` — it captured a value that can't cross a thread boundary",
                                    name
                                ),
                                "a detached task runs unsupervised; it must only hold values it owns cleanly".to_string(),
                                "fix the E1102 error at the spawn site first, then `.detach()` is safe".to_string(),
                                Some(span),
                            ));
                        }
                    }
                    self.consume_builtin_receiver(receiver, method);
                    let _ = span;
                    return None; // detach() returns nothing
                }
                ("Sender", "send") => {
                    return self.finish_sender_send(recv_ty, args, span);
                }
                _ => {}
            }
        }
        let mut refined_ret = ret.clone();
        if let Some(expected) = Collections::builtin_method_arg_types(recv_ty, method) {
            for (i, arg) in args.iter_mut().enumerate() {
                let saved_esc = self.lambda_escapes;
                if Collections::is_closure_method(method) {
                    self.lambda_escapes = false;
                }
                let saved_exp = self.expected_type.clone();
                if let Some(et) = expected.get(i) {
                    self.expected_type = Some(et.clone());
                }
                let got = self.infer(&mut arg.expr);
                self.expected_type = saved_exp;
                self.lambda_escapes = saved_esc;
                if let (Some(et), Some(gt)) = (expected.get(i), got) {
                    if Collections::is_closure_method(method) && i == 0 && method == "map" {
                        if let Type::Fn {
                            ret: Some(ref r), ..
                        } = gt
                        {
                            if let Type::List(inner) = recv_ty {
                                refined_ret = Some(Type::List(Box::new((**r).clone())));
                                let _ = inner;
                            }
                        }
                    }
                    if method == "reduce" && i == 1 {
                        if let Type::Fn {
                            ret: Some(ref r), ..
                        } = gt
                        {
                            refined_ret = Some((**r).clone());
                        }
                    }
                    if !fn_types_compatible(et, &gt) && gt != *et {
                        self.diags.push(Diagnostic::error(
                            "E0108",
                            format!(
                                "argument {} to `.{}()` should be {}, not {}",
                                i + 1,
                                method,
                                et.show(),
                                gt.show()
                            ),
                            "built-in methods need arguments of the right type".to_string(),
                            type_fix_hint(et, &gt),
                            Some(arg.expr.span()),
                        ));
                    }
                }
            }
        } else {
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
        }
        let _ = span;
        refined_ret
    }

    pub(crate) fn infer_list_lit(&mut self, elems: &mut [Expr], span: Span) -> Option<Type> {
        if self.freestanding {
            self.diags.push(e3303(span));
        }
        if elems.is_empty() {
            if let Some(expected) = self.expected_type.clone() {
                if let Type::List(inner) = expected {
                    return Some(Type::List(inner));
                }
            }
            self.diags.push(Diagnostic::error(
                "E0501",
                "an empty list needs a type".to_string(),
                "write `[]` only where the list type is already known, like `val xs: [Int] = []`"
                    .to_string(),
                "add a type annotation on the binding".to_string(),
                Some(span),
            ));
            return None;
        }
        // D-FIXARR1: a list literal in a `[T#N]` binding context keeps the fixed-size type.
        // Sema validates element types and count, codegen emits a Rust array `[e1, …]`.
        if let Some(Type::FixedList { elem: expected_inner, len }) = self.expected_type.clone() {
            if elems.len() as u64 != len {
                self.diags.push(Diagnostic::error(
                    "E0963",
                    format!(
                        "this list has {} element{}, but `[T#{}]` expects exactly {}",
                        elems.len(),
                        if elems.len() == 1 { "" } else { "s" },
                        len,
                        len,
                    ),
                    "a fixed-size list `[T#N]` requires exactly N elements".to_string(),
                    format!("provide exactly {} element{}", len, if len == 1 { "" } else { "s" }),
                    Some(span),
                ));
                return None;
            }
            let saved = self.expected_type.clone();
            self.expected_type = Some((*expected_inner).clone());
            for e in elems.iter_mut() {
                if let Some(t) = self.infer(e) {
                    self.check_type_assignable(&expected_inner, &t, e.span());
                }
            }
            self.expected_type = saved;
            return Some(Type::FixedList { elem: expected_inner, len });
        }
        if let Some(Type::List(expected_inner)) = self.expected_type.clone() {
            if let Type::TraitObject(trait_name) = expected_inner.as_ref() {
                for e in elems.iter_mut() {
                    if let Some(t) = self.infer(e) {
                        match &t {
                            Type::Named(n) if self.trait_reg.implements_trait(n, trait_name) => {
                                if let Expr::StructLit { as_trait, .. } = e {
                                    *as_trait = Some(trait_name.clone());
                                }
                            }
                            Type::Apply { name, .. }
                                if self.trait_reg.implements_trait(name, trait_name) =>
                            {
                                if let Expr::StructLit { as_trait, .. } = e {
                                    *as_trait = Some(trait_name.clone());
                                }
                            }
                            _ => {
                                self.check_type_assignable(&expected_inner, &t, e.span());
                            }
                        }
                    }
                }
                return Some(Type::List(expected_inner));
            }
            // D-SG9: a list expected at a fixed width (`[U8]`, `[I32]`) elaborates
            // each element to that width and range-checks it, so `[1, 2, 255]` is a
            // `[U8]`. This is what binary/byte APIs (and `embed_bytes`, c75) need.
            if matches!(expected_inner.as_ref(), Type::IntN { .. } | Type::Float32) {
                let saved = self.expected_type.clone();
                self.expected_type = Some(expected_inner.as_ref().clone());
                for e in elems.iter_mut() {
                    if let Some(t) = self.infer(e) {
                        self.check_type_assignable(&expected_inner, &t, e.span());
                    }
                }
                self.expected_type = saved;
                return Some(Type::List(expected_inner));
            }
        }
        let mut elem_types = Vec::new();
        for e in elems.iter_mut() {
            if let Some(t) = self.infer(e) {
                elem_types.push(t);
            }
        }
        let first = elem_types.first()?.clone();
        for (i, t) in elem_types.iter().enumerate().skip(1) {
            if *t != first {
                self.diags.push(Diagnostic::error(
                    "E0504",
                    format!(
                        "this list started as `{}` but item {} is `{}`",
                        first.name(),
                        i + 1,
                        t.name()
                    ),
                    "every item in a list literal must have the same type".to_string(),
                    "make every element the same type, or build the list in steps".to_string(),
                    Some(elems[i].span()),
                ));
            }
        }
        Some(Type::List(Box::new(first)))
    }

    pub(crate) fn infer_tuple_lit(&mut self, fields: &mut [(String, Expr)], _span: Span) -> Option<Type> {
        let mut seen = HashSet::new();
        let mut typed = Vec::with_capacity(fields.len());
        for (name, expr) in fields.iter_mut() {
            if !seen.insert(name.clone()) {
                self.diags.push(Diagnostic::error(
                    "E0003",
                    format!("tuple member `{}` appears more than once", name),
                    "each named member in a tuple must have a unique name".to_string(),
                    "rename or remove the duplicate member".to_string(),
                    Some(expr.span()),
                ));
            }
            let ty = self.infer(expr).unwrap_or(Type::Int);
            typed.push((name.clone(), ty));
        }
        let canonical = crate::AST::canonicalize_tuple_fields(typed);
        let tuple_ty = Type::Tuple(
            canonical
                .iter()
                .map(|(n, t)| (n.clone(), Box::new(t.clone())))
                .collect(),
        );
        Some(tuple_ty)
    }

    pub(crate) fn infer_fan_out(
        &mut self,
        callee: &mut Box<Expr>,
        items: &mut Vec<Expr>,
        _span: Span,
    ) -> Option<Type> {
        let callee_span = callee.span();

        // `print` is a builtin that doesn't live in scope as an ident — special-case it so
        // `print.[a, b, c]` works without triggering E0107.
        if let Expr::Ident(name, _) = callee.as_ref() {
            if name == Syntax::BUILTIN_PRINT {
                self.borrow_ctx = true;
                for item in items.iter_mut() {
                    if let Some(t) = self.infer(item) {
                        if !is_printable(&t, self.registry) {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`{}` doesn't know how to show {}", Syntax::BUILTIN_PRINT, t.show()),
                                "print shows values that have a display".to_string(),
                                "print one of its parts instead".to_string(),
                                Some(item.span()),
                            ));
                        }
                    }
                }
                self.borrow_ctx = false;
                return None;
            }
        }

        let callee_ty = self.infer(callee);

        // E0961: callee must be a one-argument function.
        let (param_ty, ret_ty) = match callee_ty {
            None => {
                for item in items.iter_mut() {
                    self.infer(item);
                }
                return None;
            }
            Some(Type::Fn { ref params, ref ret }) if params.len() == 1 => {
                (params[0].clone(), ret.as_ref().map(|r| *r.clone()))
            }
            Some(ref other) => {
                let msg = if let Type::Fn { params, .. } = other {
                    format!(
                        "fan-out `.[` needs a one-argument function, but this one takes {} argument{}",
                        params.len(),
                        if params.len() == 1 { "" } else { "s" }
                    )
                } else {
                    format!(
                        "fan-out `.[` needs a one-argument function, but this is {}",
                        other.show()
                    )
                };
                self.diags.push(Diagnostic::error(
                    "E0961",
                    msg,
                    "`f.[a, b, c]` expands to `[f(a), f(b), f(c)]` — `f` must accept exactly one argument".to_string(),
                    "use a one-argument function as the fan-out callee".to_string(),
                    Some(callee_span),
                ));
                for item in items.iter_mut() {
                    self.infer(item);
                }
                return None;
            }
        };

        // E0962: each item must match the parameter type.
        let mut had_error = false;
        for (i, item) in items.iter_mut().enumerate() {
            let saved = self.expected_type.clone();
            self.expected_type = Some(param_ty.clone());
            let item_ty = self.infer(item);
            self.expected_type = saved;
            if let Some(got) = item_ty {
                if got != param_ty {
                    had_error = true;
                    self.diags.push(Diagnostic::error(
                        "E0962",
                        format!(
                            "fan-out item {} is {}, but the function expects {}",
                            i + 1,
                            got.show(),
                            param_ty.show()
                        ),
                        "each item in `f.[a, b, c]` is passed as the argument to `f`".to_string(),
                        type_fix_hint(&param_ty, &got),
                        Some(item.span()),
                    ));
                }
            }
        }

        if had_error {
            return None;
        }

        let Some(elem) = ret_ty else {
            // void callee: side effects only, no list produced
            return None;
        };
        let len = items.len() as u64;
        if len == 0 {
            Some(Type::List(Box::new(elem)))
        } else {
            Some(Type::FixedList { elem: Box::new(elem), len })
        }
    }

    pub(crate) fn infer_map_lit(&mut self, entries: &mut [(Expr, Expr)], span: Span) -> Option<Type> {
        if self.freestanding {
            self.diags.push(e3303(span));
        }
        if entries.is_empty() {
            if let Some(expected) = self.expected_type.clone() {
                if let Type::Map { key, value } = expected {
                    return Some(Type::Map { key, value });
                }
            }
            self.diags.push(Diagnostic::error(
                "E0501",
                "an empty map needs a type".to_string(),
                    "write `[:]` only where the map type is already known, like `var m: [String, Int] = [:]`"
                    .to_string(),
                "add a type annotation on the binding".to_string(),
                Some(span),
            ));
            return None;
        }
        let mut key_ty = None;
        let mut val_ty = None;
        for (k, v) in entries.iter_mut() {
            let Some(kt) = self.infer(k) else {
                continue;
            };
            let Some(vt) = self.infer(v) else {
                continue;
            };
            if !is_map_key_type(&kt) {
                self.diags.push(Diagnostic::error(
                    "E0502",
                    format!("`{}` can't be a map key", kt.name()),
                    "map keys must be Int, String, Bool, Char, or a payload-free enum".to_string(),
                    "use a simpler key type".to_string(),
                    Some(k.span()),
                ));
            }
            if let Some(ref fk) = key_ty {
                if kt != *fk {
                    self.diags.push(Diagnostic::error(
                        "E0504",
                        format!(
                            "this map started with `{}` keys but another key is `{}`",
                            fk.name(),
                            kt.name()
                        ),
                        "every key in a map literal must have the same type".to_string(),
                        "use the same key type throughout".to_string(),
                        Some(k.span()),
                    ));
                }
            } else {
                key_ty = Some(kt);
            }
            if let Some(ref fv) = val_ty {
                if vt != *fv {
                    self.diags.push(Diagnostic::error(
                        "E0504",
                        format!(
                            "this map started with `{}` values but another value is `{}`",
                            fv.name(),
                            vt.name()
                        ),
                        "every value in a map literal must have the same type".to_string(),
                        "use the same value type throughout".to_string(),
                        Some(v.span()),
                    ));
                }
            } else {
                val_ty = Some(vt);
            }
        }
        match (key_ty, val_ty) {
            (Some(k), Some(v)) => Some(Type::Map {
                key: Box::new(k),
                value: Box::new(v),
            }),
            _ => None,
        }
    }

    pub(crate) fn infer_index(
        &mut self,
        base: &mut Box<Expr>,
        index: &mut Box<Expr>,
        span: &Span,
        kind: &mut IndexKind,
    ) -> Option<Type> {
        self.borrow_ctx = true;
        let base_ty = self.infer(base)?;
        let idx_ty = self.infer(index)?;
        match &base_ty {
            Type::List(inner) => {
                *kind = IndexKind::List;
                if idx_ty != Type::Int {
                    self.diags.push(Diagnostic::error(
                        "E0505",
                        format!(
                            "list indexes must be {}, not {}",
                            Type::Int.show(),
                            idx_ty.show()
                        ),
                        "count positions with a whole number starting at 0".to_string(),
                        "use an Int index, like `items[0]`".to_string(),
                        Some(index.span()),
                    ));
                }
                Some((**inner).clone())
            }
            // S76: [T#N] supports indexing; E0965 if the index is a literal >= N.
            Type::FixedList { elem, len } => {
                *kind = IndexKind::List;
                if idx_ty != Type::Int {
                    self.diags.push(Diagnostic::error(
                        "E0505",
                        format!(
                            "list indexes must be {}, not {}",
                            Type::Int.show(),
                            idx_ty.show()
                        ),
                        "count positions with a whole number starting at 0".to_string(),
                        "use an Int index, like `items[0]`".to_string(),
                        Some(index.span()),
                    ));
                } else if let Expr::Int(n, _, _) = index.as_ref() {
                    // E0965: compile-time out-of-bounds index.
                    if *n < 0 || *n as u64 >= *len {
                        self.diags.push(Diagnostic::error(
                            "E0965",
                            format!(
                                "index {} is out of range for a fixed-size list of {} element{}",
                                n,
                                len,
                                if *len == 1 { "" } else { "s" }
                            ),
                            "the valid indexes for `[T#N]` are 0 through N-1".to_string(),
                            format!("use an index between 0 and {}", len - 1),
                            Some(index.span()),
                        ));
                    }
                }
                Some((**elem).clone())
            }
            Type::Map { key, value } => {
                *kind = IndexKind::Map;
                if idx_ty != **key {
                    self.diags.push(Diagnostic::error(
                        "E0505",
                        format!(
                            "this map holds keys of type {}, not {}",
                            key.show(),
                            idx_ty.show()
                        ),
                        "the key in `map[key]` must match the map's key type".to_string(),
                        format!("use a {} key here", key.name()),
                        Some(index.span()),
                    ));
                }
                Some((**value).clone())
            }
            Type::String => {
                self.diags.push(Diagnostic::error(
                    "E0503",
                    "strings aren't indexed with `[ ]`".to_string(),
                    "text is counted in characters — walk them with `.chars()` or take a piece with `.slice(start..end)`".to_string(),
                    "e.g. `loop c in s.chars() { }` or `s.slice(0..2)`".to_string(),
                    Some(*span),
                ));
                None
            }
            _ => {
                self.diags.push(Diagnostic::error(
                    "E0505",
                    format!("only lists and maps can be indexed, not {}", base_ty.show()),
                    "use `[ ]` on a `List` or `Map` value".to_string(),
                    "check the value before `[`".to_string(),
                    Some(*span),
                ));
                None
            }
        }
    }

    pub(crate) fn infer_slice(
        &mut self,
        base: &mut Box<Expr>,
        start: &mut Box<Expr>,
        end: &mut Box<Expr>,
        span: Span,
    ) -> Option<Type> {
        if self.loop_depth > 0 {
            self.diags.push(Diagnostic::lint(
                "L0501",
                "slicing inside a loop copies every time".to_string(),
                "each slice makes a fresh copy of the range — that adds up in a loop".to_string(),
                "build indices outside the loop, or collect into one list".to_string(),
                Some(span),
            ));
        }
        self.borrow_ctx = true;
        let base_ty = self.infer(base)?;
        for e in [start.as_mut(), end.as_mut()] {
            let t = self.infer(e)?;
            if t != Type::Int {
                self.diags.push(Diagnostic::error(
                    "E0505",
                    format!(
                        "slice bounds must be {}, not {}",
                        Type::Int.show(),
                        t.show()
                    ),
                    "both ends of `a..b` must be whole numbers (S22, inclusive)".to_string(),
                    "use Int positions".to_string(),
                    Some(e.span()),
                ));
            }
        }
        match base_ty {
            Type::List(inner) => Some(Type::List(inner)),
            Type::String => Some(Type::String),
            other => {
                self.diags.push(Diagnostic::error(
                    "E0505",
                    format!("only lists and strings can be sliced, not {}", other.show()),
                    "slicing copies a range (S40)".to_string(),
                    "use `xs[a..b]` on a list or `s.slice(a..b)` on text".to_string(),
                    Some(span),
                ));
                None
            }
        }
    }

    pub(crate) fn infer_field(&mut self, inner: &mut Box<Expr>, member: &str, span: Span) -> Option<Type> {
        if member == "clone" {
            return self.infer(inner);
        }
        if let Expr::Ident(root, _) = &**inner {
            if root == Syntax::FOREIGN_OS && member == "environ" {
                self.diags.push(Diagnostic::error(
                    "E0039",
                    "`os.environ` is written `env.get` in Jet".to_string(),
                    "environment access lives in the `core.env` module".to_string(),
                    "import `core.env as env` and call `env.get(name)`".to_string(),
                    Some(span),
                ));
                return None;
            }
        }
        if let Expr::Ident(alias, alias_span) = &**inner {
            if let Some(module) = self.core_imports.get(alias).cloned() {
                return self.infer_core_field(&module, member, *alias_span, span);
            }
        }
        if let Expr::Ident(type_name, _) = &**inner {
            if is_json_type_name(type_name) {
                if let Some(ret) = self.check_core_json_lit(member, &mut [], span) {
                    return Some(ret);
                }
            }
            // D-NUMOPS1: numeric type constants — `U8.MAX`, `Int.MIN`,
            // `Float.INFINITY`/`NAN`/`EPSILON`.
            if let Some(nt) = crate::AST::numeric_type_from_name(type_name) {
                if let Some(cty) = numeric_const_type(&nt, member) {
                    return Some(cty);
                }
            }
            if self.is_known_enum(type_name) {
                let mut empty = Vec::new();
                return Some(self.check_enum_lit(type_name, member, &mut empty, span));
            }
        }
        self.borrow_ctx = true;
        let t = self.infer(inner)?;
        self.field_type(&t, member, span)
    }

    /// Resolve the type of `member` on the struct type `t` (S71 reuses this for
    /// `?.` chaining). Emits E0302 and returns `None` when there's no such field.
    pub(crate) fn field_type(&mut self, t: &Type, member: &str, span: Span) -> Option<Type> {
        if let Type::Named(type_name) = t {
            if let Some(fty) = core_struct_field(type_name, member) {
                return Some(fty);
            }
            if let Some(owner_mod) = self.struct_owner_module(type_name, None) {
                if let Some(fields) = self.struct_fields_of(owner_mod, type_name) {
                    for (fname, _, fty, is_ref, _) in fields {
                        if fname == member {
                            if *is_ref {
                                return None;
                            }
                            if owner_mod != self.module_idx
                                && !self.field_is_pub_in(owner_mod, type_name, member)
                            {
                                self.diags.push(private_item(member, span));
                                return None;
                            }
                            return Some(fty.clone());
                        }
                    }
                    let field_names: Vec<String> = fields.iter().map(|(n, ..)| n.clone()).collect();
                    let mut fix = format!("check the field names on `{}`", type_name);
                    if let Some(suggest) = suggest_field(member, &field_names) {
                        fix = format!("did you mean `{}`?", suggest);
                    }
                    self.diags.push(Diagnostic::error(
                        "E0302",
                        format!("`{}` has no field `{}`", type_name, member),
                        "field access only works on names declared in the struct".to_string(),
                        fix,
                        Some(span),
                    ));
                    return None;
                }
            }
        }
        if let Type::Apply { name, args } = t {
            if let Some(owner_mod) = self.struct_owner_module(name, None) {
                if let Some(fields) = self.struct_fields_of(owner_mod, name) {
                    let subst = self.struct_subst(name, args);
                    for (fname, _, fty, is_ref, _) in fields {
                        if fname == member {
                            if *is_ref {
                                return None;
                            }
                            if owner_mod != self.module_idx
                                && !self.field_is_pub_in(owner_mod, name, member)
                            {
                                self.diags.push(private_item(member, span));
                                return None;
                            }
                            return Some(self.trait_reg.instantiate_type(fty, &subst));
                        }
                    }
                    let field_names: Vec<String> = fields.iter().map(|(n, ..)| n.clone()).collect();
                    let mut fix = format!("check the field names on `{}`", name);
                    if let Some(suggest) = suggest_field(member, &field_names) {
                        fix = format!("did you mean `{}`?", suggest);
                    }
                    self.diags.push(Diagnostic::error(
                        "E0302",
                        format!("`{}` has no field `{}`", name, member),
                        "field access only works on names declared in the struct".to_string(),
                        fix,
                        Some(span),
                    ));
                    return None;
                }
            }
        }
        if let Type::Tuple(fields) = t {
            for (fname, fty) in fields {
                if fname == member {
                    return Some((**fty).clone());
                }
            }
            let field_names: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
            let mut fix = "check the member names in this tuple".to_string();
            if let Some(suggest) = suggest_field(member, &field_names) {
                fix = format!("did you mean `{}`?", suggest);
            }
            self.diags.push(Diagnostic::error(
                "E0302",
                format!("this tuple has no member `{}`", member),
                "field access only works on names declared in the tuple".to_string(),
                fix,
                Some(span),
            ));
            return None;
        }
        self.diags.push(Diagnostic::error(
            "E0302",
            format!("`.{}` only works on struct and tuple values", member),
            "enums and other values use methods or pattern tests instead".to_string(),
            format!("use a struct or tuple value before `.{}`", member),
            Some(span),
        ));
        None
    }

    pub(crate) fn infer_method_call(
        &mut self,
        receiver: &mut Box<Expr>,
        method: &str,
        span: Span,
        type_args: &[Type],
        args: &mut Vec<crate::AST::CallArg>,
        recv_type_out: &mut Option<String>,
        resolved_ret_out: &mut Option<Type>,
    ) -> Option<Type> {
        if method == "clone" {
            self.borrow_ctx = true;
            return self.infer(receiver);
        }
        // D-DIST3 (ratified 2026-06-20): `.raw()` unwraps a distinct type.
        if method == crate::Syntax::METHOD_DISTINCT_RAW {
            self.borrow_ctx = true;
            let recv_ty = self.infer(receiver)?;
            if let Type::Named(ref n) = recv_ty {
                if let Some(base) = self.registry.distinct_base(n).cloned() {
                    if !args.is_empty() {
                        self.diags.push(Diagnostic::error(
                            "E0103",
                            format!("`.{}()` takes no arguments", crate::Syntax::METHOD_DISTINCT_RAW),
                            "`.raw()` simply unwraps the base value — no arguments needed".to_string(),
                            "write `.raw()` with no arguments".to_string(),
                            Some(span),
                        ));
                    }
                    return Some(base);
                }
            }
            self.diags.push(Diagnostic::error(
                "E0311",
                format!("`.{}()` is only valid on a distinct type value", crate::Syntax::METHOD_DISTINCT_RAW),
                "`.raw()` unwraps a distinct type to its base representation".to_string(),
                format!("only call `.raw()` on a value whose type was declared with `{} distinct`", crate::Syntax::SIGIL_BIND_IMMUT),
                Some(span),
            ));
            return None;
        }
        // D-TOOL4 (E2-M11): `expect(x).snapshot()` — the special snapshot
        // assertion. Recognized by checking the receiver type.
        if method == Syntax::BUILTIN_SNAPSHOT {
            let recv_ty = self.infer(receiver);
            if recv_ty.as_ref().map(|t| t == &Type::Named("__JetExpect__".to_string())).unwrap_or(false) {
                // Valid: snapshot assertion — void, no return type.
                return None;
            }
            // Not from expect() — error.
            self.diags.push(Diagnostic::error(
                "E2901",
                format!("`.{}()` is only valid on the result of `{}(…)`", Syntax::BUILTIN_SNAPSHOT, Syntax::BUILTIN_EXPECT),
                "snapshot testing: call `expect(value).snapshot()` in a test block".to_string(),
                format!("e.g. `{}(my_result).snapshot()`", Syntax::BUILTIN_EXPECT),
                Some(span),
            ));
            return None;
        }
        if let Expr::Ident(root, _) = &**receiver {
            if root == "File" && method == Syntax::FOREIGN_OPEN {
                self.diags.push(Diagnostic::error(
                    "E0038",
                    "`File.open` is not the M10 file API".to_string(),
                    "M10 uses whole-file helpers in `core.fs`; file handles are out of scope"
                        .to_string(),
                    "import `core.fs as fs` and call `fs.read(path)` or `fs.write(path, text)`"
                        .to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        }
        // D-ENC1: nested-namespace access — `encoding.json.to_string(x)` where `encoding`
        // is a library alias (`use core.encoding`) and `json` a registered submodule. The
        // method-call receiver is `Field(Ident(alias), leaf)`; resolve to the submodule
        // `<ns>.<leaf>` as a core call. Guarded by `is_known_core_module`, so it fires only
        // for real submodules (e.g. `core.encoding.json`), never plain field access.
        if let Expr::Field(base, leaf, _) = &**receiver {
            if let Expr::Ident(alias, alias_span) = &**base {
                if let Some(ns) = self.core_imports.get(alias).cloned() {
                    let submodule = format!("{}.{}", ns, leaf);
                    if crate::Loader::is_known_core_module(&submodule) {
                        let ret =
                            self.infer_core_call(&submodule, method, *alias_span, span, type_args, args);
                        if is_polymorphic_core_special(&submodule, method) {
                            *resolved_ret_out = ret.clone();
                        }
                        return ret;
                    }
                }
            }
        }
        if let Expr::Ident(alias, alias_span) = &**receiver {
            if let Some(module) = self.core_imports.get(alias).cloned() {
                let ret = self.infer_core_call(&module, method, *alias_span, span, type_args, args);
                // c109 Phase 20: write the resolved return type back onto the node
                // for the polymorphic core specials whose type is arg-dependent and
                // NOT in `core_fixed_sig` (so the TIR can read it totally — I3). The
                // monomorphic calls (in `core_fixed_sig`) get their type from that
                // table at lowering, so leave `resolved_ret = None` for them.
                if is_polymorphic_core_special(&module, method) {
                    *resolved_ret_out = ret.clone();
                }
                return ret;
            }
            if let Some(&mod_idx) = self.imports.get(alias) {
                return self.infer_import_call(mod_idx, method, *alias_span, span, args);
            }
            // D-MOD2: inline code module call — `math.double(x)` where `math` is an
            // inline `module math { … }` in this file. Resolve via mangled name.
            if self.code_modules.contains_key(alias.as_str()) {
                let mangled = format!("{}__{}", alias, method);
                return self.infer_code_module_call(alias, &mangled, *alias_span, span, args);
            }
        }
        if let Expr::Ident(type_name, _) = &**receiver {
            if is_json_type_name(type_name) {
                if let Some(ret) = self.check_core_json_lit(method, args, span) {
                    return Some(ret);
                }
            }
            {
                let has_variant = self.resolve_enum_variants_cloned(type_name)
                    .map(|v| v.contains_key(method))
                    .unwrap_or(false);
                if has_variant {
                    let saved: Vec<Expr> = args
                        .iter_mut()
                        .map(|a| std::mem::replace(&mut a.expr, Expr::Int(0, a.span, None)))
                        .collect();
                    let mut enum_args: Vec<EnumLitArg> =
                        saved.into_iter().map(EnumLitArg::Positional).collect();
                    let ty = self.check_enum_lit(type_name, method, &mut enum_args, span);
                    for (a, ea) in args.iter_mut().zip(enum_args) {
                        if let EnumLitArg::Positional(e) = ea {
                            a.expr = e;
                        }
                    }
                    return Some(ty);
                }
            }
            if self.registry.method(type_name, method).is_some() {
                return self.check_static_method(type_name, method, span, args);
            }
            if let Some(ty) = builtin_type_from_ident(type_name) {
                if let Some(ret) = Collections::builtin_method_return(&ty, method, args.len(), true)
                {
                    return self.finish_builtin_method(receiver, method, &ty, args, span, ret);
                }
            }
        }
        self.borrow_ctx = true;
        let recv_ty = self.infer(receiver)?;
        // E0964: length-changing methods are forbidden on a fixed-size [T#N].
        if let Type::FixedList { .. } = &recv_ty {
            if matches!(method, "push" | "pop" | "insert" | "remove" | "clear") {
                self.diags.push(Diagnostic::error(
                    "E0964",
                    format!(
                        "`{}` changes a list's length, but this is a fixed-size {}",
                        method,
                        recv_ty.show()
                    ),
                    "the length of `[T#N]` is fixed at compile time and cannot change".to_string(),
                    "widen to `var r: [T] = ...` if you need a growable list".to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        }
        // E2-M7: method calls on streaming file handles (D-IO2).
        if let Type::Named(handle_ty) = &recv_ty {
            if let Some(ret) = file_handle_method_return(handle_ty, method, args.len(), span, &mut self.diags) {
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                *recv_type_out = Some(handle_ty.clone());
                return ret;
            }
        }
        // E2-M10: method calls on net/http opaque types.
        if let Type::Named(handle_ty) = &recv_ty {
            if let Some(ret) = net_method_return(handle_ty, method, args.len(), span, &mut self.diags) {
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                *recv_type_out = Some(handle_ty.clone());
                return ret;
            }
        }
        // D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): method calls on
        // Arena/Bump/Pool/Fixed allocators. E3104: use-after-free/reset.
        if let Type::Named(handle_ty) = &recv_ty {
            let handle_ty_s = handle_ty.clone();
            if let Some(ret) = alloc_method_return(&handle_ty_s, method, args, span, &mut self.diags) {
                // E3104: check for use-after-free/reset before inferring args.
                let recv_name = if let Expr::Ident(n, _) = &**receiver { Some(n.clone()) } else { None };
                // D-ALLOC-D: E3104 — `alloc` after `free` is always wrong (the allocator
                // is consumed). After `reset`, further `alloc` is valid (buffer is reused).
                if method == "alloc" {
                    if let Some(ref name) = recv_name {
                        if self.freed_allocators.contains_key(name.as_str()) {
                            self.diags.push(e3104(name, "free", span));
                        }
                    }
                }
                // Mark the allocator as freed only on `free`. `reset` keeps it alive.
                if method == "free" {
                    if let Some(ref name) = recv_name {
                        self.freed_allocators.insert(name.clone(), "free".to_string());
                    }
                }
                // D-ALLOC2: `reset`/`free` invalidate every value previously
                // allocated in this arena. Any view of it used afterward is
                // E0632 (use-after-reset/free) — the runtime `&mut self`/`self`
                // signatures would also reject, so Jet rejects first (I2).
                if method == "reset" || method == "free" {
                    if let Some(ref name) = recv_name {
                        self.kill_views_of_arena(name, method, span);
                    }
                }
                *recv_type_out = Some(handle_ty_s.clone());
                // For `alloc`, infer the argument and return its type.
                if method == "alloc" {
                    if let Some(arg) = args.get_mut(0) {
                        let inferred = self.infer(&mut arg.expr);
                        return inferred;
                    }
                    return None;
                }
                for a in args.iter_mut() { self.infer(&mut a.expr); }
                return ret;
            }
        }
        // D-ARGS1: method calls on ArgsSpec / ParsedArgs (builder and result types).
        if let Type::Named(handle_ty) = &recv_ty {
            if handle_ty == "ArgsSpec" {
                if let Some(ret) = args_spec_method_return(method, args.len(), span, &mut self.diags) {
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    *recv_type_out = Some("ArgsSpec".to_string());
                    return ret;
                }
            }
            if handle_ty == "ParsedArgs" {
                if let Some(ret) = parsed_args_method_return(method, args.len(), span, &mut self.diags) {
                    for a in args.iter_mut() { self.infer(&mut a.expr); }
                    *recv_type_out = Some("ParsedArgs".to_string());
                    return ret;
                }
            }
        }
        if let Type::Named(n) = &recv_ty {
            if let Some(param) = self.type_param_scope.iter().find(|p| p.name == *n) {
                for (trait_name, info) in &self.trait_reg.traits {
                    if let Some(msig) = info.methods.get(method) {
                        if !param.bounds.iter().any(|b| b == trait_name) {
                            self.diags.push(e0901(method, trait_name, span));
                        }
                        *recv_type_out = Some(n.clone());
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                        return msig.return_type.clone();
                    }
                }
            }
        }
        if let Some(ret) = Collections::builtin_method_return(&recv_ty, method, args.len(), false) {
            // D-NUMOPS1: hand codegen the receiver's numeric width so it picks the
            // same widening/narrowing form sema just chose for the return type.
            if recv_ty.is_numeric() {
                *recv_type_out = Some(recv_ty.name());
            }
            let result = self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
            // D-ITER1: enumerate/zip/partition return named-tuple types. Store the
            // resolved return type in `resolved_ret_out` so Tuples.rs can collect
            // the JetTup_ shape and the TIR lowering pass can read the field names.
            if let Some(ref ty) = result {
                if contains_tuple_type(ty) {
                    *resolved_ret_out = Some(ty.clone());
                }
            }
            return result;
        }
        if let Type::TraitObject(trait_name) = &recv_ty {
            let sig = self
                .trait_reg
                .traits
                .get(trait_name)
                .and_then(|t| t.methods.get(method));
            if let Some(msig) = sig {
                *recv_type_out = Some(trait_name.clone());
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return msig.return_type.clone();
            }
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("trait `{trait_name}` has no method `{method}`"),
                "check the method name on this trait value".to_string(),
                format!("add `fn {method}(…)` to `trait {trait_name}`"),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        let type_name = match &recv_ty {
            Type::Named(n) => n.clone(),
            Type::Option(inner) => match inner.as_ref() {
                Type::Named(n) => n.clone(),
                _ => {
                    self.diags.push(Diagnostic::error(
                        "E0311",
                        format!("`{}` isn't a method on this value", method),
                        "instance methods belong to struct or enum values".to_string(),
                        format!(
                            "call it on the type: `{}.{method}(...)` if it's static",
                            recv_ty.name()
                        ),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
            },
            _ => {
                self.diags.push(Diagnostic::error(
                    "E0311",
                    format!("`{}` isn't a method on this value", method),
                    "only struct and enum values have instance methods".to_string(),
                    format!("check the spelling of `{}`", method),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        };
        if let Some(fields) = self.registry.struct_fields(&type_name) {
            if let Some((_, _, field_ty, _, _)) =
                fields.iter().find(|(fname, _, _, _, _)| fname == method)
            {
                if matches!(field_ty, Type::Fn { .. }) {
                    *recv_type_out = Some(type_name.clone());
                    let mut callee =
                        Box::new(Expr::Field(receiver.clone(), method.to_string(), span));
                    let end = args.last().map(|a| a.expr.span().end).unwrap_or(span.end);
                    let call_span = Span::new(span.start, end);
                    return self.infer_call_value(&mut callee, args, call_span);
                }
            }
        }
        let Some(msig) = self.registry.method(&type_name, method).cloned() else {
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("`{}` has no method `{}`", type_name, method),
                "check the method name on this type".to_string(),
                format!("define it inside `struct {type_name}` or `impl {type_name}`"),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        if msig.is_static {
            self.diags.push(Diagnostic::error(
                "E0311",
                format!("`{}` is a static method on `{}`", method, type_name),
                "static methods belong to the type name, not a value".to_string(),
                format!("write `{}.{method}(...)` instead", type_name),
                Some(span),
            ));
        }
        *recv_type_out = Some(type_name.clone());
        // `mut self` methods change the receiver: it must be changeable,
        // free of an active `for` borrow, and not aliased by an argument.
        if msig.self_conv == Some(AccessConvention::Write) {
            if let Some(root) = expr_root_ident(receiver) {
                let root = root.to_string();
                if self.iter_borrowed.contains(&root) {
                    self.diags.push(collection_changed_in_loop(&root, span));
                }
                if let Some(info) = self.lookup(&root) {
                    if !info.mutable {
                        let (what, fix) = if root == Syntax::KW_SELF {
                            (
                                format!(
                                    "`.{}()` edits `{}`, but this method has read access only",
                                    method,
                                    Syntax::KW_SELF
                                ),
                                format!(
                                    "declare the enclosing method with `{} {}`",
                                    Syntax::KW_MUTATE,
                                    Syntax::KW_SELF
                                ),
                            )
                        } else {
                            (
                                format!(
                                    "cannot write to `{}` — it does not have edit access (`~`); required before calling `.{}()`",
                                    root,
                                    method
                                ),
                                format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT),
                            )
                        };
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            what,
                            "this method edits the value it's called on; write access (`~`) is required".to_string(),
                            fix,
                            Some(span),
                        ));
                    }
                }
                for arg in args.iter() {
                    if matches!(&arg.expr, Expr::Ident(n, _) if *n == root) {
                        self.diags.push(aliasing_while_mut(&root, arg.expr.span()));
                    }
                }
            }
        }
        if msig.self_conv == Some(AccessConvention::Move) {
            if let Expr::Ident(n, nspan) = &**receiver {
                // A borrowed parameter can't be consumed (the generated Rust
                // would move out of a `&T`/`&mut T`).
                if let Some(info) = self.lookup(n) {
                    if !type_is_copy(&info.ty)
                        && matches!(
                            info.param_conv,
                            Some(AccessConvention::Read) | Some(AccessConvention::Write)
                        )
                    {
                        self.diags.push(Diagnostic::error(
                            "E0120",
                            format!(
                                "`{}` was not moved here, so `.{}()` cannot take it (`^`)",
                                n, method
                            ),
                            "this function has read access only and does not own the value".to_string(),
                            format!(
                                "call it on a copy: `{}.clone().{}(...)` — or take ownership with `{} {}: {}`",
                                n,
                                method,
                                Syntax::KW_MOVE,
                                n,
                                info.ty.name()
                            ),
                            Some(*nspan),
                        ));
                    }
                }
                self.mark_moved(n.clone(), *nspan);
            }
        }
        self.check_method_args(&type_name, method, &msig, args, span)?;
        msig.return_type.clone()
    }

    /// Binary operators, including comparison distribution (S25):
    /// `day == "mon" || "tue"` re-applies the nearest comparison.
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
                    // S25: a plain value re-applies the nearest comparison.
                    if let Some((subject, cmp_op)) = rightmost_comparison(lhs) {
                        let rhs_span = rhs.span();
                        let new_span = Span::new(subject.span().start, rhs_span.end);
                        let old_rhs = std::mem::replace(rhs.as_mut(), Expr::Bool(false, rhs_span));
                        **rhs =
                            Expr::Binary(cmp_op, Box::new(subject), Box::new(old_rhs), new_span);
                        // Re-check the rebuilt comparison; this reports a
                        // mismatch (E0109) if the value's type doesn't fit.
                        self.infer_rebuilt(rhs);
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0110",
                            format!(
                                "the right side of `{}` must be {}, but this is {}",
                                op.spell(),
                                Type::Bool.show(),
                                rt.show()
                            ),
                            format!(
                                "right after a comparison, a plain value repeats it (`x == 1 {} 2` means `x == 1 {} x == 2`, S25) — but there's no comparison before this one",
                                op.spell(),
                                op.spell()
                            ),
                            "compare the value to something, e.g. `x == 2`".to_string(),
                            Some(rhs.span()),
                        ));
                    }
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
            let lt_is_distinct = if let Type::Named(n) = &lt { self.registry.is_distinct(n) } else { false };
            let rt_is_distinct = if let Type::Named(n) = &rt { self.registry.is_distinct(n) } else { false };
            let is_arith = matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr);
            let is_eq = matches!(op, BinOp::Eq | BinOp::Ne);
            if (lt_is_distinct || rt_is_distinct) && is_arith {
                let distinct_name = if lt_is_distinct {
                    if let Type::Named(n) = &lt { n.as_str() } else { "" }
                } else {
                    if let Type::Named(n) = &rt { n.as_str() } else { "" }
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
                        if let Type::Named(n) = &lt { n.clone() } else { lt.name() }
                    } else {
                        if let Type::Named(n) = &rt { n.clone() } else { rt.name() }
                    };
                    self.diags.push(Diagnostic::error(
                        "E0128",
                        format!("a `{}` can't be compared with a `{}`", lt.name(), rt.name()),
                        format!("`{}` is a distinct type; it only compares equal to another `{}`", dt_name, dt_name),
                        format!("use `.raw()` to compare the underlying values, or construct a `{}`", dt_name),
                        Some(span),
                    ));
                    return None;
                }
                return Some(Type::Bool);
            }
            // Implicit coercion check (non-arithmetic, non-eq): handled at assignment.
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
            BinOp::Rem | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                // D-SG9: remainder and bit ops work on any integer width, both
                // sides the same width, and keep it.
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

    /// Re-infer a node we just built ourselves (S25); it can still report
    /// a type mismatch, but never duplicates earlier errors because both
    /// halves were already clean.
    pub(crate) fn infer_rebuilt(&mut self, e: &mut Expr) {
        self.infer(e);
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

    /// Check a call. Returns:
    ///   None             — problem already reported
    ///   Some(None)       — fine, no value handed back
    ///   Some(Some(ty))   — fine, hands back `ty`
    /// D-NUMOPS1: type a `wrapping`/`saturating`/`checked` opt-in. The single
    /// argument must be one integer `+`/`-`/`*`/`/`; `wrapping`/`saturating`
    /// return the operand width, `checked` returns it optional (`null` on
    /// overflow). E1005 otherwise.
    fn check_overflow_opt_in(&mut self, call: &mut Call) -> Option<Type> {
        let kind = call.name.clone();
        if call.args.len() != 1 {
            let mut ty = None;
            for a in call.args.iter_mut() {
                ty = ty.or(self.infer(&mut a.expr));
            }
            self.diags.push(overflow_opt_in_error(&kind, call.name_span));
            // Hand back a plausible type so the use site doesn't cascade.
            return ty.filter(Type::is_integer).or(Some(Type::Int));
        }
        let arg_ty = self.infer(&mut call.args[0].expr);
        let is_arith = matches!(
            &call.args[0].expr,
            Expr::Binary(op, _, _, _)
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
        );
        let int_ok = arg_ty.as_ref().is_some_and(|t| t.is_integer());
        if !is_arith || !int_ok {
            self.diags.push(overflow_opt_in_error(&kind, call.name_span));
            return arg_ty.filter(Type::is_integer).or(Some(Type::Int));
        }
        let t = arg_ty.unwrap();
        if kind == Syntax::BUILTIN_CHECKED {
            Some(Type::Option(Box::new(t)))
        } else {
            Some(t)
        }
    }

    pub(crate) fn check_call(&mut self, call: &mut Call, _as_value: bool) -> Option<Option<Type>> {
        // D-NUMOPS1: `wrapping`/`saturating`/`checked` opt-ins wrap a single integer
        // `+`/`-`/`*`/`/`. A user-defined function of the same name shadows them.
        if matches!(
            call.name.as_str(),
            Syntax::BUILTIN_WRAPPING | Syntax::BUILTIN_SATURATING | Syntax::BUILTIN_CHECKED
        ) && !self.funcs.contains_key(&call.name)
        {
            return Some(self.check_overflow_opt_in(call));
        }
        // D-EFF1: an ambient builtin (`print`/`input`) contributes the `Io`
        // effect, unless a user function of the same name shadows it (in which
        // case the edge to that user function is recorded below).
        if !self.funcs.contains_key(&call.name) {
            if let Some(e) = builtin_effect(&call.name) {
                self.record_effect(e);
            }
        }
        if call.name == Syntax::FOREIGN_PRINTLN || call.name == Syntax::FOREIGN_EPRINTLN {
            let target = if call.name == Syntax::FOREIGN_EPRINTLN {
                "io.eprint"
            } else {
                Syntax::BUILTIN_PRINT
            };
            self.diags.push(Diagnostic::error(
                "E0037",
                format!(
                    "{} calls it `{}`, not `{}`",
                    Syntax::LANG_NAME,
                    target,
                    call.name
                ),
                "`print` writes to stdout; `io.eprint` is the stderr twin in `core.io`".to_string(),
                format!("replace `{}` with `{}`", call.name, target),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }

        if call.name == Syntax::FOREIGN_OPEN {
            self.diags.push(Diagnostic::error(
                "E0038",
                "`open` is not the M10 file API".to_string(),
                "M10 uses whole-file helpers in `core.fs`; file handles are out of scope"
                    .to_string(),
                "import `core.fs as fs` and call `fs.read(path)` or `fs.write(path, text)`"
                    .to_string(),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }

        if call.name == Syntax::FOREIGN_GETENV {
            self.diags.push(Diagnostic::error(
                "E0039",
                "`getenv` is written `env.get` in Jet".to_string(),
                "environment access lives in the `core.env` module".to_string(),
                "import `core.env as env` and call `env.get(name)`".to_string(),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }

        if matches!(
            call.name.as_str(),
            Syntax::FOREIGN_ASYNC | Syntax::FOREIGN_AWAIT
        ) {
            self.diags.push(Diagnostic::error(
                "E0040",
                format!("`{}` is not in Jet; use `tasks.spawn` instead", call.name),
                "Jet uses blocking tasks and channels, not async/await — simpler and race-free"
                    .to_string(),
                "import `core.tasks as tasks` and call `tasks.spawn(() => your_work())`".to_string(),
                Some(call.name_span),
            ));
            for a in call.args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }

        if matches!(
            call.name.as_str(),
            Syntax::FOREIGN_MUTEX | Syntax::FOREIGN_LOCK | "RwLock" | "mutex"
        ) {
            self.diags.push(Diagnostic::error(
                "E0041",
                format!(
                    "`{}` is not in Jet; share data through channels",
                    call.name
                ),
                "Jet avoids shared mutable state: tasks communicate by sending messages, not sharing memory"
                    .to_string(),
                "import `core.tasks as tasks`, create a channel, and use `sender.send`/`channel.receive`"
                    .to_string(),
                Some(call.name_span),
            ));
            for a in call.args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }

        if call.name == Syntax::BUILTIN_PRINT {
            if call.args.len() != 1 {
                self.diags.push(Diagnostic::error(
                    "E0103",
                    format!(
                        "`{}` needs exactly one thing to print",
                        Syntax::BUILTIN_PRINT
                    ),
                    "printing nothing isn't meaningful".to_string(),
                    format!("e.g. {}(\"hello\")", Syntax::BUILTIN_PRINT),
                    Some(call.name_span),
                ));
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
            let arg = &mut call.args[0];
            self.borrow_ctx = true; // print borrows via `.jet_show()`
            if let Some(t) = self.infer(&mut arg.expr) {
                if !is_printable(&t, self.registry) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` doesn't know how to show {}",
                            Syntax::BUILTIN_PRINT,
                            t.show()
                        ),
                        "print shows values that have a display".to_string(),
                        "print one of its parts instead".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
            }
            return Some(None);
        }

        // D-PRELUDE1 = B: `input` is ambient — no `use core.io` needed.
        // Resolves to the same semantics as `io.input`: optional String prompt,
        // returns Result(String, IoError). Shadowed by any user-defined `input`.
        if call.name == Syntax::BUILTIN_INPUT
            && self.funcs.get(Syntax::BUILTIN_INPUT).is_none()
            && self.lookup(Syntax::BUILTIN_INPUT).is_none()
        {
            if call.args.len() > 1 {
                self.diags
                    .push(wrong_core_arity(Syntax::BUILTIN_INPUT, 1, call.args.len(), call.name_span));
            }
            if let Some(arg) = call.args.get_mut(0) {
                self.expect_core_arg(Syntax::BUILTIN_INPUT, 0, &Type::String, arg);
            }
            return Some(Some(result_ty(Type::String, io_error_ty())));
        }

        if call.name == Syntax::BUILTIN_PANIC {
            self.check_panic_call(call);
            return Some(None);
        }

        if call.name == Syntax::BUILTIN_REQUIRE {
            self.check_require_call(call);
            return Some(None);
        }

        if call.name == Syntax::BUILTIN_REQUIRE_EQ {
            self.check_require_eq_call(call);
            return Some(None);
        }

        // D-TOOL4 (E2-M11): `expect(x)` — test-only builtin that wraps a value
        // for snapshot testing. The expression `expect(x).snapshot()` is the
        // full form; `.snapshot()` is handled in the method-call path below.
        if call.name == Syntax::BUILTIN_EXPECT {
            if call.args.len() != 1 {
                self.diags.push(Diagnostic::error(
                    "E2901",
                    format!("`{}` needs exactly one value to test", Syntax::BUILTIN_EXPECT),
                    "snapshot testing wraps one value at a time".to_string(),
                    format!("e.g. {}(my_value).snapshot()", Syntax::BUILTIN_EXPECT),
                    Some(call.name_span),
                ));
            } else {
                self.infer(&mut call.args[0].expr);
            }
            // Returns a Named type marker so the `.snapshot()` call can detect it.
            return Some(Some(Type::Named("__JetExpect__".to_string())));
        }

        if self.funcs.get(&call.name).is_none() {
            if let Some(info) = self.lookup(&call.name) {
                if matches!(info.ty, Type::Fn { .. }) {
                    let name_span = call.name_span;
                    let mut callee = Box::new(Expr::Ident(call.name.clone(), name_span));
                    let mut args = std::mem::take(&mut call.args);
                    let end = args
                        .last()
                        .map(|a| a.expr.span().end)
                        .unwrap_or(name_span.end);
                    let span = Span::new(name_span.start, end);
                    let ret = self.infer_call_value(&mut callee, &mut args, span);
                    call.args = args;
                    return Some(ret);
                }
            }
            // D-MOD3: check unqualified inline-module imports (e.g. `use math.clamp`).
            if let Some(mangled) = self.unqualified.get(&call.name).cloned() {
                let alias = mangled.split("__").next().unwrap_or(&mangled).to_string();
                let result = self.infer_code_module_call(&alias, &mangled, call.name_span, call.name_span, &mut call.args);
                return Some(result);
            }
            // D-MOD3: check unqualified file-module imports (e.g. `use math.clamp` for a file module).
            if let Some((fn_name, mod_idx)) = self.unqualified_file.get(&call.name).cloned() {
                let result = self.infer_import_call(mod_idx, &fn_name, call.name_span, call.name_span, &mut call.args);
                return Some(result);
            }
        }

        // D-DIST3 (ratified 2026-06-20): `DistinctType(expr)` — construct a distinct value.
        if self.funcs.get(&call.name).is_none() {
            if let Some(base_ty) = self.registry.distinct_base(&call.name).cloned() {
                if call.args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!(
                            "`{}` takes exactly one argument, got {}",
                            call.name,
                            call.args.len()
                        ),
                        format!("`{}` is a distinct type; construct it with `{}(value)`", call.name, call.name),
                        format!("write `{}(expr)` with a single value of type `{}`", call.name, base_ty.name()),
                        Some(call.name_span),
                    ));
                    for a in call.args.iter_mut() { self.infer(&mut a.expr); }
                    return None;
                }
                let old_expected = self.expected_type.replace(base_ty.clone());
                let arg_ty = self.infer(&mut call.args[0].expr);
                self.expected_type = old_expected;
                if let Some(at) = arg_ty {
                    if at != base_ty {
                        self.diags.push(Diagnostic::error(
                            "E0128",
                            format!(
                                "a `{}` can't be used where a `{}` is expected",
                                at.name(), call.name
                            ),
                            format!(
                                "`{}` and `{}` are different types — even though `{}` is built on `{}`, one is never accepted in place of the other",
                                call.name, at.name(), call.name, base_ty.name()
                            ),
                            format!("construct a `{}`: `{}({})`", call.name, call.name, "expr"),
                            Some(call.args[0].expr.span()),
                        ));
                        return None;
                    }
                }
                return Some(Some(Type::Named(call.name.clone())));
            }
        }

        let Some(sig) = self.funcs.get(&call.name).cloned() else {
            let mut fix = format!(
                "define it first ({} {}() {{ ... }}), or call one that exists",
                Syntax::KW_FN,
                call.name
            );
            let mut best: Option<(&str, usize)> = None;
            for cand in self
                .funcs
                .keys()
                .map(|s| s.as_str())
                .chain(Syntax::PRELUDE_IDENTS.iter().copied())
            {
                let d = edit_distance(&call.name, cand);
                if d <= 2 && best.map_or(true, |(_, bd)| d < bd) {
                    best = Some((cand, d));
                }
            }
            if let Some((cand, _)) = best {
                fix = format!("did you mean `{}`?", cand);
            }
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("nothing named `{}` exists here", call.name),
                format!(
                    "only functions that have been defined (or built in, like `{}` / `{}`) can be called",
                    Syntax::BUILTIN_PRINT, Syntax::BUILTIN_INPUT
                ),
                fix,
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        };

        // D-EFF1: record the call-graph edge for transitive effect inference.
        // A foreign (`extern`) callee has an un-inspectable body, so it forces
        // the maximal effect set; a Jet callee's effects flow in via its edge.
        if sig.is_extern {
            self.record_maximal();
        } else {
            self.record_edge(call.name.clone());
        }

        // E3103 (S58): an `#Unsafe fn` is a whole-function contract; callers
        // must take responsibility inside their own `#Unsafe` block.
        if sig.is_unsafe && !self.in_unsafe {
            self.diags.push(Diagnostic::error(
                "E3103",
                format!("`{}` is an `#Unsafe` function", call.name),
                "its contract can't be checked by the compiler, so the caller must vouch for it"
                    .to_string(),
                format!(
                    "call it inside `#{}(\"…\") {{ … }}`",
                    Syntax::KW_UNSAFE
                ),
                Some(call.name_span),
            ));
        }

        // D-NARG-D4 (S61, E0125): label validation — if a call arg has
        // `name: val`, verify it matches the parameter name at that position.
        // Labels never reorder.
        if !sig.param_info.is_empty() {
            let all_param_names: Vec<&str> =
                sig.param_info.iter().map(|(n, _)| n.as_str()).collect();
            for (i, arg) in call.args.iter().enumerate() {
                if let Some((label, label_span)) = &arg.label {
                    if let Some((param_name, _)) = sig.param_info.get(i) {
                        if label != param_name {
                            // Is the label a real param name at a different position?
                            if all_param_names.contains(&label.as_str()) {
                                // Transposed: label names a real param, but wrong position.
                                self.diags.push(Diagnostic::error(
                                    "E0125",
                                    format!(
                                        "label `{}:` doesn't match the parameter `{}` here",
                                        label, param_name
                                    ),
                                    "labels are checked documentation — each names the parameter at its own position, and arguments stay in the order they're declared".to_string(),
                                    format!(
                                        "write `{}:` here, or drop the label",
                                        param_name
                                    ),
                                    Some(*label_span),
                                ));
                            } else {
                                // Unknown: label doesn't name any parameter.
                                self.diags.push(Diagnostic::error(
                                    "E0125",
                                    format!(
                                        "`{}` has no parameter named `{}`",
                                        call.name, label
                                    ),
                                    format!(
                                        "a label must name the parameter at its position; `{}` takes {}",
                                        call.name,
                                        all_param_names.join(", ")
                                    ),
                                    format!(
                                        "use one of `{}`'s parameter names, or drop the label",
                                        call.name
                                    ),
                                    Some(*label_span),
                                ));
                            }
                        }
                    }
                }
            }
            // L2401: advisory lint — public API has a positional Bool parameter.
            // (Only warn on the callee definition side, not every call site.)
        }

        // D-NARG-D2 (S61): default-value filling — append defaults for omitted
        // trailing params. Earlier-param refs in defaults are substituted with
        // the supplied argument expression so codegen never sees an unresolved
        // identifier (invariant I2).
        if call.args.len() < sig.params.len() && !sig.defaults.is_empty() {
            let provided = call.args.len();
            let required: usize = sig
                .defaults
                .iter()
                .take_while(|d| d.is_none())
                .count();
            if provided >= required {
                // fill trailing omitted params with their defaults. We build
                // `earlier_names` incrementally so a default like `d: Int = h`
                // can reference an earlier-defaulted param `h` that was already
                // filled (and is now in call.args at position 1).
                let all_param_names: Vec<String> =
                    sig.param_info.iter().map(|(n, _)| n.clone()).collect();
                for i in provided..sig.params.len() {
                    if let Some(Some(default_expr)) = sig.defaults.get(i) {
                        // earlier_names covers all params up to (not including) i.
                        let earlier_names: Vec<String> =
                            all_param_names.iter().take(i).cloned().collect();
                        // Substitute any earlier-param idents with the supplied arg.
                        let resolved = super::substitute_param_refs(
                            default_expr.clone(),
                            &earlier_names,
                            &call.args,
                        );
                        call.args.push(crate::AST::CallArg {
                            convention: sig.params[i].0,
                            expr: resolved,
                            span: call.name_span,
                            flags: Default::default(),
                            label: None,
                        });
                    }
                }
            }
        }

        if call.args.len() != sig.params.len() {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`{}` expects {} argument{}, got {}",
                    call.name,
                    sig.params.len(),
                    if sig.params.len() == 1 { "" } else { "s" },
                    call.args.len()
                ),
                "every argument must match a parameter".to_string(),
                format!("check the definition of `{}`", call.name),
                Some(call.name_span),
            ));
        }

        let fn_type_params = self
            .trait_reg
            .fn_params
            .get(&call.name)
            .cloned()
            .unwrap_or_default();
        let mut generic_subst = HashMap::new();
        let mut pre_inferred: Vec<Option<Type>> = Vec::new();
        if !fn_type_params.is_empty() {
            for arg in call.args.iter_mut() {
                pre_inferred.push(self.infer(&mut arg.expr));
            }
            let arg_types: Vec<Type> = pre_inferred.iter().filter_map(|t| t.clone()).collect();
            if arg_types.len() == call.args.len() {
                match self.trait_reg.infer_fn_subst(
                    &sig,
                    &arg_types,
                    &fn_type_params,
                    self.expected_type.as_ref(),
                ) {
                    Ok(s) => generic_subst = s,
                    Err(p) => self.diags.push(e0904(call.name_span, &p)),
                }
            }
        }
        let effective_params: Vec<(AccessConvention, Type)> = if generic_subst.is_empty() {
            sig.params.clone()
        } else {
            sig.params
                .iter()
                .map(|(c, t)| (*c, self.trait_reg.instantiate_type(t, &generic_subst)))
                .collect()
        };
        let args_pre_inferred = !generic_subst.is_empty() && pre_inferred.len() == call.args.len();

        let mut mut_borrowed: HashSet<String> = HashSet::new();
        let mut read_borrowed: HashSet<String> = HashSet::new();

        for (i, arg) in call.args.iter_mut().enumerate() {
            if let Expr::Ident(name, span) = &arg.expr {
                if mut_borrowed.contains(name) {
                    self.diags.push(aliasing_while_mut(name, *span));
                } else if arg.convention == AccessConvention::Write && read_borrowed.contains(name)
                {
                    self.diags.push(aliasing_mut_after_read(name, *span));
                }
            }
            if !sig.is_extern {
                if let Some((AccessConvention::Read, pty)) = effective_params.get(i) {
                    if !pty.is_scalar() {
                        self.borrow_ctx = true;
                    }
                }
            } else if let Some((_, pty)) = effective_params.get(i) {
                if !pty.is_scalar() {
                    arg.flags.implicit_clone = true;
                }
            }
            let saved_exp = self.expected_type.clone();
            let saved_esc = self.lambda_escapes;
            if let Some((param_conv, param_ty)) = effective_params.get(i) {
                if matches!(param_ty, Type::Fn { .. }) {
                    self.expected_type = Some(param_ty.clone());
                    self.lambda_escapes = matches!(param_conv, AccessConvention::Move);
                } else if matches!(param_ty, Type::IntN { .. } | Type::Float32) {
                    // D-SG9: let a fixed-width literal argument adopt the parameter's
                    // width and be range-checked at the literal.
                    self.expected_type = Some(param_ty.clone());
                }
            }
            let arg_ty = if args_pre_inferred {
                pre_inferred.get(i).and_then(|t| t.clone())
            } else {
                self.infer(&mut arg.expr)
            };
            self.expected_type = saved_exp;
            self.lambda_escapes = saved_esc;
            let Some((param_conv, param_ty)) = effective_params.get(i) else {
                continue;
            };
            // D-EFF2: a function value passed to a function-typed parameter flows
            // its effects through to this caller (transparent flow-through).
            if matches!(param_ty, Type::Fn { .. }) {
                self.attribute_fn_arg(&arg.expr);
            }
            if arg.convention == AccessConvention::Write && !matches!(arg.expr, Expr::Ident(_, _))
            {
                self.diags.push(Diagnostic::error(
                    "E0202",
                    format!(
                        "`{}` needs a plain named binding after it",
                        Syntax::KW_MUTATE
                    ),
                    "write access (`~`) can only be granted to a named binding, not an expression".to_string(),
                    format!(
                        "bind the value first: `x {} ...` then pass `{} x`",
                        Syntax::SIGIL_BIND_MUT,
                        Syntax::KW_MUTATE
                    ),
                    Some(arg.span),
                ));
            }

            if let Some(arg_ty) = &arg_ty {
                let param_ty = self.resolve_type(param_ty.clone());
                let arg_ty = self.resolve_type(arg_ty.clone());
                let reported = self.check_type_assignable(&param_ty, &arg_ty, arg.expr.span());
                // D-FIXARR1: [T#N] widens to [T] at a call site — compatible but codegen
                // will emit .to_vec() on the argument.
                let fixed_widens = matches!((&param_ty, &arg_ty),
                    (Type::List(pe), Type::FixedList { elem: ae, .. }) if pe == ae);
                let compatible = arg_ty == param_ty
                    || fixed_widens
                    || (matches!(&param_ty, Type::Fn { .. })
                        && matches!(&arg_ty, Type::Fn { .. })
                        && fn_types_compatible(&param_ty, &arg_ty));
                if !reported && !compatible {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` wants {} for argument {}, but this is {}",
                            call.name,
                            param_ty.show(),
                            i + 1,
                            arg_ty.show()
                        ),
                        "every argument must match its parameter's type".to_string(),
                        type_fix_hint(&param_ty, &arg_ty),
                        Some(arg.expr.span()),
                    ));
                }
            }

            match (param_conv, arg.convention) {
                (AccessConvention::Move, AccessConvention::Read) => {
                    if let Expr::Ident(name, span) = &arg.expr {
                        if is_cloneable(param_ty, self.registry, self.structs) {
                            arg.flags.implicit_clone = true;
                            // D-L0201: only warn when the value is dead after
                            // this call (a wasteful clone).
                            if !self.is_name_live_after(name) {
                                self.diags.push(Diagnostic::lint(
                                    "L0201",
                                    format!(
                                        "implicit clone of `{}`; write `{} {}` to transfer ownership or `.clone()` to silence this warning",
                                        name,
                                        Syntax::KW_MOVE,
                                        name
                                    ),
                                    format!(
                                        "`{}` expects to take ownership of this value",
                                        call.name
                                    ),
                                    format!(
                                        "write `{} {}` to move, or `{} .clone()` to copy explicitly",
                                        Syntax::KW_MOVE,
                                        name,
                                        name
                                    ),
                                    Some(*span),
                                ));
                            }
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0201",
                                format!(
                                    "`{}` needs `{}` here — this value can't be copied",
                                    call.name,
                                    Syntax::KW_MOVE
                                ),
                                format!(
                                    "parameter {} takes ownership (`^`); passing `{}` without `{}` would have to copy it, but this type can't be copied",
                                    i + 1,
                                    name,
                                    Syntax::KW_MOVE
                                ),
                                format!(
                                    "write `{} {}` to move ownership to `{}`",
                                    Syntax::KW_MOVE,
                                    name,
                                    call.name
                                ),
                                Some(*span),
                            ));
                        }
                    }
                }
                (AccessConvention::Move, AccessConvention::Move) => {
                    // The value is given away for real.
                    if let Expr::Ident(name, span) = &arg.expr {
                        if !param_ty.is_scalar() {
                            self.mark_moved(name.clone(), *span);
                        }
                    }
                }
                (AccessConvention::Write, AccessConvention::Read) => {
                    if let Expr::Ident(name, span) = &arg.expr {
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            format!(
                                "parameter `{}` requires write access (`~`) at the call site",
                                name
                            ),
                            format!(
                                "`{}` needs to edit (`~`) this value; passing it without `{}` grants only read access",
                                call.name,
                                Syntax::KW_MUTATE
                            ),
                            format!(
                                "write `{} {}` when calling `{}`",
                                Syntax::KW_MUTATE,
                                name,
                                call.name
                            ),
                            Some(*span),
                        ));
                    }
                }
                (AccessConvention::Write, AccessConvention::Write) => {
                    // `mut x` at the call site: x itself must be changeable.
                    if let Expr::Ident(name, span) = &arg.expr {
                        if let Some(info) = self.lookup(name) {
                            if !info.mutable {
                                self.diags.push(Diagnostic::error(
                                    "E0111",
                                    format!(
                                        "`{}` was made with `{}`, so it can't be changed",
                                        name,
                                        Syntax::SIGIL_BIND_IMMUT
                                    ),
                                    format!(
                                        "`{}` will change this value, so it must be mutable (`{}`)",
                                        call.name,
                                        Syntax::SIGIL_BIND_MUT
                                    ),
                                    format!("declare it with `{} {} ...`", name, Syntax::SIGIL_BIND_MUT),
                                    Some(*span),
                                ));
                            }
                        }
                    }
                }
                (AccessConvention::Read | AccessConvention::Write, AccessConvention::Move) => {
                    self.diags.push(Diagnostic::error(
                        "E0203",
                        format!(
                            "`{}` passed to a parameter that does not consume",
                            Syntax::KW_MOVE
                        ),
                        "only `take` parameters accept a moved value at the call site".to_string(),
                        format!(
                            "remove `{}` or change the parameter to `take`",
                            Syntax::KW_MOVE
                        ),
                        Some(arg.span),
                    ));
                }
                _ => {}
            }

            if arg.convention == AccessConvention::Write {
                if let Expr::Ident(name, _) = &arg.expr {
                    mut_borrowed.insert(name.clone());
                }
            }
            if let (Some((param_conv, param_ty)), Expr::Ident(name, _)) =
                (effective_params.get(i), &arg.expr)
            {
                if matches!(param_conv, AccessConvention::Read)
                    && arg.convention == AccessConvention::Read
                    && !param_ty.is_scalar()
                {
                    read_borrowed.insert(name.clone());
                }
            }

            if self.loop_depth > 0 {
                if let Expr::Ident(name, span) = &arg.expr {
                    if let Some(info) = self.lookup(name) {
                        if matches!(info.ty, Type::Shared(_)) {
                            arg.flags.shared_auto_clone = true;
                            self.diags.push(Diagnostic::lint(
                                "L0202",
                                format!(
                                    "auto-cloned `{}` inside a loop; consider hoisting or caching",
                                    name
                                ),
                                "shared handles are cloned when used across a loop boundary"
                                    .to_string(),
                                format!("hoist `{}` before the loop, or clone once outside", name),
                                Some(*span),
                            ));
                        }
                    }
                }
            }
        }

        Some(sig.return_type.as_ref().map(|t| {
            if generic_subst.is_empty() {
                t.clone()
            } else {
                self.trait_reg.instantiate_type(t, &generic_subst)
            }
        }))
    }
}

