use crate::AST::{BinOp, EnumLitArg, Expr, IndexKind, Lambda, LambdaBody, OrFallback, Pattern, StrPart, Type};
use crate::Codegen::Cx;
use crate::Codegen::is_db_value_type_name;
use crate::Codegen::is_json_type_name;
use crate::Codegen::is_json_variant;
use crate::Codegen::is_key_variant;
use crate::Codegen::TIR::arg_conv_in_subset;
use crate::Codegen::TIR::enum_is_covered;
use crate::Codegen::TIR::foreign_struct_lit_in_subset;
use crate::Codegen::TIR::is_covered_generic_struct_ty;
use crate::Codegen::TIR::is_covered_struct_ty;
use crate::Codegen::TIR::is_numeric_bounds_const;
use crate::Codegen::TIR::is_prelude_struct_name;
use crate::Codegen::TIR::method_call_in_subset;
use crate::Codegen::TIR::stmt_in_subset;
use crate::Codegen::TIR::struct_lit_constructible;
use crate::Syntax;
use std::collections::HashSet;

pub(crate) fn expr_in_subset(e: &Expr, cx: &Cx, locals: &HashSet<String>) -> bool {
    match e {
        Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Char(..) => true,
        Expr::ComptimeSplice { value, .. } => value.is_some(),
        Expr::Str(parts, _) => parts.iter().all(|p| match p {
            StrPart::Lit(_) => true,
            StrPart::Interp(e, _) => expr_in_subset(e, cx, locals),
        }),
        // An ident must resolve to a local/param, OR (c109 Phase 13) be a bare
        // function name used as a VALUE: a non-local, non-const name in `cx.fn_types`
        // with a `Type::Fn` type. The latter emits `emit_named_fn_value`'s
        // `Box::new(move |…| …) as <fn-type>` wrapper. A non-local that is a const
        // (inlined) or an unqualified module import is still out.
        Expr::Ident(name, _) => {
            // c109 Phase 24: a comptime CONST ident (`PAGE_HEADER`) inlines its pre-rendered
            // Rust value at the use site (`cx.consts[name]` — a TOTAL string fact, the same
            // `emit_expr` Ident arm reads). Admit it when it is a known const not shadowed by
            // a local. (A const used as an arithmetic operand resolves to `None` in
            // `ast_operand_is_integer` — `env.ty_of(const)` is `None` — exactly as the AST
            // path's `operand_is_integer`, so the overflow trap is never wrongly claimed.)
            (cx.consts.contains_key(name) && !locals.contains(name))
                || locals.contains(name)
                || ident_is_named_fn_value(name, cx, locals)
        }
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. } => {
            expr_in_subset(inner, cx, locals)
        }
        // D-TAG1: a binding-free variant/group pattern test in EXPRESSION position
        // (`hot :: d == .Fire`, `d == .Fire.Burn` inside `&&`, …) lowers to a plain
        // Bool `matches!` (`TExprKind::PatternMatches`). Only user enums whose
        // owner resolves via `cx.variant_owner` — payload-binding tests stay the
        // if-let condition shape, JSON/Key keep their existing routes.
        Expr::PatternTest {
            subject,
            pattern: Pattern::Variant {
                variant, bindings, ..
            },
            ..
        } if bindings.is_empty()
            && !is_json_variant(variant)
            && !is_key_variant(variant)
            && cx.variant_owner.contains_key(variant) =>
        {
            expr_in_subset(subject, cx, locals)
        }
        Expr::Binary(_, l, r, _) => expr_in_subset(l, cx, locals) && expr_in_subset(r, cx, locals),
        // D-CHAINCMP1: `0 <= sev < 10` — in-subset iff every operand is.
        Expr::CompareChain { operands, .. } => {
            operands.iter().all(|e| expr_in_subset(e, cx, locals))
        }
        Expr::Call(c) => {
            // c109 Phase 13: `f(args)` where `f` is a LOCAL (a fn-typed binding/param)
            // parses as `Expr::Call { name: "f" }`, NOT `Expr::CallValue`. The AST path
            // (`emit_call`, env-contains-name branch) emits `(place)(args)` with args
            // lowered PLAINLY (`emit_call_args(.., None, ..)`). Cover it: the name is a
            // local (not a const) and every arg is in-subset + unlabeled.
            if locals.contains(&c.name) && !cx.consts.contains_key(&c.name) {
                return c
                    .args
                    .iter()
                    .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
            }
            // `print` is the one builtin the subset covers (exactly one arg).
            let is_print = c.name == Syntax::BUILTIN_PRINT
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() == 1;
            // D-LIN1-DROP: `drop(x)` — the discard builtin (exactly one arg, not
            // shadowed by a user `drop` fn or local). Lowers to `TExprKind::Drop`.
            let is_drop = c.name == Syntax::BUILTIN_DROP
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() == 1;
            // c109 Phase 26: the rich-runtime-report builtins `require(cond[, msg])`,
            // `require_eq(left, right)`, and `panic(msg)` (S36). Each is a bare
            // `Expr::Call` whose name is the builtin (not in `cx.sigs`, not shadowed by a
            // local) and whose argument count matches the AST `emit_require`/
            // `emit_require_eq`/`emit_panic_stop` shape. The whole statement string is
            // rendered at lowering (`TExprKind::RequireStop`). Every arg expr (cond/msg/
            // operands) must be in-subset (they are lowered + emitted via the TIR). Sema
            // validated the shape (arg count, `panic`'s 1 message arg). Excluded if a
            // user fn / local shadows the name (then the plain-call branch claims it).
            let is_require = c.name == Syntax::BUILTIN_REQUIRE
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && (c.args.len() == 1 || c.args.len() == 2);
            let is_require_eq = c.name == Syntax::BUILTIN_REQUIRE_EQ
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() == 2;
            let is_panic = c.name == Syntax::BUILTIN_PANIC
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() == 1;
            // c109 Phase 25: the ambient prelude `input(...)` (D-PRELUDE1 = B). A bare
            // `Expr::Call { name: "input" }` with NO user `input` fn (`!cx.sigs`) and no
            // local shadow lowers to the SAME `jet_std_io_input(None|Some(&(arg)))` form
            // as `io.input(...)` (the AST `emit_call` ambient-input branch, Expression.rs
            // ~L1778; sema mirrors it in CheckerInfer + returns `Result<String, IOError>`).
            // 0 args → `(None)`, 1 arg (a String prompt) → `(Some(&(arg)))`. Reproduced
            // byte-for-byte in `emit_tir_ambient_input`. Disjoint from a plain fn (those
            // ARE in `cx.sigs`) and the local-call branch (shadowing local handled above).
            let is_ambient_input = c.name == Syntax::BUILTIN_INPUT
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() <= 1;
            // c109 Phase 28: the overflow opt-out builtins `wrapping(e)`/`saturating(e)`/
            // `checked(e)` (D-NUMOPS1). The AST `emit_call` (Expression.rs ~L1756) claims
            // them when the name is one of the three AND not shadowed by a user fn
            // (`!cx.sigs`); the sole argument is one integer `Expr::Binary` (`+`/`-`/`*`/`/`),
            // lowered to `(ls).{name}_{add|sub|mul|div}(rs)` with PLAIN operands (no trap).
            // Sema validated the shape. The operands must be in-subset; `checked` yields
            // `T?`, the others `T`. Handled by a bespoke `TExprKind::OverflowOpt` — return
            // early here so the generic-call arg machinery below doesn't also claim it.
            if matches!(
                c.name.as_str(),
                Syntax::BUILTIN_WRAPPING | Syntax::BUILTIN_SATURATING | Syntax::BUILTIN_CHECKED
            ) && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
            {
                return matches!(
                    c.args.first().map(|a| &a.expr),
                    Some(Expr::Binary(
                        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div,
                        ..
                    ))
                ) && c.args.len() == 1
                    && c.args
                        .iter()
                        .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
            }
            // Otherwise the callee must be a known *plain* top-level function:
            // in `cx.sigs`, not a local, and NOT an extern/FFI function or an
            // unqualified module import (those lower to different call forms — covered
            // separately below in c109 Phase 14).
            let is_plain_fn = !locals.contains(&c.name)
                && cx.sigs.contains_key(&c.name)
                && !cx.extern_funcs.contains_key(&c.name)
                && !cx.unqualified_inline.contains_key(&c.name)
                && !cx.unqualified_file.contains_key(&c.name);
            // c109 Phase 23: a DISTINCT-type constructor `UserId(expr)` (D-DIST1) is a
            // bare `Expr::Call` whose name is a distinct type (not in `cx.sigs` — so the
            // AST `emit_call` falls through to `user_<Name>(args)` with NO sig, plain args).
            // The TIR's fallthrough `Call` form reproduces that exactly (sig lookup misses
            // → `lower_one_call_arg` with `conv: None` → plain arg, then `user_<Name>(…)`).
            // Sema validated the single-arg base-typed shape (E2 distinct checks); we admit
            // it when the name is a known distinct type, not shadowed by a local.
            let is_distinct_ctor =
                !locals.contains(&c.name) && cx.distinct_types.contains_key(&c.name);
            // D-SIMD2 / D-LINALG1: a built-in math-type constructor `F32x4(a,b,c,d)` /
            // `Vec3(x,y,z)` / `Mat3(…)`. Lowers to the prelude `jet_math_<T>_new(…)`.
            let is_math_ctor = !locals.contains(&c.name)
                && crate::Sema::is_math_type(&c.name)
                && !cx.type_names.contains(&c.name);
            let is_precise_ctor = !locals.contains(&c.name)
                && (c.name == crate::Syntax::TYPE_BIGINT || c.name == crate::Syntax::TYPE_DECIMAL)
                && !cx.type_names.contains(&c.name);
            // D-TYPEDTEXT1=D: the synthetic `Sql`/`Html` call sema rewrote a typed
            // text literal into (see `lower_expr`'s matching case).
            let is_typed_text_ctor = !locals.contains(&c.name)
                && (c.name == "Sql" || c.name == "Html")
                && !cx.type_names.contains(&c.name);
            // c109 Phase 14: FFI extern + unqualified module-import calls are now
            // covered. Each lowers to its own resolved call form (`emit_call`'s
            // `extern_funcs`/`unqualified_inline`/`unqualified_file` arms). The
            // priority MUST match `emit_call`: extern is checked before the unqualified
            // arms, and a LOCAL/print/plain-fn callee was already claimed above. These
            // are all top-level (non-local) names, so they are disjoint from the
            // local-call branch. The extern arg form uses `emit_extern_call_args`
            // (a non-scalar `Read` arg is `(…).clone()`, not `&(…)`) — reproduced in
            // lowering; the Arc (`shared_auto_clone`) form stays excluded.
            let is_extern = !locals.contains(&c.name) && cx.extern_funcs.contains_key(&c.name);
            let is_unqual_inline = !locals.contains(&c.name)
                && !cx.extern_funcs.contains_key(&c.name)
                && cx.unqualified_inline.contains_key(&c.name);
            let is_unqual_file = !locals.contains(&c.name)
                && !cx.extern_funcs.contains_key(&c.name)
                && cx.unqualified_file.contains_key(&c.name);
            // c109 Phase 13: a callee with a **Fn-typed parameter** is now covered.
            // The arg routes through `emit_call_args`'s `Box::new(…) as <fn-type>`
            // coercion (`lower_one_call_arg` reproduces it from total facts). The Fn
            // arg itself must be in-subset (a lambda, a fn-name value, or a fn-typed
            // local). No special exclusion remains — the Box-coercion is total.
            // c109 Phase 23: a call-site LABEL (`f(width: 4.0)`, S61/D-NARG1) is allowed.
            // Labels are checked DOCUMENTATION (D-NARG-D4): sema validates each label names
            // the parameter at its OWN position (E0125) — labels NEVER reorder arguments —
            // and codegen never reads `CallArg.label` (`emit_call_args` is purely
            // positional). So a labeled arg emits byte-identically to an unlabeled one.
            (is_print
                || is_drop
                || is_ambient_input
                || is_require
                || is_require_eq
                || is_panic
                || is_plain_fn
                || is_distinct_ctor
                || is_math_ctor
                || is_precise_ctor
                || is_typed_text_ctor
                || is_extern
                || is_unqual_inline
                || is_unqual_file)
                && c.args.iter().all(|a| {
                    // c109 Phase 6b: a `Shared<T>` arg auto-cloning the Arc
                    // (`shared_auto_clone`) is COVERED for the plain-fn / distinct-ctor /
                    // unqualified-import paths — all route through `lower_one_call_arg`,
                    // which reproduces `emit_call_args`' `Arc::clone(&…)` from the total
                    // flag (and the receiving `Shared<T>` param renders identically via the
                    // shared `rust_param_type`). It stays EXCLUDED on the `is_extern` path
                    // only: extern args use `lower_extern_call_arg`, which does not carry
                    // the Arc form (the FFI boundary takes a `(…).clone()`, not an Arc).
                    // Labels are sema-only (documentation), checked at their own position.
                    (!a.flags.shared_auto_clone || !is_extern)
                        && arg_conv_in_subset(a)
                        && expr_in_subset(&a.expr, cx, locals)
                })
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            if !expr_in_subset(cond, cx, locals) {
                return false;
            }
            let mut then_locals = locals.clone();
            if !then_body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut then_locals))
            {
                return false;
            }
            if !expr_in_subset(then_value, cx, &then_locals) {
                return false;
            }
            let mut else_locals = locals.clone();
            else_body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut else_locals))
                && expr_in_subset(else_value, cx, &else_locals)
        }
        // c109 Phase 3: a struct literal `S { f: v, … }`. Covered only when `S`
        // is a plain user struct the subset lowers, with no trait coercion or
        // cross-module namespace, and every field value is itself in-subset.
        Expr::StructLit {
            type_name,
            type_args,
            import_ns,
            as_trait,
            fields,
            ..
        } => {
            let core_email_struct = import_ns.as_deref().is_some_and(|alias| {
                cx.core_imports.get(alias).map(String::as_str) == Some(crate::Syntax::CORE_EMAIL_MODULE)
                    && matches!(type_name.as_str(), "RecipientReport" | "SendReport")
            });
            // c109 Phase 30: a TRAIT-OBJECT coercion (S48 — `Circle {…}` in a `[Shape]`
            // list). The AST wraps the rendered literal `Box::new(<lit>) as Box<dyn
            // user_<Trait>>` (`emit_struct_lit`'s `as_trait` branch). Covered when the trait
            // is a known user trait and the base is a PLAIN covered user struct (no import_ns,
            // no type_args — a coerced foreign/generic literal is not a construct any covered
            // program produces, so stay conservative and require the plain form). The fields
            // are checked in the plain-struct path below. A coercion to a non-trait name (or a
            // foreign/generic coerced literal) stays excluded.
            if let Some(trait_name) = as_trait {
                if !cx.trait_names.contains(trait_name)
                    || import_ns.is_some()
                    || !type_args.is_empty()
                    || is_prelude_struct_name(type_name)
                    || !is_covered_struct_ty(&Type::Named(type_name.clone()), cx)
                {
                    return false;
                }
                return fields.iter().all(|(_, _, e)| expr_in_subset(e, cx, locals));
            }
            // c109 Phase 19: a FOREIGN (imported user) struct literal — a `import_ns`
            // namespace head (`{root}{mod}::{user_<Name>}[::<args>]`, mangled fields).
            // Covered when the named foreign type is a covered foreign struct and the
            // import alias resolves; the head is resolved at lowering (`lower_expr`).
            if import_ns.is_some() && !core_email_struct {
                return foreign_struct_lit_in_subset(
                    type_name,
                    type_args,
                    import_ns.as_deref(),
                    cx,
                ) && fields.iter().all(|(_, _, e)| expr_in_subset(e, cx, locals));
            }
            // c109 Phase 19: a GENERIC struct literal carries `type_args` (`Pair<T> {…}`
            // → the turbofish `user_Pair::<T> { … }`). The base must be a covered struct
            // and every type arg covered/type-var (`is_covered_generic_struct_ty`). The
            // turbofish head is resolved at lowering via `user_type_apply_rust`.
            if !type_args.is_empty() {
                if !is_covered_generic_struct_ty(
                    &Type::Apply {
                        name: type_name.clone(),
                        args: type_args.clone(),
                    },
                    cx,
                ) {
                    return false;
                }
                return fields.iter().all(|(_, _, e)| expr_in_subset(e, cx, locals));
            }
            // c109: an UNqualified cross-module FOREIGN struct literal (`Note { … }` with
            // no `import_ns` — sema resolves the bare imported type, no `use` of the type
            // needed). The AST `emit_struct_lit` plain branch now prefixes the foreign
            // module (`{root}user_<mod>::user_<Note>`) via `user_type_apply_rust`,
            // reproduced at lowering. Cover it when the type is a registered foreign type
            // (`cx.foreign_types`); the field VALUES are checked in-subset below. (A
            // foreign type is NOT a `is_covered_struct_ty` — its fields live in another
            // module — so this needs its own admission.)
            if cx.foreign_types.contains_key(type_name) {
                return fields.iter().all(|(_, _, e)| expr_in_subset(e, cx, locals));
            }
            // c109 Phase 17: a PRELUDE struct literal (HttpRequest/HttpResponse) — the
            // `is_prelude_struct` branch of `emit_struct_lit` (a `<root>Jet…` head, PLAIN
            // field names, and an auto `params: BTreeMap::new()` for HttpRequest).
            // Reproduced in `lower_expr`'s StructLit arm. Otherwise the named type must be a
            // covered user struct (`user_<name>` head, mangled fields).
            // c109: a recursive (boxed) struct is CONSTRUCTIBLE (the boxed field value is
            // wrapped `Box::new(…)`, a total fact at lowering) even though it is not a
            // covered VALUE type (a boxed field READ needs deref, kept on the AST path).
            if !is_prelude_struct_name(type_name)
                && !is_covered_struct_ty(&Type::Named(type_name.clone()), cx)
                && !struct_lit_constructible(type_name, cx, &mut HashSet::new())
            {
                return false;
            }
            fields.iter().all(|(_, _, e)| expr_in_subset(e, cx, locals))
        }
        // c109 Phase 3: a struct field *read*. A non-Copy owning read was already
        // rewritten to a `.clone()` MethodCall by sema (which the subset excludes,
        // via the `MethodCall` arm below being absent — `_ => false`); what reaches
        // here is a borrow-position read. Cover it when the receiver is in-subset.
        // (`receiver.field` where the receiver is a known module/enum path is not a
        // `Field` value read — sema lowers those to other nodes — so a plain
        // in-subset receiver is the struct-value case.)
        Expr::Field(receiver, member, _) => {
            // `.clone` is never a real field; defensively exclude (sema's synthetic
            // clone is a MethodCall, not a Field, but a user `.clone` field read
            // would collide with the clone-emit special-case in the AST path).
            if member == "clone" {
                return false;
            }
            // c109 Phase 4: a *unit* enum literal reaches codegen as a `Field` whose
            // receiver is the enum-name ident (sema only re-types it; it does NOT
            // rewrite the node — only payload literals become `Expr::EnumLit`). The
            // AST path emits `user_<Enum>::user_<variant>` for this case. Cover it
            // when the enum is a covered scalar-payload enum and `member` is one of
            // its (unit) variants. A receiver that is a known local can't also be a
            // covered enum name, so the two branches never collide.
            if let Expr::Ident(enum_name, _) = receiver.as_ref() {
                let resolved_enum = cx
                    .core_qualified_rust_type_name(enum_name)
                    .unwrap_or(enum_name.as_str());
                if !locals.contains(enum_name)
                    && ((resolved_enum == "SmtpSecurity" && matches!(member.as_str(), "StartTls" | "Tls"))
                        || (resolved_enum == "RecipientPolicy" && matches!(member.as_str(), "RequireAll" | "DeliverAccepted")))
                {
                    return true;
                }
                if enum_name == "DataEvent"
                    && matches!(member.as_str(), "Null" | "ArrayStart" | "ArrayEnd" | "ObjectStart" | "ObjectEnd")
                {
                    return true;
                }
                if !locals.contains(enum_name)
                    && (enum_is_covered(resolved_enum, cx)
                        || crate::Codegen::core_rust_type_name(resolved_enum).is_some())
                    && cx.enum_variants.get(resolved_enum).is_some_and(|variants| {
                        variants.iter().any(|(name, payload)| {
                            name == member && matches!(payload, crate::AST::VariantPayload::Unit)
                        })
                    })
                {
                    return true;
                }
                // c109 Phase 24: the `JSON.Null` unit construction reaches codegen as a
                // `Field` (the AST `emit_expr` Field arm emits `{root}jet_std::Json::Null`,
                // Expression.rs ~L222). Cover it (the only no-arg JSON variant).
                if !locals.contains(enum_name) && is_json_type_name(enum_name) && member == "Null" {
                    return true;
                }
                // D-DBDRIVER1: `DbValue.Null` — the same no-arg-`Field` shape as
                // `JSON.Null` above, for the tagged SQL parameter/column value.
                if !locals.contains(enum_name)
                    && is_db_value_type_name(enum_name)
                    && member == "Null"
                {
                    return true;
                }
                // c109 Phase 28: a numeric BOUNDS constant (`U8.MAX`/`I32.MIN`/
                // `Float.INFINITY`/… — D-NUMOPS1) reaches codegen as a `Field` whose
                // receiver is a numeric type NAME and `member` is one of the per-type
                // const names. The AST `emit_expr` Field arm (Expression.rs ~L224) emits
                // `{rust_type}::{member}` (e.g. `u8::MAX`, `f64::INFINITY`). Cover it: a
                // non-local numeric type name + a known const member. The rendered value
                // + result type are resolved at lowering (`numeric_type_from_name`).
                if !locals.contains(enum_name)
                    && crate::AST::numeric_type_from_name(enum_name).is_some()
                    && is_numeric_bounds_const(member)
                {
                    return true;
                }
                // c109: a comptime-CONST receiver (`comptime P = Pair{…}`; then `P.left`).
                // The const inlines to its pre-rendered Rust value string (`cx.consts[P]`
                // = `user_Pair { … }`) at the use site, and reading a field off it is a
                // plain place read — the AST `emit_expr` Field arm routes the const-ident
                // `inner` through `boxed_field_read`, which calls `emit_expr(Ident)` →
                // `cx.consts[P]`, yielding `((user_Pair { … }).user_<field>)`. The TIR
                // reproduces this exactly (`lower_expr`'s Ident arm already inlines the
                // const string; the Field arm wraps it). A comptime const can hold a
                // struct or enum value; either way the field read is byte-identical.
                if !locals.contains(enum_name) && cx.consts.contains_key(enum_name) {
                    return true;
                }
                // A non-local ident receiver that is NOT a covered enum / comptime const
                // (a core/json/numeric path, an imported namespace, a module alias) is
                // excluded — those use Rust heads/spellings the subset does not emit.
                if !locals.contains(enum_name) {
                    return false;
                }
            }
            // c109: a boxed (recursive) struct field READ (`t.child` where the field is
            // `Box<…>`) is now covered — the read derefs the Box (`(*(…))`, a total
            // `boxed` fact lowered from `cx.boxed_edges`, mirroring the AST `boxed_field_read`).
            // In-subset iff the receiver is.
            expr_in_subset(receiver, cx, locals)
        }
        // c109 Phase 4: an enum literal `Enum.Variant`/`Variant(args)`/named. Covered
        // only when the named enum is a covered scalar-payload enum and every arg
        // value is itself in-subset (a scalar/Char value — the enum being covered
        // already guarantees the payload *types* are scalar, so no clone/box).
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } => {
            let resolved_type = cx
                .core_qualified_rust_type_name(type_name)
                .unwrap_or(type_name.as_str());
            if (resolved_type == "SmtpSecurity" && matches!(variant.as_str(), "StartTls" | "Tls"))
                || (resolved_type == "RecipientPolicy" && matches!(variant.as_str(), "RequireAll" | "DeliverAccepted"))
            {
                return args.is_empty();
            }
            // D-TERM1 (ratified 2026-06-22): `Key` is a core prelude enum, not in
            // the user registry, but is always covered — all payloads are scalar/Char.
            let key_type = crate::Syntax::TYPE_KEY;
            if type_name == key_type {
                if !is_key_variant(variant) {
                    return false;
                }
                return args.iter().all(|a| match a {
                    EnumLitArg::Positional(e) => expr_in_subset(e, cx, locals),
                    EnumLitArg::Named { expr, .. } => expr_in_subset(expr, cx, locals),
                });
            }
            // D-PROCESS1=A: `ProcessStreamMode` is a core dot-literal enum, always
            // covered — all three variants are unit (no payload args to check).
            if type_name == "ProcessStreamMode" {
                return matches!(variant.as_str(), "Stream" | "Inherit" | "Capture");
            }
            // D-TEXTWIDTH1=B: `TextWidth`'s two field enums, always
            // covered — every variant is unit (no payload args to check).
            if type_name == "TextWidthAmbiguous" {
                return matches!(variant.as_str(), "Narrow" | "Wide");
            }
            if type_name == "TextWidthControls" {
                return matches!(variant.as_str(), "Zero" | "Reject");
            }
            if type_name == "NetShutdown" {
                return matches!(variant.as_str(), "Read" | "Write" | "Both");
            }
            if type_name == "NetReadyInterest" {
                return matches!(variant.as_str(), "Read" | "Write" | "ReadWrite");
            }
            if !enum_is_covered(resolved_type, cx)
                && !(crate::Codegen::core_rust_type_name(resolved_type).is_some()
                    && cx.enum_variants.contains_key(resolved_type))
            {
                return false;
            }
            // Defensive: the variant must belong to this enum (sema guaranteed it).
            if !cx.enum_variants.get(resolved_type).is_some_and(|variants| {
                variants.iter().any(|(candidate, _)| candidate == variant)
            }) {
                return false;
            }
            args.iter().all(|a| match a {
                EnumLitArg::Positional(e) => expr_in_subset(e, cx, locals),
                EnumLitArg::Named { expr, .. } => expr_in_subset(expr, cx, locals),
            })
        }
        // c109 Phase 5: a list literal `[a, b, c]`. Covered when every element is
        // itself in-subset. (An empty `[]` has no elements; sema requires a context
        // type — E0501 — which a covered binding/param/return supplies, so the
        // resulting `vec![]` is type-inferred by Rust from that context.)
        Expr::ListLit(elems, _) => elems.iter().all(|e| expr_in_subset(e, cx, locals)),
        // D-VARIADIC1: list/call spread — covered when the spread operand is in-subset.
        Expr::Spread(inner, _) => expr_in_subset(inner, cx, locals),
        // c109 Phase 23: a named-tuple literal `(x: 1, y: 2)` (S73/D-SG7). Covered when
        // sema resolved the tuple TYPE (`ty.is_some()` — the canonical field order +
        // struct name come from it; an unresolved `ty` would force the AST's empty-
        // canonical `0i64` default, which the TIR must not guess) and every field value
        // is in-subset. The literal's values are reordered to the type's canonical field
        // order at lowering, reproducing `emit_expr`'s `TupleLit` arm.
        Expr::TupleLit(fields, _, ty) => {
            matches!(ty, Some(Type::Tuple(_)))
                && fields.iter().all(|(_, e)| expr_in_subset(e, cx, locals))
        }
        // c109 Phase 5: a map literal `[k: v, …]` / `[:]`. Covered when every key
        // and value is in-subset. The empty `[:]` (no entries) is always covered.
        Expr::MapLit(entries, _) => entries
            .iter()
            .all(|(k, v)| expr_in_subset(k, cx, locals) && expr_in_subset(v, cx, locals)),
        // c109 Phase 5: indexing `coll[i]`. The `IndexKind` must be sema-resolved
        // (not `Unknown`) so the helper dispatch (`jet_index_map`/`jet_index_vec`)
        // is a total fact carried onto the TIR. Base + index must be in-subset.
        Expr::Index {
            base, index, kind, ..
        } => {
            !matches!(kind, IndexKind::Unknown)
                && expr_in_subset(base, cx, locals)
                && expr_in_subset(index, cx, locals)
        }
        // c109 Phase 5: an inclusive copy slice `coll[a..b]` (lists only — the AST
        // path's `jet_slice_vec` is list-specific). Base/start/end must be in-subset.
        Expr::Slice {
            base, start, end, ..
        } => {
            expr_in_subset(base, cx, locals)
                && expr_in_subset(start, cx, locals)
                && expr_in_subset(end, cx, locals)
        }
        // c109 Phase 6: a method call. Covered in exactly two shapes:
        //   (a) the sema-inserted `.clone()` (an owning non-Copy field read /
        //       borrowed value in owning position) — `(recv).clone()`;
        //   (b) a user-defined instance method on a covered struct/enum type
        //       (`recv_type` is `Some(T)`, `(T, method)` ∈ `method_sigs`, and the
        //       method name is NOT one a core/stdlib/builtin lowering intercepts).
        // Everything else (core/stdlib/collection/string/numeric methods, static
        // calls — whose `recv_type` is `None` — fallible/optional, fan-out, …) stays
        // on the AST path.
        Expr::MethodCall {
            receiver,
            method,
            args,
            recv_type,
            ..
        } => method_call_in_subset(receiver, method, args, recv_type, cx, locals),
        // D-TAINT1: `#Tainted expr` — the tag is erased; in-subset iff the inner is.
        Expr::Tainted(inner, _) => expr_in_subset(inner, cx, locals),
        // c109 Phase 8: optional constructors `value(x)` / `null`. Covered when the
        // inner value (if any) is in-subset — they lower to `Some(x)` / `None`.
        Expr::Present(inner, _) => expr_in_subset(inner, cx, locals),
        Expr::Absent(_) => true,
        // D-SIMD2: a reduce-op marker `#Op`. Only appears inside `v.reduce(#Op)`; the
        // method lowering consumes it (it never emits on its own), so it is in-subset.
        Expr::ReduceMarker(_, _) => true,
        // c109 Phase 23: a `#Todo` typed hole. Covered when sema filled the expected
        // type (`expected_type.is_some()`); a `None` (sema didn't run/resolve) stays on
        // the AST path so the TIR never guesses the `(unknown)` fallback.
        Expr::Todo { expected_type, .. } => expected_type.is_some(),
        // c109 Phase 8: fallible constructors `ok(x)` / `err(e)`. Covered when the
        // inner value is in-subset — they lower to `Ok(x)` / `Err(e)`.
        Expr::Ok(inner, _) | Expr::Err(inner, _) => expr_in_subset(inner, cx, locals),
        // c109 Phase 8: the `?` propagation operator. The `TryConvert` decision is a
        // total sema fact (`None`/`Fallible`/`Typed(fn)`), reproduced verbatim. The
        // inner fallible value must itself be in-subset (a user fallible fn call, a
        // local, an `ok`/`err` literal). A core/stdlib fallible call (e.g. `fs.read`)
        // is NOT in-subset (it stays on the AST path — Phase 10), so a `?` on one is
        // excluded automatically.
        Expr::Try(inner, _, _) => expr_in_subset(inner, cx, locals),
        // c109 Phase 8: the `??` fallback operator. `is_option` is total. The value
        // and the fallback must be in-subset. The Panic fallback form is deferred
        // (its `safe_locals_expr` reproduction is out of subset) — only Value and
        // early-`return` fallbacks are covered.
        Expr::OrFallback {
            value, fallback, ..
        } => expr_in_subset(value, cx, locals) && orfallback_rhs_in_subset(fallback, cx, locals),
        // c109 Phase 8: optional chaining `base?.member`. The `flatten` fact is total
        // (from sema). The base must be in-subset; the member read lowers to a plain
        // `.map`/`.and_then` closure access (no further dispatch).
        Expr::OptField { base, .. } => expr_in_subset(base, cx, locals),
        // c109 Phase 11: a lambda/closure literal. Covered when its body is in-subset
        // (lowered on the outer scope extended with the lambda's params + cloned
        // captures) and every capture/escape decision is a total `Lambda.meta` fact.
        Expr::Lambda(lam) => lambda_in_subset(lam, cx, locals),
        // c109 Phase 11: fan-out `f.[a, b, c]` (S75/S76). Covered when the callee is
        // in-subset (a plain top-level fn ident, or any in-subset callee value) and
        // every item is in-subset.
        Expr::FanOut { callee, items, .. } => {
            fan_out_callee_in_subset(callee, cx, locals)
                && items.iter().all(|i| expr_in_subset(i, cx, locals))
        }
        // c109 Phase 13: a call THROUGH a fn-value `(f)(args)` (`Expr::CallValue`).
        // Covered when the callee is in-subset (a fn-typed local, a fn-name value, or
        // a lambda) and every arg is in-subset. The AST path emits `({callee})({args})`
        // with args lowered plainly (`emit_call_args(.., None, ..)`), so no convention
        // facts are needed — any in-subset arg works; labels are still excluded.
        Expr::CallValue { callee, args, .. } => {
            expr_in_subset(callee, cx, locals)
                && args
                    .iter()
                    .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals))
        }
        // c109 Phase 18: `mem.Ptr<T>.from_addr(addr)` (`Expr::PtrFromAddr`, S58). The
        // address expr must be in-subset. The cast itself is safe Rust (no `unsafe`); it
        // is only constructible inside `use core.mem` + an `#Unsafe` region (sema
        // E3101/E3102), so it never appears in a non-unsafe context. `elem` is a total
        // type on the node — emit needs no inference.
        Expr::PtrFromAddr { addr, .. } => expr_in_subset(addr, cx, locals),
        // D-CAP9: postfix `p.*` deref and prefix `*x` raw-of. Both only appear
        // inside `use core.mem` + an `#Unsafe` region (sema-gated by E0208); the
        // deref/cast forms are byte-for-byte the AST path (no convention facts).
        Expr::Deref(inner, _) | Expr::RawOf(inner, _) => expr_in_subset(inner, cx, locals),
        // D-CAP2 (D-MEM1/S4): `copy x` — in-subset whenever `x` is.
        Expr::Copy(inner, _) => expr_in_subset(inner, cx, locals),
        Expr::Paren(inner, _) => expr_in_subset(inner, cx, locals),
        // Everything else (tuples, …) is out.
        _ => false,
    }
}

