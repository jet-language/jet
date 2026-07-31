//! Type inference: core dispatch + literals + index/slice/field access.
//!
//! Split out of the original `CheckerInfer.rs`; behavior unchanged.

use super::*;
use crate::Collections::is_map_key_type;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::Sema::Diagnostics::soft_public_use;
use crate::AST::{
    AccessConvention, Call, CallArg, CallArgFlags, EnumLitArg, Expr, IndexKind, StrPart, Type,
    TypedLitBody, UnOp,
};
use std::collections::HashSet;

fn field_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(base, field, _) => Some(format!("{}.{}", field_path(base)?, field)),
        _ => None,
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn infer_name_or(&mut self, e: &mut Expr, fallback: &str) -> String {
        self.infer(e)
            .map(|t| t.name())
            .unwrap_or_else(|| fallback.to_string())
    }

    pub(crate) fn rewrite_typed_text_literal(
        &mut self,
        e: &mut Expr,
        type_name: String,
        span: Span,
    ) -> Option<Type> {
        let old = std::mem::replace(e, Expr::Absent(span));
        let Expr::Str(parts, _) = old else {
            *e = old;
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("`{type_name}.{{ … }}` needs a string recipe body"),
                format!(
                    "checked `{type_name}` is built from a quoted template, not from another expression shape"
                ),
                format!("write `{type_name}.{{\"...\"}}` with `{{value}}` holes as needed"),
                Some(span),
            ));
            return None;
        };
        let mk_lit = |s: String, span: Span| CallArg {
            convention: AccessConvention::Read,
            expr: Expr::Str(vec![StrPart::Lit(s)], span),
            span,
            flags: crate::AST::CallArgFlags::default(),
            label: None,
            spread: false,
        };
        let mut args: Vec<CallArg> = Vec::new();
        let mut cur_lit = String::new();
        for p in parts {
            match p {
                StrPart::Lit(s) => cur_lit.push_str(&s),
                StrPart::Interp(mut inner, _fmt) => {
                    args.push(mk_lit(std::mem::take(&mut cur_lit), span));
                    self.borrow_ctx = true;
                    let was_view_read = self.allow_string_view_read;
                    self.allow_string_view_read = true;
                    self.infer(&mut inner);
                    self.allow_string_view_read = was_view_read;
                    args.push(CallArg {
                        convention: AccessConvention::Read,
                        span: inner.span(),
                        expr: *inner,
                        flags: crate::AST::CallArgFlags::default(),
                        label: None,
                        spread: false,
                    });
                }
            }
        }
        args.push(mk_lit(cur_lit, span));
        *e = Expr::Call(Call {
            name: type_name.clone(),
            name_span: span,
            args,
            range_checked: false,
        });
        Some(Type::Named(type_name))
    }

    /// D-REGEX-LIT1=D: validate `Regex.{"…"}` / inferred `.{"…"}` with the
    /// same grammar gate used by the generated linear runtime.
    pub(crate) fn rewrite_regex_literal(
        &mut self,
        e: &mut Expr,
        span: Span,
    ) -> Option<Type> {
        let old = std::mem::replace(e, Expr::Absent(span));
        let Expr::Str(parts, literal_span) = old else {
            *e = old;
            self.diags.push(Diagnostic::error(
                "E0152",
                "`Regex.{ … }` needs one quoted pattern".to_string(),
                "a regex typed literal contains pattern text, not another expression shape"
                    .to_string(),
                "write `Regex.{\"...\"}`".to_string(),
                Some(span),
            ));
            return None;
        };
        let [StrPart::Lit(pattern)] = parts.as_slice() else {
            *e = Expr::Str(parts, literal_span);
            self.diags.push(Diagnostic::error(
                "E0152",
                "a `Regex` literal cannot contain interpolation".to_string(),
                "the compiler must know the complete pattern before the program runs"
                    .to_string(),
                "use a fixed `Regex.{\"...\"}` pattern, or build text and call `re.compile(text)`"
                    .to_string(),
                Some(literal_span),
            ));
            return None;
        };
        if let Err(error) = jet_foundation::RegexSyntax::validate(pattern) {
            *e = Expr::Str(parts, literal_span);
            self.diags.push(Diagnostic::error(
                "E0152",
                format!(
                    "this regex pattern is invalid at position {}",
                    error.offset
                ),
                error.reason,
                "fix the pattern at the reported position".to_string(),
                Some(literal_span),
            ));
            return None;
        }
        *e = Expr::Call(Call {
            name: Syntax::TYPE_REGEX.to_string(),
            name_span: span,
            args: vec![CallArg {
                convention: AccessConvention::Read,
                expr: Expr::Str(parts, literal_span),
                span: literal_span,
                flags: CallArgFlags::default(),
                label: None,
                spread: false,
            }],
            range_checked: false,
        });
        Some(Type::Named(Syntax::TYPE_REGEX.to_string()))
    }

    /// Infer and check an expression. Returns None when a problem was
    /// already reported (avoids error cascades).
    ///
    /// This wrapper owns the rule that depends on *where* the expression
    /// appears (`borrow_ctx`): a struct-field read in owning position is
    /// rewritten to `.clone()` so the generated Rust never moves a field out
    /// of its struct.
    pub(crate) fn infer(&mut self, e: &mut Expr) -> Option<Type> {
        if !self.enter_source_nesting(e.span()) {
            return None;
        }
        self.check_scoped_loan_read(e);
        let result = self.infer_checked(e);
        self.leave_source_nesting();
        result
    }

    fn infer_checked(&mut self, e: &mut Expr) -> Option<Type> {
        // D-QUANTITY-CONVERT1=B: destination-owned exact conversion is
        // fallible at runtime; the explicit `_rounded` spelling carries the
        // ratified mode and destination decimal precision. Codegen emits these
        // real methods on the concrete destination type from the same UnitFact
        // coefficients.
        let conversion_span = e.span();
        let explicit_unit_conversion = if let Expr::MethodCall {
            receiver,
            method,
            args,
            resolved_ret,
            ..
        } = &mut *e
        {
            if matches!(args.len(), 1 | 3) {
                if let Expr::Ident(destination_name, _) = receiver.as_ref() {
                    if let Some((destination_name, destination_fact)) = self
                        .unit_fact_for_type(&Type::Named(destination_name.clone()))
                    {
                        let source_ty = self.infer(&mut args[0].expr);
                        if let Some((source_name, source_fact)) = source_ty
                            .as_ref()
                            .and_then(|ty| self.unit_fact_for_type(ty))
                        {
                            let source_leaf =
                                source_name.rsplit('.').next().unwrap_or(&source_name);
                            let expected = Syntax::conversion_method_for_source(source_leaf);
                            let exact = args.len() == 1 && method == &expected;
                            let rounded = args.len() == 3
                                && method == &format!("{expected}_rounded")
                                && matches!(
                                    &args[1].expr,
                                    Expr::EnumLit { type_name, variant, args, .. }
                                        if type_name.is_empty()
                                            && Syntax::unit_rounding_mode(variant).is_some()
                                            && args.is_empty()
                                )
                                && matches!(
                                    args[2].label.as_ref(),
                                    Some((label, _)) if label == "digits"
                                );
                            if (exact || rounded)
                                && source_fact.package == destination_fact.package
                                && source_fact.family == destination_fact.family
                                && source_fact.dimension == destination_fact.dimension
                                && source_fact.kind == destination_fact.kind
                            {
                                if !source_fact.conversion_is_finite_to(&destination_fact) {
                                    self.diags.push(
                                        self.unit_conversion_overflow_diagnostic(conversion_span),
                                    );
                                    return Some(Type::Named(destination_name));
                                }
                                if rounded {
                                    self.expect_core_arg(
                                        method,
                                        2,
                                        &Type::Int,
                                        &mut args[2],
                                    );
                                    if matches!(
                                        &args[2].expr,
                                        Expr::Unary(
                                            crate::AST::UnOp::Neg,
                                            inner,
                                            _
                                        ) if matches!(inner.as_ref(), Expr::Int(..))
                                    ) {
                                        self.diags.push(Diagnostic::error(
                                            "E0127",
                                            "rounded unit conversion needs nonnegative digits"
                                                .to_string(),
                                            "digits counts destination decimal places"
                                                .to_string(),
                                            "write `digits: 0` or another nonnegative Int"
                                                .to_string(),
                                            Some(args[2].expr.span()),
                                        ));
                                    }
                                }
                                let ty = Type::Result {
                                    ok: Box::new(Type::Named(destination_name)),
                                    err: Box::new(Type::String),
                                };
                                *resolved_ret = Some(ty.clone());
                                Some(ty)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        if let Some(ty) = explicit_unit_conversion {
            return Some(ty);
        }

        let borrowed = std::mem::take(&mut self.borrow_ctx);
        let ty = self.infer_inner(e);
        if let Some(aggregate_ty) = ty.as_ref() {
            let constructs_value = matches!(
                e,
                Expr::ListLit(..)
                    | Expr::MapLit(..)
                    | Expr::TupleLit(..)
                    | Expr::Call(..)
                    | Expr::MethodCall { .. }
                    | Expr::CallValue { .. }
                    | Expr::StructLit { .. }
                    | Expr::EnumLit { .. }
                    | Expr::Present(..)
                    | Expr::Ok(..)
                    | Expr::Err(..)
            );
            if constructs_value && self.cell_guard_storage_is_unsupported(aggregate_ty) {
                self.report_cell_guard_storage(
                    format!(
                        "a Cell guard cannot be stored inside `{}`",
                        aggregate_ty.show()
                    ),
                    e.span(),
                );
            }
        }
        if !borrowed {
            if let Some(t) = &ty {
                let borrowed_param_place = !type_is_copy(t)
                    && matches!(e, Expr::Field(..) | Expr::Index { .. })
                    && expr_root_ident(e).is_some_and(|root| {
                        self.lookup(root).is_some_and(|info| {
                            matches!(
                                info.param_conv,
                                Some(AccessConvention::Read) | Some(AccessConvention::Write)
                            )
                        })
                    });
                if !borrowed_param_place
                    && !type_is_copy(t)
                    && field_read_to_clone(e, self.registry, self.imports)
                {
                    if let Some(root) = crate::Sema::Diagnostics::expr_root_ident(e) {
                        let borrowed_param = self.lookup(root).is_some_and(|info| {
                            matches!(
                                info.param_conv,
                                Some(AccessConvention::Read) | Some(AccessConvention::Write)
                            )
                        });
                        if borrowed_param {
                            let path = field_path(e).unwrap_or_else(|| root.to_string());
                            self.diags.push(Diagnostic::error(
                                "E0120",
                                format!("`{path}` is borrowed, so it cannot escape as an owned value"),
                                format!("`{root}` is a parameter with read access; its fields are borrowed with it"),
                                format!("copy it explicitly with `{}{path}`, or take `{root}` with `^`", Syntax::SIGIL_COPY),
                                Some(e.span()),
                            ));
                            return ty;
                        }
                    }
                    // Only auto-copy when the field type is actually cloneable.
                    // A task-handle list (and other non-cloneable fields) must
                    // partial-move; wrapping them in `Copy` skips the E0211
                    // check and becomes a rustc rejection the user sees as an
                    // ICE (I2).
                    if is_cloneable(t, self.registry) {
                        // D-CAP2 (D-MEM1/S4): the same `copy` node the user can write
                        // explicitly — one mechanism for "duplicate this value",
                        // whether the compiler inserts it or the user spells it.
                        let span = e.span();
                        let old = std::mem::replace(e, Expr::Absent(span));
                        *e = Expr::Copy(Box::new(old), span);
                    }
                }
            }
        }
        ty
    }

    pub(crate) fn infer_inner(&mut self, e: &mut Expr) -> Option<Type> {
        // D-SHAPE3b: `Ok`/`Err` are contextual identifiers, not reserved words.
        // A user function wins; otherwise the canonical one-argument spelling
        // reuses the existing Result AST nodes and diagnostics.
        let contextual_result = match &mut *e {
            Expr::Call(call)
                if !self.funcs.contains_key(&call.name)
                    && matches!(call.name.as_str(), name if name == Syntax::LIT_OK || name == Syntax::LIT_ERR)
                    && call.args.len() == 1
                    && call.args[0].label.is_none() =>
            {
                let is_ok = call.name == Syntax::LIT_OK;
                let arg = call.args.pop().unwrap().expr;
                let span = Span::new(call.name_span.start, arg.span().end);
                Some(if is_ok {
                    Expr::Ok(Box::new(arg), span)
                } else {
                    Expr::Err(Box::new(arg), span)
                })
            }
            _ => None,
        };
        if let Some(replacement) = contextual_result {
            *e = replacement;
        }

        // D-SHAPE3b: leading-dot Optional/Result variants are contextual forms.
        // Rewrite them to the existing canonical AST nodes only when the expected
        // wrapper type is known; every downstream pass then reuses one mechanism.
        let contextual_variant = match (&mut *e, self.expected_type.as_ref()) {
            (
                Expr::EnumLit { type_name, variant, args, span },
                Some(Type::Option(_)),
            ) if type_name.is_empty() && variant == Syntax::LIT_NULL && args.is_empty() => {
                Some(Expr::Absent(*span))
            }
            (
                Expr::EnumLit { type_name, variant, args, span },
                Some(Type::Option(_)),
            ) if type_name.is_empty() && variant == Syntax::LIT_VALUE && args.len() == 1 => {
                match args.pop().unwrap() {
                    EnumLitArg::Positional(inner) => Some(Expr::Present(Box::new(inner), *span)),
                    named @ EnumLitArg::Named { .. } => {
                        args.push(named);
                        None
                    }
                }
            }
            (
                Expr::EnumLit { type_name, variant, args, span },
                Some(Type::Result { .. }),
            ) if type_name.is_empty()
                && matches!(variant.as_str(), name if name == Syntax::LIT_OK || name == Syntax::LIT_ERR)
                && args.len() == 1 =>
            {
                let is_ok = variant == Syntax::LIT_OK;
                match args.pop().unwrap() {
                    EnumLitArg::Positional(inner) if is_ok => Some(Expr::Ok(Box::new(inner), *span)),
                    EnumLitArg::Positional(inner) => Some(Expr::Err(Box::new(inner), *span)),
                    named @ EnumLitArg::Named { .. } => {
                        args.push(named);
                        None
                    }
                }
            }
            _ => None,
        };
        if let Some(replacement) = contextual_variant {
            *e = replacement;
        }

        // D-EMPTYLIT1: an empty `[]` always parses as `Expr::ListLit`. When the
        // expected-type context says Map, rewrite the node to an empty
        // `Expr::MapLit` here so every downstream pass (codegen, comptime,
        // formatter-after-sema) sees the AST kind it already knows how to
        // lower — the exact same shape the retired `[:]` literal produced.
        if let Expr::ListLit(elems, span) = e {
            if elems.is_empty() {
                if let Some(Type::Map {
                    key,
                    key_span,
                    value,
                }) = self.expected_type.clone()
                {
                    let span = *span;
                    *e = Expr::MapLit(Vec::new(), span);
                    return Some(Type::Map {
                        key,
                        key_span,
                        value,
                    });
                }
            }
        }

        // D-DOTCTOR3=A: `Type.{ body }` elaborates against the head like an
        // expected-type position, then rewrites to the ordinary literal shape.
        if matches!(e, Expr::TypedLit { .. }) {
            return self.elaborate_typed_lit(e);
        }
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
                // D-FLOWTYPE1=A: same Optional presence desugar as statement `if`.
                self.rewrite_optional_flow_ne_none(cond);
                // Value-if always has an else arm; invert atomic `== None` so the
                // false branch (presence) becomes an S31 Present then-arm.
                if let Some((name, name_span, cond_span)) =
                    crate::Sema::CheckerCore::atomic_absent_optional_subject(cond)
                {
                    if self.flow_narrowable_optional_inner(&name).is_some() {
                        std::mem::swap(then_body, else_body);
                        std::mem::swap(then_value, else_value);
                        **cond = Expr::PatternTest {
                            subject: Box::new(Expr::Ident(name.clone(), name_span)),
                            pattern: crate::AST::Pattern::Present {
                                binding: name,
                                binding_span: name_span,
                                span: cond_span,
                            },
                            span: cond_span,
                        };
                    }
                }
                let bindings = self.check_condition_with_bindings(cond);
                self.push_scope();
                let mut restore_moved = Vec::new();
                for (name, ty) in bindings {
                    if let Some(restored) =
                        self.declare_condition_binding(&name, cond.span(), ty)
                    {
                        restore_moved.push(restored);
                    }
                }
                self.record_condition_view_bindings(cond);
                self.check_block(then_body, false);
                let then_ty = self.infer(then_value);
                self.pop_scope();
                for (name, at) in restore_moved {
                    self.moved.insert(name, at);
                }
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
                        } else if let Some(joined) = a.numeric_join(&b) {
                            // D-NUMJOIN1=A: two numeric producers follow the one
                            // widening law, exactly as an operator's operands do.
                            self.widen_numeric_expr(then_value, &a, &joined);
                            self.widen_numeric_expr(else_value, &b, &joined);
                            Some(joined)
                        } else {
                            if self.collect_item_types.is_empty() {
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
                            } else {
                                self.diags.push(Diagnostic::error(
                                    "E0074",
                                    "this yielding loop produces incompatible item types"
                                        .to_string(),
                                    format!(
                                        "one yielding loop builds one List, but these paths produce {} and {}",
                                        a.show(),
                                        b.show()
                                    ),
                                    "convert every item to one type, or split the loops"
                                        .to_string(),
                                    Some(span),
                                ));
                            }
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
            Expr::Int(n, span, width, _) => {
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
            // D-UNITLIT1: `500ms` — resolve the suffix against an in-scope
            // `#UnitFamily` member (PascalCased to its minted `#Numeric
            // distinct Float` type name) and rewrite this node in place to an
            // ordinary destination-owned conversion — sugar over the existing
            // distinct-type path, so every downstream check (cross-unit
            // E0127, arithmetic, `.raw()`) is the SAME machinery a
            // hand-written `Ms(500.0)` already goes through.
            Expr::UnitLit {
                int,
                float,
                suffix,
                suffix_span,
                span,
                ..
            } => {
                let type_name = crate::AST::UnitFamilyDef::type_name(suffix);
                let is_unit_member = self.registry.is_distinct(&type_name)
                    && self.registry.distinct_is_numeric(&type_name)
                    && self.registry.distinct_base(&type_name) == Some(&Type::Float);
                if !is_unit_member {
                    self.diags.push(Diagnostic::error(
                        "E0134",
                        format!("`{}` isn't a unit in scope", suffix),
                        "a unit suffix names a member of a `#UnitFamily` you've imported; this suffix isn't one here".to_string(),
                        format!("import the family that defines `{}`, or write the number without a suffix", suffix),
                        Some(*suffix_span),
                    ));
                    return None;
                }
                let value = float.unwrap_or_else(|| int.unwrap_or(0) as f64);
                let call_span = *span;
                *e = Expr::MethodCall {
                    receiver: Box::new(Expr::Ident(type_name, *suffix_span)),
                    method: Syntax::numeric_conversion_method("Float")
                        .expect("Float has a canonical conversion method")
                        .to_string(),
                    method_span: *suffix_span,
                    type_args: Vec::new(),
                    args: vec![CallArg {
                        convention: AccessConvention::Read,
                        expr: Expr::Float(value, call_span, false),
                        span: call_span,
                        flags: crate::AST::CallArgFlags::default(),
                        label: None,
                        spread: false,
                    }],
                    recv_type: None,
                    resolved_ret: None,
                };
                self.infer(e)
            }
            Expr::Bool(_, _) => Some(Type::Bool),
            // D-UNIFYLIT1=A: bare `"…"` is always `String`. Domain text elaborates
            // only from `SQL.{"…"}` / `HTML.{"…"}` / `Sh.{"…"}` (typed-literal
            // heads) via `elaborate_typed_lit` → `rewrite_typed_text_literal`.
            Expr::Str(parts, str_span) => {
                // D-MEM1/S7 (D-NOALLOC-SEM1=A): interpolation with at least one
                // `{…}` hole builds a fresh `String` (unlike a plain literal
                // with no holes, which is one constant piece of text — not
                // "concatenation/interpolation that produces a new String").
                if parts.iter().any(|p| matches!(p, StrPart::Interp(..))) {
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::Allocation,
                        *str_span,
                        "string interpolation allocates a new `String`",
                    ));
                }
                for p in parts.iter_mut() {
                    if let StrPart::Interp(inner, fmt) = p {
                        // Interpolation borrows; never moves.
                        self.borrow_ctx = true;
                        // D-MEM1 stage S5: `"{d}"` reads `d` via Display/Debug
                        // (`str` has both impls too, see `Prelude/Core.rs`) —
                        // safe for a string view, unlike most other reads.
                        let was_view_read = self.allow_string_view_read;
                        self.allow_string_view_read = true;
                        let t = self.infer(inner);
                        self.allow_string_view_read = was_view_read;
                        if let Some(t) = t {
                            if self.type_contains_observable_clock(&t) {
                                self.record_effect(
                                    crate::Sema::Effects::Effect::Time.name(),
                                    inner.span(),
                                );
                                if self.in_pure && self.det_suppress == 0 {
                                    self.diags.push(crate::Sema::e3403(
                                        "Clock formatting",
                                        Some(inner.span()),
                                    ));
                                }
                            }
                            match fmt {
                                crate::AST::StrFormat::Display => {
                                    if !is_displayable(&t, self.registry, self.trait_reg)
                                        && !self.is_unit_type(&t)
                                    {
                                        if crate::Sema::Diagnostics::is_secret_bearing_crypto_type(&t) {
                                            self.diags.push(Diagnostic::error(
                                                "E0915",
                                                format!("secret-bearing `{}` has no `Display` implementation", t.name()),
                                                "cryptographic secrets cannot enter logs or strings through interpolation".to_string(),
                                                "remove the interpolation; log a public operation label or key identifier instead".to_string(),
                                                Some(inner.span()),
                                            ));
                                            continue;
                                        }
                                        if crate::Sema::Diagnostics::is_one_pass_source(&t) {
                                            let fix =
                                                crate::Sema::Diagnostics::one_pass_materializer(&t)
                                                    .map_or_else(
                                                        || {
                                                            "consume it with a `loop` and show each item instead"
                                                                .to_string()
                                                        },
                                                        |call| {
                                                            format!(
                                                                "materialize it first: add `{call}` before the interpolation"
                                                            )
                                                        },
                                                    );
                                            self.diags.push(Diagnostic::error(
                                                "E0915",
                                                format!("the one-pass source {} has no `Display` implementation", t.show()),
                                                "reading this value consumes it, so showing it would spend the only pass".to_string(),
                                                fix,
                                                Some(inner.span()),
                                            ));
                                            continue;
                                        }
                                        // Migration: auto-printable structs without Display get a lint
                                        // and still compile via jet_show fallback in codegen.
                                        if let Type::Named(n) = &t {
                                            // D-CAPBUNDLE1: a nominal `distinct` type
                                            // starts inert — interpolating one without
                                            // `#Printable` names the granted bundles
                                            // instead of the generic E0915 wording.
                                            if self.registry.is_distinct(n) {
                                                self.diags.push(e0138(
                                                    n,
                                                    "string interpolation",
                                                    "#Printable",
                                                    self.registry.distinct_granted_bundles(n),
                                                    inner.span(),
                                                ));
                                            } else if self.trait_reg.auto_printable.contains(n)
                                                && !self
                                                    .trait_reg
                                                    .implements_trait(n, crate::Generics::DISPLAY)
                                            {
                                                self.diags.push(Diagnostic::lint(
                                                    "L0520",
                                                    format!(
                                                        "`{n}` has no `Display` impl — bare `{{}}` will require one soon"
                                                    ),
                                                    "Display is the user-facing interpolation hook; Debug is for `{value#Debug}`"
                                                        .to_string(),
                                                    format!(
                                                        "add `impl {n}.Display {{ fn display(self) => String {{ … }} }}`"
                                                    ),
                                                    Some(inner.span()),
                                                ));
                                            } else {
                                                self.diags.push(crate::Generics::e0915(
                                                    &t.show(),
                                                    inner.span(),
                                                ));
                                            }
                                        } else {
                                            self.diags.push(crate::Generics::e0915(
                                                &t.show(),
                                                inner.span(),
                                            ));
                                        }
                                    }
                                }
                                crate::AST::StrFormat::Debug => {
                                    if !is_debuggable(&t, self.registry, self.trait_reg) {
                                        if crate::Sema::Diagnostics::is_secret_bearing_crypto_type(&t) {
                                            self.diags.push(Diagnostic::error(
                                                "E0112",
                                                format!("secret-bearing `{}` cannot use `#Debug`", t.name()),
                                                "Debug output could copy cryptographic secret material into logs or diagnostics".to_string(),
                                                "remove the interpolation; log a public operation label or key identifier instead".to_string(),
                                                Some(inner.span()),
                                            ));
                                            continue;
                                        }
                                        self.diags.push(Diagnostic::error(
                                            "E0112",
                                            format!("{} can't be shown with #Debug yet", t.show()),
                                            "debug interpolation needs a debuggable value"
                                                .to_string(),
                                            "implement `Debug` or use a debuggable part"
                                                .to_string(),
                                            Some(inner.span()),
                                        ));
                                    }
                                }
                                crate::AST::StrFormat::Fixed(_) => {
                                    if t != Type::Float {
                                        self.diags.push(Diagnostic::error(
                                            "E0112",
                                            format!(
                                                "{} can't use `#Fixed(n)`",
                                                t.show()
                                            ),
                                            "fixed interpolation uses `core.fmt.decimal`, which formats `Float` values"
                                                .to_string(),
                                            "pass a `Float`, or use bare interpolation for this value"
                                                .to_string(),
                                            Some(inner.span()),
                                        ));
                                    }
                                }
                                crate::AST::StrFormat::Unit(_) => {
                                    if !self.is_unit_type(&t) {
                                        self.diags.push(Diagnostic::error(
                                            "E0112",
                                            format!(
                                                "{} can't use `#Unit(…)`",
                                                t.show()
                                            ),
                                            "unit formatting needs a quantity or a `#UnitFamily` value"
                                                .to_string(),
                                            "use bare interpolation, or pass a value that has a unit"
                                                .to_string(),
                                            Some(inner.span()),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                Some(Type::String)
            }
            // D-SHIFT1 (c7shift): a pattern-literal is legal ONLY as
            // `cursor.take_pattern("…")`'s argument — that call site
            // intercepts and consumes it directly (`CheckerInfer/calls.rs`)
            // before generic argument inference ever reaches this arm. If
            // it's reached anyway (a non-`Cursor` receiver's `take_pattern`
            // falls through to the generic "unknown method" recovery path,
            // which infers every arg defensively), report it plainly rather
            // than let an unhandled AST shape reach codegen (I2).
            Expr::StrMatchLit(_, span) => {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    "a pattern literal is only valid as a `take_pattern` argument".to_string(),
                    "this string has typed holes (`{name:Type}`), which only `take_pattern` understands"
                        .to_string(),
                    "call it as `cursor.take_pattern(\"…\")`, or drop the `:Type` for an ordinary interpolated string"
                        .to_string(),
                    Some(*span),
                ));
                None
            }
            // D-BINPAT1 / D-UNIFYLIT1=A: `[U8].{"…"}` is legal ONLY as a
            // `reader.take_pattern([U8].{"…"})` argument (or pattern arm),
            // intercepted in method_calls / pattern validate — not as a value.
            Expr::BinMatchLit(_, span) => {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    "a binary pattern literal is only valid as a `take_pattern` argument".to_string(),
                    "this `[U8].{\"…\"}` literal has typed holes (`{name:U<width>}`), which only `take_pattern` understands"
                        .to_string(),
                    "call it as `reader.take_pattern([U8].{\"…\"})`".to_string(),
                    Some(*span),
                ));
                None
            }
            Expr::Ident(name, span) => {
                // D-LOOPLABEL3=A: loop labels share the ordinary namespace but
                // are control names, not runtime values.
                if self.loop_labels.iter().any(|label| label == name) {
                    self.diags.push(Diagnostic::error(
                        "E0988",
                        format!("loop label `{name}` is not a runtime value"),
                        "a loop label names a control-flow destination, so it cannot be read, passed, or stored"
                            .to_string(),
                        format!("use `break({name})` or `next({name})` to control the loop"),
                        Some(*span),
                    ));
                    return None;
                }
                // D-PREPOST1 (E0144): `result` names the return value inside a
                // `#Post` condition; at function entry (a `#Pre` condition)
                // there is no return value yet.
                if self.in_pre_clause && name == "result" {
                    self.diags.push(Diagnostic::error(
                        "E0144",
                        "`result` isn't available in a `#Pre` condition".to_string(),
                        "`result` names the return value, which only exists once the function has returned — a `#Pre` condition runs at entry, before there is one".to_string(),
                        "use `result` only in a `#Post` condition".to_string(),
                        Some(*span),
                    ));
                    return None;
                }
                let moved_expr = Expr::Ident(name.clone(), *span);
                if self.reject_moved_expr_use(&moved_expr, *span) {
                    return None;
                }
                // D-UNINIT-SENTINEL2: reading a `Type.{ uninit }` binding before it is written.
                if self.uninit.contains_key(name) {
                    self.diags.push(Diagnostic::error(
                        "E0420",
                        format!("`{}` may be read before it is given a value", name),
                        format!(
                            "`{}` was declared with `Type.{{ uninit }}`, so it holds no value until you write to it — this read could see garbage",
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
                // was already reset. (`alloc` and `reset` go
                // through the method-call path below, so reaching here means a
                // plain read of the view's value.)
                self.check_view_use(name, *span);
                // D-MEM1 stage S5: E2307 — reading a string-view name anywhere
                // other than the two positions its bare `&str` Rust place
                // supports (`allow_string_view_read`, set only around chaining
                // `.trim()`/`.after()`/`.before()` and `copy`'s operand). This
                // is the ONE general choke point for the whole class of uses
                // that would otherwise mismatch a callee's `&String`/`String`
                // signature at the Rust level (list/tuple literal elements,
                // call arguments, plain assignment, struct fields not already
                // caught earlier, …) — every one of them reads this same
                // `Expr::Ident` node to get the name's value.
                if self.is_string_view(name)
                    && !self.allow_string_view_read
                    && !(self.in_lambda_body && self.lambda_escapes)
                {
                    self.report_string_view_unsupported_use(name, "be used directly here", *span);
                }
                if let Some(info) = self.lookup(name).cloned() {
                    self.record_local_reference(*span, &info);
                    return Some(info.ty.clone());
                }
                if let Some(t) = self.consts.get(name).cloned() {
                    self.record_const_reference(name, *span);
                    return Some(t);
                }
                if let Some(sig) = self.funcs.get(name).cloned() {
                    // D-METHODMACRO1=A: a bare top-level function name resolved here
                    // is read as a VALUE, not called (a direct call never reaches this
                    // arm — `check_call` short-circuits on a known global function
                    // name before inferring its callee as an expression). This is
                    // exactly "this function's address was taken" for E0918.
                    self.record_current_function_reference(name, *span);
                    self.inline_addr_taken.insert(name.clone());
                    if let Some((idx, (convention, _))) = sig
                        .params
                        .iter()
                        .enumerate()
                        .find(|(_, (convention, _))| *convention != AccessConvention::Read)
                    {
                        let capability = match convention {
                            AccessConvention::Write => "write access (`&`)",
                            AccessConvention::Move => "ownership (`^`)",
                            AccessConvention::Read => unreachable!(),
                        };
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{name}` cannot be used as a function value"),
                            format!(
                                "parameter {} requires {capability}, but `fn(T)` function values take every parameter with plain read access",
                                idx + 1
                            ),
                            "call this function directly; function values cannot erase write or ownership requirements"
                                .to_string(),
                            Some(*span),
                        ));
                    }
                    return Some(func_sig_to_fn_type(&sig));
                }
                self.unknown_name(name, *span);
                None
            }
            Expr::Char(_, _) => Some(Type::Char),
            Expr::Spread(_, span) => {
                self.diags.push(Diagnostic::error(
                    "E1311",
                    "spread only works inside a list literal or a variadic call".to_string(),
                    "write `[...xs, y]` to spread into a list, or `f(...xs)` when the callee accepts a rest parameter".to_string(),
                    "move the spread into `[ … ]` or a variadic call".to_string(),
                    Some(*span),
                ));
                None
            }
            Expr::ListLit(elems, span) => {
                // Rewrite before typing so the list sees call results, not nested `[T#N]`.
                // #779 demo: surface-only change; engines never learn a new construct.
                if !elems.is_empty()
                    && !matches!(self.expected_type.as_ref(), Some(Type::FixedList { .. }))
                {
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::Allocation,
                        *span,
                        "list construction allocates backing storage",
                    ));
                }
                self.infer_list_lit(elems, *span)
            }
            Expr::TupleLit(fields, span, ty_slot) => {
                let t = self.infer_tuple_lit(fields, *span);
                *ty_slot = t.clone();
                t
            }
            Expr::MapLit(entries, span) => {
                if !entries.is_empty() {
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::Allocation,
                        *span,
                        "map construction allocates backing storage",
                    ));
                }
                self.infer_map_lit(entries, *span)
            }
            Expr::Index {
                base,
                index,
                span,
                kind,
            } => {
                let result = self.infer_index(base, index, span, kind);
                if matches!(kind, IndexKind::Range) && self.loop_depth > 0 {
                    self.diags.push(Diagnostic::lint(
                        "L0501",
                        "slicing inside a loop copies every time".to_string(),
                        "each slice makes a fresh copy of the range — that adds up in a loop"
                            .to_string(),
                        "build indices outside the loop, or collect into one list".to_string(),
                        Some(*span),
                    ));
                }
                if matches!(kind, IndexKind::Range) {
                    let span = *span;
                    let base = std::mem::replace(base, Box::new(Expr::Absent(span)));
                    let range = std::mem::replace(index, Box::new(Expr::Absent(span)));
                    let zero = || Box::new(Expr::Int(0, span, None, None));
                    *e = Expr::Slice {
                        base,
                        start: zero(),
                        end: zero(),
                        range: Some(range),
                        span,
                    };
                }
                result
            }
            Expr::Slice {
                base,
                start,
                end,
                range,
                span,
            } => {
                if let Some(range) = range {
                    let base_ty = {
                        self.borrow_ctx = true;
                        self.infer(base)?
                    };
                    let range_ty = self.infer(range)?;
                    if range_ty != Type::Named(crate::Syntax::TYPE_RANGE.to_string()) {
                        self.diags.push(Diagnostic::error(
                            "E0505",
                            format!("slice bound must be Range, not {}", range_ty.show()),
                            "a stored slice bound carries its start, end, and end behavior in one Range value".to_string(),
                            "use `a..b`, `a..<b`, or a Range value".to_string(),
                            Some(range.span()),
                        ));
                        return None;
                    }
                    match base_ty {
                        Type::List(inner) => Some(Type::List(inner)),
                        Type::String => Some(Type::String),
                        other => {
                            self.diags.push(Diagnostic::error(
                                "E0505",
                                format!("only lists and strings can be sliced, not {}", other.show()),
                                "a Range projects a window from indexed storage".to_string(),
                                "use `xs[range]` on a list or `s.slice(range)` on text".to_string(),
                                Some(*span),
                            ));
                            None
                        }
                    }
                } else {
                    self.infer_slice(base, start, end, *span)
                }
            }
            Expr::Range {
                start,
                end,
                span,
                ..
            } => self.infer_range(start, end, *span),
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
                            "only calls that declare `=> Type` can be used as a value".to_string(),
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
                    if let (Expr::Int(n, ispan, width, _), Some(Type::IntN { signed: true, bits })) =
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
                        if t.is_float() || matches!(t, Type::Int | Type::IntN { signed: true, .. })
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
                let mut replacement = None;
                let ty = self.infer_binary(op, lhs, rhs, span, &mut replacement);
                if let Some(replacement) = replacement { *e = replacement; }
                ty
            }
            Expr::CompareChain {
                operands,
                ops,
                hooks,
                span,
            } => {
                let span = *span;
                let ops = ops.clone();
                self.infer_compare_chain(operands, &ops, hooks, span)
            }
            Expr::Deref(inner, span) => {
                // D-CAP9: postfix `p.*` dereferences a raw pointer — a raw
                // memory access, gated to `#Unsafe`. The result type is the
                // pointer's element type.
                let forbidden = !self.in_unsafe;
                if forbidden {
                    self.diags.push(Diagnostic::error(
                        "E0208",
                        "reading through a raw pointer requires `#Unsafe`".to_string(),
                        "`p.*` dereferences a raw pointer; that is a raw memory access, only valid inside a `#Unsafe { … }` region"
                            .to_string(),
                        "wrap this in `#Unsafe(\"why this is safe\") { … }`".to_string(),
                        Some(*span),
                    ));
                }
                let inner_t = self.infer(inner)?;
                if forbidden {
                    return None;
                }
                match crate::Sema::ptr_elem(&inner_t) {
                    Some(elem) => Some(elem),
                    None => Some(inner_t),
                }
            }
            Expr::RawOf(inner, span) => {
                // D-CAP9: prefix `*x` takes a raw pointer to `x` (raw-pointer-of),
                // legal only inside `#Unsafe`. Result type is `*T` (`Ptr<T>`).
                let forbidden = !self.in_unsafe;
                if forbidden {
                    self.diags.push(Diagnostic::error(
                        "E0208",
                        "taking a raw pointer requires `#Unsafe`".to_string(),
                        "`*x` takes a raw pointer to `x`; that is a raw memory operation, only valid inside a `#Unsafe { … }` region"
                            .to_string(),
                        "wrap this in `#Unsafe(\"why this is safe\") { … }` — to dereference a pointer use postfix `p.*`"
                            .to_string(),
                        Some(*span),
                    ));
                }
                let inner_t = self.infer(inner)?;
                if forbidden {
                    return None;
                }
                Some(crate::Sema::ptr_type(inner_t))
            }
            Expr::Copy(inner, span) => {
                // D-CAP2 (D-MEM1/S4): `copy x` explicitly duplicates `x` into a
                // fresh, independent value — a temporary, so it never needs `^`
                // and never trips E0209 regardless of what `inner` is. Suppress
                // `infer`'s automatic owning-field-read `.clone()` wrap: this
                // expression already IS that clone, and wrapping again would
                // double-clone in the generated Rust.
                self.borrow_ctx = true;
                // D-MEM1 stage S5: `copy d` on a string-view name is the one
                // legal way to materialize it into an owned `String` — the
                // general E2307 check on a bare `Expr::Ident` read must not
                // fire for `copy`'s own operand.
                let was_view_read = self.allow_string_view_read;
                self.allow_string_view_read = true;
                let inner_t = self.infer(inner);
                self.allow_string_view_read = was_view_read;
                let inner_t = inner_t?;
                if self.type_contains_observable_clock(&inner_t) {
                    self.record_effect(
                        crate::Sema::Effects::Effect::Time.name(),
                        *span,
                    );
                    if self.in_pure && self.det_suppress == 0 {
                        self.diags
                            .push(crate::Sema::e3403("Clock copy", Some(*span)));
                    }
                }
                let resource = self.is_resource_type(&inner_t);
                if resource
                    || (!type_is_copy(&inner_t) && !is_cloneable(&inner_t, self.registry))
                {
                    let cell_guard = matches!(
                        &inner_t,
                        Type::Apply { name, .. }
                            if matches!(name.as_str(), "CellReadGuard" | "CellEditGuard")
                    );
                    let shown = match &inner_t {
                        Type::Apply { name, args }
                            if cell_guard && args.len() == 1 =>
                        {
                            format!("{name}<{}>", args[0].name())
                        }
                        _ => inner_t.show(),
                    };
                    let (why, fix) = if cell_guard {
                        (
                            "a Cell guard owns one live dynamic loan; copying it would create two handles for the same loan".to_string(),
                            format!(
                                "move it instead with `{}guard`, or create a new guard after this one is dropped",
                                Syntax::SIGIL_MOVE
                            ),
                        )
                    } else if resource {
                        (
                            "a resource owns one cleanup duty; copying it would create two owners that could close the same handle".to_string(),
                            format!("move it instead with `{}name`, or acquire a second resource", Syntax::SIGIL_MOVE),
                        )
                    } else {
                        (
                            "copy needs a value made only of duplicable parts; this type holds something Jet can't duplicate — a function value, a trait value, or a type from outside Jet".to_string(),
                            format!(
                                "move it instead (`{}name` if this is its last use), or change the type so every part can be copied",
                                Syntax::SIGIL_MOVE
                            ),
                        )
                    };
                    self.diags.push(Diagnostic::error(
                        "E0211",
                        format!("`{shown}` can't be copied"),
                        why,
                        fix,
                        Some(*span),
                    ));
                    return None;
                }
                // D-MEM1/S7 (D-NOALLOC-SEM1=A): `copy` of a heap-owning type
                // is itself an allocation (the whole point of `.clone()`-style
                // duplication) — flagged regardless of whether it's cloneable.
                if matches!(inner_t, Type::Shared(_)) {
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::RetainRelease,
                        *span,
                        format!("`copy` of `{}` retains a shared reference", inner_t.show()),
                    ));
                } else if type_owns_heap(&inner_t, self.registry) {
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::Allocation,
                        *span,
                        format!("`copy` of `{}` allocates owned heap data", inner_t.show()),
                    ));
                }
                Some(inner_t)
            }
            Expr::Place(inner, access, span) => {
                if self.place_from_expr(inner).is_none() {
                    self.diags.push(Diagnostic::error(
                        "E0213",
                        "a window must start from a place".to_string(),
                        "only a name followed by fields, indexes, or one range has stable storage to read or edit without copying".to_string(),
                        "bind the call or temporary to a name first, then take a window into that name".to_string(),
                        Some(*span),
                    ));
                    return None;
                }
                if *access == crate::AST::PlaceAccess::Write {
                    self.validate_write_place(inner, *span);
                }
                self.borrow_ctx = true;
                let ty = self.infer(inner)?;
                if matches!(inner.as_ref(), Expr::Slice { .. }) {
                    let elem = match ty {
                        Type::List(elem) => *elem,
                        Type::FixedList { elem, .. } => *elem,
                        other => return Some(other),
                    };
                    Some(Type::Apply {
                        name: match access {
                            crate::AST::PlaceAccess::Read => "View",
                            crate::AST::PlaceAccess::Write => "ViewMut",
                        }
                        .to_string(),
                        args: vec![elem],
                    })
                } else {
                    Some(ty)
                }
            }
            Expr::PtrFromAddr {
                alias,
                alias_span,
                elem,
                addr,
                span,
            } => self.infer_ptr_from_addr(alias, *alias_span, elem, addr, *span),
            Expr::Field(_, _, span) => {
                let span = *span;
                // D-PROCESS-SESSION2=D: known terminal facts are checked
                // namespace members but lower to ordinary String keys. This
                // keeps `Set<String>` open to preview keys without an extra
                // public report type.
                if let Expr::Field(inner, member, _) = e {
                    if matches!(&**inner, Expr::Ident(name, _) if name == Syntax::TERMINAL_FACT_NAMESPACE)
                    {
                        if let Some(fact) = Syntax::terminal_fact(member) {
                            *e = Expr::Str(vec![StrPart::Lit(fact.to_string())], span);
                            return Some(Type::String);
                        }
                        let candidates = Syntax::TERMINAL_FACTS
                            .iter()
                            .map(|fact| (*fact).to_string())
                            .collect::<Vec<_>>();
                        let fix = suggest_field(member, &candidates)
                            .map(|fact| format!("did you mean `TerminalFact.{fact}`?"))
                            .unwrap_or_else(|| {
                                "use a documented TerminalFact key or a preview string".to_string()
                            });
                        self.diags.push(Diagnostic::error(
                            "E0302",
                            format!("`TerminalFact` has no key `{member}`"),
                            "stable terminal capability keys use the checked TerminalFact namespace"
                                .to_string(),
                            fix,
                            Some(span),
                        ));
                        return None;
                    }
                }
                // D-TAG1: fold a dotted variant path (`Damage.Fire.Burn`) into an
                // enum literal so codegen sees one EnumLit node. Single-segment
                // `Enum.Variant` keeps its existing Field route (unchanged Rust).
                if let Some((type_name, variant)) = self.fold_enum_variant_path(e) {
                    if variant.contains('.') {
                        let ty = self.check_enum_lit(&type_name, &variant, &mut [], span);
                        *e = Expr::EnumLit {
                            type_name,
                            variant,
                            args: Vec::new(),
                            span,
                        };
                        return Some(ty);
                    }
                }
                let Expr::Field(inner, member, _) = e else {
                    unreachable!("matched Field above")
                };
                let member = member.clone();
                // D-RANGE-EXCL1=C: bare `xs.indexes` is the ratified noun form
                // (Swift `.indices` style). Rewrite to the zero-arg method so
                // TIR/codegen reuse the existing member path; `indexes()` also
                // remains valid.
                if member == "indexes" {
                    self.borrow_ctx = true;
                    let base_ty = self.infer(inner)?;
                    let is_seq = matches!(
                        &base_ty,
                        Type::List(_) | Type::FixedList { .. }
                    ) || matches!(&base_ty, Type::Apply { name, .. } if name == "Iter");
                    if is_seq {
                        let receiver = std::mem::replace(
                            inner,
                            Box::new(Expr::Ident(String::new(), span)),
                        );
                        *e = Expr::MethodCall {
                            receiver,
                            method: "indexes".to_string(),
                            method_span: span,
                            type_args: Vec::new(),
                            args: Vec::new(),
                            recv_type: None,
                            resolved_ret: None,
                        };
                        return self.infer(e);
                    }
                }
                self.infer_field(inner, &member, span)
            }
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
                            format!(
                                "`?.` needs an optional on the left, but this is `{}`",
                                other.show()
                            ),
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
                // D-SHAPE3a=A: the parser's empty identifier is the unspellable
                // receiver sentinel for `.new(...)`. Resolve it only from the same
                // expected type already propagated for literals/calls; never search a
                // constructor registry. Rewriting here gives TIR/AOT/dev the ordinary
                // explicit static-call path.
                if matches!(&**receiver, Expr::Ident(name, _) if name.is_empty()) {
                    let resolved = match self.expected_type.as_ref() {
                        Some(Type::Named(name)) => Some((name.clone(), Vec::new())),
                        Some(Type::Apply { name, args }) => Some((name.clone(), args.clone())),
                        Some(Type::Shared(inner)) => {
                            Some(("Shared".to_string(), vec![(**inner).clone()]))
                        }
                        Some(Type::List(inner)) => {
                            Some(("List".to_string(), vec![(**inner).clone()]))
                        }
                        Some(Type::Map { key, value, .. }) => Some((
                            "Map".to_string(),
                            vec![(**key).clone(), (**value).clone()],
                        )),
                        Some(Type::Int) => Some(("Int".to_string(), Vec::new())),
                        Some(Type::Float) => Some(("Float".to_string(), Vec::new())),
                        Some(Type::Bool) => Some(("Bool".to_string(), Vec::new())),
                        Some(Type::String) => Some(("String".to_string(), Vec::new())),
                        Some(Type::Char) => Some(("Char".to_string(), Vec::new())),
                        Some(Type::IntN { signed, bits }) => Some((
                            crate::AST::int_spelling(*signed, *bits),
                            Vec::new(),
                        )),
                        Some(Type::Float32) => Some(("F32".to_string(), Vec::new())),
                        Some(Type::Tagged { inner, .. }) => match inner.as_ref() {
                            Type::Named(name) => Some((name.clone(), Vec::new())),
                            Type::Apply { name, args } => Some((name.clone(), args.clone())),
                            _ => None,
                        },
                        _ => None,
                    };
                    let Some((type_name, expected_args)) = resolved else {
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                        self.diags.push(Diagnostic::error(
                            "E0356",
                            "`.new(...)` needs one known receiver type here".to_string(),
                            "the inferred constructor uses the surrounding expected type; Jet does not search a global constructor registry".to_string(),
                            "add a type annotation or write the full `Type.new(...)` form".to_string(),
                            Some(*method_span),
                        ));
                        return None;
                    };
                    **receiver = Expr::Ident(type_name, receiver.span());
                    if type_args.is_empty() {
                        *type_args = expected_args;
                    }
                }
                // D-MEM1/S7 (D-NOALLOC-SEM1=A): `.push`/`.insert` may grow a
                // List/Map's backing heap allocation — capacity headroom isn't
                // statically provable in general, so ANY call of this shape is
                // flagged, full stop (no receiver-type check needed).
                if matches!(method.as_str(), "push" | "insert" | "add" | "add_new") {
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::Allocation,
                        *method_span,
                        format!("`.{method}` may allocate to grow backing storage"),
                    ));
                }
                // D-TAG1: a payload leaf under a group — `Damage.Fire.Burn(5)`
                // parses as a method call on the `Damage.Fire` field chain. Fold
                // chain + method into one dotted variant path and rewrite to an
                // enum literal (single-segment `Enum.Variant(args)` keeps its
                // existing MethodCall route — receiver is a bare Ident there,
                // which `fold_enum_variant_path` deliberately does not fold).
                if recv_type.is_none()
                    && type_args.is_empty()
                    && args.iter().all(|a| a.label.is_none())
                {
                    if let Some((type_name, prefix)) = self.fold_enum_variant_path(receiver) {
                        let variant = format!("{prefix}.{}", method);
                        let span = Span::new(
                            receiver.span().start,
                            args.last()
                                .map(|a| a.span.end)
                                .unwrap_or(method_span.end)
                                .max(method_span.end),
                        );
                        let mut enum_args: Vec<EnumLitArg> = args
                            .drain(..)
                            .map(|a| EnumLitArg::Positional(a.expr))
                            .collect();
                        let ty = self.check_enum_lit(&type_name, &variant, &mut enum_args, span);
                        *e = Expr::EnumLit {
                            type_name,
                            variant,
                            args: enum_args,
                            span,
                        };
                        return Some(ty);
                    }
                }
                // Fixed.over borrows raw backing; unlike an ordinary `&` fill
                // call it does not promise to initialize every byte. Let the
                // constructor inspect the array type without consuming its
                // write-before-read obligation.
                let fixed_uninit = if method == "over"
                    && matches!(&**receiver, Expr::Field(_, name, _) if name == "Fixed")
                {
                    args.iter()
                        .filter_map(|arg| match &arg.expr {
                            Expr::Ident(name, _) => self
                                .uninit
                                .get(name)
                                .cloned()
                                .map(|state| (name.clone(), state)),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                if fixed_uninit.is_empty() {
                    self.clear_uninit_mut_args(args);
                } else {
                    // `Fixed.over(&bytes)` is the one raw-storage adapter: it
                    // borrows the wrapper without claiming initialization.
                    for (name, _) in &fixed_uninit {
                        self.uninit.remove(name);
                    }
                }
                let inferred = self.infer_method_call(
                    receiver,
                    method,
                    *method_span,
                    type_args,
                    args,
                    recv_type,
                    resolved_ret,
                );
                self.uninit.extend(fixed_uninit);
                inferred
            }
            Expr::StructLit {
                type_name,
                type_args,
                import_ns,
                fields,
                inferred,
                span,
                ..
            } => {
                // D-DOTCTOR1: inferred `.{ … }` form — resolve type_name from context
                // and write it back so later passes (TIR lowering, codegen) see it.
                if *inferred {
                    let mut expected = self.expected_type.clone();
                    while let Some(Type::Tagged { inner, .. }) = expected {
                        expected = Some(*inner);
                    }
                    match expected {
                        Some(Type::Named(ctx_name)) => {
                            *type_name = ctx_name;
                        }
                        Some(Type::Apply { name, args }) => {
                            *type_name = name;
                            *type_args = args;
                        }
                        _ => {
                            self.diags.push(Diagnostic::error(
                                "E0119",
                                "`.{ … }` needs a known struct type here".to_string(),
                                "the inferred construction form requires an expected type \
                                 from the surrounding context (binding annotation, return type, etc.)"
                                    .to_string(),
                                "add a type annotation, e.g. `x: Point :: .{ x: 1, y: 2 }`"
                                    .to_string(),
                                Some(*span),
                            ));
                            for (_, _, e) in fields.iter_mut() {
                                self.infer(e);
                            }
                            return None;
                        }
                    }
                }
                // D-MEM1/S7 (D-NOALLOC-SEM1=A): a struct literal for a type
                // that owns heap data (directly or transitively) allocates.
                Some(self.check_struct_lit(
                    type_name,
                    type_args,
                    import_ns.as_deref(),
                    fields,
                    *span,
                ))
            }
            Expr::EnumLit {
                type_name,
                variant,
                args,
                span,
            } => {
                if type_name.is_empty() {
                    // D-ENUMDOT2=A: leading-dot variant — resolve type from expected context.
                    let mut expected = self.expected_type.clone();
                    while let Some(Type::Tagged { inner, .. }) = expected {
                        expected = Some(*inner);
                    }
                    let resolved = expected.and_then(|et| {
                        let name = match &et {
                            Type::Named(n) => Some(n.clone()),
                            Type::Apply { name, .. } => Some(name.clone()),
                            Type::Option(inner) => match inner.as_ref() {
                                Type::Named(n) => Some(n.clone()),
                                _ => None,
                            },
                            _ => None,
                        };
                        name.filter(|n| self.resolve_enum_variants_cloned(n).is_some())
                    });
                    match resolved {
                        Some(tn) => *type_name = tn,
                        None => {
                            self.diags.push(Diagnostic::error(
                                "E0330",
                                format!("can't infer the enum type for `.{}`", variant),
                                "a leading-dot variant needs a known enum type from context"
                                    .to_string(),
                                format!(
                                    "add a type annotation, or write the full form `EnumName.{}`",
                                    variant
                                ),
                                Some(*span),
                            ));
                            return None;
                        }
                    }
                }
                // D-MEM1/S7 (D-NOALLOC-SEM1=A): an enum literal for a type
                // that owns heap data (directly or transitively) allocates.
                Some(self.check_enum_lit(type_name, variant, args, *span))
            }
            // D-TAINT1: `#Tainted expr` — the value-fact tag is type-transparent.
            // Its type is exactly the inner's type; taint propagation + the E0721
            // sink check run in the dedicated taint pass (Sema/Taint.rs), erased
            // in codegen (I3).
            Expr::Tainted(inner, tag, span) => {
                let tag = tag.as_deref().unwrap_or("Input");
                if !self.tag_is_declared(tag) {
                    self.diags.push(crate::Sema::Diagnostics::undeclared_value_tag(
                        tag,
                        self.closest_declared_tag(tag).as_deref(),
                        *span,
                    ));
                }
                self.infer(inner)
            }
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
                            format!("bare `{}` needs a known optional type here", Syntax::LIT_NULL),
                            format!(
                                "`{}` only fits where a `T?` is expected (S32)",
                                Syntax::LIT_NULL
                            ),
                            format!(
                                "add a type annotation, or use `{}` where the type is already known",
                                Syntax::LIT_NULL
                            ),
                            Some(*span),
                        ));
                        None
                    }
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0308",
                        format!(
                            "bare `{}` needs a known optional type here",
                            Syntax::LIT_NULL
                        ),
                        format!(
                            "`{}` only fits where a `T?` is expected (S32)",
                            Syntax::LIT_NULL
                        ),
                        format!(
                            "add a type annotation, or use `{}` where the type is already known",
                            Syntax::LIT_NULL
                        ),
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
            // D-REDUCE-VALUE1: this node exists only to teach retired `#Op` calls.
            // The reduce method consumes it before reaching here; a bare/misplaced
            // marker is a usage error.
            Expr::ReduceMarker(name, span) => {
                self.diags.push(Diagnostic::error(
                    "E2510",
                    format!("`#{}` is only valid inside a lane `.reduce(…)`", name),
                    "a reduce-op marker names the fold operation; it isn't a value on its own"
                        .to_string(),
                    "write `v.reduce(.Add)`, `.Mul`, `.Min`, `.Max`, or `.Avg`".to_string(),
                    Some(*span),
                ));
                None
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
                let expected = match self.expected_type.as_ref() {
                    Some(Type::Named(name)) if name == "HTTPHandler" => Some(Type::Fn {
                        params: vec![Type::Named("HTTPRequest".to_string())],
                        ret: Some(Box::new(Type::Result {
                            ok: Box::new(Type::Named("HTTPResponse".to_string())),
                            err: Box::new(Type::Named("HTTPError".to_string())),
                        })),
                        effect_bound: None, return_view_provenance: None,
                    }),
                    _ => self.expected_type.clone(),
                };
                self.with_deferred_call_access(|checker| {
                    checker.check_lambda(lam, expected.as_ref())
                })
            }
            Expr::CallValue { callee, args, span } => self.infer_call_value(callee, args, *span),
            // D-CTMARKER1=C: `$name` comptime splice. Valid only in comptime contexts;
            // the Comptime interpreter resolves the value. In runtime code: E2712.
            Expr::ComptimeSplice { name, span, value } => {
                if !self.in_comptime {
                    let globals = self.current_ct_globals();
                    if let Some(v) = globals.get(name).cloned() {
                        let ty = v.jet_type();
                        *value = Some(v);
                        return Some(ty);
                    }
                    self.diags.push(Diagnostic::error(
                        "E2713",
                        format!("there is no comptime value named `{}`", name),
                        "`$name` splices a value that was computed by a `comptime` binding or `#Known {}` block".to_string(),
                        format!("define `#Known {name} :: ...` before using `${name}`"),
                        Some(*span),
                    ));
                }
                None
            }
            // D-FMTPARENS1=A: parenthesized expressions are transparent to type checking.
            Expr::Paren(inner, _) => {
                let mut inner = inner.as_mut();
                let mut entered = 0;
                while matches!(inner, Expr::Paren(..)) {
                    if !self.enter_source_nesting(inner.span()) {
                        for _ in 0..entered {
                            self.leave_source_nesting();
                        }
                        return None;
                    }
                    entered += 1;
                    let Expr::Paren(next, _) = inner else {
                        unreachable!("checked parenthesized expression")
                    };
                    inner = next;
                }
                let result = self.infer(inner);
                for _ in 0..entered {
                    self.leave_source_nesting();
                }
                result
            }
            // D-INCR1: `++`/`--` on a mutable integer lvalue.
            Expr::IncDec {
                op,
                operand,
                postfix,
                span,
            } => self.check_incdec(*op, operand, *postfix, *span),
            Expr::TypedLit { .. } => unreachable!("TypedLit elaborated before match"),
        }
    }

    /// D-DOTCTOR3=A: elaborate `Type.{ body }` / inferred `.{ body }` against the
    /// head (or expected type), rewrite to ListLit / MapLit / StructLit / value,
    /// then re-infer. Never inserts a conversion.
    pub(crate) fn elaborate_typed_lit(&mut self, e: &mut Expr) -> Option<Type> {
        let Expr::TypedLit { head, body, span } = std::mem::replace(e, Expr::Absent(Span::new(0, 0)))
        else {
            unreachable!("elaborate_typed_lit only for TypedLit");
        };
        let span = span;
        let head = match head.or_else(|| self.expected_type.clone()) {
            Some(h) => h,
            None => {
                *e = Expr::TypedLit {
                    head: None,
                    body,
                    span,
                };
                self.diags.push(Diagnostic::error(
                    "E0119",
                    "`.{ … }` needs a known type here".to_string(),
                    "an inferred typed literal needs an expected type from the surrounding context"
                        .to_string(),
                    "add a type head, e.g. `[U8].{ 1, 2 }`, or annotate the binding".to_string(),
                    Some(span),
                ));
                return None;
            }
        };

        match (head.clone(), body) {
            (Type::List(_) | Type::FixedList { .. }, TypedLitBody::Empty) => {
                *e = Expr::ListLit(Vec::new(), span);
            }
            (Type::List(_) | Type::FixedList { .. }, TypedLitBody::Elements(elems)) => {
                *e = Expr::ListLit(elems, span);
            }
            (Type::List(_) | Type::FixedList { .. }, TypedLitBody::Value(inner)) => {
                *e = Expr::ListLit(vec![*inner], span);
            }
            (Type::Map { .. }, TypedLitBody::Empty) => {
                *e = Expr::MapLit(Vec::new(), span);
            }
            (Type::Map { .. }, TypedLitBody::Entries(entries)) => {
                *e = Expr::MapLit(entries, span);
            }
            (Type::Named(name), TypedLitBody::Fields(fields)) => {
                *e = Expr::StructLit {
                    type_name: name,
                    type_args: Vec::new(),
                    import_ns: None,
                    as_trait: None,
                    fields,
                    inferred: false,
                    span,
                };
            }
            (Type::Apply { name, args }, TypedLitBody::Fields(fields)) => {
                *e = Expr::StructLit {
                    type_name: name,
                    type_args: args,
                    import_ns: None,
                    as_trait: None,
                    fields,
                    inferred: false,
                    span,
                };
            }
            (Type::Named(name), TypedLitBody::Empty) => {
                *e = Expr::StructLit {
                    type_name: name,
                    type_args: Vec::new(),
                    import_ns: None,
                    as_trait: None,
                    fields: Vec::new(),
                    inferred: false,
                    span,
                };
            }
            (Type::Apply { name, args }, TypedLitBody::Empty) => {
                *e = Expr::StructLit {
                    type_name: name,
                    type_args: args,
                    import_ns: None,
                    as_trait: None,
                    fields: Vec::new(),
                    inferred: false,
                    span,
                };
            }
            // D-UNIFYLIT1=A: `SQL.{"…"}` / `HTML.{"…"}` / `Sh.{"…"}` — typed head
            // is the sole domain-text spelling (no silent bare-quote rewrite).
            (
                Type::Named(ref type_name),
                TypedLitBody::Value(inner),
            ) if matches!(type_name.as_str(), "SQL" | "HTML")
                || type_name == Syntax::TYPE_SH =>
            {
                *e = *inner;
                return self.rewrite_typed_text_literal(e, type_name.clone(), span);
            }
            (
                Type::Named(ref type_name),
                TypedLitBody::Value(inner),
            ) if type_name == Syntax::TYPE_REGEX =>
            {
                *e = *inner;
                return self.rewrite_regex_literal(e, span);
            }
            (_, TypedLitBody::Value(inner)) => {
                *e = *inner;
            }
            (_, TypedLitBody::Elements(elems)) if elems.len() == 1 => {
                *e = elems.into_iter().next().unwrap();
            }
            (_, body) => {
                *e = Expr::TypedLit {
                    head: Some(head.clone()),
                    body,
                    span,
                };
                self.diags.push(Diagnostic::error(
                    "E0119",
                    format!("this body doesn't match typed-literal head `{}`", head.name()),
                    "a typed literal body uses the head type's own literal shape".to_string(),
                    "use elements for lists, entries for maps, fields for records, or one expression for scalars"
                        .to_string(),
                    Some(span),
                ));
                return Some(head);
            }
        }

        let saved = self.expected_type.replace(head.clone());
        let ty = self.infer(e);
        self.expected_type = saved;
        // Head wins as the expression's type when inference produced something
        // assignable; mismatches already diagnosed by infer/check_type_assignable.
        match ty {
            Some(got) => {
                if got != head {
                    self.check_type_assignable(&head, &got, e.span());
                }
                Some(head)
            }
            None => Some(head),
        }
    }

    fn infer_owned_list_element(&mut self, elem: &mut Expr) -> Option<Type> {
        let ty = self.infer(elem);
        if ty.is_some() {
            self.note_move_if_direct_ident(elem);
        }
        ty
    }

    pub(crate) fn infer_list_lit(&mut self, elems: &mut [Expr], span: Span) -> Option<Type> {
        for elem in elems.iter() {
            self.reject_fixed_storage(elem, "be stored in a list");
        }
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
                "write `[]` only where the list type is already known from around it".to_string(),
                "name the element type on the literal: `[Int].{}`".to_string(),
                Some(span),
            ));
            return None;
        }
        // D-FIXARR1: a list literal in a `[T#N]` binding context keeps the fixed-size type.
        // Sema validates element types and count, codegen emits a Rust array `[e1, …]`.
        if let Some(Type::FixedList {
            elem: expected_inner,
            len,
            ..
        }) = self.expected_type.clone()
        {
            if elems.iter().any(|e| matches!(e, Expr::Spread(..))) {
                self.diags.push(Diagnostic::error(
                    "E1311",
                    "list spread can't build a fixed-size `[T#N]` list".to_string(),
                    "spread expands a growable list — a `[T#N]` has a fixed compile-time length"
                        .to_string(),
                    "use a growable `[T]` binding (`::`/`:=`) instead of `[T#N]`".to_string(),
                    Some(span),
                ));
                return None;
            }
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
                    format!(
                        "provide exactly {} element{}",
                        len,
                        if len == 1 { "" } else { "s" }
                    ),
                    Some(span),
                ));
                return None;
            }
            let saved = self.expected_type.clone();
            self.expected_type = Some((*expected_inner).clone());
            for e in elems.iter_mut() {
                if let Some(t) = self.infer_owned_list_element(e) {
                    self.check_type_assignable(&expected_inner, &t, e.span());
                }
            }
            self.expected_type = saved;
            return Some(Type::FixedList {
                elem: expected_inner,
                len,
                len_symbol: None,
            });
        }
        if let Some(Type::List(expected_inner)) = self.expected_type.clone() {
            if let Type::TraitObject(trait_names) = expected_inner.as_ref() {
                // D-ANY-JAI1: coercion to a struct-literal's blessed single trait name
                // (`as_trait`) only applies to the S48 single-trait-object shape — a
                // multi-trait `TraitObject` (only ever produced for a variadic loop
                // element, never a list-literal's declared element type) has no one
                // trait to coerce toward, so it falls through to the ordinary
                // `check_type_assignable` path below (which checks every bound).
                let trait_name = trait_names.first().filter(|_| trait_names.len() == 1);
                for e in elems.iter_mut() {
                    if let Some(t) = self.infer_owned_list_element(e) {
                        match (&t, trait_name) {
                            (Type::Named(n), Some(trait_name))
                                if self.trait_reg.implements_trait(n, trait_name) =>
                            {
                                if let Expr::StructLit { as_trait, .. } = e {
                                    *as_trait = Some(trait_name.clone());
                                }
                            }
                            (Type::Apply { name, .. }, Some(trait_name))
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
            // D-DOTCTOR2/3 + D-SG9: any expected element type elaborates each
            // list element against it (nested `[U8]` lists, struct `.{}` forms,
            // and fixed-width scalars with range checks).
            let saved = self.expected_type.clone();
            let saved_string_view_read = self.allow_string_view_read;
            let string_view_elements = matches!(
                expected_inner.as_ref(),
                Type::Apply { name, args }
                    if name == "View"
                        && matches!(args.as_slice(), [Type::Named(inner)] if inner == "str")
            );
            if string_view_elements {
                self.allow_string_view_read = true;
            }
            self.expected_type = Some(expected_inner.as_ref().clone());
            for e in elems.iter_mut() {
                match e {
                    Expr::Spread(inner, spread_span) => {
                        let t = self.infer_owned_list_element(inner);
                        match t {
                            Some(Type::List(spread_elem)) => {
                                self.check_type_assignable(&expected_inner, &spread_elem, *spread_span);
                            }
                            Some(other) => {
                                self.diags.push(Diagnostic::error(
                                    "E1311",
                                    format!("spread needs a list, not `{}`", other.name()),
                                    "list spread `[...xs, y]` expands a list's elements in place"
                                        .to_string(),
                                    "spread a `[T]` value here".to_string(),
                                    Some(*spread_span),
                                ));
                            }
                            None => {}
                        }
                    }
                    _ => {
                        if let Some(t) = self.infer_owned_list_element(e) {
                            let string_view_compatible = string_view_elements
                                && t == Type::String
                                && (matches!(
                                    e,
                                    Expr::Ident(name, _) if self.is_string_view(name)
                                ) || self.string_view_call_source(e).is_some());
                            if !string_view_compatible {
                                self.check_type_assignable(&expected_inner, &t, e.span());
                            }
                        }
                    }
                }
            }
            self.expected_type = saved;
            self.allow_string_view_read = saved_string_view_read;
            self.check_list_view_element_aliases(elems, &expected_inner);
            return Some(Type::List(expected_inner));
        }
        let mut elem_types = Vec::new();
        for e in elems.iter_mut() {
            match e {
                Expr::Spread(inner, spread_span) => {
                    let t = self.infer_owned_list_element(inner);
                    match t {
                        Some(Type::List(spread_elem)) => {
                            elem_types.push((*spread_elem).clone());
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E1311",
                                format!("spread needs a list, not `{}`", other.name()),
                                "list spread `[...xs, y]` expands a list's elements in place"
                                    .to_string(),
                                "spread a `[T]` value here".to_string(),
                                Some(*spread_span),
                            ));
                        }
                        None => {}
                    }
                }
                _ => {
                    if let Some(t) = self.infer_owned_list_element(e) {
                        elem_types.push(t);
                    }
                }
            }
        }
        // D-NUMJOIN1=A: numeric elements widen to one element type.
        let mut first = elem_types.first()?.clone();
        for t in elem_types.iter().skip(1) {
            if *t != first {
                if let Some(joined) = first.numeric_join(t) {
                    first = joined;
                }
            }
        }
        for (i, (elem, t)) in elems.iter().zip(&elem_types).enumerate() {
            let spread_needs_conversion = matches!(elem, Expr::Spread(..)) && *t != first;
            if spread_needs_conversion
                || (*t != first && t.numeric_widening_to(&first).is_none())
            {
                self.diags.push(Diagnostic::error(
                    "E0504",
                    format!(
                        "this list resolves to `{}` but item {} is `{}`",
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
        for (elem, source) in elems.iter_mut().zip(&elem_types) {
            if !matches!(elem, Expr::Spread(..)) {
                self.widen_numeric_expr(elem, source, &first);
            }
        }
        self.check_list_view_element_aliases(elems, &first);
        Some(Type::List(Box::new(first)))
    }

    pub(crate) fn infer_tuple_lit(
        &mut self,
        fields: &mut [(String, Expr)],
        _span: Span,
    ) -> Option<Type> {
        let mut seen = HashSet::new();
        let mut typed = Vec::with_capacity(fields.len());
        for (name, expr) in fields.iter_mut() {
            self.reject_fixed_storage(expr, "be stored in a tuple");
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


    pub(crate) fn infer_map_lit(
        &mut self,
        entries: &mut [(Expr, Expr)],
        span: Span,
    ) -> Option<Type> {
        if self.freestanding {
            self.diags.push(e3303(span));
        }
        if entries.is_empty() {
            // D-EMPTYLIT1: the parser never produces an empty `Expr::MapLit`
            // directly anymore (`[:]` is retired) — this only runs if some
            // other rewrite reaches here without first resolving a Map
            // expected type. Kept as a defensive fallback with `[]` wording.
            if let Some(expected) = self.expected_type.clone() {
                if let Type::Map {
                    key,
                    key_span,
                    value,
                } = expected {
                    return Some(Type::Map {
                        key,
                        key_span,
                        value,
                    });
                }
            }
            self.diags.push(Diagnostic::error(
                "E0501",
                "an empty map needs a type".to_string(),
                "write `[]` only where the map type is already known from around it".to_string(),
                "name the key and value types on the literal: `[String: Int].{}`".to_string(),
                Some(span),
            ));
            return None;
        }
        let mut key_ty = None;
        let mut val_ty = None;
        for (k, v) in entries.iter_mut() {
            self.reject_fixed_storage(k, "be stored in a map");
            self.reject_fixed_storage(v, "be stored in a map");
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
                key_span: None,
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
                if idx_ty == Type::Named(crate::Syntax::TYPE_RANGE.to_string()) {
                    *kind = IndexKind::Range;
                    return Some(Type::List(inner.clone()));
                }
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
            Type::FixedList { elem, len, .. } => {
                *kind = IndexKind::List;
                if idx_ty != Type::Int {
                    if let Type::Named(name) = &idx_ty {
                        if let Some((lo, hi)) = self.registry.distinct_range(name) {
                            let base_is_int =
                                matches!(self.registry.distinct_base(name), Some(Type::Int));
                            if base_is_int && lo >= 0 && (hi as u64) < *len {
                                *kind = IndexKind::FixedListProof;
                                return Some((**elem).clone());
                            }
                            self.diags.push(Diagnostic::error(
                                "E0965",
                                format!(
                                    "`{}` proves {}..{}, which is not inside this fixed-size list's indexes",
                                    name, lo, hi
                                ),
                                format!(
                                    "this `[T#{}]` value only has proven indexes 0 through {}",
                                    len,
                                    len.saturating_sub(1)
                                ),
                                "use a refinement whose invariant fits the list length".to_string(),
                                Some(index.span()),
                            ));
                            return Some((**elem).clone());
                        }
                    }
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
                } else if let Expr::Int(n, _, _, _) = index.as_ref() {
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
            // D-MEM1 S6 (D-POOLID-API1=A): `pool[id]` — generation-checked read,
            // panics at runtime on a stale `Id<T>` (mirrors the array-oob panic
            // precedent, not a new diagnostic code — see `jet_pool_get`).
            Type::Apply { name, args } if name == "Pool" && args.len() == 1 => {
                *kind = IndexKind::Pool;
                let is_matching_id = matches!(&idx_ty, Type::Apply { name, args: id_args } if name == "Id" && id_args.first() == args.first());
                if !is_matching_id {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`Pool` indexes need a matching `Id<T>`, not {}",
                            idx_ty.show()
                        ),
                        "a pool slot is only reached through the `Id<T>` its own `.add()` returned"
                            .to_string(),
                        "index with the `Id<T>` handle from `.add(...)`".to_string(),
                        Some(index.span()),
                    ));
                }
                Some(args[0].clone())
            }
            // D-DYNARRAY1: `window[i]` on a `View<T>` — read-only, same bounds
            // discipline as list indexing (runtime panic on out-of-range).
            Type::Apply { name, args }
                if matches!(name.as_str(), "View" | "ViewMut") && args.len() == 1 =>
            {
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
                Some(args[0].clone())
            }
            Type::Map { key, value, .. } => {
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
            // D-SIMD2: `v[i]` lane access on a SIMD lane type returns the lane scalar.
            // (A user type sharing the name `F32x4` would be a non-indexable struct,
            // falling to the default E0505 arm.)
            Type::Named(n) if is_simd_lane_type(n) && !self.registry.contains(n) => {
                let lane_name = n.clone();
                *kind = IndexKind::Lane(lane_name.clone());
                if idx_ty != Type::Int {
                    self.diags.push(Diagnostic::error(
                        "E0505",
                        format!(
                            "lane indexes must be {}, not {}",
                            Type::Int.show(),
                            idx_ty.show()
                        ),
                        "a SIMD lane is read by position with a whole-number index starting at 0"
                            .to_string(),
                        "use an Int index, like `v[0]`".to_string(),
                        Some(index.span()),
                    ));
                } else if let Expr::Int(num, _, _, _) = index.as_ref() {
                    let lanes = math_arity(&lane_name) as i64;
                    if *num < 0 || *num >= lanes {
                        self.diags.push(Diagnostic::error(
                            "E0965",
                            format!(
                                "lane {} is out of range for `{}` ({} lanes)",
                                num, lane_name, lanes
                            ),
                            format!(
                                "the valid lanes for `{}` are 0 through {}",
                                lane_name,
                                lanes - 1
                            ),
                            format!("use a lane index between 0 and {}", lanes - 1),
                            Some(index.span()),
                        ));
                    }
                }
                Some(math_scalar_ty(&lane_name))
            }
            Type::Named(n) if self.trait_reg.index_types.contains_key(n) => {
                let (key_ty, value_ty) = self.trait_reg.index_types.get(n).unwrap();
                *kind = IndexKind::User(n.clone());
                if idx_ty != *key_ty {
                    self.diags.push(Diagnostic::error(
                        "E0505",
                        format!(
                            "this value indexes with {}, not {}",
                            key_ty.show(),
                            idx_ty.show()
                        ),
                        "the key in `value[key]` must match the type's `Index` key".to_string(),
                        format!("use a {} key here", key_ty.name()),
                        Some(index.span()),
                    ));
                }
                Some(value_ty.clone())
            }
            Type::String => {
                self.diags.push(Diagnostic::error(
                    "E0503",
                    "strings aren't indexed with `[ ]`".to_string(),
                    "text is counted in characters — walk them with `.chars()` or take a piece with `.slice(start..end)`".to_string(),
                    "e.g. `loop c, s.chars() { }` or `s.slice(0..2)`".to_string(),
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
            Type::List(inner) => {
                // D-SOA1: slicing a columnar list is deferred (v1 core surface is
                // index/field/len/push/iterate); reject rather than miscompile.
                if let Type::Named(elem) = inner.as_ref() {
                    if self.registry.is_columnar(elem) {
                        self.diags.push(Diagnostic::error(
                            "E1108",
                            format!(
                                "slicing isn't supported on a columnar list `{}` yet",
                                Type::List(inner.clone()).show()
                            ),
                            "`#Layout(columnar)` lists support the core surface in v1: indexing, field access, `len`, `is_empty`, `push`, and iteration".to_string(),
                            format!(
                                "drop `#Layout(columnar)` from `{}` to slice, or index the elements you need in a loop",
                                elem
                            ),
                            Some(span),
                        ));
                        return None;
                    }
                }
                Some(Type::List(inner))
            }
            Type::String => Some(Type::String),
            other => {
                self.diags.push(Diagnostic::error(
                    "E0505",
                    format!("only lists and strings can be sliced, not {}", other.show()),
                    "a Range can project a window only from indexed storage".to_string(),
                    "use `xs[a..b]` on a list or a text range operation on String".to_string(),
                    Some(span),
                ));
                None
            }
        }
    }

    fn infer_range(
        &mut self,
        start: &mut Box<Expr>,
        end: &mut Box<Expr>,
        _span: Span,
    ) -> Option<Type> {
        let mut valid = true;
        for bound in [start.as_mut(), end.as_mut()] {
            if self.infer(bound)? != Type::Int {
                valid = false;
                self.diags.push(Diagnostic::error(
                    "E0505",
                    "range bounds must be Int".to_string(),
                    "a Range stores whole-number start and end bounds".to_string(),
                    "use Int values on both sides of the range operator".to_string(),
                    Some(bound.span()),
                ));
            }
        }
        valid.then(|| Type::Named(crate::Syntax::TYPE_RANGE.to_string()))
    }

    pub(crate) fn infer_field(
        &mut self,
        inner: &mut Box<Expr>,
        member: &str,
        span: Span,
    ) -> Option<Type> {
        let field_expr = Expr::Field(inner.clone(), member.to_string(), span);
        if self.reject_moved_expr_use(&field_expr, span) {
            return None;
        }
        if let Expr::Field(base, leaf, _) = &**inner {
            if let Expr::Ident(alias, _) = &**base {
                if self.core_imports.get(alias).map(String::as_str) == Some("core.lang")
                    && crate::Policy::rule_arg_declaration(leaf).is_some()
                {
                    let enum_name = leaf.clone();
                    **inner = Expr::Ident(enum_name.clone(), span);
                    let mut empty = Vec::new();
                    return Some(self.check_enum_lit(&enum_name, member, &mut empty, span));
                }
                if self.core_imports.get(alias).map(String::as_str) == Some("core.encoding") {
                    let enum_name = match leaf.as_str() {
                        "DataEvent" => Some("DataEvent"),
                        "EncodingFormat" => Some("EncodingFormat"),
                        "EncodingErrorKind" => Some("EncodingErrorKind"),
                        _ => None,
                    };
                    if let Some(enum_name) = enum_name {
                        **inner = Expr::Ident(enum_name.to_string(), span);
                        let mut empty = Vec::new();
                        return Some(self.check_enum_lit(enum_name, member, &mut empty, span));
                    }
                }
                if leaf == "State" {
                    let is_declared_state = self
                        .struct_owner_module(alias, None)
                        .and_then(|owner| self.modules.and_then(|modules| modules.get(owner)))
                        .and_then(|module| module.declared_states.get(alias))
                        .is_some_and(|states| states.iter().any(|state| state == member));
                    if is_declared_state {
                        self.diags.push(Diagnostic::error(
                            "E0302",
                            format!("`{alias}.State.{member}` is not a runtime value"),
                            "the reserved `.State` plane contains compile-time facts"
                                .to_string(),
                            format!(
                                "use it in `#State({alias}.State.{member})`, `#Transition`, or type reflection"
                            ),
                            Some(span),
                        ));
                        return None;
                    }
                }
            }
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
            if let (Some(modules), Some(module_idx)) = (self.modules, self.imports.get(alias)) {
                if let Some(sig) = modules[*module_idx].funcs.get(member) {
                    if sig.c_abi_name.is_some() {
                        let ty = Type::Fn {
                            params: sig.params.iter().map(|(_, ty)| ty.clone()).collect(),
                            ret: sig.return_type.clone().map(Box::new),
                            effect_bound: None, return_view_provenance: None,
                        };
                        self.diags.push(crate::Sema::FFI::e3203(&ty, span));
                        return Some(ty);
                    }
                }
            }
        }
        if let Expr::Ident(type_name, type_span) = &**inner {
            // D-SERDE13=B: `Data.Null` etc. — retired spelling, point at `DataTree`.
            if type_name == "Data" {
                self.diags.push(data_renamed_to_datatree(*type_span));
                return Some(json_ty());
            }
            if is_json_type_name(type_name) {
                if let Some(ret) = self.check_core_json_lit(member, &mut [], span) {
                    return Some(ret);
                }
            }
            // D-DBDRIVER1: `DBValue.Null` — the only zero-arg `DBValue` variant,
            // reaching sema as a `Field` (mirrors `Data.Null` just above).
            if type_name == crate::Syntax::TYPE_DB_VALUE {
                if let Some(ret) = self.check_core_dbvalue_lit(member, &mut [], span) {
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
            if let Some(owner_mod) = self.struct_owner_module(type_name, None) {
                let is_declared_state = self
                    .modules
                    .and_then(|modules| modules.get(owner_mod))
                    .and_then(|module| module.declared_states.get(type_name))
                    .is_some_and(|states| states.iter().any(|state| state == member));
                let (what, why, fix) = if is_declared_state {
                    (
                        format!("`{type_name}.{member}` is not a value"),
                        "struct fields need a value before the dot; typestate names are compile-time facts, not runtime values"
                            .to_string(),
                        format!(
                            "use a `{type_name}` value before a field, or call a static method on `{type_name}`"
                        ),
                    )
                } else {
                    (
                        format!("`{type_name}` has no static member `{member}`"),
                        format!(
                            "`{type_name}` names a struct type; fields need a value before the dot"
                        ),
                        format!(
                            "use a `{type_name}` value before an instance field, or call a static method that exists on `{type_name}`"
                        ),
                    )
                };
                self.diags.push(Diagnostic::error(
                    "E0302",
                    what,
                    why,
                    fix,
                    Some(span),
                ));
                return None;
            }
        }
        // Numeric field spelling already emitted E0049 in the parser. The
        // recovery member cannot name a Jet field, so do not add E0302.
        if member == "0" {
            return None;
        }
        self.borrow_ctx = true;
        let suppress = self.suppress_partial_move_root_read;
        self.suppress_partial_move_root_read = true;
        let t = self.infer(inner);
        self.suppress_partial_move_root_read = suppress;
        let t = t?;
        self.field_type(&t, member, span)
    }

    /// D-FIELDPOL1: true when `member` is a computed field on struct type `t`
    /// (`Named` or `Apply`) — used at every WRITE site (`x.field = …`,
    /// `x.field++`) to reject with E0339 before `field_type` would otherwise
    /// resolve it as a normal (settable) field.
    pub(crate) fn field_is_computed(&self, t: &Type, member: &str) -> bool {
        let type_name = match t {
            Type::Named(n) => n.as_str(),
            Type::Apply { name, .. } => name.as_str(),
            _ => return false,
        };
        let Some(owner_mod) = self.struct_owner_module(type_name, None) else {
            return false;
        };
        self.computed_field_types_of(owner_mod, type_name)
            .is_some_and(|c| c.contains_key(member))
    }

    /// Resolve the type of `member` on the struct type `t` (S71 reuses this for
    /// `?.` chaining). Emits E0302 and returns `None` when there's no such field.
    pub(crate) fn field_type(&mut self, t: &Type, member: &str, span: Span) -> Option<Type> {
        // D-PIN2=A / D-PIN3=A: reaching a field through `Pin<T>` resolves against
        // `T`. The field's own declared type is the mark: a `Pin<U>` field comes
        // back as `Pin<U>` and keeps the no-move promise, every other field comes
        // back as an ordinary view. No second projection rule.
        if let Type::Apply { name, args } = t {
            if name == crate::Syntax::TYPE_PIN && args.len() == 1 {
                return self.field_type(&args[0], member, span);
            }
        }
        // D-SHAREDGUARD2=A: the guard's held value is a compiler-known place,
        // not a stored public struct field. The hidden tag records whether the
        // place is read-only or editable while diagnostics show one public
        // `SharedGuard<T>` type.
        if member == "value" {
            if let Type::Apply { name, args } = t {
                if name == crate::Syntax::TYPE_SHARED_GUARD && args.len() == 1 {
                    return Some(args[0].clone());
                }
            }
            if let Type::Tagged { marker, inner } = t {
                if matches!(
                    marker.as_str(),
                    crate::AST::SHARED_GUARD_READ_MARKER
                        | crate::AST::SHARED_GUARD_EDIT_MARKER
                ) {
                    if let Type::Apply { name, args } = inner.as_ref() {
                        if name == crate::Syntax::TYPE_SHARED_GUARD && args.len() == 1 {
                            return Some(args[0].clone());
                        }
                    }
                }
            }
        }
        if let Type::Named(type_name) = t {
            // D-SWIZZLE1: named lane swizzles on vector/SIMD types (not matrices).
            if is_swizzleable_math_type(type_name) && !self.registry.contains(type_name) {
                match parse_swizzle_member(member, type_name) {
                    SwizzleParse::Ok(lanes) => {
                        return Some(swizzle_read_type(type_name, lanes.len()));
                    }
                    SwizzleParse::InvalidLane { lane } => {
                        let valid = match math_arity(type_name) {
                            2 => "x and y",
                            3 => "x, y, and z",
                            4 => "x, y, z, and w",
                            _ => "x/y/z/w",
                        };
                        self.diags.push(Diagnostic::error(
                            "E3110",
                            format!("lane `{}` isn't valid on `{}`", lane, type_name),
                            format!(
                                "swizzle members name lanes with x/y/z/w — `{}` only has {}",
                                type_name, valid
                            ),
                            format!("use only the lanes defined for `{}`", type_name),
                            Some(span),
                        ));
                        return None;
                    }
                    SwizzleParse::NotSwizzle => {}
                }
            }
            if let Some(owner_mod) = self.struct_owner_module(type_name, None) {
                if let Some(fields) = self.struct_fields_of(owner_mod, type_name) {
                    if let Some((_, _, fty, _)) = fields.iter().find(|(fname, ..)| fname == member) {
                        let fty = fty.clone();
                            if owner_mod != self.module_idx
                                && !self.field_is_pub_in(owner_mod, type_name, member)
                            {
                                self.diags.push(private_item(member, span));
                                return None;
                            } else if owner_mod != self.module_idx
                                && Syntax::classify_identifier(member)
                                    == Syntax::IdentifierClass::SoftPublic
                            {
                                self.diags.push(soft_public_use(member, span));
                            }
                        self.record_field_reference(owner_mod, type_name, member, span);
                        return Some(fty);
                    }
                    // D-FIELDPOL1: a computed field is never in `fields` (it's
                    // not stored) but a *read* still resolves its declared
                    // type — only the write side (assignment/incdec/struct-lit)
                    // rejects it with E0339.
                    if let Some(computed) = self.computed_field_types_of(owner_mod, type_name) {
                        if let Some((_, cty)) = computed.get(member) {
                            return Some(cty.clone());
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
            if let Some(fty) = core_struct_field(type_name, member) {
                return Some(fty);
            }
        }
        if let Type::Apply { name, args } = t {
            if let Some(owner_mod) = self.struct_owner_module(name, None) {
                if let Some(fields) = self.struct_fields_of(owner_mod, name) {
                    let subst = self.struct_subst(name, args);
                    if let Some((_, _, fty, _)) = fields.iter().find(|(fname, ..)| fname == member) {
                        let fty = fty.clone();
                            if owner_mod != self.module_idx
                                && !self.field_is_pub_in(owner_mod, name, member)
                            {
                                self.diags.push(private_item(member, span));
                                return None;
                            } else if owner_mod != self.module_idx
                                && Syntax::classify_identifier(member)
                                    == Syntax::IdentifierClass::SoftPublic
                            {
                                self.diags.push(soft_public_use(member, span));
                            }
                        self.record_field_reference(owner_mod, name, member, span);
                        return Some(self.trait_reg.instantiate_type(&fty, &subst));
                    }
                    // D-FIELDPOL1: see the `Type::Named` branch above — a
                    // computed field resolves for reads even though it's
                    // absent from `fields`.
                    if let Some(computed) = self.computed_field_types_of(owner_mod, name) {
                        if let Some((_, cty)) = computed.get(member) {
                            return Some(self.trait_reg.instantiate_type(cty, &subst));
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
            } else if let Some(fty) = core_generic_struct_field(name, member, args) {
                // D-MIGRATE3=A: `DecodeResult<T>` is a reserved core generic with
                // no `struct_owner_module` — but the user-type-wins guard (D-SHIFT1
                // precedent: `Reader`/`Cursor`) means this fallback only runs when
                // no user struct claimed the name above.
                return Some(fty);
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
}
