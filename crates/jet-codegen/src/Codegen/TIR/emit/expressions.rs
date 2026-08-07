use crate::AST::{BinOp, CtFloat, Type, UnOp};
use crate::Codegen::TIR::emit::statements::PRELUDE_CARRIED;
use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::user_type_rust;
use crate::Codegen::TIR::emit::collect_select_arms;
use crate::Codegen::TIR::emit::emit_http_bridge_error;
use crate::Codegen::TIR::emit::emit_http_response_from_bridge;
use crate::Codegen::TIR::emit::emit_math_swizzle_read;
use crate::Codegen::TIR::emit::emit_field_rust;
use crate::Codegen::TIR::emit::emit_require_stop;
use crate::Codegen::TIR::emit_tir_call_args;
use crate::Codegen::TIR::emit_tir_core_call;
use crate::Codegen::TIR::emit_tir_lambda_block;
use crate::Codegen::TIR::emit_static_owner;
use crate::Codegen::TIR::emit_tir_orfallback_rhs;
use crate::Codegen::TIR::emit_tir_pattern;
use crate::Codegen::TIR::emit_tir_place;
use crate::Codegen::TIR::emit_tir_str;
use crate::Codegen::TIR::emit_tir_value_block;
use crate::Codegen::TIR::TIfCond;
use crate::Codegen::TIR::TLambda;
use crate::Codegen::TIR::ListSpreadPart;
use crate::Codegen::TIR::bin_match_scan_closure_ex;
use crate::Codegen::TIR::str_match_scan_closure_ex;
use crate::Codegen::TIR::TBuiltinOp;
use crate::Codegen::TIR::TClosureOp;
use crate::Codegen::TIR::TCoreClosureKind;
use crate::Codegen::TIR::TEnumArg;
use crate::Codegen::TIR::TEnumPayload;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TFnValueKind;
use crate::Codegen::TIR::THandleOp;
use crate::Codegen::TIR::TModuleCallForm;
use crate::Codegen::TIR::TStructExtra;
use crate::Codegen::TIR::TNumericOp;
use crate::Codegen::TIR::THostArg;
use crate::Codegen::TIR::THostCall;
use crate::Codegen::TIR::TOptionProbe;
use crate::Codegen::TIR::TTypedTextForm;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::TIR::TTryConvert;
use crate::Codegen::TIR::tuple_join;
use crate::Codegen::TIR::struct_field_type;

fn emit_tir_lambda(lam: &TLambda) -> String {
    let move_kw = if lam.is_move { "move " } else { "" };
    let closure = format!("{}|{}| {}", move_kw, lam.params.join(", "), lam.body);
    // Prefer Arc (HTTP) / Rc (cloneable Fn values) over Box (FnMut escape only).
    let wrapped = if lam.arc {
        format!("std::sync::Arc::new({closure})")
    } else if lam.rc {
        format!("std::rc::Rc::new({closure})")
    } else if lam.boxed {
        format!("Box::new({closure})")
    } else {
        closure
    };
    if lam.prep.is_empty() {
        wrapped
    } else {
        format!("{{ {} {} }}", lam.prep, wrapped)
    }
}

fn emit_shared_guard_projection(cx: &Cx, guard_ty: &Type, path: &[String]) -> String {
    let mut projection = "__jet_guard_value".to_string();
    let mut value_ty = match guard_ty {
        Type::Tagged { inner, .. } => match inner.as_ref() {
            Type::Apply { name, args }
                if name == crate::Syntax::TYPE_SHARED_GUARD && args.len() == 1 =>
            {
                args[0].clone()
            }
            _ => jet_foundation::ice!(
                None,
                "SharedGuard projection TIR lost its guarded type"
            ),
        },
        Type::Apply { name, args }
            if name == crate::Syntax::TYPE_SHARED_GUARD && args.len() == 1 =>
        {
            args[0].clone()
        }
        _ => jet_foundation::ice!(
            None,
            "SharedGuard projection TIR has a non-guard receiver"
        ),
    };
    for field in path {
        projection.push('.');
        projection.push_str(&emit_field_rust(cx, &value_ty, field));
        value_ty = struct_field_type(cx, &value_ty, field)
            .unwrap_or_else(|| {
                jet_foundation::ice!(
                    None,
                    "SharedGuard projection TIR has unknown field `{field}`"
                )
            });
    }
    projection
}

fn shared_lock_receipt_id(recv: &TExpr, cx: &Cx) -> String {
    if let TExprKind::Local(local) = &recv.kind {
        return local.rust_name();
    }
    let mut hash = 0xcbf29ce484222325u64;
    for byte in emit_tir_expr(recv, cx).bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("expr-{hash:016x}")
}