/// c109 Phase 13: is `name` a bare top-level function used as a VALUE? It must be a
/// non-local, non-const name in `cx.fn_types` whose type is a `Type::Fn`. Such a name
/// emits `emit_named_fn_value`'s `Box::new(move |…| user_<name>(…)) as <fn-type>`
/// (Source/Codegen/Statement.rs). A const (inlined value) or an unqualified module
/// import is NOT a fn-value, so this stays narrow.
pub(crate) fn ident_is_named_fn_value(name: &str, cx: &Cx, locals: &HashSet<String>) -> bool {
    !locals.contains(name)
        && !cx.consts.contains_key(name)
        && matches!(cx.fn_types.get(name), Some(Type::Fn { .. }))
}

/// c109 Phase 8/15: is a `??` fallback right-hand side in-subset? `Value` and early
/// `return [expr]` are covered (Phase 8). c109 Phase 15: the `panic(…)` form is now
/// covered too — `emit_panic_stop`/`safe_locals_expr` is resolved from the lexical
/// lowering environment at the panic site. The panic message expression must
/// be in-subset (it is lowered into the rendered panic string). `panic(…)` always takes
/// exactly one message argument (the parser builds `OrFallback::Panic{args}` from it).
pub(crate) fn orfallback_rhs_in_subset(
    fallback: &OrFallback,
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    match fallback {
        OrFallback::Value(e) => expr_in_subset(e, cx, locals),
        OrFallback::Return(None, _) => true,
        OrFallback::Return(Some(e), _) => expr_in_subset(e, cx, locals),
        OrFallback::Panic { args, .. } => {
            args.len() == 1 && args[0].label.is_none() && expr_in_subset(&args[0].expr, cx, locals)
        }
        OrFallback::Break(_) | OrFallback::Continue(_) => true,
    }
}

