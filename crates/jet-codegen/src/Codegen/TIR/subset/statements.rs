use crate::AST::{BinOp, BindPattern, ElseBranch, Expr, ForKind, IfStmt, IndexKind, LValue, PatSlot, Pattern, Stmt, SwitchArm};
use crate::Codegen::Cx;
use crate::Diagnostics::Span;
use crate::Codegen::is_json_variant;
use crate::Codegen::is_key_variant;
use crate::Codegen::TIR::add_pattern_binding_names;
use crate::Codegen::TIR::add_bin_match_pattern_binding_names;
use crate::Codegen::TIR::add_str_match_pattern_binding_names;
use crate::Codegen::TIR::add_struct_pattern_binding_names;
use crate::Codegen::TIR::arm_fallible_pattern;
use crate::Codegen::TIR::arm_head_range;
use crate::Codegen::TIR::arm_is_plain_cond;
use crate::Codegen::TIR::arm_bin_match_pattern;
use crate::Codegen::TIR::arm_str_match_pattern;
use crate::Codegen::TIR::arm_struct_pattern;
use crate::Codegen::TIR::arm_variant_pattern;
use crate::Codegen::TIR::enum_is_covered;
use crate::Codegen::TIR::expr_in_subset;
use crate::Codegen::TIR::fallible_pattern_binding;
use crate::Codegen::TIR::struct_pattern_values_in_subset;
use crate::Codegen::TIR::variant_pattern_enum;
use crate::Syntax;
use std::collections::HashSet;

fn scoped_stmts_in_subset(
    body: &[Stmt],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    let mut scoped = locals.clone();
    body.iter().all(|s| stmt_in_subset(s, cx, &mut scoped))
}