pub(crate) fn emit_host_call(call: &THostCall, recv_ty: Option<&Type>, cx: &Cx) -> String {
    match call {
        THostCall::Helper { helper, args } => {
            let arg_str = args
                .iter()
                .enumerate()
                .map(|(i, a)| match a {
                    THostArg::Expr(e) => {
                        let s = emit_tir_expr(e, cx);
                        // D-ERRCTX1: jet_context(recv, || msg)
                        if helper.ends_with("jet_context") && i == 1 {
                            format!("|| {s}")
                        } else {
                            s
                        }
                    }
                    THostArg::Borrow(e) => format!("&({})", emit_tir_expr(e, cx)),
                    THostArg::Lambda(lam) => {
                        let move_kw = if lam.is_move { "move " } else { "" };
                        format!("{}|{}| {}", move_kw, lam.params.join(", "), lam.body)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{helper}({arg_str})")
        }
        THostCall::Method { recv, method, args } => {
            let arg_str = args
                .iter()
                .map(|a| emit_tir_expr(a, cx))
                .collect::<Vec<_>>()
                .join(", ");
            if method == "edit_txn" {
                return format!(
                    "({}).edit_txn(&mut __jet_stm, {})",
                    emit_tir_expr(recv, cx),
                    arg_str
                );
            }
            if matches!(&recv.ty, Type::Shared(_))
                && matches!(method.as_str(), "guard_read" | "guard_edit")
            {
                let lock = shared_lock_receipt_id(recv, cx);
                return format!(
                    "{{ /* jet-shared-lock-order-receipt: lock={lock}; acquire={method}; order=source */ ({}).{method}({arg_str}) }}",
                    emit_tir_expr(recv, cx)
                );
            }
            format!("({}).{method}({arg_str})", emit_tir_expr(recv, cx))
        }
        THostCall::CellGuardProject {
            recv,
            paths,
            result_ty,
            editable,
            edit_paths_disjoint,
        } => {
            let borrow = if *editable { "&mut " } else { "&" };
            let render_path = |steps: &[String]| {
                let mut place = "__jet_cell_value".to_string();
                for field in steps {
                    place.push('.');
                    place.push_str(&mangle(field));
                }
                format!("{borrow}{place}")
            };
            let recv = emit_tir_expr(recv, cx);
            match paths.as_slice() {
                [path] => format!(
                    "({recv}).map(|__jet_cell_value| {})",
                    render_path(path)
                ),
                [first, second] => {
                    debug_assert!(!editable || *edit_paths_disjoint);
                    let split = format!(
                        "({recv}).split(|__jet_cell_value| ({}, {}))",
                        render_path(first),
                        render_path(second)
                    );
                    let Type::Tuple(fields) = result_ty else {
                        unreachable!("sema types Cell guard split as an exact tuple");
                    };
                    let plain = crate::Codegen::Tuples::tuple_fields_plain(fields);
                    let tuple = crate::Codegen::Tuples::tuple_struct_name(&plain);
                    let first_field = mangle(&fields[0].0);
                    let second_field = mangle(&fields[1].0);
                    format!(
                        "{{ let (__jet_cell_first, __jet_cell_second) = {split}; \
                         {tuple} {{ {first_field}: __jet_cell_first, \
                         {second_field}: __jet_cell_second }} }}"
                    )
                }
                _ => unreachable!("Cell guard projection has one or two sema paths"),
            }
        }
        THostCall::FixedListIndex { base, index } => format!(
            "(({b})[({i}).0 as usize].clone())",
            b = emit_tir_expr(base, cx),
            i = emit_tir_expr(index, cx),
        ),
        THostCall::TypedText { kind, arg } => {
            let a = emit_tir_expr(arg, cx);
            match kind {
                TTypedTextForm::SQLRaw => format!("{}jet_typed_sql_raw(({a}).clone())", cx.root_prefix),
                TTypedTextForm::HTMLRaw => format!("{}jet_typed_html_raw(({a}).clone())", cx.root_prefix),
                TTypedTextForm::ShRaw => format!("{}jet_typed_sh_raw(({a}).clone())", cx.root_prefix),
                TTypedTextForm::SQLTemplate => format!(
                    "{{ let __jet_sql = ({a}).clone(); {}jet_typed_sql_template(&__jet_sql) }}",
                    cx.root_prefix
                ),
                TTypedTextForm::SQLParams => format!(
                    "{{ let __jet_sql = ({a}).clone(); {}jet_typed_sql_params(&__jet_sql) }}",
                    cx.root_prefix
                ),
                TTypedTextForm::HTMLText => format!("{}jet_typed_html_text(({a}).clone())", cx.root_prefix),
            }
        }
        THostCall::FnName(name) => cx.mangle_name(name),
        THostCall::GcEdit {
            root,
            method_span_start,
            edges,
            edit,
            index_temp,
            kind,
        } => {
            use crate::Codegen::TIR::TGcEditKind;
            let edit_s = emit_tir_expr(edit, cx);
            let edge_list = edges.join(", ");
            let index = index_temp
                .as_ref()
                .map(|(temp, _)| temp.as_str())
                .unwrap_or("0");
            let mut emitted = match kind {
                TGcEditKind::Clear => format!(
                    "jet_gc::runtime_or_exit({root}.edit_clearing_edges(|__jet_value| {edit_s}))"
                ),
                TGcEditKind::Pop => format!(
                    "jet_gc::runtime_or_exit({root}.edit_edge_slot_pop(\"collection\", |__jet_value| {edit_s}))"
                ),
                TGcEditKind::RemoveIndex => format!(
                    "jet_gc::runtime_or_exit({root}.edit_edge_slot_remove(\"collection\", {index} as usize, |__jet_value| {edit_s}))"
                ),
                TGcEditKind::InsertIndex => format!(
                    "jet_gc::runtime_or_exit({root}.edit_edge_slot_insert(\"collection\", {index} as usize, &[{edge_list}], |__jet_value| {edit_s}))"
                ),
                TGcEditKind::Prepend => format!(
                    "jet_gc::runtime_or_exit({root}.edit_edge_slot_prepend(\"collection\", &[{edge_list}], |__jet_value| {edit_s}))"
                ),
                TGcEditKind::Additive => format!(
                    "jet_gc::runtime_or_exit({root}.edit_edge_slot_additive(\"collection\", &[{edge_list}], |__jet_value| {edit_s}))"
                ),
                TGcEditKind::Plain => {
                    format!("jet_gc::runtime_or_exit({root}.edit(|__jet_value| {edit_s}))")
                }
                TGcEditKind::EdgeSlot => format!(
                    "jet_gc::runtime_or_exit({root}.edit_edge_slot(\"method:{method_span_start}\", &[{edge_list}], |__jet_value| {edit_s}))"
                ),
            };
            if let Some((temp, value)) = index_temp {
                emitted = format!(
                    "{{ let {temp} = {}; {emitted} }}",
                    emit_tir_expr(value, cx)
                );
            }
            emitted
        }
        THostCall::GcRead { root } => {
            format!("jet_gc::runtime_or_exit({root}.read(|__jet_value| __jet_value.clone()))")
        }
        THostCall::OptionProbe { inner, kind } => {
            let inner_s = emit_tir_expr(inner, cx);
            match kind {
                TOptionProbe::IsSome => format!("({inner_s}).is_some()"),
                TOptionProbe::Unwrap => format!("({inner_s}).unwrap()"),
                TOptionProbe::Field(field) => {
                    let field_rust = recv_ty
                        .map(|ty| emit_field_rust(cx, ty, field))
                        .unwrap_or_else(|| mangle(field));
                    format!("({inner_s}).{field_rust}")
                }
            }
        }
        THostCall::StrMatchScan { parts, probe } => {
            let (closure, _) =
                str_match_scan_closure_ex(parts, cx, "_jet_switch_subject.as_str()", true);
            match probe {
                crate::Codegen::TIR::TMatchProbe::IsSome => format!("({closure}).is_some()"),
                crate::Codegen::TIR::TMatchProbe::Unwrap => format!("({closure}).unwrap()"),
            }
        }
        THostCall::BinMatchScan { parts, probe } => {
            let (closure, _) =
                bin_match_scan_closure_ex(parts, cx, "(_jet_switch_subject).as_slice()", true);
            match probe {
                crate::Codegen::TIR::TMatchProbe::IsSome => format!("({closure}).is_some()"),
                crate::Codegen::TIR::TMatchProbe::Unwrap => format!("({closure}).unwrap()"),
            }
        }
        THostCall::TupleIndex { base, index } => {
            format!("({}).{index}", emit_tir_expr(base, cx))
        }
        THostCall::SwitchSubjectField { field } => {
            let field_rust = recv_ty
                .map(|ty| emit_field_rust(cx, ty, field))
                .unwrap_or_else(|| mangle(field));
            format!("((*_jet_switch_subject).{field_rust})")
        }
        THostCall::SwitchSubjectValue => "(*_jet_switch_subject)".to_string(),
        THostCall::YieldSend { value } => {
            format!(
                "if !__jet_yield_tx.send_stream({}) {{ {} return; }}",
                emit_tir_expr(value, cx),
                crate::Codegen::TIR::STREAM_CANCEL_MARKER,
            )
        }
        THostCall::TypedTextInterp { kind, literals, holes } => {
            use crate::Codegen::TIR::TTypedTextInterpKind;
            use crate::Codegen::escape_rust_str;
            match kind {
                TTypedTextInterpKind::SQL => {
                    let hole_s = holes
                        .iter()
                        .map(|h| format!("({}).jet_show()", emit_tir_expr(h, cx)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "{}jet_typed_sql_interpolate(&[{}], vec![{hole_s}])",
                        cx.root_prefix,
                        literals
                            .iter()
                            .map(|literal| escape_rust_str(literal))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
                TTypedTextInterpKind::Sh => {
                    let hole_s = holes
                        .iter()
                        .map(|h| format!("({}).jet_show()", emit_tir_expr(h, cx)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "{}jet_typed_sh_interpolate(&[{}], vec![{hole_s}])",
                        cx.root_prefix,
                        literals
                            .iter()
                            .map(|literal| escape_rust_str(literal))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
                TTypedTextInterpKind::HTML => {
                    let hole_s = holes
                        .iter()
                        .map(|h| format!("({}).jet_show()", emit_tir_expr(h, cx)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "{}jet_typed_html_interpolate(&[{}], vec![{hole_s}])",
                        cx.root_prefix,
                        literals
                            .iter()
                            .map(|literal| escape_rust_str(literal))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
        }
        THostCall::ExpectSnapshot { value, snap_path } => {
            format!(
                "jet_expect(format!(\"{{}}\", ({}).jet_show())).snapshot({snap_path:?})?",
                emit_tir_expr(value, cx)
            )
        }
        THostCall::EnvSet { name, value, loc } => {
            use crate::Codegen::TIR::RESOURCE_CLEANUP_MARKER;
            use crate::Codegen::escape_rust_str;
            let name_s = emit_tir_expr(name, cx);
            let value_s = emit_tir_expr(value, cx);
            let locals = crate::Codegen::TIR::emit_panic_locals(loc, cx);
            format!(
                "{{ let __jet_env_name = ({name_s}); let __jet_env_value = ({value_s}); if let Err(__jet_env_error) = {root}jet_std_env_set(&__jet_env_name, &__jet_env_value) {{ {cleanup} jet_panic_rich({file}, {line}, {fn_name}, {src_line}, {col}, {caret}, &format!(\"core.env.set: {{}}\", __jet_env_error.jet_show()), &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
                root = cx.root_prefix,
                cleanup = RESOURCE_CLEANUP_MARKER,
                file = escape_rust_str(&loc.file),
                line = loc.line,
                fn_name = escape_rust_str(&loc.fn_name),
                src_line = escape_rust_str(loc.src_line.trim_end()),
                col = loc.col,
                caret = loc.caret,
            )
        }
        THostCall::NumericBounds { ty, member } => {
            format!("{}::{member}", cx.rust_type(ty))
        }
        THostCall::ExpiringSecretNew {
            value,
            duration,
            clock,
            elem,
        } => {
            format!(
                "{{ let __jet_expiring_value = {}; \
                 let __jet_expiring_ttl = &({}); \
                 let __jet_expiring_clock = &({}); \
                 let __jet_expiring_observer = __jet_expiring_clock.observer(); \
                 {}JetExpiringSecret::<{}>::new(\
                     __jet_expiring_value, __jet_expiring_ttl.as_millis(), \
                     move || __jet_expiring_observer.now()) }}",
                emit_tir_expr(value, cx),
                emit_tir_expr(duration, cx),
                emit_tir_expr(clock, cx),
                cx.root_prefix,
                cx.rust_type(elem)
            )
        }
        THostCall::ExpiringValueNew {
            value,
            duration,
            clock,
        } => {
            format!(
                "{}jet_expiring_new({}, {}jet_duration_ms_value(&({})), {}jet_clock_now(&({})))",
                cx.root_prefix,
                emit_tir_expr(value, cx),
                cx.root_prefix,
                emit_tir_expr(duration, cx),
                cx.root_prefix,
                emit_tir_expr(clock, cx)
            )
        }
        THostCall::CCallback {
            symbol,
            lambda,
            ret,
        } => {
            let ret = ret
                .as_ref()
                .map(|ty| format!(" -> {}", cx.rust_type(ty)))
                .unwrap_or_default();
            let body = if lambda.body.starts_with('{') {
                lambda.body.clone()
            } else {
                format!("{{ {} }}", lambda.body)
            };
            format!(
                "{{ extern \"C\" fn {}({}){} {} {} }}",
                symbol,
                lambda.params.join(", "),
                ret,
                body,
                symbol
            )
        }
    }
}

fn emit_tir_if_expr(cond: &TIfCond, then_block: &str, else_block: &str, cx: &Cx) -> String {
    if let TIfCond::And { left, right } = cond {
        let right = emit_tir_if_expr(right, then_block, else_block, cx);
        return emit_tir_if_expr(left, &format!("{{ {right} }}"), else_block, cx);
    }
    match cond {
        TIfCond::Plain(cond) => format!(
            "if {} {} else {}",
            emit_tir_expr(cond, cx),
            then_block,
            else_block
        ),
        TIfCond::IfLet { pattern, subj } => format!(
            "if let {} = {} {} else {}",
            emit_tir_pattern(pattern, cx),
            emit_tir_expr(subj, cx),
            then_block,
            else_block
        ),
        TIfCond::IsNone { subj } => format!(
            "if {}.is_none() {} else {}",
            emit_tir_expr(subj, cx),
            then_block,
            else_block
        ),
        TIfCond::Matches { pattern, subj } => format!(
            "if matches!(&({}), {}) {} else {}",
            emit_tir_expr(subj, cx),
            emit_tir_pattern(pattern, cx),
            then_block,
            else_block
        ),
        TIfCond::And { .. } => unreachable!("handled above"),
    }
}

/// c109 Phase 16: emit one enum-literal payload arg, applying its resolved
/// `clone`/`boxed` wrappers — `(…).clone()` first, then `Box::new(…)`, exactly as
/// `emit_boxed_enum_arg` (Expression.rs) does.
pub(crate) fn emit_tir_enum_arg(a: &TEnumArg, cx: &Cx) -> String {
    let mut s = emit_tir_expr(&a.value, cx);
    if a.clone {
        s = format!("({}).clone()", s);
    }
    if a.boxed {
        s = format!("Box::new({})", s);
    }
    s
}

fn emit_numeric_op(recv: &str, op: &TNumericOp, cx: &Cx) -> String {
    match op {
        TNumericOp::Predicate(m) => format!("({recv}).{m}()"),
        TNumericOp::BitCount { method: m, .. } => format!("(({recv}).{m}() as i64)"),
        TNumericOp::ToShow => format!("({recv}).jet_show()"),
        TNumericOp::Origin(origin) => format!(
            "{{ let _ = ({recv}); {:?}.to_string() }}",
            origin.as_deref().unwrap_or("untracked")
        ),
        TNumericOp::CastAs { dst_rust } => format!("(({recv}) as {dst_rust})"),
        TNumericOp::CheckedIntToFloat {
            source_signed,
            target_f32,
            line,
        } => {
            let checked = format!(
                "match jet_numeric_checked_widen(({recv}) as u64, {source_signed}, {target_f32}) {{ \
                 Some(value) => value, None => jet_panic({:?}, {line}, JET_NUMERIC_WIDEN_TRAP) }}",
                cx.file
            );
            if *target_f32 {
                format!("(({checked}) as f32)")
            } else {
                checked
            }
        }
        TNumericOp::TryFrom {
            dst_rust,
            dst_spelling,
            ..
        } => format!(
            "<{dst_rust}>::try_from(({recv}) as i128).map_err(|_| \
             \"value doesn't fit in {dst_spelling}\".to_string())"
        ),
        TNumericOp::FloatToInt {
            dst_rust,
            dst_spelling,
            lower,
            upper_exclusive,
            ..
        } => format!(
            "{{ let __jet_value = ({recv}); if __jet_value.is_finite() && \
             __jet_value >= ({lower} as _) && __jet_value < ({upper_exclusive} as _) {{ \
             Ok(__jet_value.trunc() as {dst_rust}) }} else {{ Err(\
             \"value doesn't fit in {dst_spelling}\".to_string()) }} }}"
        ),
        TNumericOp::FloatNarrow { dst_spelling } => format!(
            "{{ let __jet_value = ({recv}); if __jet_value.is_finite() && \
             __jet_value >= -(f32::MAX as f64) && __jet_value <= f32::MAX as f64 {{ \
             Ok(__jet_value as f32) }} else {{ Err(\
             \"value doesn't fit in {dst_spelling}\".to_string()) }} }}"
        ),
    }
}

fn zip_integer_signed(ty: &Type) -> Option<bool> {
    match ty {
        Type::Int => Some(true),
        Type::IntN { signed, .. } => Some(*signed),
        _ => None,
    }
}

fn zip_fill_target(ty: &Type) -> &Type {
    match ty {
        Type::Option(inner) | Type::Tagged { inner, .. } => zip_fill_target(inner),
        _ => ty,
    }
}

fn emit_zip_fill_value(raw: String, source: &Type, target: Option<&Type>, cx: &Cx) -> String {
    let Some(target) = target.map(zip_fill_target) else {
        return format!("({raw}).clone()");
    };
    let source = zip_fill_target(source);
    if source == target {
        return format!("({raw}).clone()");
    }
    if source.numeric_widening_to(target).is_none() {
        return format!("({raw}).clone()");
    }
    if let Some(source_signed) = zip_integer_signed(source) {
        if matches!(target, Type::Float | Type::Float32) {
            let target_f32 = matches!(target, Type::Float32);
            let checked = format!(
                "match jet_numeric_checked_widen(({raw}) as u64, {source_signed}, {target_f32}) {{ \
                 Some(value) => value, None => jet_panic({:?}, 0, JET_NUMERIC_WIDEN_TRAP) }}",
                cx.file
            );
            return if target_f32 {
                format!("(({checked}) as f32)")
            } else {
                checked
            };
        }
    }
    format!("(({raw}) as {})", cx.rust_type(target))
}

pub(crate) fn emit_tir_expr(e: &TExpr, cx: &Cx) -> String {
    match &e.kind {
        // D-SG9: width suffix is read straight off the literal — no re-inference.
        // When width is missing but the TIR type is a fixed IntN (typed list
        // elements, contextual bindings), prefer that over default `i64`.
        TExprKind::IntLit(n, width) => {
            let resolved = width.or_else(|| match &e.ty {
                Type::IntN { signed, bits } => Some((*signed, *bits)),
                _ => None,
            });
            match resolved {
                Some((signed, bits)) => {
                    format!("{}{}{}", n, if signed { 'i' } else { 'u' }, bits)
                }
                None => format!("{}i64", n),
            }
        },
        // D-FLOATW1: emit `f32` suffix when the sema-resolved width is F32.
        TExprKind::FloatLit(v) => {
            if matches!(&e.ty, Type::Float32) {
                CtFloat::f32(*v as f32).serialize()
            } else {
                CtFloat::f64(*v).serialize()
            }
        }
        TExprKind::BoolLit(b) => b.to_string(),
        TExprKind::CharLit(c) => format!("{:?}", c),
        TExprKind::StrLit(parts) => emit_tir_str(parts, cx),
        TExprKind::Local(slot) if slot.uninit_scalar => {
            format!("({}).read().clone()", slot.rust_place())
        }
        TExprKind::Local(slot) if slot.uninit_fixed => {
            format!("({}).read_array()", slot.rust_place())
        }
        TExprKind::Local(slot) => slot.rust_place(),
        // D-TAG1: binding-free enum variant/group pattern test.
        TExprKind::PatternMatches { subj, pattern } => {
            format!(
                "matches!(&({}), {})",
                emit_tir_expr(subj, cx),
                emit_tir_pattern(pattern, cx)
            )
        }
        TExprKind::Unit => "()".to_string(),
        TExprKind::DefaultLit => "Default::default()".to_string(),
        TExprKind::Uninit => match &e.ty {
            Type::FixedList { elem, len, .. } => format!(
                "jet_mem::JetUninitFixed::<{}, {}>::new()",
                cx.rust_type(elem),
                len
            ),
            _ => format!("jet_mem::JetUninit::<{}>::new()", cx.rust_type(&e.ty)),
        },
        TExprKind::CtLit(value) => {
            // D-FIXARR1: comptime list values serialize as `vec![…]` by default;
            // a `[T#N]` typed binding must bake a Rust array literal instead.
            if matches!(&e.ty, Type::FixedList { .. }) {
                if let crate::AST::CtValue::List(xs) = value {
                    let parts: Vec<String> = xs.iter().map(|x| x.serialize()).collect();
                    return format!("[{}]", parts.join(", "));
                }
            }
            value.serialize()
        }
        TExprKind::HostCall(call) => emit_host_call(call, None, cx),
        // A declared const's Rust static name, resolved from its Jet name here so
        // the TIR node carries only the name.
        TExprKind::ConstRef(name) => cx
            .consts
            .get(name)
            .cloned()
            .unwrap_or_else(|| mangle(name).to_uppercase()),
        TExprKind::DataEntriesToMap(entries) => {
            format!("{}.into_iter().collect()", entries.rust_place())
        }
        TExprKind::Print(arg) => {
            // Parallel `jet test` runs each test on its own thread (per-test
            // isolation, D-TESTKIT1 gap #3); a bare `println!` from inside a test
            // body would interleave with other threads' output and with the
            // harness's own `name: pass/FAIL` lines. In test-harness builds route
            // through the per-thread capture buffer (`jet_test_print`, TEST_PRELUDE)
            // instead; the harness flushes it right before reporting the result, so
            // a test's own output always lands directly above its status line, in
            // slot order, exactly as it did when tests ran one at a time.
            if cx.test_mode {
                format!(
                    "jet_test_print(({}).jet_show())",
                    emit_tir_expr(arg, cx)
                )
            } else {
                format!(
                    "println!(\"{{}}\", ({}).jet_show())",
                    emit_tir_expr(arg, cx)
                )
            }
        }
        // D-LIN1-DROP: `drop(x)` → Rust's safe `drop(x)`; the value moves in and
        // its `Drop` runs. No `unsafe` — the audit is sema-side (the `#Unsafe`
        // gate). The arg was lowered as a plain place/value (a move).
        TExprKind::Drop(arg) => {
            format!("drop({})", emit_tir_expr(arg, cx))
        }
        TExprKind::Close(arg) => {
            format!("user_Close::close({})", emit_tir_expr(arg, cx))
        }
        TExprKind::ResourceNew(arg) => {
            format!("JetResource::new({})", emit_tir_expr(arg, cx))
        }
        TExprKind::ResourceTake(place) => format!("{}.take()", place),
        // c109 Phase 25: ambient prelude `input(...)`, byte-for-byte the `emit_call`
        // ambient-input branch (Source/Codegen/Expression.rs): a bare call with NO arg
        // emits `{root}jet_std_io_input(None)`; with a prompt arg `{root}jet_std_io_input(Some(&(arg)))`.
        TExprKind::AmbientInput { prompt } => {
            let helper = format!("{}jet_std_io_input", cx.root_prefix);
            match prompt {
                None => format!("{}(None)", helper),
                Some(p) => format!("{}(Some(&({})))", helper, emit_tir_expr(p, cx)),
            }
        }
        TExprKind::RequireStop { kind, loc, .. } => emit_require_stop(kind, loc, cx),
        TExprKind::Call {
            name,
            type_args,
            args,
        } => {
            let arg_str = emit_tir_call_args(args, cx);
            let type_args = if type_args.is_empty() {
                String::new()
            } else {
                format!(
                    "::<{}>",
                    type_args
                        .iter()
                        .map(|ty| cx.rust_type(ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            format!("{}{}({})", cx.mangle_name(name), type_args, arg_str)
        }
        TExprKind::DistinctCtor { name, arg, .. } => {
            format!("{}({})", cx.mangle_name(name), emit_tir_expr(arg, cx))
        }
        TExprKind::RangeCheckedCtor { name, arg } => {
            format!(
                "{}::try_new({})",
                cx.mangle_name(name),
                emit_tir_expr(arg, cx)
            )
        }
        TExprKind::DistinctConvert {
            name,
            arg,
            op,
            range,
            fallible,
        } => {
            let converted = emit_numeric_op(&emit_tir_expr(arg, cx), op, cx);
            let conversion_fallible = matches!(
                op,
                TNumericOp::TryFrom { .. }
                    | TNumericOp::FloatToInt { .. }
                    | TNumericOp::FloatNarrow { .. }
            );
            let name = cx.mangle_name(name);
            match (conversion_fallible, range.is_some(), *fallible) {
                (true, true, true) => format!("({converted}).and_then({name}::try_new)"),
                (true, false, true) => format!("({converted}).map({name})"),
                (false, true, true) => format!("{name}::try_new({converted})"),
                (false, _, false) => format!("{name}({converted})"),
                _ => unreachable!("sema/TIR distinct conversion fallibility drift"),
            }
        }
        TExprKind::UnitConvert {
            destination,
            arg,
            scale,
            offset,
            rounding,
            fallible,
            file,
            line,
        } => {
            let destination = cx.rust_type(&Type::Named(destination.clone()));
            let value = format!("({}).0", emit_tir_expr(arg, cx));
            let args = format!(
                "{value}, {:?}, {:?}, {:?}, {:?}",
                scale.num.to_string(),
                scale.den.to_string(),
                offset.num.to_string(),
                offset.den.to_string(),
            );
            if let Some((mode, digits)) = rounding {
                let digits = emit_tir_expr(digits, cx);
                format!(
                    "match jet_unit_conversion_rounded({args}, UnitRoundingMode::{mode:?}, {digits}) {{ Ok(converted) => Ok({destination}(converted)), Err(error) => Err(error.to_string()) }}"
                )
            } else if *fallible {
                format!(
                    "match jet_unit_conversion_exact({args}) {{ Some(converted) => Ok({destination}(converted)), None => Err(\"unit conversion would round\".to_string()) }}"
                )
            } else {
                format!("{destination}(match jet_unit_conversion_exact({args}) {{ Some(converted) => converted, None => jet_panic({file:?}, {line}, \"unit conversion would round\") }})")
            }
        }
        // D-SIMD2 / D-LINALG1: a math constructor / static method → the prelude free
        // function `{root}jet_math_<T>_<func>(args)`. Args are plain values (floats or
        // a `[T#N]` array) — no borrow/clone decisions.
        TExprKind::MathBuiltin {
            type_name,
            func,
            args,
        } => {
            let parts: Vec<String> = args.iter().map(|a| emit_tir_expr(a, cx)).collect();
            format!(
                "{}jet_math_{}_{}({})",
                cx.root_prefix,
                type_name,
                func,
                parts.join(", ")
            )
        }
        TExprKind::PreciseBuiltin {
            type_name,
            func,
            args,
        } => {
            let parts: Vec<String> = args.iter().map(|a| emit_tir_expr(a, cx)).collect();
            let prefix = if type_name == "BigInt" {
                "jet_bigint"
            } else if type_name == "Fraction" {
                "jet_fraction"
            } else {
                "jet_decimal"
            };
            let call = if func == "from_str" {
                format!("{}{}_{}(&({}))", cx.root_prefix, prefix, func, parts[0])
            } else if func.starts_with("from_") {
                format!(
                    "{}{}_{}({})",
                    cx.root_prefix,
                    prefix,
                    func,
                    parts.join(", ")
                )
            } else if parts.len() == 1 {
                format!("{}{}_{}(&({}))", cx.root_prefix, prefix, func, parts[0])
            } else {
                format!(
                    "{}{}_{}(&({}), &({}))",
                    cx.root_prefix, prefix, func, parts[0], parts[1]
                )
            };
            call
        }
        // c109 Phase 6: the synthetic `.clone()`. Mirrors `emit_method_call`'s
        // `clone` early return: `(recv).clone()`, no deref/borrow decision (the
        // receiver was already lowered to the place the AST path would clone).
        TExprKind::Clone(recv) => {
            format!("({}).clone()", emit_tir_expr(recv, cx))
        }
        TExprKind::Borrow { place, mutable } => {
            if let TExprKind::Local(local) = &place.kind {
                if local.uninit_fixed {
                    return if *mutable {
                        format!("({}).as_array_mut()", local.rust_place())
                    } else {
                        format!("({}).as_array()", local.rust_place())
                    };
                }
            }
            let place = emit_tir_expr(place, cx);
            if *mutable {
                format!("&mut ({place})")
            } else {
                format!("&({place})")
            }
        }
        // D-MEM1 stage S5: `copy d` on a string-view local — `.to_string()`,
        // not `.clone()` (see the node's doc comment for why).
        TExprKind::MaterializeView(recv) => {
            format!("({}).to_string()", emit_tir_expr(recv, cx))
        }
        // c109 Phase 23: `.raw()` on a distinct type → `({recv}).0`. Mirrors
        // `emit_method_call`'s `METHOD_DISTINCT_RAW` early return byte-for-byte.
        TExprKind::DistinctRaw(recv) => {
            format!("({}).0", emit_tir_expr(recv, cx))
        }
        // c109 Phase 6: a user instance method call. Mirrors `emit_method_call`'s
        // final dispatch (`(recv).{method}({args})`): Rust's method autoref handles
        // the `&self`/`&mut self`/`self` receiver convention, so codegen emits the
        // receiver place as-is. The method name + arg wrappers were resolved at
        // lowering — emit only formats.
        TExprKind::MethodCall {
            recv,
            method,
            type_args,
            args,
            operator_line,
            ..
        } => {
            let method_rust = method.rust();
            let arg_str = emit_tir_call_args(args, cx);
            if let Some(line) = operator_line {
                let trait_name = match method_rust.as_str() {
                    "add" => "Add",
                    "sub" => "Sub",
                    "mul" => "Mul",
                    "div" => "Div",
                    _ => unreachable!("operator_line is only set for arithmetic hooks"),
                };
                return format!(
                    "user_{trait_name}::__jet_{method_rust}_at(&({}), {arg_str}, {:?}, {line})",
                    emit_tir_expr(recv, cx),
                    cx.file,
                );
            }
            let type_args = if type_args.is_empty() {
                String::new()
            } else {
                format!(
                    "::<{}>",
                    type_args
                        .iter()
                        .map(|ty| cx.rust_type(ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            format!(
                "({}).{}{}({})",
                emit_tir_expr(recv, cx),
                method_rust,
                type_args,
                arg_str
            )
        }
        // c109 Phase 27: a call through a fn-typed struct field. Mirrors the AST
        // `emit_method_call` fn-field branch: `(({recv}).{field})({args})`.
        TExprKind::FnFieldCall { recv, field, args } => {
            let arg_str = emit_tir_call_args(args, cx);
            let field_rust = emit_field_rust(cx, &recv.ty, field);
            format!(
                "(({}).{})({})",
                emit_tir_expr(recv, cx),
                field_rust,
                arg_str
            )
        }
        // c109 Phase 7: a static method call. Mirrors the AST type-name dispatch:
        // `user_<Type>::user_<method>(args)`. All facts resolved at lowering.
        TExprKind::StaticCall {
            owner,
            owner_type,
            method,
            type_args,
            args,
        } => {
            let arg_str = emit_tir_call_args(args, cx);
            let owner = match owner_type {
                Some(ty @ Type::Apply { .. }) => format!("<{}>", cx.rust_type(ty)),
                Some(ty) => cx.rust_type(ty),
                None => emit_static_owner(owner, cx),
            };
            let type_args = if type_args.is_empty() {
                String::new()
            } else {
                format!(
                    "::<{}>",
                    type_args
                        .iter()
                        .map(|ty| cx.rust_type(ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            format!("{}::{}{}({})", owner, method.rust(), type_args, arg_str)
        }
        // D-VALIDATE-DECODE1=B: preserve a child Result's success value and
        // frame every accumulated FieldError at the generated boundary.
        TExprKind::DecodeUnder { segment, inner } => format!(
            "jet_std::FieldError::under(&({}), {})",
            emit_tir_expr(segment, cx),
            emit_tir_expr(inner, cx)
        ),
        // c109 Phase 9: a built-in collection/string method. The Map-vs-List-vs-String
        // branch was resolved into `op` at lowering; emit only formats, reproducing
        // `emit_builtin_method` (Source/Codegen/Expression.rs) byte-for-byte. Args are
        // emitted PLAINLY (no clone/borrow wrappers — `arg(i)` is a raw `emit_expr`).
        TExprKind::BuiltinMethod { recv, op, args } => {
            // Eager fuse: `list.map(f).to_list()` → `jet_list_map` so borrowed
            // Fn captures need not be `'static` (E0521 under `jet_iter_map`).
            // Only fuse a real List/FixedList receiver — `filter(…).map(…).to_list()`
            // (and any other Iter→map chain) must stay on `jet_iter_map` + `.to_list()`;
            // cloning a move-only `JetIter` is E0599.
            if matches!(op, TBuiltinOp::IterToList | TBuiltinOp::IterCollect) {
                if let TExprKind::ClosureMethod {
                    recv: map_recv,
                    op: map_op,
                    args: map_args,
                } = &recv.kind
                {
                    if matches!(map_op, TClosureOp::Map | TClosureOp::MapMut)
                        && matches!(
                            map_recv.ty,
                            Type::List(_) | Type::FixedList { .. }
                        )
                    {
                        let list = emit_tir_expr(map_recv, cx);
                        let mut f = map_args
                            .first()
                            .map(|e| emit_tir_expr(e, cx))
                            .unwrap_or_default();
                        if let Some(rest) = f.strip_prefix("move ") {
                            f = rest.to_string();
                        }
                        let helper = if matches!(map_op, TClosureOp::MapMut) {
                            "jet_list_map_mut"
                        } else {
                            "jet_list_map"
                        };
                        return format!("{helper}(({list}).clone(), {f})");
                    }
                }
            }
            let recv_is_iter = crate::Collections::is_iter_type(&recv.ty);
            let recv = emit_tir_expr(recv, cx);
            let a = |i: usize| {
                args.get(i)
                    .map(|e| emit_tir_expr(e, cx))
                    .unwrap_or_default()
            };
            // D-ITERTOOLS1=A: Iter is IntoIterator; List clones. Adapters start from JetIter.
            let vec_src = if recv_is_iter {
                format!("({recv})")
            } else {
                format!("({recv}).clone()")
            };
            let as_iter = if recv_is_iter {
                format!("({recv})")
            } else {
                format!("jet_iter_from_vec(({recv}).clone())")
            };
            match op {
                TBuiltinOp::LenString => format!("jet_char_len(&({}))", recv),
                TBuiltinOp::LenList => {
                    if recv_is_iter {
                        format!("({recv}).len()")
                    } else {
                        format!("({recv}).len() as i64")
                    }
                }
                TBuiltinOp::IsEmpty => {
                    if recv_is_iter {
                        format!("({recv}).is_empty()")
                    } else {
                        format!("({recv}).is_empty()")
                    }
                }
                TBuiltinOp::Push => format!("({}).push({})", recv, a(0)),
                TBuiltinOp::Pop => format!("jet_outcome_of(({}).pop())", recv),
                TBuiltinOp::InsertMap => {
                    format!("jet_outcome_of(({}).insert(({}).clone(), {}))", recv, a(0), a(1))
                }
                TBuiltinOp::AddNewMap => format!(
                    "match ({}).entry(({}).clone()) {{ std::collections::btree_map::Entry::Vacant(e) => {{ e.insert({}); true }}, std::collections::btree_map::Entry::Occupied(_) => false }}",
                    recv, a(0), a(1)
                ),
                TBuiltinOp::MapMerge => {
                    format!("jet_map_merge(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::MapMergeWith => {
                    // Call through so both bare closures and `Rc`/`Box` wrappers work.
                    format!(
                        "jet_map_merge_with(&({}), &({}), |__k, __a, __b| ({})(__k, __a, __b))",
                        recv,
                        a(0),
                        a(1)
                    )
                }
                TBuiltinOp::InsertList => {
                    format!("({}).insert({} as usize, {})", recv, a(0), a(1))
                }
                TBuiltinOp::RemoveMap => format!("jet_outcome_of(({}).remove(&({}).clone()))", recv, a(0)),
                TBuiltinOp::RemoveList { line, mode } => match mode {
                    crate::Codegen::TIR::ListRemoveMode::Value => format!(
                        "jet_list_remove_value(&mut ({}), {}, {:?}, {})",
                        recv,
                        a(0),
                        cx.file,
                        line
                    ),
                    crate::Codegen::TIR::ListRemoveMode::Slot => format!(
                        "jet_list_remove_slot(&mut ({}), {}, {:?}, {})",
                        recv,
                        a(0),
                        cx.file,
                        line
                    ),
                    crate::Codegen::TIR::ListRemoveMode::Dynamic => format!(
                        "match ({}) {{ JetRemoveBy::Val => jet_list_remove_value(&mut ({}), {}, {:?}, {}), JetRemoveBy::Slot => jet_list_remove_slot(&mut ({}), {}, {:?}, {}) }}",
                        a(1), recv, a(0), cx.file, line, recv, a(0), cx.file, line
                    ),
                },
                TBuiltinOp::PriorityQueueRemove { line, mode } => match mode {
                    crate::Codegen::TIR::ListRemoveMode::Value => format!(
                        "jet_priority_queue_remove_value(&mut ({}), {})",
                        recv,
                        a(0)
                    ),
                    crate::Codegen::TIR::ListRemoveMode::Slot => format!(
                        "jet_priority_queue_remove_slot(&mut ({}), {}, {:?}, {})",
                        recv,
                        a(0),
                        cx.file,
                        line
                    ),
                    crate::Codegen::TIR::ListRemoveMode::Dynamic => format!(
                        "match ({}) {{ JetRemoveBy::Val => jet_priority_queue_remove_value(&mut ({}), {}), JetRemoveBy::Slot => jet_priority_queue_remove_slot(&mut ({}), {}, {:?}, {}) }}",
                        a(1), recv, a(0), recv, a(0), cx.file, line
                    ),
                },
                TBuiltinOp::CountList => {
                    format!("jet_list_count(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::ExtendList => {
                    format!("({}).extend(({}).clone())", recv, a(0))
                }
                TBuiltinOp::ConcatList => {
                    format!("jet_list_concat(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::GetMap => format!("jet_outcome_of(({}).get(&({}).clone()).cloned())", recv, a(0)),
                TBuiltinOp::GetList => format!("jet_outcome_of(({}).get({} as usize).cloned())", recv, a(0)),
                TBuiltinOp::First => format!("jet_outcome_of(({}).first().cloned())", recv),
                TBuiltinOp::Last => format!("jet_outcome_of(({}).last().cloned())", recv),
                TBuiltinOp::Contains => format!("({}).contains(&{})", recv, a(0)),
                TBuiltinOp::IndexOf => format!(
                    "jet_outcome_of(({}).iter().position(|x| *x == {}).map(|i| i as i64))",
                    recv,
                    a(0)
                ),
                TBuiltinOp::Reverse => format!("({}).reverse()", recv),
                TBuiltinOp::Sort => format!("({}).sort()", recv),
                // View/ViewMut are slices (`&[T]` / `&mut [T]`) — never `.clone()` the
                // receiver (ViewMut is not Clone). Iter consumes; List/View borrow via `.iter()`.
                TBuiltinOp::JoinSep => {
                    if recv_is_iter {
                        format!(
                            "({recv}).into_iter().map(|x| x.jet_show()).collect::<Vec<_>>().join(({}).as_str())",
                            a(0)
                        )
                    } else {
                        format!(
                            "({recv}).iter().map(|x| x.jet_show()).collect::<Vec<_>>().join(({}).as_str())",
                            a(0)
                        )
                    }
                }
                TBuiltinOp::Sum { float: true } => format!(
                    "{vec_src}.into_iter().fold(0.0, |__acc, __item| __acc + __item)"
                ),
                TBuiltinOp::Sum { float: false } => {
                    format!("jet_list_sum({vec_src})")
                }
                TBuiltinOp::Product { float: true } => format!(
                    "{vec_src}.into_iter().fold(1.0, |__acc, __item| __acc * __item)"
                ),
                TBuiltinOp::Product { float: false } => {
                    format!("jet_list_product({vec_src})")
                }
                TBuiltinOp::Min { float: true } => {
                    format!("jet_outcome_of({vec_src}.into_iter().reduce(|a, b| a.min(b)))")
                }
                TBuiltinOp::Max { float: true } => {
                    format!("jet_outcome_of({vec_src}.into_iter().reduce(|a, b| a.max(b)))")
                }
                TBuiltinOp::Min { float: false } => format!("jet_outcome_of({vec_src}.into_iter().min())"),
                TBuiltinOp::Max { float: false } => format!("jet_outcome_of({vec_src}.into_iter().max())"),
                TBuiltinOp::Flatten => format!("jet_iter_flatten({as_iter})"),
                TBuiltinOp::Intersperse => {
                    format!("jet_iter_intersperse({as_iter}, {})", a(0))
                }
                TBuiltinOp::Unzip { tuple_struct } => format!(
                    "{{ let mut __a = Vec::new(); let mut __b = Vec::new(); for __x in {vec_src} {{ __a.push(__x.user_a); __b.push(__x.user_b); }} {} {{ user_a: __a, user_b: __b }} }}",
                    tuple_struct
                ),
                TBuiltinOp::Clear => format!("({}).clear()", recv),
                TBuiltinOp::Chars => format!("({}).chars().collect::<Vec<char>>()", recv),
                TBuiltinOp::Bytes => {
                    format!("{}jet_string_bytes(&({}))", cx.root_prefix, recv)
                }
                TBuiltinOp::Trim => format!("jet_unicode_trim(&({}))", recv),
                TBuiltinOp::TrimStart => format!("{}jet_text_trim_start(&({}))", cx.root_prefix, recv),
                TBuiltinOp::TrimEnd => format!("{}jet_text_trim_end(&({}))", cx.root_prefix, recv),
                TBuiltinOp::Split => {
                    format!("jet_iter_string_split(&({}), &{})", recv, a(0))
                },
                // c97/D-STRPARSE1: `lines()` → `jet_string_lines` (imported via MOD_USE;
                // same bare-call convention as `jet_iter_string_split`).
                TBuiltinOp::Lines => format!("jet_string_lines(&({}))", recv),
                TBuiltinOp::ParseInt => format!(
                    "{{ let __jet_text = &({recv}); __jet_text.trim().parse::<i64>()\
                     .map_err(|_| format!(\"cannot parse `{{}}` as an integer\", __jet_text)) }}"
                ),
                TBuiltinOp::ParseFloat => format!(
                    "{{ let __jet_text = &({recv}); __jet_text.trim().parse::<f64>()\
                     .map_err(|_| format!(\"cannot parse `{{}}` as a float\", __jet_text)) }}"
                ),
                TBuiltinOp::StartsWith => format!("({}).starts_with(&{})", recv, a(0)),
                TBuiltinOp::EndsWith => format!("({}).ends_with(&{})", recv, a(0)),
                TBuiltinOp::Replace => format!("({}).replace(&{}, &{})", recv, a(0), a(1)),
                TBuiltinOp::PadStart => format!("{}jet_text_pad_start(&({}), {}, &({}))", cx.root_prefix, recv, a(0), a(1)),
                TBuiltinOp::PadEnd => format!("{}jet_text_pad_end(&({}), {}, &({}))", cx.root_prefix, recv, a(0), a(1)),
                TBuiltinOp::StringIndexOf => format!("{0}jet_outcome_of({0}jet_unicode_index_of(&({1}), &({2})))", cx.root_prefix, recv, a(0)),
                TBuiltinOp::StringCount => format!("{}jet_unicode_count(&({}), &({}))", cx.root_prefix, recv, a(0)),
                TBuiltinOp::StringIsAlphabetic => format!("{}jet_text_is_alphabetic(&({}))", cx.root_prefix, recv),
                TBuiltinOp::StringIsNumeric => format!("{}jet_text_is_numeric(&({}))", cx.root_prefix, recv),
                TBuiltinOp::StringIsWhitespace => format!("{}jet_text_is_whitespace(&({}))", cx.root_prefix, recv),
                TBuiltinOp::StringIsAscii => format!("{}jet_text_unicode_is_ascii(&({}))", cx.root_prefix, recv),
                TBuiltinOp::StringToTitle => format!("{}jet_text_title(&({}))", cx.root_prefix, recv),
                TBuiltinOp::StringMethod { method } => match method.as_str() {
                    "last_index_of" => format!(
                        "{0}jet_outcome_of({0}jet_unicode_last_index_of(&({1}), &({2})))",
                        cx.root_prefix,
                        recv,
                        a(0)
                    ),
                    "is_lower" => format!("{}jet_text_is_lower(&({}))", cx.root_prefix, recv),
                    "is_upper" => format!("{}jet_text_is_upper(&({}))", cx.root_prefix, recv),
                    "capitalize" => format!("{}jet_text_capitalize(&({}))", cx.root_prefix, recv),
                    "swapcase" => format!("{}jet_text_swapcase(&({}))", cx.root_prefix, recv),
                    "remove_prefix" => format!(
                        "{}jet_text_remove_prefix(&({}), &({}))",
                        cx.root_prefix,
                        recv,
                        a(0)
                    ),
                    "remove_suffix" => format!(
                        "{}jet_text_remove_suffix(&({}), &({}))",
                        cx.root_prefix,
                        recv,
                        a(0)
                    ),
                    "compare" => format!(
                        "{}jet_text_compare(&({}), &({}))",
                        cx.root_prefix,
                        recv,
                        a(0)
                    ),
                    "equal" => format!("({}) == ({})", recv, a(0)),
                    "copy" => format!("({}).clone()", recv),
                    "reverse" => format!("{}jet_text_reverse(&({}))", cx.root_prefix, recv),
                    "normalize" => {
                        format!("{}jet_text_normalize_nfc(&({}))", cx.root_prefix, recv)
                    }
                    "rsplit" => format!("jet_iter_string_rsplit(&({}), &{})", recv, a(0)),
                    // D-STR-DECLINE1=C: `matches`/`match` — compile the pattern
                    // through the one core.regex engine, then run the same
                    // `jet_regex_is_match`/`jet_regex_find` `jet.regex.is_match`/
                    // `jet.regex.find` already call.
                    "matches" => format!(
                        "{root}jet_std::jet_regex_compile(&({pattern})).map(|__jet_re| {root}jet_std::jet_regex_is_match(&__jet_re, &({recv})))",
                        root = cx.root_prefix,
                        pattern = a(0),
                        recv = recv,
                    ),
                    "match" => format!(
                        "{root}jet_std::jet_regex_compile(&({pattern})).map(|__jet_re| {root}jet_std::jet_regex_find(&__jet_re, &({recv})))",
                        root = cx.root_prefix,
                        pattern = a(0),
                        recv = recv,
                    ),
                    other => format!(
                        "compile_error!(\"unknown StringMethod: {}\")",
                        other
                    ),
                },
                TBuiltinOp::StringSplitOnce { tuple_struct } => format!(
                    "{0}jet_outcome_of({0}jet_unicode_split_once(&({1}), &({2})).map(|(__before, __after)| {3} {{ user_before: __before, user_after: __after }}))",
                    cx.root_prefix, recv, a(0), tuple_struct
                ),
                TBuiltinOp::ToUpper => format!("jet_unicode_upper(&({}))", recv),
                TBuiltinOp::ToLower => format!("jet_unicode_lower(&({}))", recv),
                TBuiltinOp::Repeat => format!("({}).repeat({} as usize)", recv, a(0)),
                TBuiltinOp::Slice { line } => format!(
                    "jet_string_slice(&({}), {}, {}, {:?}, {})",
                    recv,
                    a(0),
                    a(1),
                    cx.file,
                    line
                ),
                // D-STR-AFTER1: `after`/`before` — bare calls, no root prefix (same
                // MOD_USE-imported convention as `jet_iter_string_split`/`jet_string_lines`).
                TBuiltinOp::After => format!("jet_string_after(&({}), &{})", recv, a(0)),
                TBuiltinOp::Before => format!("jet_string_before(&({}), &{})", recv, a(0)),
                // D-MEM1 stage S5: zero-copy siblings, `Stmt::Val` lowering only
                // (see `lower.rs`'s `b.string_view` branch) — bare calls, no
                // `.to_string()`, no root prefix (same convention as `After`/`Before`).
                TBuiltinOp::TrimView => format!("jet_unicode_trim_view(&({}))", recv),
                TBuiltinOp::AfterView => format!("jet_string_after_view(&({}), &{})", recv, a(0)),
                TBuiltinOp::BeforeView => format!("jet_string_before_view(&({}), &{})", recv, a(0)),
                TBuiltinOp::Keys => format!("jet_map_keys(&({}))", recv),
                TBuiltinOp::Values => format!("jet_map_values(&({}))", recv),
                TBuiltinOp::ContainsKey => format!("({}).contains_key(&{})", recv, a(0)),
                TBuiltinOp::ToString => format!("({}).jet_show()", recv),
                // D-REGEXENGINE1=A: `Match.group(n)` on the std-only match value.
                TBuiltinOp::MatchGroup => {
                    format!("({}).group({})", recv, a(0))
                }
                // D-COLLBREADTH1=A: Set<T> operations.
                TBuiltinOp::SetFrom => {
                    if recv_is_iter {
                        format!("jet_iter_to_set({recv})")
                    } else {
                        format!(
                            "({}).into_iter().collect::<std::collections::HashSet<_>>()",
                            recv
                        )
                    }
                }
                TBuiltinOp::SetInsert => format!("({}).insert({})", recv, a(0)),
                TBuiltinOp::SetRemove => format!("{{({}).remove(&{});}}", recv, a(0)),
                TBuiltinOp::SetToList => {
                    // Belt: Iter mis-resolved as Set still materializes via JetIter.
                    if recv_is_iter {
                        format!("({recv}).to_list()")
                    } else {
                        format!("({}).iter().cloned().collect::<Vec<_>>()", recv)
                    }
                }
                TBuiltinOp::SetUnion => format!("jet_set_union(&({}), &({}))", recv, a(0)),
                TBuiltinOp::SetIntersection => {
                    format!("jet_set_intersection(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SetDifference => {
                    format!("jet_set_difference(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SetSymmetricDifference => {
                    format!("jet_set_symmetric_difference(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SetIsSubset => format!("jet_set_is_subset(&({}), &({}))", recv, a(0)),
                TBuiltinOp::SetIsSuperset => {
                    format!("jet_set_is_superset(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SetIsDisjoint => {
                    format!("jet_set_is_disjoint(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SetCopy => format!("({}).clone()", recv),
                TBuiltinOp::SetEqual => format!("({}) == ({})", recv, a(0)),
                TBuiltinOp::SetCapacity => format!("({}).capacity() as i64", recv),
                TBuiltinOp::SetFirst => format!("jet_outcome_of(({}).iter().next().cloned())", recv),
                // #1478: `values()` is the same materialization as `to_list()`,
                // wrapped lazy (I8 — one mechanism, `Iter<T>` typed).
                TBuiltinOp::SetValues => format!(
                    "jet_iter_from_vec(({}).iter().cloned().collect::<Vec<_>>())",
                    recv
                ),
                // #1478: native swap-in — inserts `a0`, returns the old equal
                // element if one was present (Rust's own `HashSet::replace`).
                TBuiltinOp::SetReplace => format!("jet_outcome_of(({}).replace({}))", recv, a(0)),
                // #1478: native remove-and-return — Rust's own `HashSet::take`.
                TBuiltinOp::SetTake => format!("jet_outcome_of(({}).take(&({})))", recv, a(0)),
                // D-SET-DECLINE1=C: `set.sort()` — materialize then sort the
                // copy, same as `set.to_list()` followed by `List.sort()`.
                TBuiltinOp::SetSort => format!(
                    "{{ let mut __jet_v = ({}).iter().cloned().collect::<Vec<_>>(); __jet_v.sort(); __jet_v }}",
                    recv
                ),
                // D-SET-DECLINE1=C: `set.shuffle()` — the same `jet_iter_shuffle`
                // engine `List.shuffle()` runs, materialized back to a `List`.
                TBuiltinOp::SetShuffle => format!(
                    "jet_iter_shuffle(jet_iter_from_vec(({}).iter().cloned().collect::<Vec<_>>())).to_list()",
                    recv
                ),
                TBuiltinOp::SortedSetFrom => {
                    format!(
                        "({}).into_iter().collect::<std::collections::BTreeSet<_>>()",
                        recv
                    )
                }
                TBuiltinOp::SortedSetInsert => format!("({}).insert({})", recv, a(0)),
                TBuiltinOp::SortedSetRemove => format!("{{({}).remove(&{});}}", recv, a(0)),
                TBuiltinOp::SortedSetToList => {
                    format!("({}).iter().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::SortedSetUnion => {
                    format!("jet_sorted_set_union(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SortedSetIntersection => {
                    format!("jet_sorted_set_intersection(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SortedSetDifference => {
                    format!("jet_sorted_set_difference(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SortedSetSymmetricDifference => {
                    format!("jet_sorted_set_symmetric_difference(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SortedSetIsSubset => {
                    format!("jet_sorted_set_is_subset(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SortedSetIsSuperset => {
                    format!("jet_sorted_set_is_superset(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::SortedSetIsDisjoint => {
                    format!("jet_sorted_set_is_disjoint(&({}), &({}))", recv, a(0))
                }
                TBuiltinOp::PriorityQueueFrom => {
                    format!(
                        "({}).into_iter().collect::<std::collections::BinaryHeap<_>>()",
                        recv
                    )
                }
                TBuiltinOp::PriorityQueuePeek => format!("jet_outcome_of(({}).peek().cloned())", recv),
                TBuiltinOp::PriorityQueueToSortedList => {
                    format!("({}).clone().into_sorted_vec().into_iter().rev().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::LruPut => format!("({}).put({}, {})", recv, a(0), a(1)),
                TBuiltinOp::LruAddNew => format!("({}).add_new({}, {})", recv, a(0), a(1)),
                TBuiltinOp::LruGet => format!("jet_outcome_of(({}).get(&{}))", recv, a(0)),
                TBuiltinOp::LruCapacity => format!("({}).capacity()", recv),
                TBuiltinOp::LruKeys => format!("({}).keys()", recv),
                TBuiltinOp::BitSetAdd => format!("({}).add({})", recv, a(0)),
                TBuiltinOp::BitSetRemove => format!("({}).remove(&{})", recv, a(0)),
                TBuiltinOp::BitSetCount => format!("({}).count()", recv),
                TBuiltinOp::BitSetToList => format!("({}).to_list()", recv),
                TBuiltinOp::BitSetNew => "JetBitSet::new()".to_string(),
                TBuiltinOp::ByteBufferNew => "JetByteBuffer::new()".to_string(),
                TBuiltinOp::ByteBufferWithCapacity => {
                    format!("JetByteBuffer::with_capacity({})", recv)
                }
                TBuiltinOp::ByteBufferFrom => format!("JetByteBuffer::from(&({}))", recv),
                TBuiltinOp::ByteBufferWrite { method } => {
                    if matches!(method.as_str(), "write_bytes" | "write") {
                        format!("({}).{}(&{})", recv, method, a(0))
                    } else {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                }
                TBuiltinOp::ByteBufferToBytes => format!("({}).to_bytes()", recv),
                TBuiltinOp::ByteBufferMethod { method } => match method.as_str() {
                    "clone" | "copy" => format!("({}).clone()", recv),
                    "contains" | "starts_with" | "ends_with" | "split" | "index_of"
                    | "last_index_of" | "join" => {
                        format!("({}).{}(&{})", recv, method, a(0))
                    }
                    "replace" => format!("({}).{}(&{}, &{})", recv, method, a(0), a(1)),
                    "equal" | "compare" => format!("({}).{}(&{})", recv, method, a(0)),
                    "copy_to" | "write_to" => format!("({}).{}(&mut {})", recv, method, a(0)),
                    "get" | "seek" | "read_bytes" | "read_string" => {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                    _ => format!("({}).{}()", recv, method),
                },
                // D-TAG1: Bag<T> counted multiset.
                TBuiltinOp::BagAdd => format!(
                    "{{ *({}).entry({}).or_insert(0) += 1; true }}",
                    recv,
                    a(0)
                ),
                TBuiltinOp::BagRemove => format!(
                    "{{ if let Some(c) = ({recv}).get_mut(&{arg}) {{ *c -= 1; if *c == 0 {{ ({recv}).remove(&{arg}); }} }} }}",
                    recv = recv,
                    arg = a(0)
                ),
                TBuiltinOp::BagHas => format!(
                    "({}).get(&{}).copied().unwrap_or(0) > 0",
                    recv,
                    a(0)
                ),
                TBuiltinOp::BagCount => format!(
                    "({}).get(&{}).copied().unwrap_or(0) as i64",
                    recv,
                    a(0)
                ),
                TBuiltinOp::BagLen => format!(
                    "({}).values().sum::<usize>() as i64",
                    recv
                ),
                // D-COLLBREADTH1=A: Deque<T> operations.
                TBuiltinOp::DequePushFront => format!("({}).push_front({})", recv, a(0)),
                TBuiltinOp::DequePushBack => format!("({}).push_back({})", recv, a(0)),
                TBuiltinOp::DequePopFront => format!("jet_outcome_of(({}).pop_front())", recv),
                TBuiltinOp::DequePopBack => format!("jet_outcome_of(({}).pop_back())", recv),
                TBuiltinOp::DequePeekFront => format!("jet_outcome_of(({}).front().cloned())", recv),
                TBuiltinOp::DequePeekBack => format!("jet_outcome_of(({}).back().cloned())", recv),
                TBuiltinOp::DequeCapacity => format!("({}).capacity() as i64", recv),
                TBuiltinOp::DequeContains => {
                    format!("({}).iter().any(|__x| *__x == ({}))", recv, a(0))
                }
                TBuiltinOp::DequeGet => {
                    format!("jet_outcome_of(({}).get(({}) as usize).cloned())", recv, a(0))
                }
                TBuiltinOp::DequeDelete => format!(
                    "{{ let __dq = &mut ({}); let __v = ({}); if let Some(__i) = __dq.iter().position(|__x| *__x == __v) {{ __dq.remove(__i); }} }}",
                    recv,
                    a(0)
                ),
                TBuiltinOp::DequeToList => {
                    format!("({}).iter().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::DequeJoin => format!(
                    "({}).iter().map(|x| x.jet_show()).collect::<Vec<_>>().join(({}).as_str())",
                    recv,
                    a(0)
                ),
                TBuiltinOp::DequeReverse => format!("({}).make_contiguous().reverse()", recv),
                TBuiltinOp::DequeSplit => {
                    format!("({}).split_off(({}) as usize)", recv, a(0))
                }
                TBuiltinOp::DequeFrom => {
                    format!("({}).into_iter().collect::<std::collections::VecDeque<_>>()", recv)
                }
                TBuiltinOp::TryCollect => format!("jet_list_try_collect({vec_src})"),
                // D-DYNARRAY1: `list.view(a..b)` — zero-copy window constructor.
                // `&(recv)` (not `.clone()`): the window borrows the list's OWN
                // backing storage, it never makes a second copy of it.
                TBuiltinOp::ViewNew { line } => {
                    if args.len() == 1 {
                        format!(
                            "jet_view_range_new(&({}), &({}), {:?}, {})",
                            recv,
                            a(0),
                            cx.file,
                            line
                        )
                    } else {
                        format!(
                            "jet_view_new(&({}), {}, {}, {:?}, {})",
                            recv,
                            a(0),
                            a(1),
                            cx.file,
                            line
                        )
                    }
                }
                TBuiltinOp::ViewMutNew { line } => {
                    if args.len() == 1 {
                        format!(
                            "jet_view_mut_range_new(&mut ({}), &({}), {:?}, {})",
                            recv,
                            a(0),
                            cx.file,
                            line
                        )
                    } else {
                        format!(
                            "jet_view_mut_new(&mut ({}), {}, {}, {:?}, {})",
                            recv,
                            a(0),
                            a(1),
                            cx.file,
                            line
                        )
                    }
                }
                TBuiltinOp::ComputeViewNew { line } => {
                    if args.len() == 1 {
                        format!(
                            "{}jet_compute_view_range(&({}), &({}), {:?}, {})",
                            cx.root_prefix,
                            recv,
                            a(0),
                            cx.file,
                            line
                        )
                    } else {
                        format!(
                            "{}jet_compute_view(&({}), {}, {}, false, {:?}, {})",
                            cx.root_prefix,
                            recv,
                            a(0),
                            a(1),
                            cx.file,
                            line
                        )
                    }
                }
                TBuiltinOp::ComputeViewMutNew { line } => {
                    if args.len() == 1 {
                        format!(
                            "{}jet_compute_view_mut_range(&mut ({}), &({}), {:?}, {})",
                            cx.root_prefix,
                            recv,
                            a(0),
                            cx.file,
                            line
                        )
                    } else {
                        format!(
                            "{}jet_compute_view_mut(&mut ({}), {}, {}, false, {:?}, {})",
                            cx.root_prefix,
                            recv,
                            a(0),
                            a(1),
                            cx.file,
                            line
                        )
                    }
                }
                TBuiltinOp::SplitWrite { tuple_struct } => format!(
                    "jet_split_write(&mut ({}), {}).map(|(left, right)| {} {{ user_left: left, user_right: right }})",
                    recv,
                    a(0),
                    tuple_struct
                ),
                TBuiltinOp::GetDisjointWrite => format!(
                    "jet_get_disjoint_write(&mut ({}), &({}))",
                    recv,
                    a(0)
                ),
                // D-ITERTOOLS1=A: non-closure lazy adapters return JetIter (no eager Vec).
                TBuiltinOp::Take => format!("jet_iter_take({as_iter}, {})", a(0)),
                TBuiltinOp::Skip => format!("jet_iter_skip({as_iter}, {})", a(0)),
                TBuiltinOp::StepBy => format!("jet_iter_step_by({as_iter}, {})", a(0)),
                TBuiltinOp::Dedup => format!("jet_iter_dedup({as_iter})"),
                TBuiltinOp::Chunks => format!("jet_iter_chunks({as_iter}, {})", a(0)),
                TBuiltinOp::Windows => format!("jet_iter_windows({as_iter}, {})", a(0)),
                TBuiltinOp::IterRepeat => format!("jet_iter_repeat({as_iter}, {})", a(0)),
                TBuiltinOp::IterCycle => format!("jet_iter_cycle({as_iter}, {})", a(0)),
                TBuiltinOp::IterDropLast => format!("jet_iter_drop_last({as_iter}, {})", a(0)),
                TBuiltinOp::IterShuffle => format!("jet_iter_shuffle({as_iter})"),
                TBuiltinOp::IterIsSorted => format!("jet_iter_is_sorted({as_iter})"),
                TBuiltinOp::IterLastIndexOf => {
                    format!("jet_iter_last_index_of({as_iter}, {})", a(0))
                }
                TBuiltinOp::IterAverage { float: false } => {
                    format!("jet_iter_average_int({as_iter})")
                }
                TBuiltinOp::IterAverage { float: true } => {
                    format!("jet_iter_average_float({as_iter})")
                }
                TBuiltinOp::IterCompare => {
                    format!("jet_iter_compare({as_iter}, ({}).clone())", a(0))
                }
                TBuiltinOp::IterSplit { tuple_struct } => format!(
                    "jet_iter_split_at({as_iter}, {}, |left, right| {} {{ user_left: left, user_right: right }})",
                    a(0),
                    tuple_struct
                ),
                TBuiltinOp::ListSlice => format!("jet_list_slice(&({recv}), {}, {})", a(0), a(1)),
                TBuiltinOp::ListCopy => format!("({recv}).clone()"),
                TBuiltinOp::ListEqual => format!("({}) == ({})", recv, a(0)),
                TBuiltinOp::ListBinarySearch => format!("jet_list_binary_search(&({recv}), &({}))", a(0)),
                TBuiltinOp::ListUnion => format!("jet_list_union(&({recv}), &({}))", a(0)),
                TBuiltinOp::ListIntersection => format!("jet_list_intersection(&({recv}), &({}))", a(0)),
                TBuiltinOp::ListDifference => format!("jet_list_difference(&({recv}), &({}))", a(0)),
                TBuiltinOp::ListRandom => format!("jet_list_random(&({recv}))"),
                TBuiltinOp::ListMinMax { tuple_struct } => format!(
                    "jet_list_min_max(&({recv}), |min, max| {} {{ user_min: min, user_max: max }})",
                    tuple_struct
                ),
                TBuiltinOp::MapCopy => format!("jet_map_copy(&({recv}))"),
                TBuiltinOp::MapEqual => format!("jet_map_equal(&({recv}), &({}))", a(0)),
                TBuiltinOp::MapFirst => format!("jet_map_first_key(&({recv}))"),
                TBuiltinOp::MapToList { tuple_struct } => format!(
                    "jet_map_to_list(&({recv}), |key, value| {} {{ user_key: key, user_value: value }})",
                    tuple_struct
                ),
                TBuiltinOp::MapMin => format!("jet_map_min_value(&({recv}))"),
                TBuiltinOp::MapMax => format!("jet_map_max_value(&({recv}))"),
                TBuiltinOp::MapIntersection => format!("jet_map_intersection(&({recv}), &({}))", a(0)),
                TBuiltinOp::MapSliceKeys => format!("jet_map_slice_keys(&({recv}), ({}).clone())", a(0)),
                TBuiltinOp::MapNew => "JetMap::new()".to_string(),
                TBuiltinOp::MapFromKeys => format!("jet_map_from_keys(({recv}).clone(), ({}).clone())", a(0)),
                TBuiltinOp::MapContainsValue => format!("jet_map_contains_value(&({recv}), &({}))", a(0)),
                TBuiltinOp::MapPopFirst => format!("jet_map_pop_first(&mut ({recv}))"),
                TBuiltinOp::ListReplace => format!("jet_list_replace(&({recv}), &({}), ({}).clone())", a(0), a(1)),
                TBuiltinOp::Indexed { tuple_struct } => format!(
                    "jet_iter_enumerate({as_iter}, |i, x| {} {{ user_idx: i, user_item: x }})",
                    tuple_struct
                ),
                TBuiltinOp::Indexes => format!("jet_iter_indexes(({recv}).len() as i64)"),
                TBuiltinOp::IterToList | TBuiltinOp::IterCollect => {
                    format!("({recv}).to_list()")
                }
                TBuiltinOp::ListLazy => {
                    // D-LOOPMAP1=B: enter the lazy pipeline plane.
                    format!("jet_iter_from_vec(({recv}).clone())")
                }
                TBuiltinOp::Zip {
                    tuple_struct,
                    mode,
                    fields,
                    flatten,
                    input_count,
                    fill_mode,
                    field_types,
                } => {
                    if *input_count == 0 {
                        let elem = crate::Collections::iter_elem(&e.ty)
                            .map(|ty| cx.rust_type(ty))
                            .unwrap_or_else(|| cx.rust_type(&e.ty));
                        return format!("jet_iter_empty::<{elem}>()");
                    }
                    if *input_count == 1 {
                        return as_iter;
                    }
                    let default_pad = matches!(
                        (mode, fill_mode),
                        (
                            crate::Codegen::TIR::TZipMode::Pad,
                            crate::Codegen::TIR::TZipFillMode::DefaultNone
                        )
                    );
                    let mut iters = Vec::with_capacity(*input_count);
                    iters.push(as_iter);
                    for index in 0..(*input_count - 1) {
                        let value = a(index);
                        let is_iter = args
                            .get(index)
                            .is_some_and(|expr| crate::Collections::is_iter_type(&expr.ty));
                        iters.push(if is_iter {
                            format!("({value})")
                        } else {
                            format!("jet_iter_from_vec(({value}).clone())")
                        });
                    }
                    if default_pad {
                        for iter in &mut iters {
                            *iter = format!("jet_iter_some({iter})");
                        }
                    }
                    let fill_expr = |index: usize| -> String {
                        match fill_mode {
                            crate::Codegen::TIR::TZipFillMode::DefaultNone => {
                                format!("Err({}JetAbsent)", cx.root_prefix)
                            }
                            crate::Codegen::TIR::TZipFillMode::Common => {
                                args.get(*input_count - 1)
                                    .map(|expr| {
                                        emit_zip_fill_value(
                                            emit_tir_expr(expr, cx),
                                            &expr.ty,
                                            field_types.get(index),
                                            cx,
                                        )
                                    })
                                    .unwrap_or_else(|| format!("Err({}JetAbsent)", cx.root_prefix))
                            }
                            crate::Codegen::TIR::TZipFillMode::Columns => {
                                let Some(tuple) = args.get(*input_count - 1) else {
                                    return "()".to_string();
                                };
                                let field = fields
                                    .get(index)
                                    .map_or_else(|| format!("column_{index}"), |name| name.clone());
                                let source_ty = match &tuple.ty {
                                    Type::Tuple(tuple_fields) => tuple_fields
                                        .iter()
                                        .find(|(name, _)| name == &field)
                                        .map(|(_, ty)| ty.as_ref()),
                                    _ => None,
                                };
                                let raw = format!("({}).user_{field}", emit_tir_expr(tuple, cx));
                                source_ty.map_or_else(
                                    || format!("({raw}).clone()"),
                                    |source_ty| {
                                        emit_zip_fill_value(
                                            raw.clone(),
                                            source_ty,
                                            field_types.get(index),
                                            cx,
                                        )
                                    },
                                )
                            }
                        }
                    };
                    let nested_fill = |count: usize| -> String {
                        let mut value = fill_expr(0);
                        for index in 1..count {
                            value = format!("({}, {})", value, fill_expr(index));
                        }
                        value
                    };
                    let nested_value = |index: usize| -> String {
                        let mut value = "x".to_string();
                        let mut remaining = *input_count - 1;
                        while remaining > 1 {
                            if index < remaining - 1 {
                                value.push_str(".0");
                                remaining -= 1;
                            } else {
                                value.push_str(".1");
                                break;
                            }
                        }
                        value
                    };
                    let mut current = iters[0].clone();
                    for stage in 0..(*input_count - 1) {
                        let right = &iters[stage + 1];
                        let last = stage + 1 == *input_count - 1;
                        let body = if last {
                            let mut assignments = Vec::with_capacity(*input_count);
                            for index in 0..*input_count {
                                let value = if index + 1 == *input_count {
                                    "y".to_string()
                                } else if *input_count == 2 && !*flatten {
                                    if index == 0 { "x".to_string() } else { "y".to_string() }
                                } else {
                                    nested_value(index)
                                };
                                let field = fields
                                    .get(index)
                                    .map_or_else(|| format!("column_{index}"), |name| name.clone());
                                assignments.push(format!("user_{field}: {value}"));
                            }
                            format!("{} {{ {} }}", tuple_struct, assignments.join(", "))
                        } else {
                            "(x, y)".to_string()
                        };
                        current = match mode {
                            crate::Codegen::TIR::TZipMode::Short => {
                                format!("jet_iter_zip({current}, {right}, |x, y| {body})")
                            }
                            crate::Codegen::TIR::TZipMode::Strict => {
                                format!("jet_iter_zip_strict({current}, {right}, |x, y| {body})")
                            }
                            crate::Codegen::TIR::TZipMode::Pad => format!(
                                "jet_iter_zip_pad({current}, {right}, {}, {}, |x, y| {body})",
                                nested_fill(stage + 1),
                                fill_expr(stage + 1)
                            ),
                        };
                    }
                    current
                }
                // D-HOLE1: `.zip` on `T?` — Rust's native `Option::zip`, wrapped into
                // the named-tuple struct (present only when both operands are).
                TBuiltinOp::OptionZip { tuple_struct, .. } => format!(
                    "({}).clone().zip(({}).clone())\
                     .map(|(x, y)| {} {{ user_a: x, user_b: y }})",
                    recv,
                    a(0),
                    tuple_struct
                ),
            }
        }
        // c109 Phase 12: a numeric predicate / bit-pop / width-conversion method. The
        // width source/target + widening-vs-narrowing branch were resolved into `op` at
        // lowering; emit only formats, reproducing `emit_builtin_method`'s numeric arms
        // + `numeric_conversion` (Source/Codegen/Expression.rs) byte-for-byte.
        TExprKind::NumericMethod { recv, op } => {
            let recv = emit_tir_expr(recv, cx);
            emit_numeric_op(&recv, op, cx)
        }
        // c109 Phase 28: an overflow opt-out builtin. `prefix`/`op` were resolved at
        // lowering; reproduce `emit_call`'s `(ls).{name}_{suffix}(rs)` byte-for-byte.
        TExprKind::OverflowOpt {
            prefix,
            op,
            lhs,
            rhs,
        } => {
            let ls = emit_tir_expr(lhs, cx);
            let rs = emit_tir_expr(rhs, cx);
            let call = format!("({}).{}_{}({})", ls, prefix, op, rs);
            // D-FAIL-CARRIER1=A: `checked_*` answers Rust's plumbing `Option`;
            // `wrapping_*`/`saturating_*` answer the value itself. Only the
            // first is a `T?`, so only the first becomes the carrier.
            if matches!(e.ty, Type::Option(_)) {
                format!("{}jet_outcome_of({call})", cx.root_prefix)
            } else {
                call
            }
        }
        // c109 Phase 10: a core/stdlib module call. Reproduces `emit_core_call`
        // (Source/Codegen/Expression.rs) byte-for-byte. `module`/`method` were
        // resolved at lowering; `cx.root_prefix`/`cx.ffi_crate` are program-level
        // (read here, like Phase 9's `cx.file`). Args were lowered plainly, with
        // D-FIXARR1 widening explicit; per-arm borrow/move wrappers stay baked into
        // each arm exactly as `emit_core_call` requires.
        TExprKind::CoreCall {
            module,
            method,
            args,
            widen_to_vec,
            ..
        } => {
            let call = emit_tir_core_call(module, method, args, widen_to_vec, &e.ty, cx);
            // D-FAIL-CARRIER1=A: Core answers a `T?` with Rust's own `Option`,
            // the same plumbing shape it holds lists in. This is the one place
            // that shape becomes the carrier.
            if matches!(e.ty, Type::Option(_)) {
                format!("{}jet_outcome_of({call})", cx.root_prefix)
            } else {
                call
            }
        }
        TExprKind::Binary {
            op,
            overflow,
            line,
            lhs,
            rhs,
        } => {
            let ls = emit_tir_expr(lhs, cx);
            let rs = emit_tir_expr(rhs, cx);
            if matches!(op, BinOp::Pow | BinOp::FloorDiv | BinOp::Mod | BinOp::Rem) {
                // D-EXPSEM1 / D-FLOORDIV1: Rust spells neither operator, so `^`
                // and `/%` always call the one Prelude helper. Whole numbers
                // keep the exact, trapping rule; floats use the float helper.
                let (file, line) = (&cx.file, *line);
                let float = matches!(e.ty, Type::Float | Type::Float32);
                match (*op, float) {
                    (BinOp::Pow, true) => format!("({}).jet_pow({})", ls, rs),
                    (BinOp::Pow, false) => {
                        format!("({}).jet_pow(({}) as i128, {:?}, {})", ls, rs, file, line)
                    }
                    (BinOp::FloorDiv, true) => format!("({}).jet_floordiv({})", ls, rs),
                    (BinOp::FloorDiv, false) => {
                        format!("({}).jet_floordiv({}, {:?}, {})", ls, rs, file, line)
                    }
                    // D-MODSEM1=A: both remainders are whole-number only, so
                    // there is no float shape to choose between. Each traps on
                    // a zero divisor rather than letting Rust's own panic out
                    // (I2), so neither can use the bare Rust operator.
                    (BinOp::Mod, _) => format!("({}).jet_mod({}, {:?}, {})", ls, rs, file, line),
                    _ => format!("({}).jet_trunc_rem({}, {:?}, {})", ls, rs, file, line),
                }
            } else if *overflow {
                // Trapping helper: source location was resolved at lowering, so
                // the panic message matches the AST path exactly.
                let (file, line) = (&cx.file, *line);
                match op {
                    // D-NUMOPS1: shift-count traps. The count is widened to `i128`
                    // so a count of any integer width reaches `jet_shl`/`jet_shr`.
                    BinOp::Shl => {
                        format!("({}).jet_shl(({}) as i128, {:?}, {})", ls, rs, file, line)
                    }
                    BinOp::Shr => {
                        format!("({}).jet_shr(({}) as i128, {:?}, {})", ls, rs, file, line)
                    }
                    _ => {
                        let method = match op {
                            BinOp::Add => "jet_add",
                            BinOp::Sub => "jet_sub",
                            BinOp::Mul => "jet_mul",
                            BinOp::Div => "jet_div",
                            BinOp::Rem => "jet_rem",
                            _ => unreachable!("overflow flag only set for +,-,*,/,%,<<,>>"),
                        };
                        format!("({}).{}(({}), {:?}, {})", ls, method, rs, file, line)
                    }
                }
            } else {
                format!("(({}) {} ({}))", ls, op.rust_spell().expect(PRELUDE_CARRIED), rs)
            }
        }
        // D-CHAINCMP1: `0 <= sev < 10` — a Rust block expression binds each
        // operand to a temp exactly once (single-evaluation for the shared
        // middle operands), then ANDs the adjacent-pair comparisons over
        // those temps: `{ let __jcc0 = (e0); let __jcc1 = (e1); …
        // (__jcc0 op0 __jcc1) && (__jcc1 op1 __jcc2) && … }`.
        TExprKind::CompareChain {
            operands,
            ops,
            hooks,
        } => {
            let mut block = String::from("{ ");
            for (i, operand) in operands.iter().enumerate() {
                let os = emit_tir_expr(operand, cx);
                block.push_str(&format!("let __jcc{} = ({}); ", i, os));
            }
            let pairs: Vec<String> = ops
                .iter()
                .enumerate()
                .map(|(i, op)| {
                    if hooks.get(i).copied().unwrap_or(false) {
                        let (cmp, variant) = match op {
                            BinOp::Lt => ("==", "Less"),
                            BinOp::Le => ("!=", "Greater"),
                            BinOp::Gt => ("==", "Greater"),
                            BinOp::Ge => ("!=", "Less"),
                            _ => unreachable!(),
                        };
                        format!(
                            "(user_Comparable::compare(&__jcc{}, &__jcc{}) {} user_Ordering::user_{})",
                            i,
                            i + 1,
                            cmp,
                            variant
                        )
                    } else {
                        format!("(__jcc{} {} __jcc{})", i, op.rust_spell().expect(PRELUDE_CARRIED), i + 1)
                    }
                })
                .collect();
            block.push_str(&format!("({}) }}", pairs.join(" && ")));
            block
        }
        // D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 1): `>=`/`<=`/`==` between
        // layout values register a `Constraint`, so it's a function call, not
        // a Rust operator.
        TExprKind::LayoutCompare { op, lhs, rhs } => {
            let ls = emit_tir_expr(lhs, cx);
            let rs = emit_tir_expr(rhs, cx);
            let func = match op {
                BinOp::Ge => "ge",
                BinOp::Le => "le",
                BinOp::Eq => "eq_",
                _ => unreachable!("layout comparisons are only >=, <=, =="),
            };
            format!("jet_layout::{}(({}), ({}))", func, ls, rs)
        }
        TExprKind::LayoutLit { inner } => {
            let i = emit_tir_expr(inner, cx);
            format!("jet_layout::LinExpr::from_const(({}) as f64)", i)
        }
        TExprKind::Unary { op, operand } => {
            let i = emit_tir_expr(operand, cx);
            match op {
                UnOp::Neg => format!("(-({}))", i),
                UnOp::Not => format!("(!({}))", i),
            }
        }
        TExprKind::IncDec {
            op, place, postfix, ..
        } => {
            let delta = match op {
                crate::AST::IncDecOp::Inc => "+",
                crate::AST::IncDecOp::Dec => "-",
            };
            let place = emit_tir_place(place, cx);
            if *postfix {
                format!("{{ let __jet_old = {place}; {place} {delta}= 1; __jet_old }}")
            } else {
                format!("{{ {place} {delta}= 1; {place} }}")
            }
        }
        // c109 Phase 3: `user_S { f: v, … }`. The Rust head and mangled field
        // names were resolved at lowering; values format like any other node.
        TExprKind::StructLit {
            fields,
            extra,
            as_trait,
        } => {
            let literal_ty = as_trait
                .as_ref()
                .map(|(_, concrete)| Type::Named(concrete.clone()))
                .unwrap_or_else(|| e.ty.clone());
            // Value-position generic heads need turbofish (`Foo::<T> {…}`);
            // `cx.rust_type` spells type-position `Foo<T>` and rustc rejects it.
            let rust_type = match &literal_ty {
                Type::Apply { name, args } if !args.is_empty() => {
                    let head = match cx.foreign_types.get(name) {
                        Some(rust_mod) => {
                            format!("{}{}::{}", cx.root_prefix, rust_mod, user_type_rust(name))
                        }
                        None => {
                            if let Some((alias, leaf)) = name.split_once('.') {
                                cx.import_mods.get(alias).map_or_else(
                                    || user_type_rust(name),
                                    |rust_mod| {
                                        format!(
                                            "{}{}::{}",
                                            cx.root_prefix,
                                            rust_mod,
                                            user_type_rust(leaf)
                                        )
                                    },
                                )
                            } else if matches!(name.as_str(), "DkimConfig" | "SMTPConfig") {
                                format!("{}jet_email::{}", cx.root_prefix, name)
                            } else {
                                user_type_rust(name)
                            }
                        }
                    };
                    format!(
                        "{}::<{}>",
                        head,
                        args.iter()
                            .map(|a| cx.rust_type(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
                // rust_type embeds `<ffi::Secret>` in type position; value
                // constructors need turbofish or rustc parses `<` as comparison.
                Type::Named(n) if matches!(n.as_str(), "DkimConfig" | "SMTPConfig") => {
                    let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
                    format!(
                        "{}jet_email::{}::<{}::Secret>",
                        cx.root_prefix, n, ffi
                    )
                }
                _ => cx.rust_type(&literal_ty),
            };
            let plain_fields = matches!(
                &literal_ty,
                Type::Named(n) | Type::Apply { name: n, .. } if crate::Codegen::net_handle_rust_type(n).is_some()
                    || matches!(
                        n.as_str(),
                        "TextWidth"
                            | "TerminalSize"
                            | "TerminalPolicy"
                            | "DataLineOptions"
                            | "AsyncPolicy"
                            | "FieldError"
                            | "CBOROptions"
                            | "CBORError"
                            | "XMLLimits"
                            | "XMLParseOptions"
                            | "XMLRenderOptions"
                            | "XMLCanonical"
                            | "XMLError"
                            | "RecipientReport"
                            | "SendReport"
                            | "Limits"
                            | "DkimConfig"
                            | "SMTPConfig"
                    )
            );
            let mut parts = fields
                .iter()
                .map(|(field, v, boxed)| {
                    let field_rust = if plain_fields {
                        field.clone()
                    } else {
                        emit_field_rust(cx, &literal_ty, field)
                    };
                    let value = emit_tir_expr(v, cx);
                    let value = if *boxed {
                        format!("Box::new({value})")
                    } else {
                        value
                    };
                    format!("{field_rust}: {value}")
                })
                .collect::<Vec<_>>();
            if let Some(TStructExtra::HTTPRequestParams) = extra {
                parts.push("params: std::collections::BTreeMap::new()".to_string());
                parts.push("route_template: None".to_string());
            }
            let lit = format!("{rust_type} {{ {} }}", parts.join(", "));
            match as_trait {
                Some((trait_name, _)) => {
                    let trait_rust = crate::Generics::user_trait_rust(trait_name);
                    format!("Box::new({lit}) as Box<dyn {trait_rust}>")
                }
                None => lit,
            }
        }
        // c109 Phase 3: `(recv).field`. Mirrors the AST `Expr::Field` emit form
        // exactly (no deref, no clone — owning reads were rewritten to a `.clone()`
        // MethodCall in sema and excluded from the subset).
        TExprKind::Field { recv, field, boxed } => {
            let field_rust = emit_field_rust(cx, &recv.ty, field);
            let read = format!("({}).{field_rust}", emit_tir_expr(recv, cx));
            if *boxed {
                format!("(*{read})")
            } else {
                read
            }
        }
        TExprKind::SharedGuardValue { guard, .. } => {
            format!("(*({}))", emit_tir_expr(guard, cx))
        }
        TExprKind::SharedGuardMap {
            guard,
            path,
            editable,
        } => {
            let projection = emit_shared_guard_projection(cx, &guard.ty, path);
            let method = if *editable { "map_edit" } else { "map_read" };
            let borrow = if *editable { "&mut " } else { "&" };
            format!(
                "({}).{method}(|__jet_guard_value| {borrow}{})",
                emit_tir_expr(guard, cx),
                projection
            )
        }
        TExprKind::SharedGuardSplit {
            guard,
            first,
            second,
            editable,
        } => {
            let first = emit_shared_guard_projection(cx, &guard.ty, first);
            let second = emit_shared_guard_projection(cx, &guard.ty, second);
            let method = if *editable { "split_edit" } else { "split_read" };
            let borrow = if *editable { "&mut " } else { "&" };
            let fields = match &e.ty {
                Type::Tuple(fields) => crate::Codegen::Tuples::tuple_fields_plain(fields),
                _ => Vec::new(),
            };
            let tuple = crate::Codegen::Tuples::tuple_struct_name(&fields);
            format!(
                "{{ let (__jet_first, __jet_second) = ({}).{method}(|__jet_guard_value| ({borrow}{}, {borrow}{})); {} {{ {}: __jet_first, {}: __jet_second }} }}",
                emit_tir_expr(guard, cx),
                first,
                second,
                tuple,
                mangle("first"),
                mangle("second"),
            )
        }
        TExprKind::SharedGuardWait {
            guard,
            condition,
            predicate,
        } => format!(
            "({}).wait(&({}), {})",
            emit_tir_expr(guard, cx),
            emit_tir_expr(condition, cx),
            emit_tir_lambda(predicate)
        ),
        TExprKind::ConditionNotify { condition, all } => format!(
            "({}).notify_{}()",
            emit_tir_expr(condition, cx),
            if *all { "all" } else { "one" }
        ),
        TExprKind::PtrFromAddr { elem, addr } => {
            format!(
                "(({}) as usize as *mut {})",
                emit_tir_expr(addr, cx),
                cx.rust_type(elem)
            )
        }
        // D-CAP9: postfix `p.*` deref → Rust `(*(p))`. The `unsafe` is supplied by
        // the enclosing `#Unsafe` region (sema-gated); this node adds no `unsafe`.
        TExprKind::Deref(operand) => format!("(*({}))", emit_tir_expr(operand, cx)),
        // D-CAP9: prefix `*x` raw-of → `(&({}) as *const _ as *mut _)`. The result
        // is `*mut T` to match the canonical raw-pointer type (`Ptr<T>` lowers to
        // `*mut`). Forming the pointer is safe Rust; only dereferencing it needs
        // the surrounding `#Unsafe`. The const→mut cast is the standard idiom.
        TExprKind::RawOf(operand) => {
            if matches!(&operand.ty, Type::Apply { name, .. } if name == "Ptr") {
                emit_tir_expr(operand, cx)
            } else {
                format!("(&({}) as *const _ as *mut _)", emit_tir_expr(operand, cx))
            }
        }
        // c109 Phase 19: the arena allocator constructor — the ctor tail was rendered whole
        // at lowering (`jet_mem::Jet<Alloc>::new()` / `::with_capacity(...)`), so emit just
        // splices it. Byte-for-byte `emit_method_call`'s arena constructor branch.
        TExprKind::AllocNew { ctor } => {
            debug_assert!(!ctor.starts_with("__JET_FIXED_INLINE:"));
            ctor.clone()
        }
        // c109 Phase 4/16: an enum literal. Prefix + payload were resolved at lowering;
        // emit applies each arg's resolved `clone`/`boxed` wrappers (mirroring
        // `emit_boxed_enum_arg`: `(…).clone()` first, then `Box::new(…)`).
        TExprKind::EnumLit {
            enum_type,
            variant,
            payload,
        } => {
            let prefix = crate::Codegen::TIR::tir_enum_lit_prefix(cx, enum_type, variant);
            match payload {
            TEnumPayload::Unit => prefix,
            TEnumPayload::Positional(vals) => {
                let pos = vals
                    .iter()
                    .map(|a| emit_tir_enum_arg(a, cx))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", prefix, pos)
            }
            TEnumPayload::Named(fields) => {
                let parts = fields
                    .iter()
                    .map(|(name, a)| format!("{}: {}", name, emit_tir_enum_arg(a, cx)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {{ {} }}", prefix, parts)
            }
        }
        },
        // c109 Phase 24: a JSON construction — `{root}jet_std::JSON::<Variant>(<arg>)`.
        // Reproduces `emit_core_json_lit` (Expression.rs): the arg is wrapped in
        // `(…).clone()` iff its `implicit_clone` flag was set; `Null` has no arg.
        // D-ENC-DYN1=A+ / D-SERDE2: a dynamic `DataTree` construction. Object
        // literals cross directly into the ordered pair representation so source/Codable
        // field order survives; routing the literal through Jet's key-sorted Map would
        // silently alphabetize the wire shape. A computed Map still collects in its
        // ordinary Map iteration order. Scalars/`Array` bind directly.
        TExprKind::JSONLit { variant, arg } => {
            let prefix = format!("{}jet_std::DataTree", cx.root_prefix);
            match arg {
                None => format!("{}::{}", prefix, variant),
                Some(boxed) => {
                    let (val, implicit_clone) = boxed.as_ref();
                    let s = emit_tir_expr(val, cx);
                    let arg_str = if *implicit_clone {
                        format!("({}).clone()", s)
                    } else {
                        s
                    };
                    if variant == "Object" {
                        if let Some(pairs) = object_ordered_pairs_rust(val, cx) {
                            format!("{}::{}(vec![{}])", prefix, variant, pairs)
                        } else {
                            format!(
                                "{}::{}(({}).into_iter().collect())",
                                prefix, variant, arg_str
                            )
                        }
                    } else {
                        format!("{}::{}({})", prefix, variant, arg_str)
                    }
                }
            }
        }
        // D-DBDRIVER1: a `DBValue` construction — `{root}jet_std::DBValue::<Variant>(<arg>)`.
        // Same shape as `JSONLit` (a foreign prelude enum), minus the recursive
        // `Array`/`Object` special-case (`DBValue` has no compound variants).
        TExprKind::DBValueLit { variant, arg } => {
            let prefix = format!("{}jet_std::DBValue", cx.root_prefix);
            match arg {
                None => format!("{}::{}", prefix, variant),
                Some(boxed) => {
                    let (val, implicit_clone) = boxed.as_ref();
                    let s = emit_tir_expr(val, cx);
                    let arg_str = if *implicit_clone {
                        format!("({}).clone()", s)
                    } else {
                        s
                    };
                    format!("{}::{}({})", prefix, variant, arg_str)
                }
            }
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            let then_block = emit_tir_value_block(then_body, then_value, cx);
            let else_block = emit_tir_value_block(else_body, else_value, cx);
            emit_tir_if_expr(cond, &then_block, &else_block, cx)
        }
        // c109 Phase 5: `[a, b, c]` → `vec![a, b, c]` (growable) or `[a, b, c]` (fixed).
        // D-FIXARR1: if the expression type is FixedList, emit a Rust array literal `[…]`.
        TExprKind::ListLit(elems) => {
            let mut common_owner = None;
            let mut mutable_ranges = Vec::new();
            let mut range_values = None;
            for elem in elems {
                let TExprKind::BuiltinMethod {
                    recv,
                    op: TBuiltinOp::ViewMutNew { line },
                    args,
                } = &elem.kind
                else {
                    mutable_ranges.clear();
                    break;
                };
                if !matches!(args.len(), 1 | 2)
                    || range_values.is_some_and(|range| range != (args.len() == 1))
                {
                    mutable_ranges.clear();
                    break;
                }
                range_values.get_or_insert(args.len() == 1);
                let owner = emit_tir_expr(recv, cx);
                if common_owner.as_ref().is_some_and(|seen| seen != &owner) {
                    mutable_ranges.clear();
                    break;
                }
                common_owner.get_or_insert(owner);
                mutable_ranges.push(if args.len() == 1 {
                    format!("({}, {})", emit_tir_expr(&args[0], cx), line)
                } else {
                    format!(
                        "({}, {}, {})",
                        emit_tir_expr(&args[0], cx),
                        emit_tir_expr(&args[1], cx),
                        line
                    )
                });
            }
            if !mutable_ranges.is_empty() && mutable_ranges.len() == elems.len() {
                return format!(
                    "{}(&mut ({}), &[{}], {:?})",
                    if range_values == Some(true) {
                        "jet_views_mut_range_new"
                    } else {
                        "jet_views_mut_new"
                    },
                    common_owner.expect("mutable view list has an owner"),
                    mutable_ranges.join(", "),
                    cx.file,
                );
            }
            let trait_rust = match &e.ty {
                Type::List(inner) => match inner.as_ref() {
                    Type::TraitObject(names) if names.len() == 1 => {
                        Some(crate::Generics::user_trait_rust(&names[0]))
                    }
                    _ => None,
                },
                _ => None,
            };
            let list_elem_ty = match &e.ty {
                Type::List(inner) | Type::FixedList { elem: inner, .. } => Some(inner.as_ref()),
                _ => None,
            };
            let parts = elems
                .iter()
                .map(|elem| {
                    // D-SG9: a `[U8]`/`[I32]` typed list must emit matching suffixes
                    // even when an element IntLit still carries width None.
                    let value = match (list_elem_ty, &elem.kind) {
                        (
                            Some(Type::IntN { signed, bits }),
                            TExprKind::IntLit(n, width),
                        ) if width.is_none() => {
                            format!("{}{}{}", n, if *signed { 'i' } else { 'u' }, bits)
                        }
                        _ => emit_tir_expr(elem, cx),
                    };
                    if let Some(trait_rust) = &trait_rust {
                        if !matches!(elem.ty, Type::TraitObject(_)) {
                            return format!("Box::new({value}) as Box<dyn {trait_rust}>");
                        }
                    }
                    value
                })
                .collect::<Vec<_>>()
                .join(", ");
            if matches!(&e.ty, Type::FixedList { .. }) {
                format!("[{}]", parts)
            } else {
                format!("vec![{}]", parts)
            }
        }
        TExprKind::ListSpread { parts } => {
            let mut s = String::from("{ let mut __jet_sp = Vec::new(); ");
            for part in parts {
                match part {
                    ListSpreadPart::Elem(elem) => {
                        s.push_str(&format!(
                            "__jet_sp.push(({}).clone()); ",
                            emit_tir_expr(elem, cx)
                        ));
                    }
                    ListSpreadPart::Spread(list) => {
                        s.push_str(&format!(
                            "__jet_sp.extend(({}).clone()); ",
                            emit_tir_expr(list, cx)
                        ));
                    }
                }
            }
            s.push_str("__jet_sp }");
            s
        }
        // D-SOA1: a columnar list literal → `user_<S>_columns::from_aos(vec![…])`.
        TExprKind::ColumnarListLit { columns_ty, elems } => {
            let parts = elems
                .iter()
                .map(|e| emit_tir_expr(e, cx))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}::from_aos(vec![{}])", columns_ty, parts)
        }
        // D-SOA1: `xs[i]` on a columnar list → bounds-checked gather of the logical S.
        TExprKind::ColumnarGather { base, index, line } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            format!("({}).gather_at({}, {:?}, {})", b, i, cx.file, line)
        }
        // D-SOA1: `xs[i].field` on a columnar list → direct column read.
        TExprKind::ColumnarColumnRead {
            base,
            index,
            column_rust,
            line,
        } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            format!(
                "jet_index_vec(&({}).{}, {}, {:?}, {})",
                b, column_rust, i, cx.file, line
            )
        }
        // c109 Phase 23: a named-tuple literal → `JetTup_<hash> { user_<f>: <v>, … }`.
        // Mirrors `emit_expr`'s `TupleLit` arm byte-for-byte (fields canonical-ordered,
        // resolved at lowering).
        TExprKind::TupleLit {
            struct_name,
            fields,
        } => {
            let parts = fields
                .iter()
                .map(|(n, v)| format!("{}: {}", n, emit_tir_expr(v, cx)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ {} }}", struct_name, parts)
        }
        // #779: only empty map literals remain after lowering (`[:]` / desugar seed).
        // Non-empty `[k: v, …]` becomes IfExpr + IndexAssign inserts.
        TExprKind::MapLit(entries) => {
            if entries.is_empty() {
                "JetMap::new()".to_string()
            } else {
                // Defensive: should not appear after #779 lowering.
                let mut s = String::from("{ let mut _m = JetMap::new(); ");
                for (k, v) in entries {
                    s.push_str(&format!(
                        "_m.insert(({}).clone(), {}); ",
                        emit_tir_expr(k, cx),
                        emit_tir_expr(v, cx)
                    ));
                }
                s.push_str("_m }");
                s
            }
        }
        // c109 Phase 5: `coll[i]`. Dispatch on the total `is_map` fact (never
        // re-inferred). Mirrors the AST `Expr::Index` form: a map index borrows the
        // key (`&(i)`), a vec index does not.
        TExprKind::Index {
            base,
            index,
            is_map,
            uninit_fixed,
            line,
        } => {
            let b = if *uninit_fixed {
                match &base.kind {
                    TExprKind::Local(slot) if slot.uninit_fixed => slot.rust_place(),
                    _ => emit_tir_expr(base, cx),
                }
            } else {
                emit_tir_expr(base, cx)
            };
            let i = emit_tir_expr(index, cx);
            if *is_map {
                format!("jet_index_map(&({}), &({}), {:?}, {})", b, i, cx.file, line)
            } else if *uninit_fixed {
                format!("(({b})[({i}) as usize].clone())")
            } else if matches!(
                &e.ty,
                Type::Apply { name, .. } if name == "ViewMut"
            ) {
                format!(
                    "(&mut **jet_index_vec_mut(&mut ({}), {}, {:?}, {}))",
                    b, i, cx.file, line
                )
            } else {
                format!("jet_index_vec(&({}), {}, {:?}, {})", b, i, cx.file, line)
            }
        }
        // D-MEM1 S6: `pool[id]` / `pool[id].field` — generation-checked get or
        // get_mut place. Mutable writes and mutating receivers use get_mut.
        TExprKind::PoolSlot {
            pool,
            id,
            mutable,
            field,
            line,
        } => {
            let p = emit_tir_expr(pool, cx);
            let i = emit_tir_expr(id, cx);
            let base = if *mutable {
                format!(
                    "(*{root}jet_std::jet_pool_get_mut(&mut ({p}), {i}, {file:?}, {line}))",
                    root = cx.root_prefix,
                    file = cx.file,
                )
            } else {
                format!(
                    "{root}jet_std::jet_pool_get(&({p}), {i}, {file:?}, {line})",
                    root = cx.root_prefix,
                    file = cx.file,
                )
            };
            match field {
                Some(field) => {
                    let field_rust = emit_field_rust(cx, &pool.ty, field);
                    format!("{base}.{field_rust}")
                }
                None => base,
            }
        }
        TExprKind::IndexHook {
            type_name,
            base,
            index,
            line,
        } => {
            let ty = user_type_rust(type_name);
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            format!(
                "{{ match <{ty} as user_Index>::get(&({b}), {i}) {{ Ok(_jet_v) => _jet_v, Err(_) => {root}jet_panic({:?}, {}, \"index miss\") }} }}",
                cx.file,
                line,
                root = cx.root_prefix,
            )
        }
        // D-SIMD2: `v[i]` lane read → the bounds-checked prelude helper.
        TExprKind::MathLaneIndex {
            lane_ty,
            base,
            index,
            line,
        } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            format!(
                "{}jet_math_{}_lane(&({}), {}, {:?}, {})",
                cx.root_prefix, lane_ty, b, i, cx.file, line
            )
        }
        // D-SWIZZLE1: read swizzle `v.xyz` → lane extract (+ `VecN` ctor when N>1).
        TExprKind::MathSwizzleRead {
            type_name,
            recv,
            lanes,
        } => emit_math_swizzle_read(cx, type_name, recv, lanes),
        // c109 Phase 5: `coll[a..b]` → `jet_slice_vec`. Mirrors the AST `Expr::Slice`.
        TExprKind::Slice {
            base,
            start,
            end,
            range,
            line,
        } => {
            let b = emit_tir_expr(base, cx);
            let is_tensor = matches!(&base.ty, Type::Named(name) if name == "Tensor")
                || matches!(&base.ty, Type::Apply { name, .. } if name == "Tensor");
            if let Some(range) = range {
                return format!(
                    "{}(&({}), &({}), {:?}, {})",
                    if is_tensor {
                        format!("{}jet_compute_slice_range", cx.root_prefix)
                    } else {
                        "jet_slice_range".to_string()
                    },
                    b,
                    emit_tir_expr(range, cx),
                    cx.file,
                    line
                );
            }
            let a = emit_tir_expr(start, cx);
            let e = emit_tir_expr(end, cx);
            if is_tensor {
                format!(
                    "{}jet_compute_slice(&({}), {}, {}, false, {:?}, {})",
                    cx.root_prefix, b, a, e, cx.file, line
                )
            } else {
                format!("jet_slice_vec(&({}), {}, {}, {:?}, {})", b, a, e, cx.file, line)
            }
        }
        // D-FAIL-CARRIER1=A: the optional view builds the one carrier. A present
        // payload is the value side; an absence is the clean report.
        TExprKind::Present(inner) => format!("Ok({})", emit_tir_expr(inner, cx)),
        TExprKind::Absent => format!("Err({}JetAbsent)", cx.root_prefix),
        // c109 Phase 23: a `#Todo` typed hole → diverging `todo!(…)`. Byte-for-byte the
        // AST `Expr::Todo` arm (Expression.rs): file/line/expected-type baked into the
        // panic string. `cx.file` is program-level (read here, like every other use).
        TExprKind::Todo {
            line,
            expected_type,
        } => format!(
            "todo!(\"#{} at {}:{} — expected {}\")",
            crate::Syntax::KW_TODO,
            cx.file,
            line,
            expected_type
        ),
        // Card #1440: sema proved the dispatch exhaustive (E0307); this arm is
        // dead on every input. `unreachable!` diverges so it type-checks in any
        // value position (I1: no unsafe).
        TExprKind::Unreachable { line } => format!(
            "unreachable!(\"exhaustive dispatch at {}:{} (sema-proved, E0307)\")",
            cx.file, line
        ),
        // c109 Phase 8: `Ok(x)` → `Ok(x)` / `Err(e)` → `Err(e)`. Mirrors the AST
        // `Expr::Ok`/`Expr::Err`.
        TExprKind::Ok(inner) => format!("Ok({})", emit_tir_expr(inner, cx)),
        TExprKind::Err(inner) => format!("Err({})", emit_tir_expr(inner, cx)),
        // c109 Phase 8: the `?` propagation operator. Mirrors `Expr::Try` byte-for-byte
        // (Expression.rs): a debug trace frame wraps the value, then the error is
        // converted per the total `TryConvert`, then `?` propagates. `file`/`fn_name`
        // were pre-escaped at lowering; `line` is plain.
        TExprKind::Try {
            inner,
            convert,
            file,
            line,
            fn_name,
        } => {
            let v = emit_tir_expr(inner, cx);
            match convert {
                // S80/D-LIB3: error implements Fallible → `.map_err(|e| e.to_error())`.
                TTryConvert::Fallible => format!(
                    "jet_trace_err({}.map_err(|e| e.to_error()), {}, {}, {})?",
                    v, file, line, fn_name
                ),
                // D-ERR-CONV: declared `impl Source => Target` → `.map_err(<fn>)`.
                TTryConvert::Typed(conv_fn) => format!(
                    "jet_trace_err({}.map_err({}), {}, {}, {})?",
                    v, conv_fn, file, line, fn_name
                ),
                // D-UNIONTYPE1=A: member error → anonymous union wrap.
                TTryConvert::WidenUnion { enum_name, tag } => format!(
                    "jet_trace_err({}.map_err(|e| user_{enum_name}::{tag}(e)), {}, {}, {})?",
                    v, file, line, fn_name
                ),
                // Error types match — bare propagate.
                TTryConvert::None => {
                    format!("jet_trace_err({}, {}, {}, {})?", v, file, line, fn_name)
                }
            }
        }
        // c109 Phase 8: the `??` fallback operator. Mirrors `emit_or_fallback`
        // (Statement.rs). D-FAIL-CARRIER1=A: one carrier, so one unwrap — the value
        // side is the payload and the report side runs the fallback.
        TExprKind::OrFallback { value, fallback } => {
            let v = emit_tir_expr(value, cx);
            let fb = emit_tir_orfallback_rhs(fallback, cx);
            format!("match {} {{ Ok(__jet_ok) => __jet_ok, Err(_) => {} }}", v, fb)
        }
        // c109 Phase 8: optional chaining `base?.member`. Mirrors `Expr::OptField`:
        // `(base).clone().{and_then|map}(|__optv| __optv.{member})`. The combinator is
        // the total `flatten` fact (flatten → `and_then`, else → `map`).
        TExprKind::OptField {
            base,
            member,
            flatten,
        } => {
            let combinator = if *flatten { "and_then" } else { "map" };
            let member_rust = emit_field_rust(cx, &base.ty, member);
            format!(
                "({}).clone().{combinator}(|__optv| __optv.{member_rust})",
                emit_tir_expr(base, cx),
            )
        }
        // c109 Phase 11: a lambda/closure literal. All decisions (prep/move/box) were
        // resolved at lowering off `Lambda.meta`; emit only assembles, byte-for-byte
        // `emit_lambda`: `{move }|params| body`, wrapped `Box::new(…)` when it escapes,
        // and prefixed with the `{ <prep> … }` block when there are cloned captures.
        TExprKind::Lambda(lam) => emit_tir_lambda(lam),
        TExprKind::InlineBlock(stmts) => {
            let mut rendered = String::from("{\n");
            emit_tir_lambda_block(stmts, cx, &mut rendered, 1);
            rendered.push('}');
            rendered
        }
        TExprKind::HostBorrowCallback { callable, params } => {
            let callable = emit_tir_expr(callable, cx);
            let declarations = params
                .iter()
                .enumerate()
                .map(|(index, ty)| format!("__jet_para_{index}: &{}", cx.rust_type(ty)))
                .collect::<Vec<_>>()
                .join(", ");
            let arguments = params
                .iter()
                .enumerate()
                .map(|(index, ty)| {
                    if ty.is_scalar() {
                        format!("*__jet_para_{index}")
                    } else {
                        format!("__jet_para_{index}")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("move |{declarations}| ({callable})({arguments})")
        }
        // D-HOLE1: `Option.lift2(f, a, b)` — `a.zip(b).map(|(x, y)| f(x, y))`. `f` is
        // any lowered function value (lambda or fn ident), called via Rust's
        // call-operator syntax on the (possibly boxed) closure.
        TExprKind::OptionLift2 { f, a, b } => {
            let f = emit_tir_expr(f, cx);
            let a = emit_tir_expr(a, cx);
            let b = emit_tir_expr(b, cx);
            format!(
                "({}).clone().zip(({}).clone()).map(|(x, y)| ({})(x, y))",
                a, b, f
            )
        }
        // c109 Phase 11: a closure-taking collection method. The receiver-type +
        // Fn-vs-FnMut dispatch was resolved into `op` at lowering; emit only formats,
        // reproducing `emit_builtin_method`'s closure arms byte-for-byte. Args (the
        // lambda + any seed) are emitted PLAINLY (raw `arg(i)`).
        TExprKind::ClosureMethod { recv, op, args } => {
            let recv_is_fixed = matches!(recv.ty, Type::FixedList { .. });
            let recv_is_iter = crate::Collections::is_iter_type(&recv.ty);
            let recv = emit_tir_expr(recv, cx);
            // para_* needs a concrete Vec; materialize Iter at the parallel boundary.
            let para_recv = if recv_is_fixed {
                format!("({recv}).to_vec()")
            } else if recv_is_iter {
                format!("({recv}).to_list()")
            } else {
                format!("({recv}).clone()")
            };
            let vec_src = if recv_is_iter {
                format!("({recv})")
            } else {
                format!("({recv}).clone()")
            };
            let as_iter = if recv_is_iter {
                format!("({recv})")
            } else {
                format!("jet_iter_from_vec(({recv}).clone())")
            };
            let a = |i: usize| {
                args.get(i)
                    .map(|e| emit_tir_expr(e, cx))
                    .unwrap_or_default()
            };
            match op {
                TClosureOp::EditDisjoint => {
                    format!("jet_edit_disjoint(&mut ({}), &({}), {})", recv, a(0), a(1))
                }
                TClosureOp::Map => {
                    if recv_is_iter {
                        format!("jet_iter_map({as_iter}, {})", a(0))
                    } else {
                        // D-LOOPMAP1=B: List.map is eager.
                        let mut f = a(0);
                        if let Some(rest) = f.strip_prefix("move ") {
                            f = rest.to_string();
                        }
                        format!("jet_list_map({vec_src}, {f})")
                    }
                }
                TClosureOp::MapMut => {
                    if recv_is_iter {
                        format!("jet_iter_map_mut({as_iter}, {})", a(0))
                    } else {
                        let mut f = a(0);
                        if let Some(rest) = f.strip_prefix("move ") {
                            f = rest.to_string();
                        }
                        format!("jet_list_map_mut({vec_src}, {f})")
                    }
                }
                // D-HOLE1/D-MEM-PARAM1: `.map` on `T?` lends the payload to
                // its plain callback instead of cloning/moving it.
                TClosureOp::OptionMap => format!("({}).map_ref({})", recv, a(0)),
                TClosureOp::Filter => {
                    if recv_is_iter {
                        format!("jet_iter_filter({as_iter}, {})", a(0))
                    } else {
                        let mut f = a(0);
                        if let Some(rest) = f.strip_prefix("move ") {
                            f = rest.to_string();
                        }
                        format!("jet_list_filter({vec_src}, {f})")
                    }
                }
                TClosureOp::Each => format!("jet_list_each({vec_src}, {})", a(0)),
                TClosureOp::EachMut => format!("jet_list_each_mut({vec_src}, {})", a(0)),
                TClosureOp::EachRef => format!("jet_list_each_ref(&({}), {})", recv, a(0)),
                TClosureOp::EachMap => format!("jet_map_each(({}).clone(), {})", recv, a(0)),
                TClosureOp::MapAny => format!("jet_map_any(({}).clone(), {})", recv, a(0)),
                TClosureOp::MapAll => format!("jet_map_all(({}).clone(), {})", recv, a(0)),
                TClosureOp::MapFilter => format!("jet_map_filter(({}).clone(), {})", recv, a(0)),
                TClosureOp::MapMap => format!("jet_map_map_values(({}).clone(), {})", recv, a(0)),
                TClosureOp::MapFold => format!("jet_map_fold(({}).clone(), {}, {})", recv, a(0), a(1)),
                TClosureOp::MapFlatMap => format!("jet_map_flat_map(({}).clone(), {})", recv, a(0)),
                TClosureOp::ListBinarySearchBy => format!("jet_list_binary_search_by(&({}), {})", recv, a(0)),
                TClosureOp::ListMinMaxBy { tuple_struct } => format!(
                    "jet_list_min_max_by(&({}), {}, |min, max| {} {{ user_min: min, user_max: max }})",
                    recv, a(0), tuple_struct
                ),
                TClosureOp::Find => format!("jet_list_find({vec_src}, {})", a(0)),
                TClosureOp::Any => format!("jet_list_any({vec_src}, {})", a(0)),
                TClosureOp::BagAny => format!("({}).keys().any({})", recv, a(0)),
                TClosureOp::All => format!("jet_list_all({vec_src}, {})", a(0)),
                TClosureOp::SortBy => format!("{{ jet_list_sort_by(&mut {}, {}); }}", recv, a(0)),
                TClosureOp::Reduce => {
                    format!("jet_list_reduce({vec_src}, {}, {})", a(0), a(1))
                }
                // D-ITERTOOLS1=A: closure adapters return JetIter (no intermediate Vec).
                TClosureOp::TakeWhile => {
                    format!("jet_iter_take_while({as_iter}, {})", a(0))
                }
                TClosureOp::SkipWhile => {
                    format!("jet_iter_skip_while({as_iter}, {})", a(0))
                }
                TClosureOp::FlatMap => {
                    format!("jet_iter_flat_map({as_iter}, {})", a(0))
                }
                TClosureOp::FilterMap => {
                    format!("jet_iter_filter_map({as_iter}, {})", a(0))
                }
                // D-PARCAPTURE1=D: all adapters share the bounded `para_` engine.
                TClosureOp::ParaMap => format!("jet_list_para_map({}, {})", para_recv, a(0)),
                TClosureOp::ParaFilter => {
                    format!("jet_list_para_filter({}, {})", para_recv, a(0))
                }
                TClosureOp::ParaPartition { tuple_struct } => {
                    format!(
                        "jet_list_para_partition({}, {}, |__f, __t| \
                         {} {{ user_false_: __f, user_true_: __t }})",
                        para_recv,
                        a(0),
                        tuple_struct
                    )
                }
                TClosureOp::ParaFold => {
                    format!(
                        "jet_list_para_fold({}, {}, {}, {})",
                        para_recv,
                        a(0),
                        a(1),
                        a(2)
                    )
                }
                TClosureOp::Scan => {
                    format!("jet_iter_scan({as_iter}, {}, {})", a(0), a(1))
                }
                TClosureOp::Fold => {
                    format!("jet_list_fold({vec_src}, {}, {})", a(0), a(1))
                }
                // D-DYNARRAY1: `recv` is already a `&[T]` borrow — fold/map it
                // directly, no `.clone()`-to-owned-Vec (that would defeat the
                // zero-copy point of `.view(...)`).
                TClosureOp::ViewFold => {
                    format!("jet_view_fold(({}), {}, {})", recv, a(0), a(1))
                }
                TClosureOp::ViewMap => format!("jet_view_map(({}), {})", recv, a(0)),
                TClosureOp::Position => {
                    format!("jet_list_position({vec_src}, {})", a(0))
                }
                TClosureOp::MinBy => {
                    format!("jet_list_min_by({vec_src}, {})", a(0))
                }
                TClosureOp::MaxBy => {
                    format!("jet_list_max_by({vec_src}, {})", a(0))
                }
                TClosureOp::GroupBy => {
                    format!("jet_list_group_by({vec_src}, {})", a(0))
                }
                TClosureOp::CountBy => {
                    format!("jet_list_count_by({vec_src}, {})", a(0))
                }
                TClosureOp::DedupBy => {
                    format!("jet_iter_dedup_by({as_iter}, {})", a(0))
                }
                TClosureOp::IsSortedBy => {
                    format!("jet_iter_is_sorted_by({as_iter}, {})", a(0))
                }
                TClosureOp::ChunkWhile => {
                    format!("jet_iter_chunk_while({as_iter}, {})", a(0))
                }
                TClosureOp::Partition { tuple_struct } => {
                    // `partition` passes each element by value (T: Clone).
                    // The lambda `f` takes T by value, but `Iterator::partition`
                    // passes `&T` to its predicate. Use jet_list_partition helper.
                    format!(
                        "jet_list_partition({vec_src}, {}, |__t, __f| \
                         {} {{ user_false_: __f, user_true_: __t }})",
                        a(0),
                        tuple_struct
                    )
                }
            }
        }
        // c109 Phase 13: a method ON a handle. The handle-receiver branch was resolved
        // into `op` at lowering; emit only formats, reproducing the handle arms of
        // `emit_builtin_method` (Source/Codegen/Expression.rs) byte-for-byte. Args are
        // emitted PLAINLY (raw `arg(i)`). `cx.root_prefix` is program-level.
        TExprKind::HandleMethod { recv, op, args } => {
            let recv_ty_for_stream = recv.ty.clone();
            let recv = emit_tir_expr(recv, cx);
            let a = |i: usize| {
                args.get(i)
                    .map(|e| emit_tir_expr(e, cx))
                    .unwrap_or_default()
            };
            let web_handler = |i: usize| {
                let mut rendered = a(i);
                if let Some(end) = rendered.rfind('>') {
                    rendered.insert_str(end, " + Send + Sync");
                }
                rendered
            };
            let root = &cx.root_prefix;
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            match op {
                THandleOp::DurationNew { unit, float } => {
                    let helper = if *float {
                        "jet_duration_from_float"
                    } else {
                        "jet_duration_from_int"
                    };
                    format!(
                        "{}{helper}({}, {}jet_std::DurationUnit::{unit})",
                        root, recv, root
                    )
                }
                THandleOp::FileReaderReadLine => {
                    format!("{}jet_std_file_reader_read_line(&mut ({}))", root, recv)
                }
                THandleOp::FileWriterWriteLine => format!(
                    "{}jet_std_file_writer_write_line(&mut ({}), &({}))",
                    root,
                    recv,
                    a(0)
                ),
                THandleOp::FileWriterFlush => {
                    format!("{}jet_std_file_writer_flush(&mut ({}))", root, recv)
                }
                THandleOp::JSONReaderNext => {
                    format!("{}jet_enc_json_reader_next(&mut ({}))", root, recv)
                }
                THandleOp::JSONWriterWrite => format!(
                    "{}jet_enc_json_writer_write(&mut ({}), {})",
                    root, recv, a(0)
                ),
                THandleOp::JSONWriterFlush => {
                    format!("{}jet_enc_json_writer_flush(&mut ({}))", root, recv)
                }
                THandleOp::JSONWriterFinish => {
                    format!("{}jet_enc_json_writer_finish(&mut ({}))", root, recv)
                }
                THandleOp::JSONLReaderNext => {
                    format!("{}jet_enc_jsonl_reader_next(&mut ({}))", root, recv)
                }
                THandleOp::JSONLWriterWrite => format!(
                    "{}jet_enc_jsonl_writer_write(&mut ({}), {})",
                    root, recv, a(0)
                ),
                THandleOp::JSONLWriterFlush => {
                    format!("{}jet_enc_jsonl_writer_flush(&mut ({}))", root, recv)
                }
                THandleOp::JSONLWriterFinish => {
                    format!("{}jet_enc_jsonl_writer_finish(&mut ({}))", root, recv)
                }
                THandleOp::CSVReaderNext => format!("{}jet_enc_csv_reader_next(&mut ({}))", root, recv),
                THandleOp::DataStreamNext => {
                    let row = match &recv_ty_for_stream {
                        Type::Apply { name, args } if name == "DataStream" => args
                            .first()
                            .map(|t| cx.rust_type(t))
                            .unwrap_or_else(|| "()".to_string()),
                        _ => "()".to_string(),
                    };
                    format!(
                        "{}jet_data_stream_next::<{}>(&mut ({}))",
                        root, row, recv
                    )
                }
                THandleOp::XMLReaderNext => format!("{}jet_enc_xml_reader_next(&mut ({}))", root, recv),
                THandleOp::XMLWriterWrite => format!("{}jet_enc_xml_writer_write(&mut ({}), {})", root, recv, a(0)),
                THandleOp::XMLWriterFlush => format!("{}jet_enc_xml_writer_flush(&mut ({}))", root, recv),
                THandleOp::XMLWriterFinish => format!("{}jet_enc_xml_writer_finish(&mut ({}))", root, recv),
                THandleOp::CSVWriterWrite => format!("{}jet_enc_csv_writer_write(&mut ({}), {})", root, recv, a(0)),
                THandleOp::CSVWriterFlush => format!("{}jet_enc_csv_writer_flush(&mut ({}))", root, recv),
                THandleOp::CSVWriterFinish => format!("{}jet_enc_csv_writer_finish(&mut ({}))", root, recv),
                THandleOp::CBORReaderNext => format!("{}jet_enc_cbor_reader_next(&mut ({}))", root, recv),
                THandleOp::CBORWriterWrite => format!("{}jet_enc_cbor_writer_write(&mut ({}), {})", root, recv, a(0)),
                THandleOp::CBORWriterFlush => format!("{}jet_enc_cbor_writer_flush(&mut ({}))", root, recv),
                THandleOp::CBORWriterFinish => format!("{}jet_enc_cbor_writer_finish(&mut ({}))", root, recv),
                THandleOp::StdinReadLine => {
                    format!("{}jet_std_io_stdin_read_line(&mut ({}))", root, recv)
                }
                THandleOp::StdoutWrite => {
                    format!("{}jet_std_io_stdout_write(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StdoutWriteLine => {
                    format!("{}jet_std_io_stdout_write_line(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StdoutWriteBytes => {
                    format!("{}jet_std_io_stdout_write_bytes(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StdoutFlush => {
                    format!("{}jet_std_io_stdout_flush(&mut ({}))", root, recv)
                }
                THandleOp::StdoutIsTty => {
                    format!("{}jet_std_io_stdout_is_tty(&({}))", root, recv)
                }
                THandleOp::StderrWrite => {
                    format!("{}jet_std_io_stderr_write(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StderrWriteLine => {
                    format!("{}jet_std_io_stderr_write_line(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StderrWriteBytes => {
                    format!("{}jet_std_io_stderr_write_bytes(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::StderrFlush => {
                    format!("{}jet_std_io_stderr_flush(&mut ({}))", root, recv)
                }
                THandleOp::StderrIsTty => {
                    format!("{}jet_std_io_stderr_is_tty(&({}))", root, recv)
                }
                THandleOp::StopwatchElapsedMillis => {
                    format!("{}jet_stopwatch_elapsed_millis(&({}))", root, recv)
                }
                // D-DET1: deterministic injected Clock/Rng capability methods.
                THandleOp::ClockNow => format!("{}jet_clock_now(&({}))", root, recv),
                THandleOp::ClockTick => {
                    format!("{}jet_clock_tick(&mut ({}), {})", root, recv, a(0))
                }
                // D-DET-CAPAPI: absolute set + Duration advance; widened Rng; Duration read.
                THandleOp::ClockAdvance => {
                    format!("{}jet_clock_advance(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::ClockWait => {
                    format!("{}jet_clock_wait(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::RngInt => {
                    format!("{}jet_rng_int(&mut ({}), {}, {})", root, recv, a(0), a(1))
                }
                THandleOp::RngFloat => format!("{}jet_rng_float(&mut ({}))", root, recv),
                THandleOp::RngFloatRange => {
                    format!("{}jet_rng_float_range(&mut ({}), {}, {})", root, recv, a(0), a(1))
                }
                THandleOp::RngBool => format!("{}jet_rng_bool(&mut ({}))", root, recv),
                THandleOp::RngBoolP => {
                    format!("{}jet_rng_bool_p(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::RngNormal => {
                    format!("{}jet_rng_normal(&mut ({}), {}, {})", root, recv, a(0), a(1))
                }
                THandleOp::RngExponential => {
                    format!("{}jet_rng_exponential(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::RngBytes => {
                    format!("{}jet_rng_bytes(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::RngSplit => format!("{}jet_rng_split(&mut ({}))", root, recv),
                THandleOp::RngPick => {
                    format!("{}jet_rng_pick(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::RngWeightedPick => {
                    format!("{}jet_rng_weighted_pick(&mut ({}), &({}), &({}))", root, recv, a(0), a(1))
                }
                THandleOp::RngSample => {
                    format!("{}jet_rng_sample(&mut ({}), &({}), {})", root, recv, a(0), a(1))
                }
                THandleOp::RngShuffle => {
                    format!("{}jet_rng_shuffle(&mut ({}), &mut ({}))", root, recv, a(0))
                }
                // D-SOLVER-LIB1=A: explicit finite solver state.
                THandleOp::SolverNew => format!("{}jet_solver_new({})", root, recv),
                THandleOp::SolverRequire => {
                    format!("{}jet_solver_require(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::SolverFailureCount => {
                    format!("{}jet_solver_failure_count(&({}))", root, recv)
                }
                THandleOp::SolverStatus => format!("{}jet_solver_status(&({}))", root, recv),
                THandleOp::GameSceneNew => format!("{}jet_game_scene_new(&({}))", root, recv),
                THandleOp::GameReplayRecord => {
                    format!("{}jet_game_replay_record(&({}))", root, recv)
                }
                THandleOp::GameBackendHeadless => format!("{}jet_game_backend_headless()", root),
                THandleOp::GameBackendShouldContinue => {
                    format!("{}jet_game_backend_should_continue(&({}))", root, recv)
                }
                THandleOp::GameBackendPresent => {
                    format!("{}jet_game_backend_present(&mut ({}))", root, recv)
                }
                THandleOp::TLSClientConfigDefault => {
                    format!("{}jet_tls_client_config_default()", root)
                }
                THandleOp::TLSClientConfigWithAlpn => format!(
                    "{}jet_tls_client_config_with_alpn(({}).clone(), &({}))",
                    root,
                    recv,
                    a(0)
                ),
                THandleOp::TLSRootCertificatesFromPem => {
                    let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
                    format!(
                        "{}jet_tls_root_certificates_from_pem(&({}), {}::jet_net_tls_validate_roots_impl)",
                        root, a(0), ffi,
                    )
                }
                THandleOp::TLSClientIdentityFromPem => {
                    let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
                    format!(
                        "{}jet_tls_client_identity_from_pem(&({}), &({}), {}::jet_net_tls_validate_identity_impl)",
                        root, a(0), a(1), ffi,
                    )
                }
                THandleOp::TLSClientConfigWithTrust => format!(
                    "{}jet_tls_client_config_with_trust(({}).clone(), ({}).clone())",
                    root, recv, a(0),
                ),
                THandleOp::TLSClientConfigWithIdentity => format!(
                    "{}jet_tls_client_config_with_client_identity(({}).clone(), &({}))",
                    root, recv, a(0),
                ),
                THandleOp::TLSClientConfigWithVersionBounds => format!(
                    "{}jet_tls_client_config_with_version_bounds(({}).clone(), ({}).clone(), ({}).clone())",
                    root, recv, a(0), a(1),
                ),
                THandleOp::HTTPClientNew => {
                    let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
                    format!(
                        "JetHTTPClient::new({ffi}::jet_http_client_new_impl(), {ffi}::jet_http_client_drop_impl)"
                    )
                }
                THandleOp::GameSceneOnFrame => {
                    format!("{}jet_game_scene_on_frame(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::GameSceneComponent => {
                    format!("{}jet_game_scene_component(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::GameSceneQuery => {
                    format!("{}jet_game_scene_query(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::GameAssetsImage => {
                    format!("{}jet_game_assets_image(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::GameAssetsSound => {
                    format!("{}jet_game_assets_sound(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::GameInputBind => {
                    format!("{}jet_game_input_bind(&({}), &({}), &({}))", root, recv, a(0), a(1))
                }
                THandleOp::GameInputPressed => {
                    format!("{}jet_game_input_pressed(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::DurationIn { unit } => {
                    let unit = unit.map_or_else(
                        || a(0),
                        |unit| format!("{}jet_std::DurationUnit::{unit}", root),
                    );
                    format!("{}jet_duration_in(&({}), &({}))", root, recv, unit)
                }
                THandleOp::DurationIsZero => {
                    format!("{}jet_duration_is_zero(&({}))", root, recv)
                }
                THandleOp::DurationTotalSeconds => {
                    format!("{}jet_duration_total_seconds(&({}))", root, recv)
                }
                THandleOp::DurationDifference => {
                    format!("{}jet_duration_difference(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::PreciseMethod { type_name, method } => {
                    let prefix = if type_name == "BigInt" {
                        "jet_bigint"
                    } else if type_name == "Fraction" {
                        "jet_fraction"
                    } else {
                        "jet_decimal"
                    };
                    if method == "to_string"
                        || method == "neg"
                        || method == "numerator"
                        || method == "denominator"
                        || method == "to_float"
                        || method == "is_zero"
                    {
                        format!("{}{}_{}(&({}))", root, prefix, method, recv)
                    } else {
                        format!(
                            "{}{}_{}(&({}), &({}))",
                            root, prefix, method, recv, a(0)
                        )
                    }
                }
                THandleOp::TcpListenerAccept => if args.is_empty() {
                    format!("{}jet_net_tcp_accept(&({}))", root, recv)
                } else {
                    format!("{}jet_net_tcp_accept_deadline(&({}), &({}))", root, recv, a(0))
                },
                THandleOp::TcpListenerLocalAddr => {
                    format!("{}jet_net_listener_local_addr(&({}))", root, recv)
                }
                THandleOp::TcpStreamRead => format!("{}jet_net_tcp_read(&mut ({}))", root, recv),
                THandleOp::TcpStreamWrite => {
                    format!("{}jet_net_tcp_write(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::TcpStreamPeerAddr => {
                    format!("{}jet_net_tcp_peer_addr(&({}))", root, recv)
                }
                THandleOp::TcpStreamLocalAddr => {
                    format!("{}jet_net_tcp_local_addr(&({}))", root, recv)
                }
                THandleOp::TcpStreamClose => {
                    format!("{}jet_net_tcp_close(&mut ({}))", root, recv)
                }
                THandleOp::TcpStreamReadBytes => {
                    if args.len() == 1 {
                        format!("{}jet_net_tcp_read_bytes(&mut ({}), {})", root, recv, a(0))
                    } else {
                        format!("{}jet_net_tcp_read_bytes_deadline(&mut ({}), {}, &({}))", root, recv, a(0), a(1))
                    }
                }
                THandleOp::TcpStreamReadText => {
                    if args.len() == 1 {
                        format!("{}jet_net_tcp_read_text(&mut ({}), {})", root, recv, a(0))
                    } else {
                        format!("{}jet_net_tcp_read_text_deadline(&mut ({}), {}, &({}))", root, recv, a(0), a(1))
                    }
                }
                THandleOp::TcpStreamWriteBytes => {
                    if args.len() == 1 {
                        format!("{}jet_net_tcp_write_bytes(&mut ({}), &({}))", root, recv, a(0))
                    } else {
                        format!("{}jet_net_tcp_write_bytes_deadline(&mut ({}), &({}), &({}))", root, recv, a(0), a(1))
                    }
                }
                THandleOp::TcpStreamWriteAllBytes => {
                    if args.len() == 1 {
                        format!("{}jet_net_tcp_write_all_bytes(&mut ({}), &({}))", root, recv, a(0))
                    } else {
                        format!("{}jet_net_tcp_write_all_bytes_deadline(&mut ({}), &({}), &({}))", root, recv, a(0), a(1))
                    }
                }
                THandleOp::TcpStreamWriteText => {
                    if args.len() == 1 {
                        format!("{}jet_net_tcp_write_text(&mut ({}), &({}))", root, recv, a(0))
                    } else {
                        format!("{}jet_net_tcp_write_text_deadline(&mut ({}), &({}), &({}))", root, recv, a(0), a(1))
                    }
                }
                THandleOp::TcpStreamShutdown => {
                    format!("{}jet_net_tcp_shutdown(&mut ({}), {})", root, recv, a(0))
                }
                THandleOp::TcpStreamReady => format!(
                    "{}jet_net_tcp_ready_deadline(&mut ({}), {}, &({}))", root, recv, a(0), a(1)
                ),
                THandleOp::UdpSocketReady => format!(
                    "{}jet_net_udp_ready(&({}), {}, &({}))", root, recv, a(0), a(1)
                ),
                THandleOp::UdpSocketClose => {
                    format!("{}jet_net_udp_close(&({}))", root, recv)
                }
                THandleOp::UdpSocketReceiveDeadline => format!(
                    "{}jet_net_udp_receive_deadline(&({}), {}, &({}))", root, recv, a(0), a(1)
                ),
                THandleOp::UdpSocketSendToDeadline => format!(
                    "{}jet_net_udp_send_bytes_to_deadline(&({}), &({}), &({}), &({}))", root, recv, a(0), a(1), a(2)
                ),
                THandleOp::UnixListenerAcceptDeadline => format!(
                    "{}jet_net_unix_accept_deadline(&({}), &({}))", root, recv, a(0)
                ),
                THandleOp::UnixStreamReadDeadline => format!(
                    "{}jet_net_unix_read_bytes_deadline(&mut ({}), {}, &({}))", root, recv, a(0), a(1)
                ),
                THandleOp::UnixStreamWriteAllDeadline => format!(
                    "{}jet_net_unix_write_all_bytes_deadline(&mut ({}), &({}), &({}))", root, recv, a(0), a(1)
                ),
                THandleOp::UnixStreamReady => format!(
                    "{}jet_net_unix_ready(&({}), {}, &({}))", root, recv, a(0), a(1)
                ),
                THandleOp::UnixStreamClose => format!(
                    "{}jet_net_unix_close(&mut ({}))", root, recv
                ),
                THandleOp::UnixStreamSetTimeout => format!(
                    "{}jet_net_unix_set_timeout(&mut ({}), &({}))", root, recv, a(0)
                ),
                THandleOp::TLSStreamReadDeadline => format!(
                    "{}jet_net_tls_read_bytes_deadline(&mut ({}), {}, &({}))", root, recv, a(0), a(1)
                ),
                THandleOp::TLSStreamWriteAllDeadline => format!(
                    "{}jet_net_tls_write_all_bytes_deadline(&mut ({}), &({}), &({}))", root, recv, a(0), a(1)
                ),
                THandleOp::TLSStreamReady => format!(
                    "{}jet_net_tls_ready(&({}), {}, &({}))", root, recv, a(0), a(1)
                ),
                THandleOp::TLSStreamClose => format!(
                    "{}jet_net_tls_close(&mut ({}))", root, recv
                ),
                THandleOp::TLSStreamCloseWrite => format!(
                    "{}jet_net_tls_close_write(&mut ({}), &({}))", root, recv, a(0)
                ),
                THandleOp::TLSStreamPeerIdentity => format!(
                    "{}jet_net_tls_peer_identity(&({}))", root, recv
                ),
                // c109 Phase 19: arena allocator methods (byte-for-byte the AST arms).
                THandleOp::AllocAlloc => {
                    let a0 = emit_tir_expr(&args[0], cx);
                    format!("({}).alloc({})", recv, a0)
                }
                THandleOp::AllocReset => format!("({}).reset()", recv),
                // c109 Phase 20: HTTPRequest/HTTPResponse accessors, byte-for-byte the
                // `emit_builtin_method` arms. The plain field accessors clone the field;
                // `header` does a map lookup; `param` calls the prelude helper.
                THandleOp::HTTPReqField(field) | THandleOp::HTTPRespField(field) => {
                    format!("({}).{}.clone()", recv, field)
                }
                THandleOp::HTTPReqHeader | THandleOp::HTTPRespHeader => {
                    format!("({}).headers.get(&{}).cloned()", recv, a(0))
                }
                THandleOp::HTTPReqParam => {
                    format!("{}jet_http_request_param(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::HTTPReqTrailers => {
                    format!("{}jet_http_srv_req_trailers(&({}))", root, recv)
                }
                THandleOp::HTTPRespTrailers => {
                    format!("{}jet_http_srv_response_trailers({}, {})", root, recv, a(0))
                }
                // c109 Phase 21: Task/Channel/Sender methods, byte-for-byte the
                // `emit_builtin_method` arms (Source/Codegen/Expression.rs). The handle
                // value's prelude methods take `&self`, so the receiver is emitted plainly
                // (Rust autoref); args are plain (raw `emit_expr`). `join` reuses the
                // no-arg `join` arm (`(recv).join()`); `detach` drops the handle (D-DETACH1).
                // D-ARGS1: ArgsSpec builder methods (consuming by value; builder is moved on each call).
                THandleOp::ArgsSpecFlag => {
                    format!("{}jet_args_flag({}, &({}), &({}))", root, recv, a(0), a(1))
                }
                THandleOp::ArgsSpecFlagShort => format!(
                    "{}jet_args_flag_short({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecOption => format!(
                    "{}jet_args_option({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecOptionShort => format!(
                    "{}jet_args_option_short({}, &({}), &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2),
                    a(3)
                ),
                THandleOp::ArgsSpecOptionDefault => format!(
                    "{}jet_args_option_default({}, &({}), &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2),
                    a(3)
                ),
                THandleOp::ArgsSpecOptionEnv => format!(
                    "{}jet_args_option_env({}, &({}), &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2),
                    a(3)
                ),
                THandleOp::ArgsSpecOptionInt => format!(
                    "{}jet_args_option_int({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecOptionFloat => format!(
                    "{}jet_args_option_float({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecOptionChoice => format!(
                    "{}jet_args_option_choice({}, &({}), &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2),
                    a(3)
                ),
                THandleOp::ArgsSpecRepeat => format!(
                    "{}jet_args_repeat({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecRequiredOption => format!(
                    "{}jet_args_required_option({}, &({}), &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecPositional => format!(
                    "{}jet_args_positional({}, &({}), &({}))",
                    root,
                    recv,
                    a(0),
                    a(1)
                ),
                THandleOp::ArgsSpecSubcommand => format!(
                    "{}jet_args_subcommand({}, &({}), &({}), {})",
                    root,
                    recv,
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ArgsSpecVersion => {
                    format!("{}jet_args_version({}, &({}))", root, recv, a(0))
                }
                THandleOp::ArgsSpecCompletion => {
                    format!("{}jet_args_completion(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ArgsSpecHelp => format!("({}).help()", recv),
                THandleOp::ArgsSpecParse => {
                    format!("{}jet_args_parse(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ArgsSpecParseOrExit => {
                    format!("{}jet_args_parse_or_exit(&({}), &({}))", root, recv, a(0))
                }
                // D-ARGS1: ParsedArgs query methods.
                THandleOp::ParsedArgsFlag => {
                    format!("{}jet_parsed_flag(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ParsedArgsOption => {
                    format!("{}jet_parsed_option(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ParsedArgsOptionInt => {
                    format!("{}jet_parsed_option_int(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ParsedArgsOptionFloat => {
                    format!("{}jet_parsed_option_float(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ParsedArgsOptions => {
                    format!("{}jet_parsed_options(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::ParsedArgsPositional => {
                    format!("{}jet_parsed_positional(&({}), {})", root, recv, a(0))
                }
                THandleOp::ParsedArgsSubcommand => {
                    format!("{}jet_parsed_subcommand(&({}))", root, recv)
                }
                THandleOp::ProcessSpecMethod { method } => match method.as_str() {
                    "cwd" => format!("{}jet_process_spec_cwd({}, &({}))", root, recv, a(0)),
                    "env" => format!(
                        "{}jet_process_spec_env({}, &({}), &({}))",
                        root,
                        recv,
                        a(0),
                        a(1)
                    ),
                    "env_remove" => {
                        format!("{}jet_process_spec_env_remove({}, &({}))", root, recv, a(0))
                    }
                    "env_clear" => format!("{}jet_process_spec_env_clear({})", root, recv),
                    "stdin" => {
                        format!("{}jet_process_spec_stdin({}, &({}))", root, recv, a(0))
                    }
                    "stdout" => {
                        format!("{}jet_process_spec_stdout({}, &({}))", root, recv, a(0))
                    }
                    "stderr" => {
                        format!("{}jet_process_spec_stderr({}, &({}))", root, recv, a(0))
                    }
                    "timeout" => {
                        format!("{}jet_process_spec_timeout({}, &({}))", root, recv, a(0))
                    }
                    "output_limit" => {
                        format!("{}jet_process_spec_output_limit({}, {})", root, recv, a(0))
                    }
                    "detached" => format!("{}jet_process_spec_detached({})", root, recv),
                    // D-PROCESS-SESSION1=A: terminal-backed session opt-in.
                    "terminal" if args.is_empty() => {
                        format!("{}jet_process_spec_terminal({})", root, recv)
                    }
                    "terminal" => format!(
                        "{}jet_process_spec_terminal_with_policy({}, &({}))",
                        root,
                        recv,
                        a(0)
                    ),
                    "capabilities" => {
                        format!("{}jet_process_spec_capabilities(&({}))", root, recv)
                    }
                    "run" => format!("{}jet_process_spec_run(&({}))", root, recv),
                    "run_checked" => {
                        format!("{}jet_process_spec_run_checked(&({}))", root, recv)
                    }
                    "spawn" => format!("{}jet_process_spec_spawn(&({}))", root, recv),
                    _ => format!("/* unsupported ProcessSpec.{method} */ {{ unreachable!() }}"),
                },
                THandleOp::ProcessChildMethod { method } => match method.as_str() {
                    "id" => format!("{}jet_process_child_id(&({}))", root, recv),
                    "wait" => format!("{}jet_process_child_wait(&({}))", root, recv),
                    // #1481 core.process: a non-blocking poll alongside the
                    // blocking `wait()` — same handle, no new mechanism.
                    "exited" => format!("{}jet_process_child_exited(&({}))", root, recv),
                    "kill" => format!("{}jet_process_child_kill(&({}))", root, recv),
                    "terminate" => {
                        format!("{}jet_process_child_terminate(&({}))", root, recv)
                    }
                    "interrupt" => format!("{}jet_process_child_interrupt(&({}))", root, recv),
                    _ => format!("/* unsupported ProcessChild.{method} */ {{ unreachable!() }}"),
                },
                THandleOp::TerminalSessionResize => {
                    format!("{}jet_terminal_session_resize(&({}), &({}))", root, recv, a(0))
                }
                // D-PROCESS1=A: `child.stdin.write(text)` — recv is already the
                // lowered `(child).stdin` field access (a writer handle).
                THandleOp::ProcessStdinWrite => {
                    format!("{}jet_process_stdin_write(&({}), &({}))", root, recv, a(0))
                }
                // D-ANY-JAI1 (c7jaiany §6): Value/Field are plain inherent-method
                // passthroughs, same shape as `ArgsSpecHelp`.
                THandleOp::ReflectValueTypeName => format!("({}).type_name()", recv),
                THandleOp::ReflectValueDisplay => format!("({}).display()", recv),
                THandleOp::ReflectValueFields => format!("({}).fields()", recv),
                THandleOp::ReflectFieldName => format!("({}).name()", recv),
                THandleOp::ReflectFieldValue => format!("({}).value()", recv),
                THandleOp::TaskJoin => format!("({}).join()", recv),
                THandleOp::TaskDetach => format!("({}).detach()", recv),
                THandleOp::TaskPause => {
                    if args.is_empty() {
                        format!("({}).pause()", recv)
                    } else {
                        format!("({}).pause_with_mode({})", recv, a(0))
                    }
                }
                THandleOp::TaskResume => format!("({}).resume()", recv),
                THandleOp::TaskCancel => format!("({}).cancel()", recv),
                THandleOp::TaskTrace => format!("({}).trace()", recv),
                THandleOp::TaskException => format!("({}).exception()", recv),
                THandleOp::TaskDetachAll => {
                    format!("{}jet_std::jet_task_detach_all({})", cx.root_prefix, recv)
                }
                THandleOp::TaskCancelAll => {
                    format!("{}jet_std::jet_task_cancel_all(&({}))", cx.root_prefix, recv)
                }
                THandleOp::TaskPauseAll => {
                    if args.is_empty() {
                        format!("{}jet_std::jet_task_pause_all(&({}))", cx.root_prefix, recv)
                    } else {
                        format!(
                            "{}jet_std::jet_task_pause_all_mode(&({}), {})",
                            cx.root_prefix,
                            recv,
                            a(0)
                        )
                    }
                }
                THandleOp::TaskResumeAll => {
                    format!("{}jet_std::jet_task_resume_all(&({}))", cx.root_prefix, recv)
                }
                THandleOp::TaskTraceAll => {
                    format!("{}jet_std::jet_task_trace_all(&({}))", cx.root_prefix, recv)
                }
                THandleOp::ChannelReceive => format!("({}).receive()", recv),
                THandleOp::ChannelClose => format!("({}).close()", recv),
                THandleOp::SenderSend => format!("({}).send({})", recv, a(0)),
                // D-REACT1=B: reactive Signal/Derived reads and writes.
                THandleOp::ReactiveGet => format!("({}).get()", recv),
                THandleOp::ReactiveSet => format!("({}).set({})", recv, a(0)),
                // D-EVENT1=D: first-party typed Event/Hook runtime family.
                THandleOp::EventMethod { method } => match method.as_str() {
                    "on" | "once" => format!("({}).{}(&({}), {})", recv, method, a(0), a(1)),
                    "on_priority" => {
                        format!("({}).on_priority(&({}), {}, {})", recv, a(0), a(1), a(2))
                    }
                    "emit" | "emit_async" | "cancel" | "unsubscribe" | "active_count"
                    | "trace" | "listener_count" | "queued_count" | "summary" | "delivered"
                    | "queued" | "dropped" | "close" | "running_count" | "blocked_count"
                    | "accepted" | "delivered_handlers" | "state" | "failures" => {
                        if args.is_empty() {
                            format!("({}).{}()", recv, method)
                        } else {
                            format!("({}).{}({})", recv, method, a(0))
                        }
                    }
                    "is_active" => format!("({}).active()", recv),
                    "run" => format!("({}).run({}, {})", recv, a(0), a(1)),
                    _ => format!("({}).{}()", recv, method),
                },
                // D-WATCH-SCOPE1: unified watcher handle/set runtime.
                THandleOp::WatchMethod { method, .. } => match method.as_str() {
                    "on" | "once" => format!("({}).{}(&({}), {})", recv, method, a(0), a(1)),
                    "add" => format!("({}).add({})", recv, a(0)),
                    "poll" | "events" | "summary" | "cancel" => {
                        if args.is_empty() {
                            format!("({}).{}()", recv, method)
                        } else {
                            format!("({}).{}({})", recv, method, a(0))
                        }
                    }
                    "is_active" => format!("({}).active()", recv),
                    _ => format!("({}).{}()", recv, method),
                },
                // D-HONESTNUM1=A: Measurement<Float> arithmetic + accessors.
                THandleOp::MeasurementMethod { method } => {
                    if args.is_empty() {
                        format!("({}).{}()", recv, method)
                    } else {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                }
                // D-LAYOUT1 / D-LAYOUT-GATES1: `LayoutHandle`/`Constraint`
                // methods — every Jet method name IS the Rust method name.
                THandleOp::LayoutMethod { method } => {
                    let joined = (0..args.len()).map(a).collect::<Vec<_>>().join(", ");
                    format!("({}).{}({})", recv, method, joined)
                }
                // D-PENDING1=B: Loadable<T,E> methods.
                THandleOp::LoadableMethod { method } => {
                    if args.is_empty() {
                        format!("({}).{}()", recv, method)
                    } else {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                }
                // D-SHAPE-CTORVERB1=C: generic ExpiringValue<T> methods.
                THandleOp::ExpiringMethod { method } => match method.as_str() {
                    "get" => format!(
                        "{}jet_expiring_get(&({}), {}jet_clock_now(&({})))",
                        root, recv, root, a(0)
                    ),
                    "is_valid" => format!(
                        "({}).is_valid({}jet_clock_now(&({})))",
                        recv, root, a(0)
                    ),
                    _ => format!("({}).{}()", recv, method),
                },
                // D-RENDERTGT2=A (c133 M1): NullBackend measure/layout/paint/on_event/commands.
                THandleOp::UiBackendMethod { method } => match method.as_str() {
                    "measure" => format!(
                        "({}).measure_node(({}).clone(), ({}).clone())",
                        recv,
                        a(0),
                        a(1)
                    ),
                    "layout" => format!(
                        "({}).layout_node(({}).clone(), ({}).clone())",
                        recv,
                        a(0),
                        a(1)
                    ),
                    "paint" => format!("({}).paint_node(({}).clone())", recv, a(0)),
                    // D-UI-MOUNT1=A
                    "mount" if args.len() == 1 => {
                        format!("({}).mount_node_default(({}).clone())", recv, a(0))
                    }
                    "mount" => format!(
                        "({}).mount_node(({}).clone(), ({}).clone())",
                        recv,
                        a(0),
                        a(1)
                    ),
                    "on_event" => format!("({}).dispatch_event(({}).clone())", recv, a(0)),
                    "commands" => format!("({}).paint_commands()", recv),
                    "frame_lines" => format!("({}).frame_lines()", recv),
                    "render_count" => format!("({}).render_count()", recv),
                    // D-A11YGATE1=B (c134 Phase 6): keyboard focus routing.
                    "set_focus_group" => {
                        format!("({}).set_focus_group(({}).clone())", recv, a(0))
                    }
                    "focused_label" => format!("({}).focused_label()", recv),
                    // D-UIDEVSHELL1=A (c134 Phase 8): native GTK4 retained widgets.
                    "label" => format!("({}).label(&({}))", recv, a(0)),
                    "button" => format!("({}).button(&({}))", recv, a(0)),
                    "set_text" => format!("({}).set_text({}, &({}))", recv, a(0), a(1)),
                    "set_size" => format!("({}).set_size({}, {}, {})", recv, a(0), a(1), a(2)),
                    "set_color" => format!("({}).set_color({}, &({}))", recv, a(0), a(1)),
                    "on_click" => format!("({}).on_click({}, {})", recv, a(0), a(1)),
                    "present" => format!("({}).present(&({}))", recv, a(0)),
                    _ => format!("({}).{}()", recv, method),
                },
                // c-devserver (owner-directed 2026-07-01): DevServer builder
                // methods — the Rust method names match the Jet ones exactly.
                THandleOp::DevServerMethod { method } => match method.as_str() {
                    "html" => format!("({}).html(({}).clone())", recv, a(0)),
                    "port" => format!("({}).port({})", recv, a(0)),
                    "serve" => format!("({}).serve()", recv),
                    _ => format!("({}).{}()", recv, method),
                },
                // D-WEBAPP1=D: WebApp builder methods — Rust names match Jet.
                THandleOp::WebAppMethod { method } => match method.as_str() {
                    "route" | "page" | "layout" | "action" | "form" | "data" => format!(
                        "({}).{}(({}).clone(), {})",
                        recv,
                        method,
                        a(0),
                        web_handler(1)
                    ),
                    "mount" => match args.len() {
                        2 => format!(
                            "({}).mount(({}).clone(), {})",
                            recv,
                            a(0),
                            web_handler(1)
                        ),
                        _ => format!(
                            "({}).mount(({}).clone(), {})",
                            recv,
                            a(0),
                            web_handler(1)
                        ),
                    },
                    "routes" | "security" | "assets" | "split" | "code_split" | "cache" | "a11y"
                    | "adapter" => {
                        format!("({}).{}(({}).clone())", recv, method, a(0))
                    }
                    "csr" | "ssr" | "ssg" | "stream" | "streaming" | "island" | "hydration_dev"
                    | "hydration_release" | "facts_json" => {
                        format!("({}).{}()", recv, method)
                    }
                    "serve" if args.is_empty() => format!("({}).serve()", recv),
                    "serve" => format!("({}).serve_on({})", recv, a(0)),
                    _ => format!("({}).{}()", recv, method),
                },
                // D-NETDEP1=A / D-HTTPLIB1=A: HTTP client method call.
                // "body"/"header" dispatch by arity: 0-arg=response accessor, 1-arg=request builder.
                THandleOp::HTTPClientMethod { kind, method } => {
                    let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
                    if kind == "HTTPClient" {
                        let policy = |call: String| {
                            let error = emit_http_bridge_error(ffi, "error");
                            format!(
                                "{{ let _client = ({}); let _next = ({call}).map_err(|error| {error}); _client.policy(_next, {ffi}::jet_http_client_drop_impl) }}",
                                recv,
                            )
                        };
                        match method.as_str() {
                            "cookies" => policy(format!(
                                "{{ let _jar = &({}); match _jar {{ JetHTTPCookieJar::Memory => {ffi}::jet_http_client_cookies_impl(_client.owner.handle, true), }} }}",
                                a(0),
                            )),
                            "redirects" => {
                                let error = emit_http_bridge_error(ffi, "error");
                                format!(
                                    "{{ let _client = ({recv}); let _policy = &({arg}); let _next = (match _policy {{ JetHTTPRedirectPolicy::Follow {{ max, same_origin_credentials }} => {ffi}::jet_http_client_redirects_impl(_client.owner.handle, *max, *same_origin_credentials), }}).map_err(|error| {error}); _client.policy(_next, {ffi}::jet_http_client_drop_impl) }}",
                                    recv = recv,
                                    arg = a(0),
                                    ffi = ffi,
                                    error = error,
                                )
                            }
                            "protocols" => policy(format!(
                                "{ffi}::jet_http_client_protocols_impl(_client.owner.handle, {}, {}, {})",
                                a(0), a(1), a(2)
                            )),
                            "timeouts" => policy(format!(
                                "{ffi}::jet_http_client_timeouts_impl(_client.owner.handle, {}, {}, {}, {}, {}, {}, Some({}))",
                                a(0), a(1), a(2), a(3), a(4), a(5), a(6)
                            )),
                            "raw_encoding" => policy(format!(
                                "{ffi}::jet_http_client_decompression_impl(_client.owner.handle, false)"
                            )),
                            "proxy" => {
                                let error = emit_http_bridge_error(ffi, "error");
                                format!(
                                    "{{ let _client = ({recv}); let _proxy = &({arg}); let _next = (match _proxy {{ JetHTTPProxy::FromEnvironment => {ffi}::jet_http_client_proxy_from_environment_impl(_client.owner.handle), JetHTTPProxy::None => {ffi}::jet_http_client_proxy_impl(_client.owner.handle, None), JetHTTPProxy::Url(url) => {ffi}::jet_http_client_proxy_impl(_client.owner.handle, Some(url.as_str())), }}).map_err(|error| {error}); _client.policy(_next, {ffi}::jet_http_client_drop_impl) }}",
                                    recv = recv,
                                    arg = a(0),
                                    ffi = ffi,
                                    error = error,
                                )
                            }
                            "tls" => {
                                let error = emit_http_bridge_error(ffi, "error");
                                format!(
                                    "{{ let _client = ({recv}); let (_trust, _roots, _cert, _key, _min, _max) = jet_tls_client_config_http_parts(&({arg})); let _next = {ffi}::jet_http_client_tls_impl(_client.owner.handle, _trust, &_roots, &_cert, &_key, _min, _max).map_err(|error| {error}); _client.policy(_next, {ffi}::jet_http_client_drop_impl) }}",
                                    recv = recv,
                                    arg = a(0),
                                    ffi = ffi,
                                    error = error,
                                )
                            }
                            "allow_http_downgrade" => policy(format!(
                                "{ffi}::jet_http_client_allow_http_downgrade_impl(_client.owner.handle, {})",
                                a(0)
                            )),
                            "retries" => {
                                let error = emit_http_bridge_error(ffi, "error");
                                format!(
                                    "{{ let _client = ({recv}); let _policy = &({arg}); let _mode = match _policy {{ JetHTTPRetryPolicy::None => 0_i64, JetHTTPRetryPolicy::Safe => 1_i64, JetHTTPRetryPolicy::Idempotent => 2_i64, }}; let _next = {ffi}::jet_http_client_retries_impl(_client.owner.handle, _mode).map_err(|error| {error}); _client.policy(_next, {ffi}::jet_http_client_drop_impl) }}",
                                    recv = recv,
                                    arg = a(0),
                                    ffi = ffi,
                                    error = error,
                                )
                            },
                            "send" => {
                                let call = format!(
                                    "{ffi}::jet_http_client_send_with_stream_impl(_client.owner.handle, &_r.method, &_r.url, &_r.headers.to_flat(), body_len, has_body, &mut body_read, _r.timeout_ms, _r.connect_timeout_ms, _r.read_timeout_ms, _r.total_timeout_ms, _r.dns_timeout_ms, _r.tls_timeout_ms, _r.write_timeout_ms, _r.first_byte_timeout_ms, _r.redirects, _r.proxy.as_deref(), &_r.cookies, &_r.form, &_r.multipart)"
                                );
                                let response = emit_http_response_from_bridge(call, ffi);
                                let error = emit_http_bridge_error(ffi, "error");
                                format!(
                                    "{{ let _client = &({recv}); match &_client.policy_error {{ Some(error) => Err(error.clone()), None => {{ let _r = &({arg}); match &_r.header_error {{ Some(error) => Err(error.clone()), None => {{ {ffi}::JetHTTPAmbientDeadline::push(jet_deadline_remaining_ms()).map_err(|error| {error}).and_then(|_ambient| jet_http_client_body_upload(_r).and_then(|(body_len, has_body, mut chunks)| {{ let mut body_read = || -> Result<Option<Vec<u8>>, {ffi}::JetHTTPBridgeError> {{ match chunks.as_mut() {{ None => Ok(None), Some(iter) => match iter.next() {{ None => Ok(None), Some(Ok(chunk)) => Ok(Some(chunk)), Some(Err(_)) => Err({ffi}::JetHTTPBridgeError::IO) }} }} }}; {response} }})) }} }} }} }} }}",
                                    recv = recv,
                                    arg = a(0),
                                    ffi = ffi,
                                    error = error,
                                    response = response,
                                )
                            }
                            _ => unreachable!("unknown HTTPClient method {method}"),
                        }
                    } else if kind == "HTTPBody" {
                        match method.as_str() {
                            "bytes" => format!("{}jet_http_body_bytes(&({}), {})", root, recv, a(0)),
                            "text" => format!("{}jet_http_body_text(&({}), {})", root, recv, a(0)),
                            "json" => {
                                let target = match &e.ty {
                                    Type::Result { ok, .. } => cx.rust_type(ok),
                                    _ => unreachable!("Body.json must return Result<T, HTTPError>"),
                                };
                                format!("{}jet_http_body_json::<{}>(&({}), {})", root, target, recv, a(0))
                            }
                            "chunks" => format!(
                                "{}jet_http_body_chunks(&({}), {})",
                                root,
                                recv,
                                if args.is_empty() { "65536".to_string() } else { a(0) },
                            ),
                            "copy_to" => format!("{}jet_http_body_copy_to(&({}), &mut ({}), {})", root, recv, a(0), a(1)),
                            _ => unreachable!("unknown HTTPBody method {method}"),
                        }
                    } else if kind == "HTTPHeaders" {
                        match method.as_str() {
                            "first" => format!("{}jet_http_headers_first(&({}), &({}))", root, recv, a(0)),
                            "all" => format!("{}jet_http_headers_all(&({}), &({}))", root, recv, a(0)),
                            "append" => format!("{}jet_http_headers_append({}, &({}), &({}))", root, recv, a(0), a(1)),
                            "set" => format!("{}jet_http_headers_set({}, &({}), &({}))", root, recv, a(0), a(1)),
                            "remove" => format!("{}jet_http_headers_remove({}, &({}))", root, recv, a(0)),
                            _ => unreachable!("unknown HTTPHeaders method {method}"),
                        }
                    } else if kind == "HTTPRequest" {
                        match method.as_str() {
                            "trailers" => format!("{}jet_http_srv_req_trailers(&({}))", root, recv),
                            "header" => format!(
                                "{}jet_http_client_request_header({}, &({}), &({}))",
                                root,
                                recv,
                                a(0),
                                a(1)
                            ),
                            "body" => {
                                let arg = &args[0];
                                if matches!(&arg.ty, Type::Named(name) if name == "HTTPBody") {
                                    format!(
                                        "{}jet_http_client_request_body_stream({}, ({}))",
                                        root,
                                        recv,
                                        a(0)
                                    )
                                } else {
                                    format!(
                                        "{}jet_http_client_request_body({}, &({}))",
                                        root,
                                        recv,
                                        a(0)
                                    )
                                }
                            }
                            "timeout" => format!(
                                "{}jet_http_client_request_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "connect_timeout" => format!(
                                "{}jet_http_client_request_connect_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "read_timeout" => format!(
                                "{}jet_http_client_request_read_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "total_timeout" => format!(
                                "{}jet_http_client_request_total_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "dns_timeout" => format!(
                                "{}jet_http_client_request_dns_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "tls_timeout" => format!(
                                "{}jet_http_client_request_tls_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "write_timeout" => format!(
                                "{}jet_http_client_request_write_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "first_byte_timeout" => format!(
                                "{}jet_http_client_request_first_byte_timeout({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "redirects" => format!(
                                "{}jet_http_client_request_redirects({}, {})",
                                root,
                                recv,
                                a(0)
                            ),
                            "proxy" => format!(
                                "{}jet_http_client_request_proxy({}, &({}))",
                                root,
                                recv,
                                a(0)
                            ),
                            "cookie" => format!(
                                "{}jet_http_client_request_cookie({}, &({}), &({}))",
                                root,
                                recv,
                                a(0),
                                a(1)
                            ),
                            "form" => format!(
                                "{}jet_http_client_request_form({}, &({}), &({}))",
                                root,
                                recv,
                                a(0),
                                a(1)
                            ),
                            "multipart_text" => format!(
                                "{}jet_http_client_request_multipart_text({}, &({}), &({}))",
                                root,
                                recv,
                                a(0),
                                a(1)
                            ),
                            "send" => {
                                // call bridge with req fields; assemble JetHTTPResponse
                                let call = format!(
                                    "{ffi}::jet_http_client_send_stream_impl(&_r.method, &_r.url, &_r.headers.to_flat(), body_len, has_body, &mut body_read, _r.timeout_ms, _r.connect_timeout_ms, _r.read_timeout_ms, _r.total_timeout_ms, _r.dns_timeout_ms, _r.tls_timeout_ms, _r.write_timeout_ms, _r.first_byte_timeout_ms, _r.redirects, _r.proxy.as_deref(), &_r.cookies, &_r.form, &_r.multipart)",
                                    ffi = ffi
                                );
                                let response = emit_http_response_from_bridge(call, ffi);
                                let error = emit_http_bridge_error(ffi, "error");
                                format!(
                                    "{{ let _r = &({recv}); match &_r.header_error {{ Some(error) => Err(error.clone()), None => {{ {ffi}::JetHTTPAmbientDeadline::push(jet_deadline_remaining_ms()).map_err(|error| {error}).and_then(|_ambient| jet_http_client_body_upload(_r).and_then(|(body_len, has_body, mut chunks)| {{ let mut body_read = || -> Result<Option<Vec<u8>>, {ffi}::JetHTTPBridgeError> {{ match chunks.as_mut() {{ None => Ok(None), Some(iter) => match iter.next() {{ None => Ok(None), Some(Ok(chunk)) => Ok(Some(chunk)), Some(Err(_)) => Err({ffi}::JetHTTPBridgeError::IO) }} }} }}; {response} }})) }} }} }}",
                                    recv = recv,
                                    ffi = ffi,
                                    error = error,
                                    response = response,
                                )
                            }
                            _ => format!("({}).{}()", recv, method),
                        }
                    } else {
                        match method.as_str() {
                            "trailers" => format!("{}jet_http_srv_response_trailers({}, {})", root, recv, a(0)),
                            "status" => {
                                format!("{}jet_http_client_response_status(&({}))", root, recv)
                            }
                            "body" => format!("{}jet_http_client_response_body(&({}))", root, recv),
                            // D-HTTP-JSON1=A: read the body, then decode it.
                            // Without a limit the shared body cap applies.
                            "json" => {
                                let target = match &e.ty {
                                    Type::Result { ok, .. } => cx.rust_type(ok),
                                    _ => unreachable!("resp.json must return Result<T, HTTPError>"),
                                };
                                let limit = if args.is_empty() {
                                    "None".to_string()
                                } else {
                                    format!("Some({})", a(0))
                                };
                                format!(
                                    "{root}jet_http_body_json_defaulted::<{target}>(&({root}jet_http_client_response_body(&({recv}))), {limit})"
                                )
                            }
                            "header" => format!(
                                "{}jet_http_client_response_header(&({}), &({}))",
                                root,
                                recv,
                                a(0)
                            ),
                            "cookies" => {
                                format!("{}jet_http_response_cookies(&({}))", root, recv)
                            }
                            "protocol" => format!(
                                "{}jet_http_client_response_protocol(&({}))", root, recv
                            ),
                            "remote_address" => format!(
                                "{}jet_http_client_response_remote_address(&({}))", root, recv
                            ),
                            "redirect_history" => format!(
                                "{}jet_http_client_response_redirect_history(&({}))", root, recv
                            ),
                            "timings" => format!(
                                "{}jet_http_client_response_timings(&({}))", root, recv
                            ),
                            "reused_connection" => format!(
                                "{}jet_http_client_response_reused(&({}))", root, recv
                            ),
                            "raw_content_encoding" => format!(
                                "{}jet_http_client_response_raw_encoding(&({}))", root, recv
                            ),
                            _ => format!("({}).{}()", recv, method),
                        }
                    }
                }
                // D-NETDEP1=A / D-HTTPLIB1=A: HTTP server method call.
                THandleOp::HTTPServerMethod { kind, method } => {
                    match (kind.as_str(), method.as_str()) {
                        ("HTTPMux", "get" | "post" | "put" | "delete" | "patch" | "head" | "options") => {
                            format!(
                                "{{ {}jet_http_mux_add_handler(&({}), \"{}\", &({}), {}) }}",
                                root,
                                recv,
                                method.to_uppercase(),
                                a(0),
                                a(1)
                            )
                        }
                        ("HTTPMux", "middleware") => {
                            format!("{{ {}jet_http_mux_middleware(&({}), {}) }}", root, recv, a(0))
                        }
                        ("HTTPHandler", "handle") => format!("({})({})", recv, a(0)),
                        ("HTTPRequest", "method") => {
                            format!("{}jet_http_srv_req_method(&({}))", root, recv)
                        }
                        ("HTTPRequest", "path") => {
                            format!("{}jet_http_srv_req_path(&({}))", root, recv)
                        }
                        ("HTTPRequest", "body") => {
                            format!("{}jet_http_srv_req_body(&({}))", root, recv)
                        }
                        // D-HTTP-JSON1=A: the ratified body cap frames the read.
                        ("HTTPRequest", "json") => {
                            let target = match &e.ty {
                                Type::Result { ok, .. } => cx.rust_type(ok),
                                _ => unreachable!("req.json must return Result<T, HTTPError>"),
                            };
                            format!(
                                "{root}jet_http_body_json_defaulted::<{target}>(&({root}jet_http_srv_req_body(&({recv}))), None)"
                            )
                        }
                        ("HTTPRequest", "trailers") => {
                            format!("{}jet_http_srv_req_trailers(&({}))", root, recv)
                        }
                        ("HTTPRequest", "param") => {
                            format!("{}jet_http_srv_req_param(&({}), &({}))", root, recv, a(0))
                        }
                        ("HTTPRequest", "header") => {
                            format!("{}jet_http_srv_req_header(&({}), &({}))", root, recv, a(0))
                        }
                        ("HTTPRequest", "body_len") => {
                            format!("{}jet_http_srv_req_body_len(&({}))", root, recv)
                        }
                        ("HTTPRequest", "under_limit") => format!(
                            "{}jet_http_srv_req_under_limit(&({}), {})",
                            root,
                            recv,
                            a(0)
                        ),
                        ("WsConn", "send_text") => {
                            format!("{}jet_ws_send_text(&({}), &({}))", root, recv, a(0))
                        }
                        ("WsConn", "send_bytes") => {
                            format!("{}jet_ws_send_binary(&({}), &({}))", root, recv, a(0))
                        }
                        ("WsConn", "recv") => format!("{}jet_ws_recv(&({}))", root, recv),
                        ("WsConn", "close") => format!(
                            "{}jet_ws_close(&({}), {}, &({}))",
                            root,
                            recv,
                            a(0),
                            a(1)
                        ),
                        ("WsMessage", "is_text") => {
                            format!("{}jet_ws_message_is_text(&({}))", root, recv)
                        }
                        ("WsMessage", "is_binary") => {
                            format!("{}jet_ws_message_is_binary(&({}))", root, recv)
                        }
                        ("WsMessage", "is_close") => {
                            format!("{}jet_ws_message_is_close(&({}))", root, recv)
                        }
                        ("WsMessage", "text") => format!("{}jet_ws_message_text(&({}))", root, recv),
                        ("WsMessage", "bytes") => {
                            format!("{}jet_ws_message_bytes(&({}))", root, recv)
                        }
                        // D-BROWSER-AUTO1=A: native BiDi handle methods.
                        ("Browser", "capabilities") => {
                            format!("{}jet_browser_capabilities(&({}))", root, recv)
                        }
                        ("Browser", "context") => {
                            format!("{}jet_browser_context(&({}))", root, recv)
                        }
                        ("Browser", "subscribe") => format!(
                            "{}jet_browser_subscribe(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("Browser", "next_event") => format!(
                            "{}jet_browser_next_event(&({}), {})",
                            root,
                            recv,
                            a(0)
                        ),
                        ("Browser", "add_intercept") => format!(
                            "{}jet_browser_add_intercept(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("Browser", "add_intercept_url") => format!(
                            "{}jet_browser_add_intercept_url(&({}), &({}), &({}))",
                            root,
                            recv,
                            a(0),
                            a(1)
                        ),
                        ("Browser", "continue_request") => format!(
                            "{}jet_browser_continue_request(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("Browser", "fail_request") => format!(
                            "{}jet_browser_fail_request(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("Browser", "fulfill_request") => format!(
                            "{}jet_browser_fulfill_request(&({}), &({}), {}, &({}))",
                            root,
                            recv,
                            a(0),
                            a(1),
                            a(2)
                        ),
                        ("Browser", "allow_downloads") => format!(
                            "{}jet_browser_allow_downloads(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("Browser", "protocol") => format!(
                            "{}jet_browser_protocol(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("Browser", "trace") => {
                            format!("{}jet_browser_trace(&({}))", root, recv)
                        }
                        ("Browser", "privacy") => {
                            format!("{}jet_browser_privacy(&({}))", root, recv)
                        }
                        ("Browser", "receipt") => {
                            format!("{}jet_browser_receipt(&({}))", root, recv)
                        }
                        ("Browser", "close") => {
                            format!("{}jet_browser_close(&({}))", root, recv)
                        }
                        ("BrowserContext", "page") => {
                            format!("{}jet_browser_context_page(&({}))", root, recv)
                        }
                        ("BrowserContext", "tab") => {
                            format!("{}jet_browser_context_tab(&({}))", root, recv)
                        }
                        ("BrowserContext", "close") => {
                            format!("{}jet_browser_context_close(&({}))", root, recv)
                        }
                        ("BrowserContext", "isolated") => {
                            format!("{}jet_browser_context_isolated(&({}))", root, recv)
                        }
                        ("BrowserContext", "user_hash") => {
                            format!("{}jet_browser_context_user_hash(&({}))", root, recv)
                        }
                        ("BrowserPage", "goto") => format!(
                            "{}jet_browser_page_goto(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserPage", "get_by_role") => format!(
                            "{}jet_browser_page_get_by_role(&({}), &({}), &({}))",
                            root,
                            recv,
                            a(0),
                            a(1)
                        ),
                        ("BrowserPage", "get_by_text") => format!(
                            "{}jet_browser_page_get_by_text(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserPage", "get_by_label") => format!(
                            "{}jet_browser_page_get_by_label(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserPage", "get_by_placeholder") => format!(
                            "{}jet_browser_page_get_by_placeholder(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserPage", "get_by_test_id") => format!(
                            "{}jet_browser_page_get_by_test_id(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserPage", "get_by_css") => format!(
                            "{}jet_browser_page_get_by_css(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserPage", "main_frame") => {
                            format!("{}jet_browser_page_main_frame(&({}))", root, recv)
                        }
                        ("BrowserPage", "frames") => {
                            format!("{}jet_browser_page_frames(&({}))", root, recv)
                        }
                        ("BrowserPage", "close") => {
                            format!("{}jet_browser_page_close(&({}))", root, recv)
                        }
                        ("BrowserPage", "screenshot") => {
                            format!("{}jet_browser_page_screenshot(&({}))", root, recv)
                        }
                        ("BrowserPage", "pdf") => {
                            format!("{}jet_browser_page_pdf(&({}))", root, recv)
                        }
                        ("BrowserPage", "set_cookie") => format!(
                            "{}jet_browser_page_set_cookie(&({}), &({}), &({}), &({}))",
                            root,
                            recv,
                            a(0),
                            a(1),
                            a(2)
                        ),
                        ("BrowserPage", "cookie") => format!(
                            "{}jet_browser_page_cookie(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserPage", "clear_cookies") => {
                            format!("{}jet_browser_page_clear_cookies(&({}))", root, recv)
                        }
                        ("BrowserPage", "storage_get") => format!(
                            "{}jet_browser_page_storage_get(&({}), &({}), &({}))",
                            root,
                            recv,
                            a(0),
                            a(1)
                        ),
                        ("BrowserPage", "storage_set") => format!(
                            "{}jet_browser_page_storage_set(&({}), &({}), &({}), &({}))",
                            root,
                            recv,
                            a(0),
                            a(1),
                            a(2)
                        ),
                        ("BrowserPage", "storage_clear") => format!(
                            "{}jet_browser_page_storage_clear(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserFrame", "close") => {
                            format!("{}jet_browser_frame_close(&({}))", root, recv)
                        }
                        ("BrowserLocator", "wait") => format!(
                            "{}jet_browser_locator_wait(&({}), {})",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserLocator", "wait_gone") => format!(
                            "{}jet_browser_locator_wait_gone(&({}), {})",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserLocator", "click") => {
                            format!("{}jet_browser_locator_click(&({}))", root, recv)
                        }
                        ("BrowserLocator", "hover") => {
                            format!("{}jet_browser_locator_hover(&({}))", root, recv)
                        }
                        ("BrowserLocator", "fill") => format!(
                            "{}jet_browser_locator_fill(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserLocator", "press") => format!(
                            "{}jet_browser_locator_press(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserLocator", "set_files") => format!(
                            "{}jet_browser_locator_set_files(&({}), &({}))",
                            root,
                            recv,
                            a(0)
                        ),
                        ("BrowserIntercept", "remove") => {
                            format!("{}jet_browser_intercept_remove(&({}))", root, recv)
                        }
                        ("BrowserEvent", "kind") => {
                            format!("{}jet_browser_event_kind(&({}))", root, recv)
                        }
                        ("BrowserEvent", "request_id") => {
                            format!("{}jet_browser_event_request_id(&({}))", root, recv)
                        }
                        ("BrowserEvent", "request_method") => {
                            format!("{}jet_browser_event_request_method(&({}))", root, recv)
                        }
                        ("BrowserEvent", "url_hash") => {
                            format!("{}jet_browser_event_url_hash(&({}))", root, recv)
                        }
                        ("BrowserEvent", "is_blocked") => {
                            format!("{}jet_browser_event_is_blocked(&({}))", root, recv)
                        }
                        ("BrowserEvent", "status_code") => {
                            format!("{}jet_browser_event_status_code(&({}))", root, recv)
                        }
                        ("BrowserEvent", "download_id") => {
                            format!("{}jet_browser_event_download_id(&({}))", root, recv)
                        }
                        ("BrowserEvent", "suggested_filename_hash") => format!(
                            "{}jet_browser_event_suggested_filename_hash(&({}))",
                            root, recv
                        ),
                        ("BrowserProtocol", "send") => format!(
                            "{}jet_browser_protocol_send(&({}), &({}), &({}))",
                            root,
                            recv,
                            a(0),
                            a(1)
                        ),
                        ("BrowserCapabilities", "bidi") => format!(
                            "{}jet_browser_capabilities_bidi(&({}))",
                            root,
                            recv
                        ),
                        ("BrowserCapabilities", "cdp") => format!(
                            "{}jet_browser_capabilities_cdp(&({}))",
                            root,
                            recv
                        ),
                        ("BrowserCapabilities", "profile") => format!(
                            "{}jet_browser_capabilities_profile(&({}))",
                            root,
                            recv
                        ),
                        ("BrowserTrace", "entry_count") => {
                            format!("{}jet_browser_trace_entry_count(&({}))", root, recv)
                        }
                        ("BrowserTrace", "redacted") => {
                            format!("{}jet_browser_trace_redacted(&({}))", root, recv)
                        }
                        ("BrowserTrace", "summary") => {
                            format!("{}jet_browser_trace_summary(&({}))", root, recv)
                        }
                        ("BrowserReceipt", "entry_count") => {
                            format!("{}jet_browser_receipt_entry_count(&({}))", root, recv)
                        }
                        ("BrowserReceipt", "redacted") => {
                            format!("{}jet_browser_receipt_redacted(&({}))", root, recv)
                        }
                        ("BrowserReceipt", "summary") => {
                            format!("{}jet_browser_receipt_summary(&({}))", root, recv)
                        }
                        ("BrowserReceipt", "isolated") => {
                            format!("{}jet_browser_receipt_isolated(&({}))", root, recv)
                        }
                        ("BrowserReceipt", "cleaned") => {
                            format!("{}jet_browser_receipt_cleaned(&({}))", root, recv)
                        }
                        ("BrowserPrivacy", "isolated_profiles") => format!(
                            "{}jet_browser_privacy_isolated_profiles(&({}))",
                            root, recv
                        ),
                        ("BrowserPrivacy", "redact_receipts") => format!(
                            "{}jet_browser_privacy_redact_receipts(&({}))",
                            root, recv
                        ),
                        ("BrowserPrivacy", "shared_profiles") => format!(
                            "{}jet_browser_privacy_shared_profiles(&({}))",
                            root, recv
                        ),
                        ("BrowserLocked", "engine") => {
                            format!("{}jet_browser_locked_engine(&({}))", root, recv)
                        }
                        ("BrowserLocked", "version") => {
                            format!("{}jet_browser_locked_version(&({}))", root, recv)
                        }
                        ("BrowserLocked", "binary") => {
                            format!("{}jet_browser_locked_binary(&({}))", root, recv)
                        }
                        ("BrowserLocked", "protocol") => {
                            format!("{}jet_browser_locked_protocol(&({}))", root, recv)
                        }
                        ("BrowserLocked", "verify") => {
                            format!("{}jet_browser_locked_verify(&({}))", root, recv)
                        }
                        ("HTTPResponse", "header") => format!(
                            "{}jet_http_srv_response_header({}, &({}), &({}))",
                            root,
                            recv,
                            a(0),
                            a(1)
                        ),
                        ("HTTPResponse", "status") => format!("{}jet_http_srv_response_status(&({}))", root, recv),
                        ("HTTPResponse", "body") => format!("{}jet_http_srv_response_body(&({}))", root, recv),
                        ("HTTPResponse", "trailers") => format!(
                            "{}jet_http_srv_response_trailers({}, {})",
                            root,
                            recv,
                            a(0)
                        ),
                        ("HTTPResponse", "cookies") => format!("{}jet_http_response_cookies(&({}))", root, recv),
                        ("HTTPServer", "local_addr") => format!("{}jet_http_server_local_addr(&({})).map_err(|_| JetHTTPError::IO {{ operation: \"local address\".to_string() }})", root, recv),
                        ("HTTPServer", "serve") => format!("{}jet_http_server_serve(&({})).map_err(|_| JetHTTPError::IO {{ operation: \"serve\".to_string() }})", root, recv),
                        ("HTTPServer", "shutdown") => format!("{}jet_http_server_shutdown(&({}), &({})).map_err(|_| JetHTTPError::IO {{ operation: \"shutdown\".to_string() }})", root, recv, a(0)),
                        _ => {
                            if args.is_empty() {
                                format!("({}).{}()", recv, method)
                            } else {
                                format!("({}).{}({})", recv, method, a(0))
                            }
                        }
                    }
                }
                // D-TIMEDEPTH1=A: civil-time method call.
                THandleOp::CivilTimeMethod { kind: _, method } => match method.as_str() {
                    "add_days" => format!("({}).add_days({})", recv, a(0)),
                    "add_months" => format!("({}).add_months({})", recv, a(0)),
                    "add_period" => format!("({}).add_period(&({}))", recv, a(0)),
                    "add_duration" => {
                        format!("{}jet_zoned_add_duration(&({}), &({}))", root, recv, a(0))
                    }
                    "diff_days" => format!("({}).diff_days(&({}))", recv, a(0)),
                    "plus_duration" => {
                        format!("{}jet_datetime_plus_duration(&({}), &({}))", root, recv, a(0))
                    }
                    "difference" => {
                        format!("{}jet_datetime_difference(&({}), &({}))", root, recv, a(0))
                    }
                    "elapsed" => format!("{}jet_instant_elapsed(&({}))", root, recv),
                    "in_zone" => format!("({}).in_zone(&({}))", recv, a(0)),
                    "truncate" | "round" | "floor" | "ceil" => {
                        format!("({}).{}(&({}))", recv, method, a(0))
                    }
                    "replace" if args.len() == 3 => {
                        format!(
                            "({}).replace({}, {}, {})",
                            recv,
                            a(0),
                            a(1),
                            a(2)
                        )
                    }
                    "replace" => {
                        format!(
                            "({}).replace({}, {}, {}, {}, {}, {})",
                            recv,
                            a(0),
                            a(1),
                            a(2),
                            a(3),
                            a(4),
                            a(5)
                        )
                    }
                    "format" => format!("({}).format_pattern(&({}))", recv, a(0)),
                    "to_string" => format!("({}).to_string_fmt()", recv),
                    _ => {
                        if args.is_empty() {
                            format!("({}).{}()", recv, method)
                        } else {
                            format!("({}).{}({})", recv, method, a(0))
                        }
                    }
                },
                // D-URL1=A: typed Url/Mime methods.
                THandleOp::UrlMimeMethod { kind: _, method } => match method.as_str() {
                    "join" | "param" => format!("({}).{}(&({}))", recv, method, a(0)),
                    "set_query" | "add_query" => {
                        format!("({}).{}(&({}), &({}))", recv, method, a(0), a(1))
                    }
                    "to_string" => format!("({}).to_string_value()", recv),
                    _ => {
                        if args.is_empty() {
                            format!("({}).{}()", recv, method)
                        } else {
                            format!("({}).{}({})", recv, method, a(0))
                        }
                    }
                },
                THandleOp::EmailMethod { method } => match method.as_str() {
                    "envelope" => format!("({}).envelope().clone()", recv),
                    "with_envelope" => format!("({}).with_envelope(&({}))", recv, a(0)),
                    "send" => format!("({}).send({})", recv, a(0)),
                    _ => unreachable!("unknown email method"),
                },
                THandleOp::RegexMethod { kind: _, method } => match method.as_str() {
                    "match" => format!("({}).match_value(&({}))", recv, a(0)),
                    "is_match" | "find" | "find_all" | "matches" | "split" | "name" | "count" => {
                        format!("({}).{}(&({}))", recv, method, a(0))
                    }
                    "replace" | "replace_all" => {
                        format!("({}).{}(&({}), &({}))", recv, method, a(0), a(1))
                    }
                    "split_limit" => {
                        format!("({}).{}(&({}), {})", recv, method, a(0), a(1))
                    }
                    "replace_all_with" => {
                        format!("({}).replace_all_with(&({}), {})", recv, a(0), a(1))
                    }
                    "group" | "group_start" | "group_end" => {
                        format!("({}).{}({})", recv, method, a(0))
                    }
                    "start" | "end" | "pattern" | "source" | "flags" | "options" | "names"
                    | "named_captures" => format!("({}).{}()", recv, method),
                    _ => {
                        if args.is_empty() {
                            format!("({}).{}()", recv, method)
                        } else {
                            format!("({}).{}({})", recv, method, a(0))
                        }
                    }
                },
                // D-APPROX1=A: sketch method call. `add` args may be string borrows;
                // `count`/`quantile` pass by value; `sample` returns Vec<String>.
                THandleOp::SketchMethod { sketch, method } => {
                    match method.as_str() {
                        "add" if sketch == "TDigest" => format!("({}).add({})", recv, a(0)),
                        "add" if sketch == "ReservoirSampler" => {
                            format!("({}).add(({}).clone())", recv, a(0))
                        }
                        "add" => format!("({}).add(&({}))", recv, a(0)),
                        // HLL.count() and CMS.count(key) — different arities.
                        "count" if args.is_empty() => format!("({}).count()", recv),
                        "count" => format!("({}).count(&({}))", recv, a(0)),
                        _ => {
                            if args.is_empty() {
                                format!("({}).{}()", recv, method)
                            } else {
                                format!("({}).{}({})", recv, method, a(0))
                            }
                        }
                    }
                }
                // c109 Phase 25: HTTPRouter route registration, byte-for-byte the
                // `emit_builtin_method` router arm (Source/Codegen/Expression.rs ~L937).
                // `recv` is `&mut`-borrowed; the path is plain (args[0]); the handler is
                // the pre-rendered boxed closure.
                THandleOp::HTTPRouterRegister {
                    verb,
                    handler,
                    file,
                    line,
                } => format!(
                    "{}jet_http_router_register(&mut ({}), \"{}\".to_string(), {}, {}, {:?}, {})",
                    root,
                    recv,
                    verb,
                    a(0),
                    handler,
                    file,
                    line
                ),
                // D-SIMD2 / D-LINALG1: a math-type instance method → the prelude free
                // function `jet_math_<type>_<method>(&(recv), <args>)`. `reduce`
                // dispatches on the validated marker op. All take `&recv` (immutable;
                // these types are value semantics — every op returns a fresh value).
                THandleOp::MathMethod {
                    type_name,
                    method,
                    reduce_op,
                } => {
                    let fname = match reduce_op {
                        Some(op) => format!("jet_math_{}_reduce_{}", type_name, op.to_lowercase()),
                        None => format!("jet_math_{}_{}", type_name, method),
                    };
                    let mut call = format!("{}{}(&({})", root, fname, recv);
                    for i in 0..args.len() {
                        call.push_str(&format!(", {}", a(i)));
                    }
                    call.push(')');
                    call
                }
                // D-SERDE-ACCESS=B: DataTree accessor methods.
                THandleOp::DataTreeField => format!("({}).field(&({}))", recv, a(0)),
                THandleOp::DataTreeAt => format!("({}).at({})", recv, a(0)),
                THandleOp::DataTreeInt => format!("({}).int()", recv),
                THandleOp::DataTreeText => format!("({}).text()", recv),
                THandleOp::DataTreeBool => format!("({}).bool()", recv),
                THandleOp::DataTreeFloat => format!("({}).float()", recv),
                THandleOp::DataTreeDecode(target) => format!(
                    "<{} as user_Decode>::jet_decode(&({}))",
                    cx.rust_type(target),
                    recv
                ),
                THandleOp::SerdeEncode => format!("user_Encode::jet_encode(&({}))", recv),
                // D-SERDE-ACCESS=B: same accessors on JSON/Data.
                THandleOp::JSONField => format!("({}).field(&({}))", recv, a(0)),
                THandleOp::JSONAt => format!("({}).at({})", recv, a(0)),
                THandleOp::JSONInt => format!("({}).int()", recv),
                THandleOp::JSONText => format!("({}).text()", recv),
                THandleOp::JSONBool => format!("({}).bool()", recv),
                THandleOp::JSONFloat => format!("({}).float()", recv),
                // D-PATHFS1: Path object methods.
                THandleOp::PathFrom => format!("{}jet_path_from(&({}))", root, recv),
                THandleOp::PathJoin => format!("{}jet_path_join(&({}), &({}))", root, recv, a(0)),
                THandleOp::PathParent => format!("{}jet_path_parent(&({}))", root, recv),
                THandleOp::PathExtension => format!("{}jet_path_extension(&({}))", root, recv),
                THandleOp::PathStem => format!("{}jet_path_stem(&({}))", root, recv),
                THandleOp::PathToString => format!("({}).jet_show()", recv),
                THandleOp::PathWriteAtomic => {
                    format!("{}jet_path_write_atomic(&({}), &({}))", root, recv, a(0))
                }
                THandleOp::PathWalk => format!("{}jet_path_walk(&({}))", root, recv),
                // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor`. Every read is
                // fallible (`Result<T, String>`) — a bounds/match miss is an ordinary
                // `Err`, never a panic (I1/L2).
                THandleOp::ReaderOver => format!("{}jet_reader_over(&({}))", root, recv),
                THandleOp::ReaderReadU8 => format!("{}jet_reader_read_u8(&mut ({}))", root, recv),
                THandleOp::ReaderReadU16Le => {
                    format!("{}jet_reader_read_u16_le(&mut ({}))", root, recv)
                }
                THandleOp::ReaderReadU16Be => {
                    format!("{}jet_reader_read_u16_be(&mut ({}))", root, recv)
                }
                THandleOp::ReaderReadU32Le => {
                    format!("{}jet_reader_read_u32_le(&mut ({}))", root, recv)
                }
                THandleOp::ReaderReadU32Be => {
                    format!("{}jet_reader_read_u32_be(&mut ({}))", root, recv)
                }
                THandleOp::ReaderReadU64Le => {
                    format!("{}jet_reader_read_u64_le(&mut ({}))", root, recv)
                }
                THandleOp::ReaderReadU64Be => {
                    format!("{}jet_reader_read_u64_be(&mut ({}))", root, recv)
                }
                THandleOp::ReaderTake => {
                    // D-BINREAD-LEN1=A: sema admits only Int/U8/U16/U32 here.
                    // The host boundary widens the accepted unsigned lengths
                    // to Reader.take's internal Int representation.
                    format!("{}jet_reader_take(&mut ({}), ({}) as i64)", root, recv, a(0))
                }
                THandleOp::ReaderRemaining => format!("{}jet_reader_remaining(&({}))", root, recv),
                THandleOp::ReaderAtEnd => format!("{}jet_reader_at_end(&({}))", root, recv),
                THandleOp::CursorOver => format!("{}jet_cursor_over(&({}))", root, recv),
                THandleOp::CursorTakeUntil => {
                    format!("{}jet_cursor_take_until(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::CursorSkipWs => format!("{}jet_cursor_skip_ws(&mut ({}))", root, recv),
                // D-SHIFT1: `cursor.take_pattern("…")` — inline scan (I8: the
                // D-PARSESTR1 engine in consume mode, `str_match_scan_closure_ex`),
                // built entirely here since it needs `recv`'s already-emitted
                // Rust text (unlike the other `THandleOp` arms, which only
                // format-string it — this one embeds it in a bigger block).
                THandleOp::CursorTakePattern { parts, canonical } => {
                    let (closure, holes) =
                        str_match_scan_closure_ex(parts, cx, "__jet_tail", false);
                    let mut bind_vars: Vec<String> = holes
                        .iter()
                        .map(|(n, _)| format!("__jet_sm_{}", mangle(n)))
                        .collect();
                    bind_vars.push("__jet_consumed".to_string());
                    let bind_pat = tuple_join(&bind_vars);
                    let ok_val = if canonical.is_empty() {
                        "()".to_string()
                    } else {
                        let struct_name = crate::Codegen::Tuples::tuple_struct_name(canonical);
                        let field_inits: Vec<String> = canonical
                            .iter()
                            .zip(holes.iter())
                            .map(|((n, _), (hn, _))| {
                                format!("{}: __jet_sm_{}", mangle(n), mangle(hn))
                            })
                            .collect();
                        format!("{} {{ {} }}", struct_name, field_inits.join(", "))
                    };
                    format!(
                        "{{ let __jet_cur = &mut ({recv}); let __jet_tail: &str = &__jet_cur.buf[__jet_cur.pos..]; match {closure} {{ Some(({bind_pat})) => {{ __jet_cur.pos += __jet_consumed; Ok({ok_val}) }}, None => Err(format!(\"pattern did not match at cursor position {{}}\", __jet_cur.pos)) }} }}",
                        recv = recv,
                        closure = closure,
                        bind_pat = bind_pat,
                        ok_val = ok_val,
                    )
                }
                // D-BINPAT1 (card #506 follow-up): `reader.take_pattern(b"…")` —
                // inline scan (I8: the D-BINPAT1 engine in consume mode,
                // `bin_match_scan_closure_ex`), byte-mode sibling of
                // `CursorTakePattern` immediately above — same shape, `&[u8]`
                // tail instead of `&str`.
                THandleOp::ReaderTakePattern { parts, canonical } => {
                    let (closure, holes) =
                        bin_match_scan_closure_ex(parts, cx, "__jet_tail", false);
                    let mut bind_vars: Vec<String> = holes
                        .iter()
                        .map(|(n, _)| format!("__jet_bm_{}", mangle(n)))
                        .collect();
                    bind_vars.push("__jet_consumed".to_string());
                    let bind_pat = tuple_join(&bind_vars);
                    let ok_val = if canonical.is_empty() {
                        "()".to_string()
                    } else {
                        let struct_name = crate::Codegen::Tuples::tuple_struct_name(canonical);
                        let field_inits: Vec<String> = canonical
                            .iter()
                            .zip(holes.iter())
                            .map(|((n, _), (hn, _))| {
                                format!("{}: __jet_bm_{}", mangle(n), mangle(hn))
                            })
                            .collect();
                        format!("{} {{ {} }}", struct_name, field_inits.join(", "))
                    };
                    format!(
                        "{{ let __jet_rdr = &mut ({recv}); let __jet_tail: &[u8] = &__jet_rdr.buf[__jet_rdr.pos..]; match {closure} {{ Some(({bind_pat})) => {{ __jet_rdr.pos += __jet_consumed; Ok({ok_val}) }}, None => Err(format!(\"pattern did not match at reader position {{}}\", __jet_rdr.pos)) }} }}",
                        recv = recv,
                        closure = closure,
                        bind_pat = bind_pat,
                        ok_val = ok_val,
                    )
                }
                // D-DBDRIVER1: `DBConnection` instance methods. `query`/`query_one`/
                // `execute` cross the FFI bridge boundary as plain wire text (params
                // encoded, rows/count/error decoded) — see `Source/Prelude/DB.rs` and
                // `jet_std::jet_db_{encode_params,decode_query_result,decode_execute_result}`
                // in `Source/Prelude/CoreLib.rs`.
                THandleOp::DBWithPolicy => format!(
                    "{root}JetDbScope {{ handle: ({recv}).handle, policy: ({policy}).clone(), user: ({user}).clone() }}",
                    policy = a(0),
                    user = a(1),
                ),
                THandleOp::ServiceRuntimeSend => format!(
                    "jet_services_runtime_send(&({recv}), &({}), &({}), &({}))",
                    a(0),
                    a(1),
                    a(2)
                ),
                THandleOp::ServiceRuntimeRetry => format!(
                    "jet_services_runtime_retry(&({recv}), &({}))",
                    a(0)
                ),
                THandleOp::ServiceRuntimeDeadLetter => format!(
                    "jet_services_runtime_dead_letter(&({recv}), &({}))",
                    a(0)
                ),
                THandleOp::ServiceRuntimeRetain => format!(
                    "jet_services_runtime_retain(&({recv}), &({}))",
                    a(0)
                ),
                THandleOp::ServiceRuntimeCommit => format!(
                    "jet_services_runtime_commit(&({recv}), &({}))",
                    a(0)
                ),
                THandleOp::DBQuery => format!(
                    "{{ let __jet_scope = &({recv}); let __jet_sql = ({}).clone(); let __jet_params = ({}).clone(); jet_db_scope_query(__jet_scope, &__jet_sql, &__jet_params) }}",
                    a(0),
                    a(1)
                ),
                THandleOp::DBQueryOne => format!(
                    "{{ let __jet_scope = &({recv}); let __jet_sql = ({}).clone(); let __jet_params = ({}).clone(); jet_db_scope_query(__jet_scope, &__jet_sql, &__jet_params).map(|__rows| jet_outcome_of(__rows.into_iter().next())) }}",
                    a(0),
                    a(1)
                ),
                THandleOp::DBExecute => format!(
                    "{{ let __jet_scope = &({recv}); let __jet_sql = ({}).clone(); let __jet_params = ({}).clone(); jet_db_scope_execute(__jet_scope, &__jet_sql, &__jet_params) }}",
                    a(0),
                    a(1)
                ),
                THandleOp::DBLive => format!(
                    "{{ let __jet_scope = &({recv}); let __jet_sql = ({}).clone(); let __jet_params = ({}).clone(); match {root}jet_std::jet_db_apply_policy(&__jet_sql, &__jet_params, &__jet_scope.policy.table, &__jet_scope.policy.expression, &__jet_scope.user) {{ Ok((__sql, __params)) => match {root}jet_std::jet_db_decode_query_result(&{ffi}::jet_db_query(__jet_scope.handle, &__sql, &{root}jet_std::jet_db_encode_params(&__params))) {{ Ok(__rows) => Ok({root}jet_app_live(format!(\"db:{{}}:{{}}\", __jet_scope.policy.table, __jet_sql), format!(\"{{:?}}\", __rows))), Err(e) => Err(e) }}, Err(e) => Err(e) }} }}",
                    a(0),
                    a(1)
                ),
                THandleOp::DBBegin => format!("{ffi}::jet_db_begin(({recv}).handle)"),
                THandleOp::DBCommit => format!("{ffi}::jet_db_commit(({recv}).handle)"),
                THandleOp::DBRollback => format!("{ffi}::jet_db_rollback(({recv}).handle)"),
                THandleOp::DBClose => format!("{ffi}::jet_db_close(({recv}).handle)"),
                // D-DBDRIVER1: `DBValue` accessors — plain inherent Rust methods on
                // the always-compiled `jet_std::DBValue` enum (no FFI bridge involved).
                THandleOp::DBValueInt => format!("({}).int()", recv),
                THandleOp::DBValueFloat => format!("({}).float()", recv),
                THandleOp::DBValueText => format!("({}).text()", recv),
                THandleOp::DBValueBool => format!("({}).bool()", recv),
                THandleOp::DBValueIsNull => format!("({}).is_null()", recv),
                // D-DEP-WASM1=A / D-PLUGIN1=B (c81): `Plugin.call`/`.call_int` —
                // a homogeneous scalar call across the sandboxed Component
                // Model boundary, wire-encoded exactly like `DBQuery` above
                // (args encoded, result decoded; see `Prelude/Plugin.rs` and
                // `jet_std::jet_plugin_{encode_args_float,decode_result_float}`
                // in `Prelude/CoreLib.rs`).
                THandleOp::PluginCall => format!(
                    "{root}jet_std::jet_plugin_decode_result_float(&{ffi}::jet_plugin_call(({recv}).handle, &({}), &{root}jet_std::jet_plugin_encode_args_float(&({}))))",
                    a(0),
                    a(1)
                ),
                THandleOp::PluginCallInt => format!(
                    "{root}jet_std::jet_plugin_decode_result_int(&{ffi}::jet_plugin_call(({recv}).handle, &({}), &{root}jet_std::jet_plugin_encode_args_int(&({}))))",
                    a(0),
                    a(1)
                ),
                THandleOp::ReactiveEffectMethod { method } => match method.as_str() {
                    "unsubscribe" => format!("({recv}).unsubscribe()"),
                    "is_active" => format!("({recv}).active()"),
                    _ => unreachable!("sema admitted only Effect lifecycle methods"),
                },
            }
        }
        // c109 Phase 13: a closure-taking core call. The closure was rendered at
        // lowering; emit assembles the bespoke shape, byte-for-byte `emit_core_call`
        // (Source/Codegen/Expression.rs).
        TExprKind::CoreClosureCall { kind } => match kind {
            TCoreClosureKind::Spawn {
                group,
                spawn_closure,
                scoped,
                ..
            } => match group {
                Some(group) => format!(
                    "({}).{}({})",
                    emit_tir_expr(group, cx),
                    if *scoped { "spawn_scoped" } else { "spawn" },
                    spawn_closure
                ),
                None => format!(
                    "{}jet_std::JetTask::spawn({})",
                    cx.root_prefix, spawn_closure
                ),
            },
            TCoreClosureKind::SpawnGroup {
                count,
                spawn_closure,
                ..
            } => format!(
                "{}jet_std::jet_task_spawn_group({}, {})",
                cx.root_prefix,
                emit_tir_expr(count, cx),
                spawn_closure
            ),
            TCoreClosureKind::Serve { addr, closure } => format!(
                "{}jet_http_serve(&({}), {})",
                cx.root_prefix,
                emit_tir_expr(addr, cx),
                closure
            ),
            TCoreClosureKind::Guard { closure, .. } => {
                format!("{}jet_scope_guard({})", cx.root_prefix, closure)
            }
            // D-TXN3: register a post-commit hook on the transaction handle. Boxed so
            // hooks of differing closure types share one queue; run LIFO in Drop, but
            // only after `commit()` (the `JetTransaction` prelude type).
            TCoreClosureKind::OnCommit { handle, closure, .. } => {
                format!("{}.on_commit(Box::new({}))", handle, closure)
            }
            // D-TXN-ROLLBACK (layer 3): the rollback-hook registration, run LIFO on a
            // `?`-failure and dropped un-run on commit (the `JetTransaction` prelude type).
            TCoreClosureKind::OnRollback { handle, closure, .. } => {
                format!("{}.on_rollback(Box::new({}))", handle, closure)
            }
            // D-REACT1=B: a derived value recomputed from its signals.
            TCoreClosureKind::ReactiveDerived { closure, .. } => {
                format!("{}jet_std::JetDerived::new({})", cx.root_prefix, closure)
            }
            // D-REACT1=B: an effect re-run when a signal it read changes.
            TCoreClosureKind::ReactiveEffect { closure, .. } => {
                format!(
                    "{}jet_std::jet_reactive_effect({})",
                    cx.root_prefix, closure
                )
            }
            TCoreClosureKind::UiReactiveRender { closure, .. } => {
                format!("{}jet_ui_reactive_render({})", cx.root_prefix, closure)
            }
            // D-WEB-CLICK-PORT1=D: portable `ui.button(label, on_click:)`.
            TCoreClosureKind::UiButtonOnClick {
                label,
                closure,
                ..
            } => {
                let label = emit_tir_expr(label, cx);
                format!(
                    "{}jet_ui_button_on_click(&({}), {})",
                    cx.root_prefix, label, closure
                )
            }
        },
        // D-TASKSCOPE1=A: `g.all([h1, h2, …])` — join each handle in list order.
        TExprKind::TaskGroupAll { tasks } => {
            let list = emit_tir_expr(tasks, cx);
            format!("{}jet_std::jet_task_all({list})", cx.root_prefix)
        }
        TExprKind::TaskGroupRace { tasks } => {
            let list = emit_tir_expr(tasks, cx);
            format!("{}jet_std::jet_task_race({list})", cx.root_prefix)
        }
        TExprKind::TaskGroupAny { tasks } => {
            let list = emit_tir_expr(tasks, cx);
            format!("{}jet_std::jet_task_any({list})", cx.root_prefix)
        }
        TExprKind::SelectStart => {
            format!("{}jet_std::JetSelectBuilder::start()", cx.root_prefix)
        }
        TExprKind::SelectRecv { builder, channel } => {
            let b = emit_tir_expr(builder, cx);
            let ch = emit_tir_expr(channel, cx);
            format!("{b}.recv({ch})")
        }
        TExprKind::SelectAfter {
            builder,
            millis,
            value,
        } => {
            let b = emit_tir_expr(builder, cx);
            let ms = emit_tir_expr(millis, cx);
            if let Some(value) = value {
                let v = emit_tir_expr(value, cx);
                format!("{b}.after_value({ms}, {v})")
            } else {
                format!("{b}.after({ms})")
            }
        }
        TExprKind::SelectRead { builder, stream } => {
            let b = emit_tir_expr(builder, cx);
            let s = emit_tir_expr(stream, cx);
            format!("{b}.read({s})")
        }
        TExprKind::SelectWait { builder } => {
            let (recvs, afters) = collect_select_arms(builder, cx);
            let recv_list = if recvs.is_empty() {
                "&[]".to_string()
            } else {
                format!("&[&{}]", recvs.join(", &"))
            };
            let after_list = if afters.is_empty() {
                "Vec::new()".to_string()
            } else {
                format!("vec![{}]", afters.join(", "))
            };
            format!(
                "{}jet_std::jet_select_wait({}, {})",
                cx.root_prefix, recv_list, after_list
            )
        }
        // c109 Phase 13: a fn-typed value. A bare fn-name value echoes the
        // already-rendered `Box::new(move |…| …) as <fn-type>` wrapper; a call through
        // a fn-value emits `({callee})({args})`, byte-for-byte `emit_expr`'s
        // `Expr::CallValue` (Source/Codegen/Expression.rs).
        TExprKind::FnValue { kind } => match kind {
            TFnValueKind::NamedFn { wrapper, .. } => wrapper.clone(),
            TFnValueKind::Call { callee, args } => {
                format!(
                    "({})({})",
                    emit_tir_expr(callee, cx),
                    emit_tir_call_args(args, cx)
                )
            }
        },
        // c109 Phase 14: a cross-module call. The path form was resolved at lowering;
        // emit prepends `cx.root_prefix` exactly where the AST path does (both the
        // qualified `{root}{mod}::{fn}` form and the inline `{root}user_{mangled}` form
        // prefix with root). Args were resolved into `TCallArg`s (`emit_tir_call_args`).
        TExprKind::ModuleCall {
            form,
            type_args,
            args,
        } => {
            let arg_str = emit_tir_call_args(args, cx);
            let type_args = if type_args.is_empty() {
                String::new()
            } else {
                format!(
                    "::<{}>",
                    type_args
                        .iter()
                        .map(|ty| cx.rust_type(ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            match form {
                TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    format!(
                        "{}{}::{}{}({})",
                        cx.root_prefix, rust_mod, rust_fn, type_args, arg_str
                    )
                }
                TModuleCallForm::InlineMangled { mangled } => {
                    format!("{}user_{}{}({})", cx.root_prefix, mangled, type_args, arg_str)
                }
            }
        }
        // c109 Phase 14: an FFI extern call. Reproduces `emit_call`'s `extern_funcs`
        // arm: `{ffi_crate}::{wrapper}(args)`. `cx.ffi_crate` is program-level (read
        // here, like Phase 10's regex form); the AST falls back to "jet_ffi" when it is
        // `None` (always `Some` when an extern call is present, but mirror it exactly).
        // Args use the extern arg form (`(…).clone()` for a non-scalar Read).
        TExprKind::ExternCall { wrapper, args } => {
            let crate_name = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            let arg_str = args
                .iter()
                .map(|a| {
                    let s = emit_tir_expr(&a.value, cx);
                    if a.clone {
                        format!("({}).clone()", s)
                    } else {
                        s
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}::{}({})", crate_name, wrapper, arg_str)
        }
    }
}

/// Recover source-order Object pairs from a MapLit or its #779 IfExpr desugar.
fn object_ordered_pairs_rust(val: &TExpr, cx: &Cx) -> Option<String> {
    let entries: Vec<(&TExpr, &TExpr)> = match &val.kind {
        TExprKind::MapLit(entries) => entries.iter().map(|(k, v)| (k, v)).collect(),
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            ..
        } => {
            let TIfCond::Plain(c) = cond.as_ref() else {
                return None;
            };
            if !matches!(&c.kind, TExprKind::BoolLit(true)) {
                return None;
            }
            let TExprKind::Local(result) = &then_value.kind else {
                return None;
            };
            let (first, rest) = then_body.split_first()?;
            let TStmt::Let { name, init, .. } = first else {
                return None;
            };
            if name != &result.name {
                return None;
            }
            if !matches!(&init.kind, TExprKind::MapLit(entries) if entries.is_empty()) {
                return None;
            }
            let mut pairs = Vec::with_capacity(rest.len());
            for stmt in rest {
                let TStmt::IndexAssign {
                    base,
                    index,
                    is_map: true,
                    value,
                    ..
                } = stmt
                else {
                    return None;
                };
                let TExprKind::Local(base_local) = &base.kind else {
                    return None;
                };
                if base_local.name != *name {
                    return None;
                }
                pairs.push((index, value));
            }
            pairs
        }
        _ => return None,
    };
    Some(
        entries
            .into_iter()
            .map(|(key, value)| {
                format!(
                    "(({}).clone(), {})",
                    emit_tir_expr(key, cx),
                    emit_tir_expr(value, cx)
                )
            })
            .collect::<Vec<_>>()
            .join(", "),
    )
}