/// c109 Phase 11: is a lambda/closure literal in-subset? The body must be entirely
/// in-subset when classified against the outer scope extended with the lambda's
/// params (new locals) and its captures. The capture/escape/Fn-vs-FnMut facts are
/// all total (`Lambda.meta`), so nothing is re-derived; the gate only proves the
/// body lowers. A `take_names` capture is an outer local (already in `locals`); a
/// param shadows. The body sees: outer locals (captures resolve via them — the AST
/// rebinds a cloned capture to `_jet_cap_<n>` but the *name* stays in scope) plus
/// the params.
pub(crate) fn lambda_in_subset(lam: &Lambda, cx: &Cx, locals: &HashSet<String>) -> bool {
    let mut body_locals = locals.clone();
    for p in &lam.params {
        body_locals.insert(p.name.clone());
    }
    match &lam.body {
        LambdaBody::Expr(e) => expr_in_subset(e, cx, &body_locals),
        LambdaBody::Block(stmts) => stmts
            .iter()
            .all(|s| stmt_in_subset(s, cx, &mut body_locals)),
    }
}

/// c109 Phase 11: is a fan-out callee (`f` in `f.[a, b, c]`) in-subset? The AST
/// path routes an `Ident` callee through `emit_call` (handling builtins) and any
/// other callee through `(f)(item)` (a fn-value call). We cover ONLY the cleanest,
/// byte-reproducible case: an `Ident` that resolves to a *plain top-level function*
/// (in `cx.sigs`, not a local, not an extern/FFI or unqualified-module-import call,
/// not a builtin like `print`/`panic`). Those lower exactly as the Phase-1 `Call`
/// arm does (a synthetic single-arg call). A fn-value callee (`(f)(item)`) needs the
/// deferred Fn-typed-value emit, so it stays on the AST path.
pub(crate) fn fan_out_callee_in_subset(callee: &Expr, cx: &Cx, locals: &HashSet<String>) -> bool {
    let Expr::Ident(name, _) = callee else {
        return false;
    };
    !locals.contains(name)
        && cx.sigs.contains_key(name)
        && !cx.extern_funcs.contains_key(name)
        && !cx.unqualified_inline.contains_key(name)
        && !cx.unqualified_file.contains_key(name)
        // Exclude the ambient builtins `emit_call` special-cases before the plain
        // dispatch (a user-defined fn of the same name is in `cx.sigs`, so the
        // `contains_key` above already admits it — but a bare builtin name with no
        // user sig would have failed `contains_key`; guard anyway for clarity).
        && name != Syntax::BUILTIN_PRINT
        && name != Syntax::BUILTIN_PANIC
        && name != Syntax::BUILTIN_INPUT
        && name != Syntax::BUILTIN_REQUIRE
        && name != Syntax::BUILTIN_REQUIRE_EQ
        && name != Syntax::BUILTIN_EXPECT
        && name != Syntax::BUILTIN_WRAPPING
        && name != Syntax::BUILTIN_SATURATING
        && name != Syntax::BUILTIN_CHECKED
}