/// `locals` is the set of names bound as params/locals so far in this scope.
/// It is threaded so an `Expr::Ident` can be classified: a name that is not a
/// local must not be a const/fn-value (excluded). Bindings extend it in order.
pub(crate) fn stmt_in_subset(s: &Stmt, cx: &Cx, locals: &mut HashSet<String>) -> bool {
    match s {
        Stmt::Val(b) => {
            // c109 Phase 19: an `arena_view` binding (`x :: arena.alloc(v)` / `x ::
            // arena.alloc(v)`) IS covered — it lowers to a plain `let <x> = <init>;` (no
            // type, no mut) with a deref'd slot, exactly as `emit_let`'s `arena_view`
            // branch (the init is a covered `arena.alloc(v)` handle call). The escape/
            // use-after-reset rules (E0631/E0632) are enforced entirely in sema.
            match &b.pattern {
                // c109 Phase 23: a TUPLE-destructuring binding `(a, b) :: <init>` (S74,
                // `BindPattern::Tuple`). The AST `emit_stmt` borrows the init into a temp,
                // then binds each name from `(tmp).user_<canonical-field>.clone()` (pairing
                // elems to the type's canonical fields BY POSITION). Covered when the init
                // is in-subset (its lowered `.ty` is a `Type::Tuple` — sema guarantees a
                // tuple pattern destructures a tuple value, so the canonical field names
                // are total at lowering). The Struct/List destructure forms stay on the
                // AST path (no live-suite use; can be a later slice).
                Some(BindPattern::Tuple { elems, .. }) => {
                    let ok = !b.is_comptime && !b.uninit && expr_in_subset(&b.init, cx, locals);
                    for e in elems {
                        locals.insert(e.name.clone());
                    }
                    ok
                }
                // c109 Phase 26: a LIST-destructuring binding `[a, b, c] :: <init>` (S74,
                // `BindPattern::List`). The AST `emit_stmt` borrows the init into a temp,
                // then binds each name via `jet_unpack_vec(tmp, want, i, file, line)`
                // (a runtime bounds-checked element move). Covered when the init is
                // in-subset; the element type partiality (`expr_jet_ty`'s
                // `Some(List(inner))`-only match) is reproduced at lowering. The
                // fan-out result-list destructure (`41_fan_out` `main`) is exactly this.
                Some(BindPattern::List { elems, .. }) => {
                    let ok = !b.is_comptime && !b.uninit && expr_in_subset(&b.init, cx, locals);
                    for e in elems {
                        locals.insert(e.name.clone());
                    }
                    ok
                }
                // c109: a STRUCT-destructuring binding `Type { x, y } :: <init>`
                // (S74, `BindPattern::Struct`). The AST `emit_stmt` borrows the init
                // into a temp, then binds each field via `(tmp).user_<field>.clone()`
                // (the pattern's field name is both the bound local and the read).
                // Covered when the init is in-subset; the per-field type comes from
                // `cx.struct_fields` at lowering (total — sema proved the pattern
                // destructures a struct value).
                Some(BindPattern::Struct { fields, .. }) => {
                    let ok = !b.is_comptime && !b.uninit && expr_in_subset(&b.init, cx, locals);
                    for f in fields {
                        // D-DESTRUCT1: the LOCAL (possibly renamed) name is what's
                        // in scope, not the source field name.
                        locals.insert(f.local_name().to_string());
                    }
                    ok
                }
                // Forward-safety default: a future BindPattern variant defaults to the
                // safe exclusion. Currently unreachable — Tuple/List/Struct are all matched.
                #[allow(unreachable_patterns)]
                Some(_) => false,
                None => {
                    // D-UNINIT1 engine, reused unchanged by D-UNINIT-SENTINEL1: a
                    // `:= uninit` binding needs no init expression to be in-subset —
                    // lower.rs emits `MaybeUninit::uninit().assume_init()` verbatim
                    // (the placeholder `Expr::Int(0, …)` init is never evaluated or
                    // lowered).
                    //
                    // c109 (S57/M9.5): a comptime LOCAL `comptime name = expr`. Sema
                    // evaluates the value into `b.ct` and the AST `emit_let` emits it as
                    // literal data (`let <name>[: <ty>] = <ct.serialize()>;`) — the runtime
                    // `init` expr is NEVER emitted, so it need not be in-subset. Covered
                    // whenever the resolved value is present (`b.ct.is_some()`).
                    // `b.uninit` cannot co-occur with comptime or pattern.
                    let ok = if b.uninit {
                        true
                    } else if b.is_comptime {
                        b.ct.is_some()
                    } else {
                        expr_in_subset(&b.init, cx, locals)
                    };
                    // The binding's name is in scope for subsequent statements.
                    locals.insert(b.name.clone());
                    ok
                }
            }
        }
        Stmt::Assign { target, value, .. } => match target {
            LValue::Local { .. } => expr_in_subset(value, cx, locals),
            // c109 Phase 5: indexed assignment `coll[i] = v`. The base, index, and
            // value must all be in-subset; the `IndexKind` (List/Map) is carried
            // totally from sema and dispatched at lowering (never re-inferred). An
            // `IndexKind::Unknown` means sema did not resolve it — exclude (the AST
            // path falls back to an env type-inference the TIR must not reproduce).
            LValue::Index {
                base, index, kind, ..
            } => {
                !matches!(kind, IndexKind::Unknown)
                    && expr_in_subset(base, cx, locals)
                    && expr_in_subset(index, cx, locals)
                    && expr_in_subset(value, cx, locals)
            }
            // D-MUTSELF1: a field-assignment `place.field = v`. The base place (a
            // field-read expr, e.g. `self`) and the value must both be in-subset; the
            // place is rendered through the same field-read lowering, so its
            // resolution is total. Compound ops (`+=`, S17) ride the same path.
            LValue::Field { base, .. } => {
                expr_in_subset(base, cx, locals) && expr_in_subset(value, cx, locals)
            }
        },
        Stmt::Return(Some(e), _) => expr_in_subset(e, cx, locals),
        Stmt::Return(None, _) => true,
        // D-STREAMYIELD1: `yield e` inside a generator.
        Stmt::Yield(e, _) => expr_in_subset(e, cx, locals),
        // D-IGNORERET2=A: `.drop("reason")` lowers to an ExprStmt of the receiver;
        // the method call itself is erased. Covered iff the receiver is in-subset.
        Stmt::Expr(Expr::Call(call)) if call.name == Syntax::INTERNAL_DEFER_CLOSE => call
            .args
            .first()
            .is_some_and(|arg| expr_in_subset(&arg.expr, cx, locals)),
        Stmt::Expr(Expr::MethodCall {
            receiver, method, ..
        }) if method == Syntax::METHOD_DROP => expr_in_subset(receiver, cx, locals),
        Stmt::Expr(e) => expr_in_subset(e, cx, locals),
        Stmt::If(ifs) => if_in_subset(ifs, cx, locals),
        // c109 Phase 2: control-flow loops. Each loop body is its own scope; check
        // it on a clone so a `let` inside the loop doesn't leak past it.
        Stmt::Loop { body, .. } => {
            let mut body_locals = locals.clone();
            body.iter().all(|s| stmt_in_subset(s, cx, &mut body_locals))
        }
        Stmt::While { cond, body, .. } => {
            if !expr_in_subset(cond, cx, locals) {
                return false;
            }
            let mut body_locals = locals.clone();
            body.iter().all(|s| stmt_in_subset(s, cx, &mut body_locals))
        }
        // D-LOOP-SEMICOLON1=A: init var is in scope for cond, step, and body.
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            if !expr_in_subset(&init.init, cx, locals) {
                return false;
            }
            let mut inner = locals.clone();
            inner.insert(init.name.clone());
            if !expr_in_subset(cond, cx, &inner) {
                return false;
            }
            if !stmt_in_subset(step.as_ref(), cx, &mut inner) {
                return false;
            }
            body.iter().all(|s| stmt_in_subset(s, cx, &mut inner))
        }
        Stmt::For {
            var,
            var2,
            kind,
            body,
            ..
        } => match kind {
            // `loop i in start..end [step k]` — start/end/step must be in-subset
            // integer expressions; the loop var `i` is an Int local in the body.
            // The two-binding `key, value` form is map iteration (a collection),
            // outside this phase.
            ForKind::Range { start, end, step } if var2.is_none() => {
                if !expr_in_subset(start, cx, locals) || !expr_in_subset(end, cx, locals) {
                    return false;
                }
                if let Some(st) = step {
                    if !expr_in_subset(st, cx, locals) {
                        return false;
                    }
                }
                let mut body_locals = locals.clone();
                body_locals.insert(var.clone());
                body.iter().all(|s| stmt_in_subset(s, cx, &mut body_locals))
            }
            // c109 Phase 5/22: `loop x in coll` / `loop k, v in map` (ForKind::In).
            // A method-call collection (`.chars()`/`.lines()`/`.split(…)`) takes a
            // distinct `emit_for_in` branch; Phase 22 reproduces each (`forin_method_
            // collection_in_subset`). A non-method-call collection is the plain
            // `.iter()` form (single- or two-binding map). The loop var(s) bind in the
            // body scope with an *unresolved* type (matching the AST slot's `jet_ty:
            // None`, so they never enable the overflow trap).
            ForKind::In { collection } => {
                // The TWO-BINDING map form (`loop k, v in map`) ALWAYS emits
                // `({coll}).iter()` (the `var2` branch of `emit_for_in` fires first,
                // before the `.chars()`/`.lines()` method-call branches). So a method-call
                // collection in the two-binding position (notably an owning-field-read
                // `idx.notes` that sema rewrote to `idx.notes.clone()` — the Phase-3
                // finding) is just a plain in-subset collection value, not one of the
                // single-binding `chars`/`lines` special forms. Check it as a plain
                // expr. The SINGLE-binding form keeps the method-call classification
                // (`.chars()`/`.lines()`/`.split(…)` → `forin_method_collection_in_subset`).
                if var2.is_some() {
                    if !expr_in_subset(collection, cx, locals) {
                        return false;
                    }
                } else if let Expr::MethodCall { .. } = collection {
                    // A single-binding method-call collection: the form must be one
                    // `emit_for_in` reproduces (`chars`/`lines`/`.iter().cloned()` default).
                    if !forin_method_collection_in_subset(collection, cx, locals) {
                        return false;
                    }
                } else if !expr_in_subset(collection, cx, locals) {
                    return false;
                }
                let mut body_locals = locals.clone();
                body_locals.insert(var.clone());
                if let Some((v2, _)) = var2 {
                    body_locals.insert(v2.clone());
                }
                body.iter().all(|s| stmt_in_subset(s, cx, &mut body_locals))
            }
            // The Range form with a second binding (`k, v in a..b`) is not a Jet
            // construct; stay on the AST path defensively.
            _ => false,
        },
        // `break`/`continue`, labeled or not, carry no sub-expressions to check.
        // The parser only admits them inside a loop body, so they are always valid
        // where they appear; the label name is reproduced verbatim at lowering.
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => true,
        // c109 Phase 4: a `when`/match (`Stmt::Switch`). Covered only in the two
        // shapes the TIR reproduces exactly — an exhaustive enum match or an
        // all-range-arm scalar switch (see `switch_in_subset`).
        Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        } => switch_in_subset(subject, arms, else_body, *span, cx, locals),
        // D-CTMARKER1 (ratified 2026-06-25, piece 2): `comptime { … }` erases entirely.
        // Always "in subset" since it emits nothing in Rust (I3).
        Stmt::ComptimeBlock { .. } => true,
        // Scope classification mirrors lowering: an emitted Rust block gets a cloned
        // locals set. Selected comptime-if, `layout`, and `.setup` are the only statement
        // bodies emitted inline, so only those intentionally extend `locals`.
        // c109 Phase 15: a resolved comptime-if (`Stmt::ComptimeIf`). Sema picks the
        // branch (`selected_then`); codegen emits ONLY that branch's statements inline.
        // The gate must classify the SELECTED branch (the unselected one is dropped and
        // never reaches codegen — it is name-resolution-only, D-WHEN2). Its statements
        // leak into the outer scope (the AST shares `&mut env`), so they extend `locals`.
        // Before sema resolves `selected_then` (a `build_cx`-only gate test), default to
        // the `then` branch so the gate is still exercised; at real codegen
        // `selected_then` is always set.
        Stmt::ComptimeIf {
            then_body,
            else_body,
            selected_then,
            ..
        } => {
            let chosen: &[Stmt] = match selected_then {
                Some(true) | None => then_body,
                Some(false) => else_body.as_deref().unwrap_or(&[]),
            };
            chosen.iter().all(|s| stmt_in_subset(s, cx, locals))
        }
        // c109 Phase 18: an audited `@Unsafe { … }` gate region (`Stmt::Unsafe`). It
        // emits a Rust lexical block, so body declarations stay in a child locals set.
        // The `#Audit("…")` annotation emits
        // nothing. I1: this is the source gate — the only place a Rust `unsafe` block is
        // produced — so admitting it here cannot introduce an ungated `unsafe`.
        Stmt::Unsafe { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        // D-CTEFFECT1: `@Impure` erases to a plain block at codegen (I3).
        Stmt::Impure { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        // Reactive bodies emit as closures, another lexical boundary.
        Stmt::Reactive { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        // D-SHIELDNAME1=A: runtime enter/RAII-leave guards wrap a lexical block.
        Stmt::Shield { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        // D-CANVASSTATE1=D: `@Off` erases; `@DebugOnly` lowers in a lexical
        // debug-only block, so its local declarations do not extend `locals`.
        Stmt::Off { .. } => true,
        Stmt::DebugOnly { body, .. } => {
            let mut scoped = locals.clone();
            body.iter().all(|s| stmt_in_subset(s, cx, &mut scoped))
        }
        // Plain Rust lexical blocks.
        Stmt::Region { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        Stmt::Policy { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        Stmt::TaskGroup { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        // D-LAYOUT1 / D-LAYOUT-GATES1: `layout name { … }`. UNLIKE `Region`/
        // `TaskGroup`, `name` is a REAL runtime binding that must stay valid
        // for statements AFTER this one (`lower_stmt`/`TStmt::Layout` binds
        // it before lowering the body) — register it into `locals` here too,
        // so a later `form.value(…)` etc. is itself recognized as in-subset.
        Stmt::Layout { name, body, .. } => {
            locals.insert(name.clone());
            body.iter().all(|s| stmt_in_subset(s, cx, locals))
        }
        // c109 Phase 19: a `@Context(field: value) { … }` block (D-CTX1) — a plain block
        // with a per-field guard. Field values use the outer scope; the body is lexical.
        Stmt::ContextBlock { fields, body, .. } => {
            fields.iter().all(|(_, v, _)| expr_in_subset(v, cx, locals))
                && scoped_stmts_in_subset(body, cx, locals)
        }
        // c109 Phase 26: a `@Caps(Io) { … }` effect-restriction region (D-EFF1/D-QUAL1)
        // erases to a plain Rust block — `emit_stmt`'s `Stmt::Caps` arm is byte-for-byte
        // identical to `Stmt::Region` (`{ <body> }`). The cap set is enforced entirely
        // in sema (E0741); codegen is dumb (I3).
        Stmt::Caps { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        // D-SCAP1: a `#grant(Fs) { caps -> … }` scoped-capability grant erases to a
        // plain Rust block (the grant/revoke is a compile-time capability fact, I3).
        // The capability handle is sema-only — it is NOT emitted, so the body lowers
        // exactly like a lexical `Stmt::Region`.
        Stmt::Grant { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        // D-TERM1 (ratified 2026-06-22): `live { … }` lowers to a guarded Rust block.
        Stmt::Live { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        // D-DOTSCOPE1: a `@Test` scope member — in-subset iff its region body is.
        // Args are literals folded at lowering, not lowered as exprs, so only the
        // body gates the subset. `.setup` emits inline and intentionally extends the
        // test scope; every other member emits a Rust lexical block.
        Stmt::ScopeMember { name, body, .. } if name == Syntax::SCOPE_TEST_SETUP => {
            body.iter().all(|s| stmt_in_subset(s, cx, locals))
        }
        Stmt::ScopeMember { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        // D-DET1: `assume_deterministic { … }` erases to a plain Rust block (the
        // determinism suspension is a compile-time fact, I3).
        Stmt::AssumeDet { body, .. } => scoped_stmts_in_subset(body, cx, locals),
        // D-TXN1–D-TXN4 (ratified 2026-06-24): `@Transact(name) { … }` lowers to a
        // transaction-guarded Rust block. The handle `name` is a covered local
        // inside the body (so `name.on_commit(…)` resolves); check the body with it
        // in scope.
        Stmt::Transact { name, body, .. } => {
            let mut inner = locals.clone();
            if let Some(name) = name {
                inner.insert(name.clone());
            }
            body.iter().all(|s| stmt_in_subset(s, cx, &mut inner))
        }
        // Forward-safety default: a future Stmt variant defaults to the safe AST path
        // (I2 — a false negative keeps a fn off the TIR; a false positive would be unsafe).
        // Currently unreachable because every variant above is matched.
        #[allow(unreachable_patterns)]
        _ => false,
    }
}

/// c109 Phase 22: is a method-call collection iteration (`loop x in <coll>` where
/// `<coll>` is an `Expr::MethodCall`) in-subset? Mirrors `emit_for_in`'s
/// `Expr::MethodCall` branches (Source/Codegen/Statement.rs):
///  - `.chars()` — char iteration; only the *receiver* (a string) is emitted, so it
///    must be in-subset.
///  - `.lines()` — streaming `BufRead::lines`; the receiver is a `FileReader`/
///    `StdinHandle` (or inline `io.stdin()`), again emitted on its own, so it must be
///    in-subset. (Both lines shapes route here; the FileReader-vs-stdin split is
///    resolved at lowering off `tir_recv_jet_ty`/the inline-`stdin` shape.)
///  - any other method — the `.iter().cloned()` default, which emits the WHOLE method
///    call as the collection value, so the whole call must be in-subset (e.g. a
///    Phase-9 `.split(…)` builtin returns a `[String]` value).
pub(crate) fn forin_method_collection_in_subset(
    collection: &Expr,
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    let Expr::MethodCall {
        receiver, method, ..
    } = collection
    else {
        return false;
    };
    match method.as_str() {
        "chars" | "lines" => expr_in_subset(receiver, cx, locals),
        _ => expr_in_subset(collection, cx, locals),
    }
}

/// c109 Phase 22: classify an `if` condition. Returns `None` if the condition is not
/// in-subset; otherwise returns the binding name(s) the condition introduces into the
/// then-branch scope (empty for a plain/`is_none` condition). Mirrors `emit_if`'s three
/// condition shapes via `if_pattern_test` (Source/Codegen/Statement.rs):
///  - a plain boolean expr → in-subset iff `expr_in_subset`, no bindings;
///  - an `x == null` test (`Pattern::Absent`) → `is_none`, subject in-subset, no bindings;
///  - an optional-binding test (`value(b)`/`Ok(b)`/`Err(b)`) → if-let, subject in-subset,
///    the binding `b` in scope. Variant/Or/Range patterns in an `if` condition stay on
///    the AST path (conservative — not covered here).
pub(crate) fn if_cond_in_subset(
    cond: &Expr,
    cx: &Cx,
    locals: &HashSet<String>,
) -> Option<Vec<String>> {
    if let Expr::Binary(BinOp::And, left, right, _) = cond {
        let bindings = if_cond_in_subset(left, cx, locals)?;
        if !bindings.is_empty() {
            let mut right_locals = locals.clone();
            right_locals.extend(bindings.iter().cloned());
            return expr_in_subset(right, cx, &right_locals).then_some(bindings);
        }
        if expr_in_subset(left, cx, locals) {
            return if_cond_in_subset(right, cx, locals);
        }
    }
    // The `x == null` (`Pattern::Absent`) form: `if {subj}.is_none()`.
    if let Expr::PatternTest {
        subject,
        pattern: Pattern::Absent(_),
        ..
    } = cond
    {
        return expr_in_subset(subject, cx, locals).then(Vec::new);
    }
    // The optional-binding (if-let) form — only a DIRECT `PatternTest` (not the
    // `Binary(And, …)` shape `if_pattern_test` also admits, which we leave on the AST
    // path). Covered patterns: `value(b)`/`Ok(b)`/`Err(b)` (single binding). Variant/
    // Or/Range patterns are excluded (conservative).
    if let Expr::PatternTest {
        subject, pattern, ..
    } = cond
    {
        if !expr_in_subset(subject, cx, locals) {
            return None;
        }
        // c109 Phase 24: a JSON variant if-let (`if data == Object(entries)` /
        // `if port == Number(n)`). The prelude JSON enum is matched via a single-payload
        // variant pattern (`Object`/`Number`/`Text`/`Boolean`/`Array`) binding one name.
        // The Rust if-let pattern (`{root}jet_std::Json::Object(user_entries)`) is produced
        // by the JSON-aware `emit_if_let_pattern` (reused at lowering), and the binding's
        // type comes from `core_json_pattern_types` (totality). Cover ONLY the JSON-variant
        // single-bind case (a user-enum variant if-let stays on the AST path — conservative,
        // not yet covered as an if-condition form); `Null` is the `Absent`-style form, but
        // `data == Null` would parse as a variant pattern with no binding (not used in the
        // live suite — excluded here, single-bind only).
        if let Pattern::Variant {
            variant,
            bindings,
            span: _,
        } = pattern
        {
            if is_json_variant(variant)
                && bindings.len() == 1
                && matches!(bindings[0], PatSlot::Bind { .. })
            {
                if let PatSlot::Bind { name, .. } = &bindings[0] {
                    return Some(vec![name.clone()]);
                }
            }
            // D-TERM1 (ratified 2026-06-22): a `Key` variant if-let.
            // `if k == Key.Char(c)` → `if let JetKey::Char(user_c) = (k).clone() { … }`.
            // Unit variants (`if k == Key.Enter`) → `if let JetKey::Enter = (k).clone()`.
            if is_key_variant(variant) {
                if bindings.is_empty()
                    || (bindings.len() == 1 && matches!(bindings[0], PatSlot::Bind { .. }))
                {
                    let names: Vec<String> = bindings
                        .iter()
                        .filter_map(|s| {
                            if let PatSlot::Bind { name, .. } = s {
                                Some(name.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    return Some(names);
                }
            }
            // c109 (B4): a USER-enum variant if-let (`if m == Ping(n)`). Covered when
            // the variant is a single-payload variant (one `Bind` slot) of a covered
            // user enum — the AST `emit_if` already emits the correct
            // `if let user_E::user_V(user_b) = <subj>` head. The subject was checked
            // above; require the owning enum to be covered so the prefix/payload are
            // total. Multi-bind / unit variants stay on the AST path (the single-bind
            // shape mirrors the JSON-variant if-let exactly).
            if !is_json_variant(variant)
                && bindings.len() == 1
                && matches!(bindings[0], PatSlot::Bind { .. })
            {
                if let Some(owner) = cx.variant_owner.get(variant) {
                    if enum_is_covered(owner, cx) {
                        if let PatSlot::Bind { name, .. } = &bindings[0] {
                            return Some(vec![name.clone()]);
                        }
                    }
                }
            }
            // c109 (D-PATW): a USER-enum variant if-let with a WILDCARD payload slot
            // (`if w == Some(_)`). The `_` binds nothing, so the then-branch gains no
            // local; `emit_if_let_pattern` already renders the slot as `_`, producing
            // `if let user_E::user_V(_) = <subj>` (byte-for-byte the AST `emit_if`). A
            // single-payload covered-enum variant whose one slot is a wildcard is in
            // subset, introducing NO binding (empty bindings vec). (The recently-covered
            // user-variant if-let bound a name; this binds `_`.)
            if !is_json_variant(variant)
                && bindings.len() == 1
                && matches!(bindings[0], PatSlot::Wildcard)
            {
                if let Some(owner) = cx.variant_owner.get(variant) {
                    if enum_is_covered(owner, cx) {
                        return Some(Vec::new());
                    }
                }
            }
            // D-TAG1: a binding-free variant/group test (`if d == .Fire { … }`)
            // is a plain Bool condition — the expression subset lowers it to
            // `matches!` (`TExprKind::PatternMatches`), no if-let, no bindings.
            if !is_json_variant(variant)
                && bindings.is_empty()
                && cx.variant_owner.contains_key(variant)
            {
                return Some(Vec::new());
            }
            return None;
        }
        return match pattern {
            Pattern::Present { binding, .. }
            | Pattern::Ok { binding, .. }
            | Pattern::Err { binding, .. } => Some(vec![binding.clone()]),
            _ => None,
        };
    }
    // A plain boolean condition.
    expr_in_subset(cond, cx, locals).then(Vec::new)
}

pub(crate) fn if_in_subset(ifs: &IfStmt, cx: &Cx, locals: &mut HashSet<String>) -> bool {
    let Some(cond_bindings) = if_cond_in_subset(&ifs.cond, cx, locals) else {
        return false;
    };
    // Each branch scopes its own bindings; check on a clone so a `let` in the
    // `then` arm doesn't leak into the `else` arm's classification. An optional-binding
    // condition introduces its binding(s) into the then-branch scope.
    let mut then_locals = locals.clone();
    for b in &cond_bindings {
        then_locals.insert(b.clone());
    }
    if !ifs
        .then_body
        .iter()
        .all(|s| stmt_in_subset(s, cx, &mut then_locals))
    {
        return false;
    }
    match &ifs.else_branch {
        None => true,
        Some(ElseBranch::Else(body)) => {
            let mut else_locals = locals.clone();
            body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals))
        }
        Some(ElseBranch::ElseIf(next)) => if_in_subset(next, cx, locals),
    }
}

/// c109 Phase 4: is a `Stmt::Switch` (`when`/match) inside the subset? Covered in
/// exactly the two shapes the TIR reproduces byte-for-byte:
///   (A) **exhaustive enum match** — every arm is a variant pattern over a covered
///       enum subject (`switch_arm_pattern_owned` is Some, none are ranges). Lowers
///       to a Rust `match` (`emit_pattern_match_switch`).
///   (B) **range switch** — every arm is an arm-head range pattern (`0..59 -> …`)
///       over a scalar subject AND an `else` is present. Lowers to an if/else chain
///       (`emit_mixed_switch`).
/// Anything else (mixed comparison/Bool arms, optional/`ok`/`err` patterns, a
/// non-covered subject) stays on the AST path.
pub(crate) fn switch_in_subset(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    span: Span,
    cx: &Cx,
    locals: &mut HashSet<String>,
) -> bool {
    if arms.is_empty() {
        return false;
    }
    // The subject must itself be in-subset (so it lowers + so `it` never escapes).
    if !expr_in_subset(subject, cx, locals) {
        return false;
    }
    if crate::AST::is_subjectless_guard(subject, span) {
        for arm in arms {
            let Some(bindings) = if_cond_in_subset(&arm.cond, cx, locals) else {
                return false;
            };
            let mut body_locals = locals.clone();
            body_locals.extend(bindings);
            if !arm
                .body
                .iter()
                .all(|stmt| stmt_in_subset(stmt, cx, &mut body_locals))
            {
                return false;
            }
        }
        return else_body.as_ref().is_none_or(|body| {
            let mut else_locals = locals.clone();
            body.iter().all(|stmt| stmt_in_subset(stmt, cx, &mut else_locals))
        });
    }
    // Shape A: all arms are variant patterns (exhaustive enum match).
    if arms
        .iter()
        .all(|a| arm_variant_pattern(cx, &a.cond, subject).is_some())
    {
        // Subject must be a covered enum (its variants are scalar-payload only).
        let subj_enum = arms.iter().find_map(|a| {
            arm_variant_pattern(cx, &a.cond, subject).and_then(|p| variant_pattern_enum(cx, &p))
        });
        let Some(enum_name) = subj_enum else {
            return false;
        };
        if !enum_is_covered(&enum_name, cx) {
            return false;
        }
        for a in arms {
            let pat = arm_variant_pattern(cx, &a.cond, subject).expect("checked above");
            // Each arm's payload bindings extend the body scope; check on a clone.
            let mut body_locals = locals.clone();
            add_pattern_binding_names(&pat, &mut body_locals);
            if !a
                .body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut body_locals))
            {
                return false;
            }
        }
        if let Some(body) = else_body {
            let mut else_locals = locals.clone();
            if !body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals)) {
                return false;
            }
        }
        return true;
    }
    // Shape B: all arms are arm-head range patterns over a scalar subject, with an
    // `else`. (Range arms bind nothing.) The subject's type must resolve to an
    // integer/char local so the conditions type-check.
    if else_body.is_some()
        && arms
            .iter()
            .all(|a| arm_head_range(cx, &a.cond, subject).is_some())
    {
        // The subject must be a plain in-subset scalar place (an Ident local/param)
        // so `_jet_switch_subject`/the conditions read it directly. Anything more
        // complex is excluded (the AST path re-emits the subject per arm).
        if !matches!(subject, Expr::Ident(name, _) if locals.contains(name)) {
            return false;
        }
        for a in arms {
            let mut body_locals = locals.clone();
            if !a
                .body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut body_locals))
            {
                return false;
            }
        }
        if let Some(body) = else_body {
            let mut else_locals = locals.clone();
            if !body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals)) {
                return false;
            }
        }
        return true;
    }
    // Shape C (c109 Phase 8): a fallible/optional pattern match — every arm head is
    // an `Ok(b)`/`Err(b)`/`value(b)`/`null` pattern over the subject. Lowers to a
    // Rust `match` over the subject's `Result`/`Option`, exactly like the enum-match
    // shape but with `Ok(..)`/`Err(..)`/`Some(..)`/`None` patterns. The subject must
    // be in-subset (checked above) and resolve to a `Result`/`Option` — but a covered
    // subject already guarantees that here (its type came from a covered fn/local).
    if arms
        .iter()
        .all(|a| arm_fallible_pattern(cx, &a.cond, subject).is_some())
    {
        for a in arms {
            let pat = arm_fallible_pattern(cx, &a.cond, subject).expect("checked above");
            let mut body_locals = locals.clone();
            // `Ok(b)`/`Err(b)`/`value(b)` bind one name; `null` binds nothing.
            if let Some(b) = fallible_pattern_binding(&pat) {
                body_locals.insert(b);
            }
            if !a
                .body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut body_locals))
            {
                return false;
            }
        }
        if let Some(body) = else_body {
            let mut else_locals = locals.clone();
            if !body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals)) {
                return false;
            }
        }
        return true;
    }
    // Shape D (c109 Phase 15): a MIXED comparison/Bool switch — the general
    // `emit_mixed_switch` `if/else if … else` chain used when the arms are NOT all
    // variant (shape A), NOT all range (shape B), and NOT all fallible (shape C). Every
    // arm head must be a PLAIN in-subset comparison/Bool expression — i.e. the
    // `_ => emit_expr(cond)` branch of `emit_switch_arm_cond` (NOT a variant/Eq-variant
    // pattern, which would route through `emit_pattern_matches`).
    //
    // D-IF3: a range head (`400..499 ->`) is admitted into this chain too, lowered to
    // `subject >= lo && subject <= hi`, so a value+range mix (`200 -> …` next to
    // `400..499 -> …`) is covered — provided the subject is a scalar ident local so the
    // emitted range condition type-checks (the same constraint shape B imposes).
    // D-DESTRUCT1: a struct-pattern arm head (`.{ kind: "page", title, .. }`) also
    // lowers through this chain: value fields become boolean equality checks, and bind
    // fields clone from the borrowed `_jet_switch_subject` at the top of the arm body.
    // Conservative: a variant/fallible pattern-test arm in the chain excludes the whole
    // switch (stays on the AST path). The `else` is optional.
    let has_range = arms
        .iter()
        .any(|a| arm_head_range(cx, &a.cond, subject).is_some());
    let subject_is_scalar_ident = matches!(subject, Expr::Ident(name, _) if locals.contains(name));
    if arms.iter().all(|a| {
        arm_is_plain_cond(cx, &a.cond, subject)
            || arm_head_range(cx, &a.cond, subject).is_some()
            || arm_struct_pattern(cx, &a.cond, subject).is_some()
            || arm_str_match_pattern(cx, &a.cond, subject).is_some()
            || arm_bin_match_pattern(cx, &a.cond, subject).is_some()
    }) && (!has_range || subject_is_scalar_ident)
    {
        // D-PARSESTR1: sema already proved a str-match arm's subject is
        // `String` (E0305 otherwise) — trusted here, same as shape C trusts
        // sema for `ok`/`err`/`value` subject types (I3).
        for a in arms {
            // A range head lowers to a comparison string from `subject_str`; only the
            // PLAIN-cond arms carry a sub-expression that must itself be in-subset.
            if arm_head_range(cx, &a.cond, subject).is_none()
                && arm_struct_pattern(cx, &a.cond, subject).is_none()
                && arm_str_match_pattern(cx, &a.cond, subject).is_none()
                && arm_bin_match_pattern(cx, &a.cond, subject).is_none()
                && !expr_in_subset(&a.cond, cx, locals)
            {
                return false;
            }
            let mut body_locals = locals.clone();
            if let Some(pat) = arm_struct_pattern(cx, &a.cond, subject) {
                if !struct_pattern_values_in_subset(&pat, cx, locals) {
                    return false;
                }
                add_struct_pattern_binding_names(&pat, &mut body_locals);
            }
            if let Some(pat) = arm_str_match_pattern(cx, &a.cond, subject) {
                add_str_match_pattern_binding_names(&pat, &mut body_locals);
            }
            if let Some(pat) = arm_bin_match_pattern(cx, &a.cond, subject) {
                add_bin_match_pattern_binding_names(&pat, &mut body_locals);
            }
            if !a
                .body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut body_locals))
            {
                return false;
            }
        }
        if let Some(body) = else_body {
            let mut else_locals = locals.clone();
            if !body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals)) {
                return false;
            }
        }
        return true;
    }
    false
}
