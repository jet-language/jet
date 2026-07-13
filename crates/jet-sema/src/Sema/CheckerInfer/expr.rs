//! Type inference: core dispatch + literals + index/slice/field access.
//!
//! Split out of the original `CheckerInfer.rs`; behavior unchanged.

use super::*;
use crate::Collections::is_map_key_type;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{
    AccessConvention, Call, CallArg, EnumLitArg, Expr, IndexKind, StrPart, Type, UnOp,
};
use std::collections::HashSet;

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
            return Some(Type::Named(type_name));
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

    /// Infer and check an expression. Returns None when a problem was
    /// already reported (avoids error cascades).
    ///
    /// This wrapper owns the rule that depends on *where* the expression
    /// appears (`borrow_ctx`): a struct-field read in owning position is
    /// rewritten to `.clone()` so the generated Rust never moves a field out
    /// of its struct.
    pub(crate) fn infer(&mut self, e: &mut Expr) -> Option<Type> {
        let borrowed = std::mem::take(&mut self.borrow_ctx);
        let ty = self.infer_inner(e);
        if !borrowed {
            if let Some(t) = &ty {
                if !type_is_copy(t) && field_read_to_clone(e, self.registry, self.imports) {
                    // D-CAP2 (D-MEM1/S4): the same `copy` node the user can write
                    // explicitly — one mechanism for "duplicate this value",
                    // whether the compiler inserts it or the user spells it.
                    let span = e.span();
                    let old = std::mem::replace(e, Expr::Absent(span));
                    *e = Expr::Copy(Box::new(old), span);
                }
            }
        }
        ty
    }

    pub(crate) fn infer_inner(&mut self, e: &mut Expr) -> Option<Type> {
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
                }) = self.expected_type.clone() {
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
            // `#UnitFamily` member (PascalCased to its minted `@Numeric
            // distinct Float` type name) and rewrite this node in place to an
            // ordinary constructor call — sugar over the existing
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
                *e = Expr::Call(Call {
                    name: type_name,
                    name_span: *suffix_span,
                    args: vec![CallArg {
                        convention: AccessConvention::Read,
                        expr: Expr::Float(value, call_span, false),
                        span: call_span,
                        flags: crate::AST::CallArgFlags::default(),
                        label: None,
                        spread: false,
                    }],
                    range_checked: false,
                });
                self.infer(e)
            }
            Expr::Bool(_, _) => Some(Type::Bool),
            // D-TYPEDTEXT1=D: a string literal in a position whose expected
            // type is `Sql`/`Html`/`Sh` elaborates to that typed value instead of
            // `String` — the same expected-type law as `.{ }` construction.
            // Each `{hole}` becomes a bound parameter (Sql) or an escaped
            // insertion (Html) at codegen; it is checked here like any other
            // value, not run through the Display-ability check below (a hole
            // is never printed as text).
            Expr::Str(_, str_span) if matches!(&self.expected_type, Some(Type::Named(n)) if n == "Sql" || n == "Html" || n == Syntax::TYPE_SH) =>
            {
                let Some(Type::Named(type_name)) = self.expected_type.clone() else {
                    unreachable!()
                };
                let span = *str_span;
                self.rewrite_typed_text_literal(e, type_name, span)
            }
            Expr::Str(parts, str_span) => {
                // D-MEM1/S7 (D-NOALLOC-SEM1=A): interpolation with at least one
                // `{…}` hole builds a fresh `String` (unlike a plain literal
                // with no holes, which is one constant piece of text — not
                // "concatenation/interpolation that produces a new String").
                if self.no_alloc && parts.iter().any(|p| matches!(p, StrPart::Interp(..))) {
                    self.diags.push(no_alloc_violation(
                        "string interpolation allocates a new `String`".to_string(),
                        *str_span,
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
                            match fmt {
                                crate::AST::StrFormat::Display => {
                                    if !is_displayable(&t, self.trait_reg) {
                                        // Migration: auto-printable structs without Display get a lint
                                        // and still compile via jet_show fallback in codegen.
                                        if let Type::Named(n) = &t {
                                            // D-CAPBUNDLE1: a nominal `distinct` type
                                            // starts inert — interpolating one without
                                            // `@Printable` names the granted bundles
                                            // instead of the generic E0915 wording.
                                            if self.registry.is_distinct(n) {
                                                self.diags.push(e0138(
                                                    n,
                                                    "string interpolation",
                                                    "@Printable",
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
                                                    "Display is the user-facing interpolation hook; Debug is for `{value@Debug}`"
                                                        .to_string(),
                                                    format!(
                                                        "add `impl {n}.Display {{ fn display(self) -> String {{ … }} }}`"
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
                                        self.diags.push(Diagnostic::error(
                                            "E0112",
                                            format!("{} can't be shown with @Debug yet", t.show()),
                                            "debug interpolation needs a debuggable value"
                                                .to_string(),
                                            "implement `Debug` or use a debuggable part"
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
            // D-BINPAT1 (card #506 follow-up): byte-mode sibling of the arm
            // above — `b"…"` with holes is legal ONLY as `reader.take_pattern(b"…")`'s
            // argument, intercepted there (`CheckerInfer/calls/method_calls.rs`)
            // before generic inference ever reaches this arm.
            Expr::BinMatchLit(_, span) => {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    "a binary pattern literal is only valid as a `take_pattern` argument".to_string(),
                    "this `b\"…\"` literal has typed holes (`{name:U<width>}`), which only `take_pattern` understands"
                        .to_string(),
                    "call it as `reader.take_pattern(b\"…\")`".to_string(),
                    Some(*span),
                ));
                None
            }
            Expr::Ident(name, span) => {
                // D-PREPOST1 (E0144): `result` names the return value inside a
                // `@Post` condition; at function entry (a `@Pre` condition)
                // there is no return value yet.
                if self.in_pre_clause && name == "result" {
                    self.diags.push(Diagnostic::error(
                        "E0144",
                        "`result` isn't available in a `@Pre` condition".to_string(),
                        "`result` names the return value, which only exists once the function has returned — a `@Pre` condition runs at entry, before there is one".to_string(),
                        "use `result` only in a `@Post` condition".to_string(),
                        Some(*span),
                    ));
                    return None;
                }
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
                            "give away a copy instead (`{} {}`) where it moved",
                            Syntax::KW_COPY,
                            name
                        ),
                        Some(*span),
                    ));
                    self.moved.remove(name); // report once
                    return None;
                }
                // D-UNINIT-SENTINEL1: reading a `:= uninit` binding before it is written.
                if self.uninit.contains_key(name) {
                    self.diags.push(Diagnostic::error(
                        "E0420",
                        format!("`{}` may be read before it is given a value", name),
                        format!(
                            "`{}` was declared `:= uninit`, so it holds no value until you write to it — this read could see garbage",
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
                if self.is_string_view(name) && !self.allow_string_view_read {
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
                self.infer_binary(op, lhs, rhs, span)
            }
            Expr::CompareChain {
                operands,
                ops,
                span,
            } => {
                let span = *span;
                let ops = ops.clone();
                self.infer_compare_chain(operands, &ops, span)
            }
            Expr::Deref(inner, span) => {
                // D-CAP9: postfix `p.*` dereferences a raw pointer — a raw
                // memory access, gated to `#Unsafe`. The result type is the
                // pointer's element type.
                if !self.in_unsafe {
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
                match crate::Sema::ptr_elem(&inner_t) {
                    Some(elem) => Some(elem),
                    None => Some(inner_t),
                }
            }
            Expr::RawOf(inner, span) => {
                // D-CAP9: prefix `*x` takes a raw pointer to `x` (raw-pointer-of),
                // legal only inside `#Unsafe`. Result type is `*T` (`Ptr<T>`).
                if !self.in_unsafe {
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
                if !type_is_copy(&inner_t) && !is_cloneable(&inner_t, self.registry, self.structs) {
                    self.diags.push(Diagnostic::error(
                        "E0211",
                        format!("`{}` can't be copied", inner_t.show()),
                        "copy needs a value made only of duplicable parts; this type holds something Jet can't duplicate — a function value, a trait value, or a type from outside Jet".to_string(),
                        format!(
                            "move it instead (`{}name` if this is its last use), or change the type so every part can be copied",
                            Syntax::SIGIL_MOVE
                        ),
                        Some(*span),
                    ));
                    return None;
                }
                // D-MEM1/S7 (D-NOALLOC-SEM1=A): `copy` of a heap-owning type
                // is itself an allocation (the whole point of `.clone()`-style
                // duplication) — flagged regardless of whether it's cloneable.
                if self.no_alloc && type_owns_heap(&inner_t, self.registry) {
                    self.diags.push(no_alloc_violation(
                        format!(
                            "`copy` of `{}` allocates — it owns heap data",
                            inner_t.show()
                        ),
                        *span,
                    ));
                }
                Some(inner_t)
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
                // D-MEM1/S7 (D-NOALLOC-SEM1=A): `.push`/`.insert` may grow a
                // List/Map's backing heap allocation — capacity headroom isn't
                // statically provable in general, so ANY call of this shape is
                // flagged, full stop (no receiver-type check needed).
                if self.no_alloc && matches!(method.as_str(), "push" | "insert" | "add" | "add_new") {
                    self.diags.push(no_alloc_violation(
                        format!(
                            "`.{}` may allocate to grow this collection's heap allocation",
                            method
                        ),
                        *method_span,
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
                // D-UNINIT1: a `mut` arg is the fill site, not a read.
                self.clear_uninit_mut_args(args);
                self.infer_method_call(
                    receiver,
                    method,
                    *method_span,
                    type_args,
                    args,
                    recv_type,
                    resolved_ret,
                )
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
                    match self.expected_type.clone() {
                        Some(Type::Named(ctx_name)) => {
                            *type_name = ctx_name;
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
                if self.no_alloc && type_owns_heap(&Type::Named(type_name.clone()), self.registry) {
                    self.diags.push(no_alloc_violation(
                        format!(
                            "constructing `{}` here allocates — it owns heap data",
                            type_name
                        ),
                        *span,
                    ));
                }
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
                    let resolved = self.expected_type.clone().and_then(|et| {
                        let name = match &et {
                            Type::Named(n) => Some(n.clone()),
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
                if self.no_alloc && type_owns_heap(&Type::Named(type_name.clone()), self.registry) {
                    self.diags.push(no_alloc_violation(
                        format!(
                            "constructing `{}` here allocates — it owns heap data",
                            type_name
                        ),
                        *span,
                    ));
                }
                Some(self.check_enum_lit(type_name, variant, args, *span))
            }
            // D-TAINT1: `#Tainted expr` — the value-fact tag is type-transparent.
            // Its type is exactly the inner's type; taint propagation + the E0721
            // sink check run in the dedicated taint pass (Sema/Taint.rs), erased
            // in codegen (I3).
            Expr::Tainted(inner, _span) => self.infer(inner),
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
            // D-SIMD2: a reduce-op marker is only meaningful inside `v.reduce(#Op)`.
            // The reduce method consumes it before reaching here; a bare/misplaced
            // marker is a usage error.
            Expr::ReduceMarker(name, span) => {
                self.diags.push(Diagnostic::error(
                    "E2510",
                    format!("`#{}` is only valid inside a lane `.reduce(…)`", name),
                    "a reduce-op marker names the fold operation; it isn't a value on its own"
                        .to_string(),
                    "write `v.reduce(#Add)` / `#Mul` / `#Min` / `#Max`".to_string(),
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
                let expected = self.expected_type.clone();
                self.check_lambda(lam, expected.as_ref())
            }
            Expr::CallValue { callee, args, span } => self.infer_call_value(callee, args, *span),
            Expr::FanOut {
                callee,
                items,
                span,
            } => self.infer_fan_out(callee, items, *span),
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
                        "`$name` splices a value that was computed by a `comptime` binding or `comptime {}` block".to_string(),
                        format!("define `comptime {name} = ...` before using `${name}`"),
                        Some(*span),
                    ));
                }
                None
            }
            // D-FMTPARENS1=A: parenthesized expressions are transparent to type checking.
            Expr::Paren(inner, _) => self.infer(inner),
            // D-INCR1: `++`/`--` on a mutable integer lvalue.
            Expr::IncDec {
                op,
                operand,
                postfix,
                span,
            } => self.check_incdec(*op, operand, *postfix, *span),
        }
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
                "write `[]` only where the list type is already known, like `xs: [Int] :: []`"
                    .to_string(),
                "add a type annotation on the binding".to_string(),
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
                if let Some(t) = self.infer(e) {
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
                    if let Some(t) = self.infer(e) {
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
            match e {
                Expr::Spread(inner, spread_span) => {
                    let t = self.infer(inner);
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
                    if let Some(t) = self.infer(e) {
                        elem_types.push(t);
                    }
                }
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

    pub(crate) fn infer_tuple_lit(
        &mut self,
        fields: &mut [(String, Expr)],
        _span: Span,
    ) -> Option<Type> {
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
        // `print.[a, b, c]` works without triggering E0107. D-PRELUDEX1=A: skipped under `#NoPrelude`.
        if let Expr::Ident(name, _) = callee.as_ref() {
            if name == Syntax::BUILTIN_PRINT && !self.no_prelude {
                self.borrow_ctx = true;
                for item in items.iter_mut() {
                    if let Some(t) = self.infer(item) {
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
            Some(Type::Fn {
                ref params,
                ref ret,
                ..
            }) if params.len() == 1 => (params[0].clone(), ret.as_ref().map(|r| *r.clone())),
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
            Some(Type::FixedList {
                elem: Box::new(elem),
                len,
                len_symbol: None,
            })
        }
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
                "write `[]` only where the map type is already known, like `var m: [String: Int] = []`"
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
            Type::Apply { name, args } if name == "View" && args.len() == 1 => {
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
                    "slicing copies a range (S40)".to_string(),
                    "use `xs[a..b]` on a list or `s.slice(a..b)` on text".to_string(),
                    Some(span),
                ));
                None
            }
        }
    }

    pub(crate) fn infer_field(
        &mut self,
        inner: &mut Box<Expr>,
        member: &str,
        span: Span,
    ) -> Option<Type> {
        if let Expr::Field(base, leaf, _) = &**inner {
            if let Expr::Ident(alias, _) = &**base {
                if self.core_imports.get(alias).map(String::as_str) == Some("core.encoding")
                    && leaf == "DataEvent"
                {
                    **inner = Expr::Ident("DataEvent".to_string(), span);
                    let mut empty = Vec::new();
                    return Some(self.check_enum_lit("DataEvent", member, &mut empty, span));
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
                            effect_bound: None,
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
            // D-DBDRIVER1: `DbValue.Null` — the only zero-arg `DbValue` variant,
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
        }
        self.borrow_ctx = true;
        let t = self.infer(inner)?;
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
